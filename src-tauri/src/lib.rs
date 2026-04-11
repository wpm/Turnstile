#![warn(unused_qualifications)]
//! Tauri app glue: command handlers, app state, and LSP message dispatch.
//!
//! # Data flows
//!
//! **Edit → diagnostics:**
//! Frontend change → `update_document` → `textDocument/didChange` → LSP emits
//! `textDocument/publishDiagnostics` → `lsp-diagnostics` Tauri event → frontend.
//!
//! **Cursor → goal state:**
//! Cursor move → `get_goal_state` → `$/lean/plainGoal` (awaited) → response
//! `{ "rendered": "..." }` → frontend goal panel.

pub mod chat;
pub mod lsp;
mod setup;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::json;
use tauri::{AppHandle, Emitter, Manager};

use lsp::{CompletionItem, LspClient, LspStatus};

pub struct AppState {
    lsp_client: Arc<tokio::sync::Mutex<Option<LspClient>>>,
    /// Absolute path to the managed Lean project directory
    project_path: PathBuf,
    /// Document version counter (starts at 2; didOpen uses version 1)
    doc_version: AtomicI64,
    /// Whether setup is currently running (prevents double-start); shared with the setup task
    setup_running: Arc<AtomicBool>,
    /// Chat conversation state, shared with Tauri command handlers.
    pub chat_state: Arc<tokio::sync::Mutex<chat::ChatState>>,
    /// LLM backend (mock or real Anthropic).
    pub chat_backend: Arc<dyn chat::ChatBackend>,
}

impl AppState {
    fn doc_uri(&self) -> String {
        lsp::path_to_file_uri(&self.project_path.join("Proof.lean"))
    }

    fn root_uri(&self) -> String {
        lsp::path_to_file_uri(&self.project_path)
    }
}

#[derive(serde::Serialize)]
struct SetupStatusResponse {
    complete: bool,
    project_path: String,
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn get_setup_status(app: AppHandle) -> SetupStatusResponse {
    let state = app.state::<AppState>();
    SetupStatusResponse {
        complete: setup::check_setup_complete(&state.project_path),
        project_path: state.project_path.to_string_lossy().to_string(),
    }
}

#[tauri::command]
async fn start_setup(app: AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();

    if state.setup_running.swap(true, Ordering::SeqCst) {
        return Err("Setup is already running".to_string());
    }

    let project_path = state.project_path.clone();
    let setup_running = state.setup_running.clone();
    let app_clone = app.clone();

    tokio::spawn(async move {
        setup::run_setup(app_clone, project_path, setup_running).await;
    });

    Ok(())
}

#[tauri::command]
async fn start_lsp(app: AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();

    let lean_cmd = setup::lean_bin();
    let lean_str = lean_cmd.to_string_lossy().to_string();

    let args_str = std::env::var("TURNSTILE_LSP_ARGS").unwrap_or_else(|_| "--server".to_string());
    let args: Vec<String> = args_str.split_whitespace().map(String::from).collect();
    let args_refs: Vec<&str> = args.iter().map(String::as_str).collect();

    let mut client = LspClient::spawn(&lean_str, &args_refs, &state.project_path)?;

    app.emit(
        "lsp-status",
        LspStatus {
            state: String::new(),
            message: format!("initializing ({lean_str})..."),
        },
    )
    .ok();

    let stdout = client.take_stdout().ok_or("Failed to take LSP stdout")?;

    let token_types = client.token_types.clone();
    let pending = client.pending.clone();
    let writer = client.writer.clone();
    let app_handle = app.clone();

    std::thread::spawn(move || {
        LspClient::read_messages(stdout, &pending, |msg| {
            handle_lsp_message(&app_handle, &token_types, &writer, msg);
        });

        app_handle
            .emit(
                "lsp-status",
                LspStatus {
                    state: "error".to_string(),
                    message: "server disconnected".to_string(),
                },
            )
            .ok();
    });

    let root_uri = state.root_uri();
    let init_result = client
        .send_request_await("initialize", lsp::initialize_params(&root_uri))
        .await
        .map_err(|e| format!("LSP initialize failed: {e}"))?;

    handle_initialize_response(&app, &client.token_types, &init_result);

    {
        let mut lock = state.lsp_client.lock().await;
        *lock = Some(client);
        if let Some(client) = lock.as_ref() {
            client.send_notification("initialized", json!({})).await?;
            let doc_uri = state.doc_uri();
            client
                .send_notification(
                    "textDocument/didOpen",
                    json!({
                        "textDocument": {
                            "uri": doc_uri,
                            "languageId": "lean4",
                            "version": 1,
                            "text": "",
                        }
                    }),
                )
                .await?;
        }
    }

    Ok(())
}

#[tauri::command]
async fn update_document(app: AppHandle, content: String) -> Result<(), String> {
    let state = app.state::<AppState>();
    let doc_uri = state.doc_uri();
    let version = state.doc_version.fetch_add(1, Ordering::SeqCst);

    // Clone the client Arc so we can release the lock before the semantic token request.
    let client_arc = {
        let lock = state.lsp_client.lock().await;
        lock.as_ref().map(|_| state.lsp_client.clone())
    };

    let Some(client_arc) = client_arc else {
        return Ok(());
    };

    {
        let lock = client_arc.lock().await;
        if let Some(client) = lock.as_ref() {
            client
                .send_notification(
                    "textDocument/didChange",
                    json!({
                        "textDocument": {
                            "uri": doc_uri,
                            "version": version,
                        },
                        "contentChanges": [{ "text": content }],
                    }),
                )
                .await?;
        }
    }

    // Request semantic tokens separately, outside the didChange lock scope,
    // so concurrent get_goal_state calls are not blocked.
    {
        let lock = client_arc.lock().await;
        if let Some(client) = lock.as_ref() {
            client
                .send_request(
                    "textDocument/semanticTokens/full",
                    json!({ "textDocument": { "uri": doc_uri } }),
                )
                .await?;
        }
    }

    Ok(())
}

#[tauri::command]
#[allow(clippy::significant_drop_tightening)] // lock must be held while awaiting on client
async fn get_goal_state(app: AppHandle, line: u32, col: u32) -> Result<String, String> {
    let state = app.state::<AppState>();
    let lock = state.lsp_client.lock().await;

    let Some(client) = lock.as_ref() else {
        return Err("LSP not connected".to_string());
    };

    let doc_uri = state.doc_uri();
    let result = client
        .send_request_await(
            "$/lean/plainGoal",
            json!({
                "textDocument": { "uri": doc_uri },
                "position": { "line": line, "character": col },
            }),
        )
        .await?;

    let rendered = result
        .get("rendered")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    Ok(rendered)
}

#[tauri::command]
#[allow(clippy::significant_drop_tightening)] // lock must be held while awaiting on client
async fn get_completions(
    app: AppHandle,
    line: u32,
    col: u32,
) -> Result<Vec<CompletionItem>, String> {
    let state = app.state::<AppState>();
    let lock = state.lsp_client.lock().await;

    let Some(client) = lock.as_ref() else {
        return Ok(Vec::new());
    };

    let doc_uri = state.doc_uri();
    let result = client
        .send_request_await(
            "textDocument/completion",
            json!({
                "textDocument": { "uri": doc_uri },
                "position": { "line": line, "character": col },
            }),
        )
        .await?;

    Ok(lsp::parse_completion_items(&result))
}

fn handle_lsp_message(
    app: &AppHandle,
    token_types: &Arc<Mutex<Vec<String>>>,
    writer: &Arc<tokio::sync::Mutex<Box<dyn std::io::Write + Send>>>,
    msg: &serde_json::Value,
) {
    if let Some(result) = msg.get("result") {
        if result.get("capabilities").is_some() {
            handle_initialize_response(app, token_types, result);
        } else if let Some(data) = result.get("data") {
            handle_semantic_tokens_response(app, token_types, data);
        }
        return;
    }

    // Server→client requests have both "method" and "id"; ack them with null.
    if let Some(id) = msg.get("id") {
        if msg.get("method").is_some() {
            if let Err(e) = lsp::ack_request(writer, id) {
                log::warn!("Failed to ack LSP request: {e}");
            }
            return;
        }
    }

    if let Some(method) = msg.get("method").and_then(|m| m.as_str()) {
        let params = msg.get("params").cloned().unwrap_or_else(|| json!({}));
        match method {
            "textDocument/publishDiagnostics" => {
                app.emit("lsp-diagnostics", lsp::parse_diagnostics(&params))
                    .ok();
            }
            "window/logMessage" | "window/showMessage" => {
                if let Some(message) = params.get("message").and_then(|m| m.as_str()) {
                    log::info!("LSP: {message}");
                }
            }
            _ => {
                log::debug!("Unhandled LSP notification: {method}");
            }
        }
    }
}

fn handle_initialize_response(
    app: &AppHandle,
    token_types: &Arc<Mutex<Vec<String>>>,
    result: &serde_json::Value,
) {
    let legend = lsp::parse_token_legend(result);
    if let Ok(mut types) = token_types.lock() {
        *types = legend;
    }

    app.emit(
        "lsp-status",
        LspStatus {
            state: "connected".to_string(),
            message: "connected".to_string(),
        },
    )
    .ok();

    log::info!("LSP initialize complete");
}

fn handle_semantic_tokens_response(
    app: &AppHandle,
    token_types: &Arc<Mutex<Vec<String>>>,
    data: &serde_json::Value,
) {
    let Some(arr) = data.as_array() else { return };
    let data_u32: Vec<u32> = arr
        .iter()
        .filter_map(|v| v.as_u64().and_then(|n| u32::try_from(n).ok()))
        .collect();

    let guard = token_types
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let tokens = lsp::decode_semantic_tokens(&data_u32, &guard);
    drop(guard);
    app.emit("lsp-semantic-tokens", tokens).ok();
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
/// # Panics
///
/// Panics if the Tauri application fails to build or run.
pub fn run() {
    env_logger::init();

    tauri::Builder::default()
        .setup(|app| {
            let project_path = app
                .path()
                .app_data_dir()
                .expect("Failed to resolve app data directory")
                .join("lean-project");

            #[cfg(feature = "mock-llm")]
            let chat_backend: Arc<dyn chat::ChatBackend> = Arc::new(chat::MockBackend::from_env());

            #[cfg(not(feature = "mock-llm"))]
            let chat_backend: Arc<dyn chat::ChatBackend> = {
                match chat::AnthropicBackend::from_env() {
                    Ok(b) => Arc::new(b),
                    Err(_) => Arc::new(chat::MockBackend::echo()),
                }
            };

            app.manage(AppState {
                lsp_client: Arc::new(tokio::sync::Mutex::new(None)),
                project_path,
                doc_version: AtomicI64::new(2),
                setup_running: Arc::new(AtomicBool::new(false)),
                chat_state: Arc::new(tokio::sync::Mutex::new(chat::ChatState::default())),
                chat_backend,
            });

            Ok(())
        })
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            // A second instance was launched; focus the existing window instead.
            if let Some(window) = app.get_webview_window("main") {
                window.set_focus().ok();
            }
        }))
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            get_setup_status,
            start_setup,
            start_lsp,
            update_document,
            get_goal_state,
            get_completions,
            chat::send_chat_message,
            chat::get_chat_state,
            chat::load_chat_state,
        ])
        .run(tauri::generate_context!())
        .expect("error while running turnstile");
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicI64, Ordering};

    #[test]
    fn doc_version_strictly_increasing_after_did_open() {
        let doc_version = AtomicI64::new(2);
        let did_open_version: i64 = 1;

        let v1 = doc_version.fetch_add(1, Ordering::SeqCst);
        let v2 = doc_version.fetch_add(1, Ordering::SeqCst);
        let v3 = doc_version.fetch_add(1, Ordering::SeqCst);

        assert!(v1 > did_open_version);
        assert!(v2 > v1);
        assert!(v3 > v2);
    }
}
