//! The [`Client`] type: the primary interface to a running `lean --server` process.
//!
//! [`Client::start`] creates a temporary Lean project, spawns the child process,
//! completes the LSP `initialize`/`initialized` handshake, and opens `Proof.lean`.
//! Named LSP methods (`hover`, `did_open`, etc.) are added to `Client` by
//! [`crate::lean::messages`]. Call [`Client::stop`] for a clean shutdown;
//! dropping without calling `stop` kills the child process via `kill_on_drop`.

#![allow(dead_code)]

use async_lsp::Error::ServiceStopped;
use async_lsp::{LanguageServer, Result, ServerSocket};
#[allow(unused_imports)]
use lsp_types::notification::Exit;
use lsp_types::request::{Request, Shutdown};
use lsp_types::{
    ClientCapabilities, DidCloseTextDocumentParams, DidOpenTextDocumentParams, InitializeParams,
    InitializeResult, InitializedParams, SemanticTokensClientCapabilities,
    SemanticTokensFullOptions, SemanticTokensServerCapabilities,
    SemanticTokensWorkspaceClientCapabilities, TextDocumentClientCapabilities,
    TextDocumentIdentifier, TokenFormat, WorkspaceClientCapabilities, WorkspaceFolder,
};

macro_rules! lines_from_file {
    ($path:literal) => {
        include_str!($path)
            .lines()
            .filter(|l| !l.is_empty())
            .map(Into::into)
            .collect()
    };
}

fn client_capabilities() -> ClientCapabilities {
    let mut capabilities = ClientCapabilities::default();
    let mut sem_tok = SemanticTokensClientCapabilities::default();
    sem_tok.requests.full = Some(SemanticTokensFullOptions::Bool(true));
    sem_tok.token_types = lines_from_file!("token_types.txt");
    sem_tok.token_modifiers = lines_from_file!("token_modifiers.txt");
    sem_tok.formats = vec![TokenFormat::RELATIVE];
    capabilities.text_document = Some(TextDocumentClientCapabilities {
        semantic_tokens: Some(sem_tok),
        ..Default::default()
    });
    capabilities.workspace = Some(WorkspaceClientCapabilities {
        semantic_tokens: Some(SemanticTokensWorkspaceClientCapabilities {
            refresh_support: Some(true),
        }),
        ..Default::default()
    });
    capabilities
}
use super::server::{Server, ServerStarted};
use crate::lean::messages::turnstile::TurnstileMessage;
use crate::lean::messages::DisplayLspParams;
use crate::lean::protocol::Protocol;
use lsp_types::notification::Notification;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use url::Url;

/// Handle to a running `lean --server` process.
///
/// Created by [`Client::start`], which creates a temporary project
/// directory, spawns the child process, completes the LSP
/// `initialize`/`initialized` handshake, and opens `Proof.lean` before
/// returning. Call [`Client::stop`] for a clean shutdown; dropping without
/// calling `stop` kills the child process via `kill_on_drop`.
pub struct Client {
    pub(super) socket: ServerSocket,
    pub init_result: InitializeResult,
    /// URI of the `Proof.lean` file opened in this session.
    pub proof_uri: Url,
    process: Server,
    exit_expected: Arc<AtomicBool>,
    pub(super) protocol: Arc<Protocol>,
}

fn log_lsp(method: &str, params: &impl DisplayLspParams) {
    let p = params.display().to_string();
    if p.is_empty() {
        tracing::debug!("Client→LSP: {}", method);
    } else {
        tracing::debug!("Client→LSP: {}\n{}", method, p);
    }
}

impl Client {
    /// Spawn `lean --server` against a fresh temporary project and complete the
    /// LSP initialization handshake. The `on_message` callback is called for
    /// every [`TurnstileMessage`] produced by the server.
    pub async fn start(
        initial_content: &str,
        on_message: impl Fn(TurnstileMessage) + Send + 'static,
    ) -> Result<Self> {
        let protocol = Arc::new(Protocol::new());
        let ServerStarted {
            process,
            mut socket,
            root_uri,
            proof_uri,
            server_rx,
            exit_expected,
        } = Server::start(initial_content, Arc::clone(&protocol)).await?;
        let init_result = socket
            .initialize(InitializeParams {
                process_id: Some(std::process::id()),
                workspace_folders: Some(vec![WorkspaceFolder {
                    uri: root_uri,
                    name: "turnstile".to_string(),
                }]),
                capabilities: client_capabilities(),
                ..InitializeParams::default()
            })
            .await?;
        socket.initialized(InitializedParams {})?;

        let mut server_rx = server_rx;
        tokio::spawn(async move {
            while let Some(lean_msg) = server_rx.recv().await {
                if let Some(t) = crate::lean::messages::turnstile::from_lean(lean_msg) {
                    on_message(t);
                }
            }
        });

        let client = Self {
            socket,
            init_result,
            proof_uri,
            process,
            exit_expected,
            protocol,
        };
        client.did_open(DidOpenTextDocumentParams {
            text_document: lsp_types::TextDocumentItem {
                uri: client.proof_uri.clone(),
                language_id: "lean4".to_string(),
                version: 0,
                text: initial_content.to_string(),
            },
        })?;

        Ok(client)
    }

    /// Perform a clean shutdown: close `Proof.lean`, send LSP `shutdown`/`exit`,
    /// and wait for the child process to exit.
    pub async fn stop(mut self) -> Result<()> {
        self.did_close(DidCloseTextDocumentParams {
            text_document: TextDocumentIdentifier {
                uri: self.proof_uri.clone(),
            },
        })?;
        self.exit_expected.store(true, Ordering::Release);
        self.request::<Shutdown>(()).await?;
        self.notify::<Exit>(())?;
        self.process
            .process
            .wait()
            .await
            .map_err(|_| ServiceStopped)?;
        Ok(())
    }

    /// Apply a batch of incremental edits to `Proof.lean`. Bumps the document
    /// version and sends an incremental `didChange` to the server. Returns
    /// the new version.
    ///
    /// All edits to the document must go through this method (or
    /// [`Client::replace_document`]) so that [`Protocol`]'s version counter —
    /// the one used for staleness filtering — matches what the server sees.
    pub fn change_document(
        &self,
        content_changes: Vec<lsp_types::TextDocumentContentChangeEvent>,
    ) -> Result<i32> {
        use lsp_types::{DidChangeTextDocumentParams, VersionedTextDocumentIdentifier};
        let version = self.protocol.next_version();
        self.did_change(DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier {
                uri: self.proof_uri.clone(),
                version,
            },
            content_changes,
        })?;
        Ok(version)
    }

    /// Replace the entire content of `Proof.lean` with `new_text`. Bumps the
    /// document version and sends a full-sync `didChange` to the server.
    pub fn replace_document(&self, new_text: String) -> Result<i32> {
        use lsp_types::{
            DidChangeTextDocumentParams, TextDocumentContentChangeEvent,
            VersionedTextDocumentIdentifier,
        };
        let version = self.protocol.next_version();
        self.did_change(DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier {
                uri: self.proof_uri.clone(),
                version,
            },
            content_changes: vec![TextDocumentContentChangeEvent {
                range: None,
                range_length: None,
                text: new_text,
            }],
        })?;
        Ok(version)
    }

    /// The document version most recently sent to the server.
    pub fn current_version(&self) -> i32 {
        self.protocol.current_version()
    }

    /// Returns the semantic token type and modifier legends from the server's `InitializeResult`.
    pub fn token_legend(&self) -> (Vec<String>, Vec<String>) {
        fn extract(legend: &lsp_types::SemanticTokensLegend) -> (Vec<String>, Vec<String>) {
            (
                legend
                    .token_types
                    .iter()
                    .map(|t| t.as_str().to_owned())
                    .collect(),
                legend
                    .token_modifiers
                    .iter()
                    .map(|m| m.as_str().to_owned())
                    .collect(),
            )
        }
        match &self.init_result.capabilities.semantic_tokens_provider {
            Some(SemanticTokensServerCapabilities::SemanticTokensOptions(o)) => extract(&o.legend),
            Some(SemanticTokensServerCapabilities::SemanticTokensRegistrationOptions(o)) => {
                extract(&o.semantic_tokens_options.legend)
            }
            None => (vec![], vec![]),
        }
    }

    /// Send an LSP request to `lean --server` and await the response.
    pub async fn request<R>(&self, params: R::Params) -> Result<R::Result>
    where
        R: Request,
        R::Params: Send + DisplayLspParams,
    {
        log_lsp(R::METHOD, &params);
        self.socket.request::<R>(params).await
    }

    /// Send a fire-and-forget LSP notification to `lean --server`.
    pub fn notify<N>(&self, params: N::Params) -> Result<()>
    where
        N: Notification,
        N::Params: DisplayLspParams,
    {
        log_lsp(N::METHOD, &params);
        self.socket.notify::<N>(params)
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        // `kill_on_drop` handles the actual process kill. Log when stop() was
        // not called so ungraceful shutdowns are visible in traces.
        if self.process.process.id().is_some() {
            tracing::warn!("Client dropped without stop() — killing lean --server");
        }
    }
}
