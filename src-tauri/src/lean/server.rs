//! Spawns `lean --server` and wires up the async-lsp main loop.

#![allow(dead_code)]

use crate::lean::messages::lean::LeanMessage;
use crate::lean::messages::DisplayLspParams;
use crate::lean::protocol::Protocol;
use crate::setup;
use async_lsp::concurrency::ConcurrencyLayer;
use async_lsp::lsp_types::notification::{
    LogMessage, Notification as LspNotification, PublishDiagnostics, ShowMessage,
};
use async_lsp::panic::CatchUnwindLayer;
use async_lsp::router::Router;
use async_lsp::tracing::TracingLayer;
use async_lsp::Error::ServiceStopped;
use async_lsp::{LanguageClient, MainLoop, ResponseError, Result, ServerSocket};
use lsp_types::{
    LogMessageParams, PublishDiagnosticsParams, Range, ShowMessageParams, TextDocumentIdentifier,
};
use serde::{Deserialize, Serialize};
use std::ops::ControlFlow;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::process::{Child, Command};
use tokio::sync::mpsc;
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};
use tower::ServiceBuilder;
use url::Url;

/// One processing interval in a `$/lean/fileProgress` notification.
#[derive(Debug, Serialize, Deserialize)]
pub struct FileProgressInterval {
    pub range: Range,
}

/// `$/lean/fileProgress` notification params.
#[derive(Debug, Serialize, Deserialize)]
pub struct FileProgressParams {
    #[serde(rename = "textDocument")]
    pub text_document: TextDocumentIdentifier,
    pub processing: Vec<FileProgressInterval>,
    /// Version from `textDocument.version` in Lean's notification payload.
    #[serde(default, rename = "version", skip_serializing)]
    pub version: Option<i32>,
}

enum FileProgress {}
impl LspNotification for FileProgress {
    type Params = FileProgressParams;
    const METHOD: &'static str = "$/lean/fileProgress";
}

/// Owns the `lean --server` child process and the temporary project directory
/// it runs in. Both are released together when this struct is dropped.
pub(super) struct Server {
    pub(super) process: Child,
    pub(super) project_dir: tempfile::TempDir,
}

pub(super) struct ServerStarted {
    pub(super) process: Server,
    pub(super) socket: ServerSocket,
    pub(super) root_uri: Url,
    pub(super) proof_uri: Url,
    pub(super) server_rx: mpsc::Receiver<LeanMessage>,
    pub(super) exit_expected: Arc<AtomicBool>,
}

struct ServerListener {
    server_tx: mpsc::Sender<LeanMessage>,
    protocol: Arc<Protocol>,
}

impl ServerListener {
    fn notify<N: LspNotification>(
        &self,
        params: N::Params,
        to_msg: impl FnOnce(N::Params) -> LeanMessage,
    ) -> ControlFlow<Result<()>>
    where
        N::Params: DisplayLspParams,
    {
        tracing::debug!("LSP→Client: {}\n{}", N::METHOD, params.display());
        let msg = to_msg(params);
        if self.protocol.keep_notification(&msg) {
            self.server_tx.try_send(msg).ok();
        } else {
            tracing::trace!("dropped stale {}", N::METHOD);
        }
        ControlFlow::Continue(())
    }
}

impl LanguageClient for ServerListener {
    type Error = ResponseError;
    type NotifyResult = ControlFlow<Result<()>>;

    fn semantic_tokens_refresh(
        &mut self,
        _params: <lsp_types::request::SemanticTokensRefresh as lsp_types::request::Request>::Params,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), ResponseError>> + Send>>
    {
        self.server_tx.try_send(LeanMessage::TokenRefresh).ok();
        Box::pin(std::future::ready(Ok(())))
    }

    fn show_message(&mut self, params: ShowMessageParams) -> Self::NotifyResult {
        self.notify::<ShowMessage>(params, LeanMessage::ShowMessage)
    }

    fn log_message(&mut self, params: LogMessageParams) -> Self::NotifyResult {
        self.notify::<LogMessage>(params, LeanMessage::LogMessage)
    }

    fn publish_diagnostics(&mut self, params: PublishDiagnosticsParams) -> Self::NotifyResult {
        self.notify::<PublishDiagnostics>(params, LeanMessage::Diagnostics)
    }
}

impl Server {
    pub(super) async fn start(
        initial_content: &str,
        protocol: Arc<Protocol>,
    ) -> Result<ServerStarted> {
        let project_dir = make_project_dir(initial_content).await?;
        let canonical = tokio::fs::canonicalize(project_dir.path())
            .await
            .map_err(|_| ServiceStopped)?;
        let root_uri = Url::from_file_path(&canonical).map_err(|()| ServiceStopped)?;
        let proof_uri =
            Url::from_file_path(canonical.join("Proof.lean")).map_err(|()| ServiceStopped)?;

        let mut process = Command::new(setup::lean_bin())
            .arg("--server")
            .current_dir(&canonical)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .map_err(|_| ServiceStopped)?;
        let stdin = process.stdin.take().ok_or(ServiceStopped)?.compat_write();
        let stdout = process.stdout.take().ok_or(ServiceStopped)?.compat();

        let (server_tx, server_rx) = mpsc::channel(64);
        let (main_loop, socket) = MainLoop::new_client(|_server| {
            let protocol_for_progress = Arc::clone(&protocol);
            let progress_tx = server_tx.clone();
            let mut router = Router::from_language_client(ServerListener {
                server_tx,
                protocol,
            });
            router.notification::<FileProgress>(move |_, params| {
                let msg = LeanMessage::FileProgress(params);
                if protocol_for_progress.keep_notification(&msg) {
                    progress_tx.try_send(msg).ok();
                } else {
                    tracing::trace!("dropped stale fileProgress");
                }
                ControlFlow::Continue(())
            });
            ServiceBuilder::new()
                .layer(TracingLayer::default())
                .layer(CatchUnwindLayer::default())
                .layer(ConcurrencyLayer::default())
                .service(router)
        });

        let exit_expected = Arc::new(AtomicBool::new(false));
        let exit_expected_clone = Arc::clone(&exit_expected);
        tokio::spawn(async move {
            match main_loop.run_buffered(stdout, stdin).await {
                Ok(()) => tracing::debug!("lean --server exited"),
                Err(err) if exit_expected_clone.load(Ordering::Acquire) => {
                    tracing::debug!("lean --server exited after shutdown: {err}");
                }
                Err(err) => {
                    tracing::error!("lean --server exited unexpectedly: {err}");
                }
            }
        });

        Ok(ServerStarted {
            process: Self {
                process,
                project_dir,
            },
            socket,
            root_uri,
            proof_uri,
            server_rx,
            exit_expected,
        })
    }
}

async fn make_project_dir(initial_content: &str) -> Result<tempfile::TempDir> {
    let dir = tempfile::tempdir().map_err(|_| ServiceStopped)?;
    let src = setup::project_dir();

    tokio::fs::symlink(
        src.join("lean-toolchain"),
        dir.path().join("lean-toolchain"),
    )
    .await
    .map_err(|_| ServiceStopped)?;
    tokio::fs::symlink(src.join("lakefile.toml"), dir.path().join("lakefile.toml"))
        .await
        .map_err(|_| ServiceStopped)?;
    tokio::fs::symlink(
        src.join("lake-manifest.json"),
        dir.path().join("lake-manifest.json"),
    )
    .await
    .map_err(|_| ServiceStopped)?;
    tokio::fs::create_dir(dir.path().join(".lake"))
        .await
        .map_err(|_| ServiceStopped)?;
    tokio::fs::symlink(
        src.join(".lake/packages"),
        dir.path().join(".lake/packages"),
    )
    .await
    .map_err(|_| ServiceStopped)?;
    tokio::fs::write(dir.path().join("Proof.lean"), initial_content)
        .await
        .map_err(|_| ServiceStopped)?;

    Ok(dir)
}
