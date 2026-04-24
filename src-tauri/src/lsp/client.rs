//! Lean LSP client built on `async-lsp`.
//!
//! [`LeanClient`] owns the full lifetime of a `lean --server` process: it
//! spawns the child, runs the initialization handshake, and provides a clean
//! [`LeanClient::stop`] for the proper LSP teardown sequence. If the
//! value is dropped without calling `shutdown`, the child is killed as a
//! best-effort fallback via `kill_on_drop`.

use async_lsp::concurrency::ConcurrencyLayer;
use async_lsp::lsp_types::{notification::Notification, request::Request};
use async_lsp::lsp_types::{
    ClientCapabilities, InitializeParams, InitializeResult, InitializedParams, Url, WorkspaceFolder,
};
use async_lsp::panic::CatchUnwindLayer;
use async_lsp::router::Router;
use async_lsp::tracing::TracingLayer;
use async_lsp::{LanguageClient, LanguageServer, MainLoop, ResponseError, ServerSocket};
use lsp_types::notification::{
    DidChangeTextDocument, DidCloseTextDocument, DidOpenTextDocument, DidSaveTextDocument, Exit,
};
use lsp_types::request::{
    CodeActionRequest, CodeActionResolveRequest, Completion, DocumentSymbolRequest, Formatting,
    GotoDefinition, HoverRequest, SemanticTokensFullRequest, Shutdown,
};
use lsp_types::{
    CodeAction, CodeActionClientCapabilities, CodeActionKindLiteralSupport,
    CodeActionLiteralSupport, CodeActionParams, CodeActionResponse, CompletionClientCapabilities,
    CompletionItemCapability, CompletionParams, CompletionResponse,
    DocumentFormattingClientCapabilities, DocumentSymbolClientCapabilities, DocumentSymbolParams,
    DocumentSymbolResponse, GotoCapability, GotoDefinitionParams, GotoDefinitionResponse, Hover,
    HoverClientCapabilities, HoverParams, LogMessageParams, PublishDiagnosticsClientCapabilities,
    PublishDiagnosticsParams, Range, SemanticTokensClientCapabilities,
    SemanticTokensClientCapabilitiesRequests, SemanticTokensFullOptions, SemanticTokensParams,
    SemanticTokensResult, SemanticTokensServerCapabilities, ShowMessageParams,
    TextDocumentClientCapabilities, TextDocumentPositionParams, TextDocumentSyncClientCapabilities,
};
use lsp_types::{DocumentFormattingParams, TextEdit};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::future::Future;
use std::ops::ControlFlow;
use std::path::Path;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use thiserror::Error;
use tokio::process::{Child, Command};
use tokio::sync::mpsc;
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};
use tower::ServiceBuilder;
use tracing::{debug, error, instrument, warn, Instrument};

use crate::lsp::events::LspEvent;

/// Response type for the Lean `$/lean/plainGoal` extension request.
#[derive(Debug, Serialize, Deserialize)]
pub struct PlainGoal {
    pub rendered: String,
}

/// Custom request type for `$/lean/plainGoal`.
pub enum LeanPlainGoal {}

/// Response type for the Lean `$/lean/plainTermGoal` extension request.
#[derive(Debug, Serialize, Deserialize)]
pub struct PlainTermGoal {
    pub goal: String,
}

/// Custom request type for `$/lean/plainTermGoal`.
pub enum LeanPlainTermGoal {}

impl Request for LeanPlainGoal {
    type Params = TextDocumentPositionParams;
    type Result = Option<PlainGoal>;
    const METHOD: &'static str = "$/lean/plainGoal";
}

impl Request for LeanPlainTermGoal {
    type Params = TextDocumentPositionParams;
    type Result = Option<PlainTermGoal>;
    const METHOD: &'static str = "$/lean/plainTermGoal";
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FileProgressInterval {
    pub range: Range,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FileProgressParams {
    #[serde(rename = "textDocument")]
    pub text_document: lsp_types::TextDocumentIdentifier,
    pub processing: Vec<FileProgressInterval>,
}

/// Custom notification type for `$/lean/fileProgress`.
pub enum LeanFileProgress {}

impl Notification for LeanFileProgress {
    type Params = FileProgressParams;
    const METHOD: &'static str = "$/lean/fileProgress";
}

/// Inbound half of the Lean LSP client.
/// Handlers run on the `MainLoop` task when the server pushes messages.
struct LeanListener {
    tx: mpsc::Sender<LspEvent>,
}

impl LanguageClient for LeanListener {
    type Error = ResponseError;
    type NotifyResult = ControlFlow<async_lsp::Result<()>>;

    fn semantic_tokens_refresh(
        &mut self,
        _params: <lsp_types::request::SemanticTokensRefresh as Request>::Params,
    ) -> std::pin::Pin<Box<dyn Future<Output = async_lsp::Result<(), Self::Error>> + Send>> {
        Box::pin(std::future::ready(Ok(())))
    }

    #[instrument(skip_all)]
    fn show_message(&mut self, params: ShowMessageParams) -> Self::NotifyResult {
        self.tx.try_send(LspEvent::ShowMessage(params)).ok();
        ControlFlow::Continue(())
    }

    #[instrument(skip_all)]
    fn log_message(&mut self, params: LogMessageParams) -> Self::NotifyResult {
        self.tx.try_send(LspEvent::LogMessage(params)).ok();
        ControlFlow::Continue(())
    }

    #[instrument(skip_all)]
    fn publish_diagnostics(&mut self, params: PublishDiagnosticsParams) -> Self::NotifyResult {
        self.tx.try_send(LspEvent::Diagnostics(params)).ok();
        ControlFlow::Continue(())
    }
}

const fn text_document_sync_caps() -> TextDocumentSyncClientCapabilities {
    TextDocumentSyncClientCapabilities {
        dynamic_registration: Some(false),
        will_save: Some(false),
        will_save_wait_until: Some(false),
        did_save: Some(true),
    }
}

fn semantic_tokens_caps() -> SemanticTokensClientCapabilities {
    use lsp_types::TokenFormat;
    SemanticTokensClientCapabilities {
        dynamic_registration: Some(false),
        requests: SemanticTokensClientCapabilitiesRequests {
            full: Some(SemanticTokensFullOptions::Bool(true)),
            ..SemanticTokensClientCapabilitiesRequests::default()
        },
        token_types: vec![
            "namespace".into(),
            "type".into(),
            "class".into(),
            "enum".into(),
            "interface".into(),
            "struct".into(),
            "typeParameter".into(),
            "parameter".into(),
            "variable".into(),
            "property".into(),
            "enumMember".into(),
            "event".into(),
            "function".into(),
            "method".into(),
            "macro".into(),
            "keyword".into(),
            "modifier".into(),
            "comment".into(),
            "string".into(),
            "number".into(),
            "regexp".into(),
            "operator".into(),
            "decorator".into(),
        ],
        token_modifiers: vec![
            "declaration".into(),
            "definition".into(),
            "readonly".into(),
            "static".into(),
            "deprecated".into(),
            "abstract".into(),
            "async".into(),
            "modification".into(),
            "documentation".into(),
            "defaultLibrary".into(),
        ],
        formats: vec![TokenFormat::RELATIVE],
        multiline_token_support: Some(false),
        overlapping_token_support: Some(false),
        ..SemanticTokensClientCapabilities::default()
    }
}

fn text_document_caps() -> TextDocumentClientCapabilities {
    use lsp_types::MarkupKind;
    TextDocumentClientCapabilities {
        synchronization: Some(text_document_sync_caps()),
        publish_diagnostics: Some(PublishDiagnosticsClientCapabilities {
            related_information: Some(true),
            ..PublishDiagnosticsClientCapabilities::default()
        }),
        semantic_tokens: Some(semantic_tokens_caps()),
        completion: Some(CompletionClientCapabilities {
            completion_item: Some(CompletionItemCapability {
                snippet_support: Some(false),
                documentation_format: Some(vec![MarkupKind::PlainText]),
                ..CompletionItemCapability::default()
            }),
            context_support: Some(false),
            ..CompletionClientCapabilities::default()
        }),
        hover: Some(HoverClientCapabilities {
            dynamic_registration: Some(false),
            content_format: Some(vec![MarkupKind::Markdown, MarkupKind::PlainText]),
        }),
        definition: Some(GotoCapability {
            dynamic_registration: Some(false),
            link_support: Some(false),
        }),
        code_action: Some(CodeActionClientCapabilities {
            dynamic_registration: Some(false),
            resolve_support: Some(lsp_types::CodeActionCapabilityResolveSupport {
                properties: vec!["edit".to_string()],
            }),
            code_action_literal_support: Some(CodeActionLiteralSupport {
                code_action_kind: CodeActionKindLiteralSupport { value_set: vec![] },
            }),
            data_support: Some(true),
            ..CodeActionClientCapabilities::default()
        }),
        document_symbol: Some(DocumentSymbolClientCapabilities {
            dynamic_registration: Some(false),
            hierarchical_document_symbol_support: Some(true),
            ..DocumentSymbolClientCapabilities::default()
        }),
        formatting: Some(DocumentFormattingClientCapabilities {
            dynamic_registration: Some(false),
        }),
        ..TextDocumentClientCapabilities::default()
    }
}

fn full_client_capabilities() -> ClientCapabilities {
    ClientCapabilities {
        text_document: Some(text_document_caps()),
        experimental: Some(serde_json::json!({ "plainGoal": true })),
        ..ClientCapabilities::default()
    }
}

/// Outbound half of the Lean LSP client.
/// A running `lean --server` process with an established LSP connection.
/// Send requests and notifications to the server through this.
pub struct LeanClient {
    socket: ServerSocket,
    pub init_result: InitializeResult,
    child: Child,
    exit_expected: Arc<AtomicBool>,
}

impl LeanClient {
    /// Spawn `lean --server`, run `initialize` + `initialized`, return a
    /// connected `LeanClient`.
    ///
    /// * `lean_bin` — path to the `lean` executable.
    /// * `cwd` — working directory for the child; should be the project root.
    /// * `root_uri` — `file://` URL of the Lean project root.
    /// * `event_tx` — channel for forwarding inbound server notifications.
    ///
    /// # Errors
    ///
    /// Returns `StartError` if spawning the child process fails, if the child
    /// does not expose stdio pipes, or if the LSP `initialize` / `initialized`
    /// handshake fails.
    #[instrument(skip_all)]
    pub async fn start(
        lean_bin: &Path,
        cwd: &Path,
        root_uri: Url,
        event_tx: mpsc::Sender<LspEvent>,
    ) -> Result<Self, StartError> {
        let mut child = Command::new(lean_bin)
            .arg("--server")
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .map_err(StartError::Spawn)?;

        let stdin = child
            .stdin
            .take()
            .ok_or(StartError::MissingStdio)?
            .compat_write();
        let stdout = child
            .stdout
            .take()
            .ok_or(StartError::MissingStdio)?
            .compat();

        let exit_expected = Arc::new(AtomicBool::new(false));

        let (main_loop, mut socket) = MainLoop::new_client(|_server_socket| {
            ServiceBuilder::new()
                .layer(TracingLayer::default())
                .layer(CatchUnwindLayer::default())
                .layer(ConcurrencyLayer::default())
                .service({
                    let tx = event_tx.clone();
                    let mut router = Router::from_language_client(LeanListener { tx });
                    router.notification::<LeanFileProgress>(move |_, params| {
                        event_tx.try_send(LspEvent::FileProgress(params)).ok();
                        ControlFlow::Continue(())
                    });
                    router
                })
        });

        let expected = Arc::clone(&exit_expected);
        tokio::spawn(
            async move {
                match main_loop.run_buffered(stdout, stdin).await {
                    Ok(()) => debug!("lean --server exited"),
                    Err(_) if expected.load(Ordering::Acquire) => {
                        debug!("lean --server exited");
                    }
                    Err(err) => error!(%err, "lean --server exited with error"),
                }
            }
            .instrument(tracing::Span::current()),
        );
        debug!(pid = child.id().unwrap_or(0), "lean --server started");

        let init_params = InitializeParams {
            process_id: Some(std::process::id()),
            workspace_folders: Some(vec![WorkspaceFolder {
                uri: root_uri.clone(),
                name: "turnstile".to_string(),
            }]),
            capabilities: full_client_capabilities(),
            ..InitializeParams::default()
        };

        let init_result = socket
            .initialize(init_params)
            .await
            .map_err(StartError::Initialize)?;

        socket
            .initialized(InitializedParams {})
            .map_err(StartError::Initialized)?;
        debug!(server_info = %DisplayServerInfo(init_result.server_info.as_ref()), "Language server initialized");

        Ok(Self {
            socket,
            init_result,
            child,
            exit_expected,
        })
    }

    /// Extract the semantic token type and modifier legends from the server's
    /// `InitializeResult`. Returns `(types, modifiers)`.
    #[must_use]
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

    /// Perform the proper LSP shutdown sequence: `shutdown` request → `exit`
    /// notification → wait for the process to exit.
    ///
    /// # Errors
    ///
    /// Returns `StopError` if the `shutdown` request, `exit` notification, or
    /// the wait for the child process to exit fails.
    #[instrument(skip_all)]
    pub async fn stop(mut self) -> Result<(), StopError> {
        self.shutdown().await.map_err(StopError::Shutdown)?;
        self.exit().map_err(StopError::Exit)?;
        self.child.wait().await?;
        Ok(())
    }

    /// # Errors
    ///
    /// Returns an error if the notification could not be queued to the server.
    #[instrument(skip_all)]
    pub fn did_open(
        &self,
        params: <DidOpenTextDocument as Notification>::Params,
    ) -> async_lsp::Result<()> {
        self.notify::<DidOpenTextDocument>(params)
    }

    /// # Errors
    ///
    /// Returns an error if the notification could not be queued to the server.
    #[instrument(skip_all)]
    pub fn did_close(
        &self,
        params: <DidCloseTextDocument as Notification>::Params,
    ) -> async_lsp::Result<()> {
        self.notify::<DidCloseTextDocument>(params)
    }

    /// # Errors
    ///
    /// Returns an error if the notification could not be queued to the server.
    #[instrument(skip_all)]
    pub fn did_change(
        &self,
        params: <DidChangeTextDocument as Notification>::Params,
    ) -> async_lsp::Result<()> {
        self.notify::<DidChangeTextDocument>(params)
    }

    /// # Errors
    ///
    /// Returns an error if the notification could not be queued to the server.
    #[instrument(skip_all)]
    pub fn did_save(
        &self,
        params: <DidSaveTextDocument as Notification>::Params,
    ) -> async_lsp::Result<()> {
        self.notify::<DidSaveTextDocument>(params)
    }

    #[instrument(skip_all)]
    pub fn hover(
        &self,
        params: HoverParams,
    ) -> impl Future<Output = async_lsp::Result<Option<Hover>>> + use<'_> {
        self.request::<HoverRequest>(params)
    }

    #[instrument(skip_all)]
    pub fn completion(
        &self,
        params: CompletionParams,
    ) -> impl Future<Output = async_lsp::Result<Option<CompletionResponse>>> + use<'_> {
        self.request::<Completion>(params)
    }

    #[instrument(skip_all)]
    pub fn definition(
        &self,
        params: GotoDefinitionParams,
    ) -> impl Future<Output = async_lsp::Result<Option<GotoDefinitionResponse>>> + use<'_> {
        self.request::<GotoDefinition>(params)
    }

    #[instrument(skip_all)]
    pub fn code_action(
        &self,
        params: CodeActionParams,
    ) -> impl Future<Output = async_lsp::Result<Option<CodeActionResponse>>> + use<'_> {
        self.request::<CodeActionRequest>(params)
    }

    #[instrument(skip_all)]
    pub fn resolve_code_action(
        &self,
        params: CodeAction,
    ) -> impl Future<Output = async_lsp::Result<CodeAction>> + use<'_> {
        self.request::<CodeActionResolveRequest>(params)
    }

    #[instrument(skip_all)]
    pub fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> impl Future<Output = async_lsp::Result<Option<DocumentSymbolResponse>>> + use<'_> {
        self.request::<DocumentSymbolRequest>(params)
    }

    #[instrument(skip_all)]
    pub fn semantic_tokens_full(
        &self,
        params: SemanticTokensParams,
    ) -> impl Future<Output = async_lsp::Result<Option<SemanticTokensResult>>> + use<'_> {
        self.request::<SemanticTokensFullRequest>(params)
    }

    #[instrument(skip_all)]
    pub fn plain_goal(
        &self,
        params: TextDocumentPositionParams,
    ) -> impl Future<Output = async_lsp::Result<Option<PlainGoal>>> + use<'_> {
        self.request::<LeanPlainGoal>(params)
    }

    #[instrument(skip_all)]
    pub fn plain_term_goal(
        &self,
        params: TextDocumentPositionParams,
    ) -> impl Future<Output = async_lsp::Result<Option<PlainTermGoal>>> + use<'_> {
        self.request::<LeanPlainTermGoal>(params)
    }

    #[instrument(skip_all)]
    pub fn formatting(
        &self,
        params: DocumentFormattingParams,
    ) -> impl Future<Output = async_lsp::Result<Option<Vec<TextEdit>>>> + use<'_> {
        self.request::<Formatting>(params)
    }

    #[instrument(skip_all)]
    pub fn shutdown(&self) -> impl Future<Output = async_lsp::Result<()>> + use<'_> {
        self.request::<Shutdown>(())
    }

    /// # Errors
    ///
    /// Returns an error if the `exit` notification could not be queued.
    #[instrument(skip_all)]
    pub fn exit(&self) -> async_lsp::Result<()> {
        self.exit_expected.store(true, Ordering::Release);
        self.notify::<Exit>(())
    }

    /// Send an LSP request and await the response.
    ///
    /// # Errors
    ///
    /// Returns an error if the request could not be sent or the server returned
    /// an error response.
    #[instrument(skip_all, fields(method = R::METHOD))]
    pub async fn request<R>(&self, params: R::Params) -> async_lsp::Result<R::Result>
    where
        R: Request,
        R::Params: Send,
    {
        self.socket.request::<R>(params).await
    }

    /// Send an LSP notification (fire-and-forget; queued asynchronously).
    ///
    /// # Errors
    ///
    /// Returns an error if the notification could not be queued to the server.
    #[instrument(skip_all, fields(method = N::METHOD))]
    pub fn notify<N>(&self, params: N::Params) -> async_lsp::Result<()>
    where
        N: Notification,
    {
        self.socket.notify::<N>(params)
    }
}

impl fmt::Display for LeanClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            DisplayServerInfo(self.init_result.server_info.as_ref())
        )
    }
}

impl Drop for LeanClient {
    fn drop(&mut self) {
        // kill_on_drop(true) handles the actual kill; this log makes it visible
        // when shutdown() wasn't called (e.g. a test panicked).
        if self.child.id().is_some() {
            warn!("LeanClient dropped without calling shutdown — killing child process");
        }
    }
}

struct DisplayServerInfo<'a>(Option<&'a lsp_types::ServerInfo>);

impl fmt::Display for DisplayServerInfo<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            Some(info) => match &info.version {
                Some(version) => write!(f, "{} {}", info.name, version),
                None => write!(f, "{}", info.name),
            },
            None => write!(f, "<unknown server>"),
        }
    }
}

/// Errors that can occur while spawning Lean and running the initialization handshake.
#[derive(Debug, Error)]
pub enum StartError {
    #[error("failed to spawn lean process: {0}")]
    Spawn(#[source] std::io::Error),

    #[error("lean child process did not expose stdin/stdout")]
    MissingStdio,

    #[error("initialize request failed: {0}")]
    Initialize(#[source] async_lsp::Error),

    #[error("initialized notification failed: {0}")]
    Initialized(#[source] async_lsp::Error),
}

#[derive(Debug, Error)]
pub enum StopError {
    #[error("Shutdown failed: {0}")]
    Shutdown(#[source] async_lsp::Error),

    #[error("Exit failed: {0}")]
    Exit(#[source] async_lsp::Error),

    #[error("Wait for exit failed: {0}")]
    Wait(#[from] std::io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn init_tracing() {
        let _ = tracing_subscriber::fmt()
            .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
            .with_test_writer()
            .try_init();
    }

    #[tokio::test]
    async fn spawns_and_initializes() {
        init_tracing();

        let lean = lean_bin();
        if !lean.exists() {
            eprintln!("skipping: lean not found at {}", lean.display());
            return;
        }

        let project = make_project();
        let root_uri = Url::from_directory_path(project.path()).expect("file URL");
        let (tx, _rx) = mpsc::channel(32);

        let client = LeanClient::start(&lean, project.path(), root_uri, tx)
            .await
            .expect("start failed");

        assert!(
            client.init_result.capabilities.text_document_sync.is_some(),
            "expected text_document_sync capability"
        );

        client.stop().await.expect("Client stop failed");
    }

    #[tokio::test]
    async fn opens_and_closes_conjunction() {
        use async_lsp::lsp_types::{
            DidCloseTextDocumentParams, DidOpenTextDocumentParams, TextDocumentIdentifier,
            TextDocumentItem,
        };

        init_tracing();

        let lean = lean_bin();
        if !lean.exists() {
            eprintln!("skipping: lean not found at {}", lean.display());
            return;
        }

        let project = make_project();
        let root_uri = Url::from_directory_path(project.path()).expect("root URL");

        let file_path = copy_fixture(project.path(), "01_conjunction.lean");
        let file_uri = Url::from_file_path(&file_path).expect("file URL");
        let text = std::fs::read_to_string(&file_path).expect("read fixture");

        let (tx, _rx) = mpsc::channel(32);
        let client = LeanClient::start(&lean, project.path(), root_uri, tx)
            .await
            .expect("start failed");

        client
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: file_uri.clone(),
                    language_id: "lean".to_string(),
                    version: 1,
                    text,
                },
            })
            .expect("didOpen notification");

        client
            .did_close(DidCloseTextDocumentParams {
                text_document: TextDocumentIdentifier { uri: file_uri },
            })
            .expect("didClose notification");

        client.stop().await.expect("Client stop failed");
    }

    /// Absolute path to the fixtures directory (resolved at compile time).
    const FIXTURES_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");

    /// Copy a fixture into `project_dir` and return the destination path.
    fn copy_fixture(project_dir: &Path, fixture: &str) -> PathBuf {
        let src = PathBuf::from(FIXTURES_DIR).join(fixture);
        let dst = project_dir.join(fixture);
        std::fs::copy(&src, &dst)
            .unwrap_or_else(|e| panic!("copy {} -> {}: {}", src.display(), dst.display(), e));
        dst
    }

    /// Locate a `lean` binary: prefer `TURNSTILE_LSP_CMD`, fall back to `~/.elan/bin/lean`.
    fn lean_bin() -> PathBuf {
        if let Ok(p) = std::env::var("TURNSTILE_LSP_CMD") {
            return PathBuf::from(p);
        }
        let home = std::env::var("HOME").expect("HOME not set");
        let mut p = PathBuf::from(home);
        p.push(".elan/bin/lean");
        if cfg!(windows) {
            p.set_extension("exe");
        }
        p
    }

    /// Create a throwaway Lean project directory with a pinned toolchain.
    fn make_project() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            dir.path().join("lean-toolchain"),
            "leanprover/lean4:v4.29.0\n",
        )
        .expect("write lean-toolchain");
        dir
    }
}
