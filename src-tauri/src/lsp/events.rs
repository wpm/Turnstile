//! Typed LSP server-push event dispatch.
//!
//! [`LspEvent`] is the typed channel message that [`super::client::LeanListener`]
//! sends for every inbound server notification. [`forward_lsp_events`] receives
//! those events and translates them into Tauri events and `AppState` mutations,
//! keeping `client.rs` free of any Tauri dependency.

use std::sync::atomic::Ordering;

use lsp_types::{LogMessageParams, MessageType, PublishDiagnosticsParams, ShowMessageParams};
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::mpsc;

use crate::lsp::client::FileProgressParams;
use crate::lsp::parse::{
    parse_diagnostics, parse_file_progress, DiagnosticInfo, FileProgressRange,
};
use crate::{proof, AppState};

/// A server-push notification forwarded from [`super::client::LeanListener`].
pub enum LspEvent {
    Diagnostics(PublishDiagnosticsParams),
    FileProgress(FileProgressParams),
    LogMessage(LogMessageParams),
    ShowMessage(ShowMessageParams),
}

#[derive(Clone, serde::Serialize)]
struct LspShowMessage {
    severity: &'static str,
    message: String,
}

const fn message_type_severity(typ: MessageType) -> &'static str {
    match typ {
        MessageType::ERROR => "error",
        MessageType::WARNING => "warning",
        MessageType::INFO => "info",
        _ => "log",
    }
}

fn lsp_server_log(typ: MessageType, message: &str) {
    match typ {
        MessageType::ERROR => tracing::error!("LSP server: {message}"),
        MessageType::WARNING => tracing::warn!("LSP server: {message}"),
        _ => tracing::debug!("LSP server: {message}"),
    }
}

/// Read [`LspEvent`]s from `rx` and translate them into Tauri events / state mutations.
///
/// Runs until the channel closes (i.e. the `LeanClient` is dropped).
/// Spawn this with `tauri::async_runtime::spawn` after creating the channel.
pub async fn forward_lsp_events(mut rx: mpsc::Receiver<LspEvent>, app: AppHandle) {
    while let Some(event) = rx.recv().await {
        match event {
            LspEvent::Diagnostics(params) => {
                handle_diagnostics(&app, params);
            }
            LspEvent::FileProgress(params) => {
                handle_file_progress(&app, params);
            }
            LspEvent::LogMessage(p) => {
                lsp_server_log(p.typ, &p.message);
            }
            LspEvent::ShowMessage(p) => {
                lsp_server_log(p.typ, &p.message);
                app.emit(
                    "lsp-show-message",
                    LspShowMessage {
                        severity: message_type_severity(p.typ),
                        message: p.message,
                    },
                )
                .ok();
            }
        }
    }
    tracing::debug!("LSP event forwarder stopped");
}

fn handle_diagnostics(app: &AppHandle, params: PublishDiagnosticsParams) {
    let diagnostics: Vec<DiagnosticInfo> = parse_diagnostics(params);
    let state = app.state::<AppState>();
    (*state.current_diagnostics.lock().unwrap()).clone_from(&diagnostics);
    app.emit("lsp-diagnostics", &diagnostics).ok();
    let proof = state.proof.clone();
    let app_handle = app.clone();
    tauri::async_runtime::spawn(async move {
        let items = {
            let mut guard = proof.lock().await;
            guard.annotations.set_diagnostics(&diagnostics);
            guard.annotations.items.clone()
        };
        app_handle
            .emit(proof::ANNOTATIONS_UPDATED_EVENT, &items)
            .ok();
    });
}

fn handle_file_progress(app: &AppHandle, params: FileProgressParams) {
    let ranges: Vec<FileProgressRange> = parse_file_progress(params);
    let elaboration_done = ranges.is_empty();
    app.emit("lsp-file-progress", ranges).ok();
    if elaboration_done {
        let state = app.state::<AppState>();
        let seq = state.goal_state_seq.fetch_add(1, Ordering::SeqCst) + 1;
        crate::spawn_goal_state_refresh(app.clone(), seq);
        app.emit("lsp-elaboration-done", ()).ok();
    }
}
