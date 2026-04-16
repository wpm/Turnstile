#![warn(unused_qualifications)]
//! Tauri app glue: command handlers, app state, and LSP message dispatch.
//!
//! # Data flows
//!
//! **Edit → diagnostics:**
//! Frontend change → `update_document` → `textDocument/didChange` → LSP emits
//! `textDocument/publishDiagnostics` → `lsp-diagnostics` Tauri event → frontend.
//!
//! **Elaboration complete → goal state:**
//! LSP emits `$/lean/fileProgress` with empty ranges → `spawn_goal_state_refresh`
//! debounces 150 ms, then issues `$/lean/plainGoal` at end-of-doc and at every
//! line-end → `goal-state-updated` Tauri event → frontend goal panel.

pub mod assistant;
pub mod llm;
pub mod lsp;
pub mod menu;
pub mod proof;
pub mod session;
pub mod settings;
mod setup;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::json;
use tauri::{AppHandle, Emitter, Manager};

use lsp::{
    CodeActionInfo, CompletionItem, DefinitionLocation, DocumentSymbolInfo, HoverInfo, LspClient,
    LspError, LspNotification, LspStatus, WorkspaceEditDto,
};

pub struct AppState {
    pub lsp_client: Arc<tokio::sync::Mutex<Option<LspClient>>>,
    /// Absolute path to the managed Lean project directory
    project_path: PathBuf,
    /// Document version counter (starts at 2; didOpen uses version 1)
    doc_version: AtomicI64,
    /// Whether setup is currently running (prevents double-start); shared with the setup task
    setup_running: Arc<AtomicBool>,
    /// The proof currently being developed — formal + prose + goal state.
    pub proof: Arc<tokio::sync::Mutex<proof::Proof>>,
    /// Assistant conversation state.
    pub transcript: Arc<tokio::sync::Mutex<assistant::Transcript>>,
    /// LLM backend (mock or real Anthropic).
    pub llm: Arc<dyn llm::Llm>,
    /// Persisted user settings.
    pub settings: Arc<tokio::sync::Mutex<settings::Settings>>,
    /// Path of the currently open `.turn` file (None if unsaved).
    pub current_session_path: Arc<tokio::sync::Mutex<Option<PathBuf>>>,
    /// Whether the session has unsaved changes.
    pub session_dirty: Arc<AtomicBool>,
    /// Latest LSP diagnostics for the Lean source file.
    pub current_diagnostics: Arc<Mutex<Vec<lsp::DiagnosticInfo>>>,
    /// Whether the formal proof has changed since the last prose generation.
    pub prose_dirty: Arc<AtomicBool>,
    /// Monotonically increasing sequence number for prose generation requests.
    /// Used to discard stale results when the source changes mid-generation.
    pub prose_generation_seq: Arc<AtomicU64>,
    /// Monotonically increasing sequence number for goal-state refresh
    /// requests. Bumped on every `update_document` and on every empty
    /// `$/lean/fileProgress` event, so that a stale background refresh task
    /// does not overwrite the panel with old data.
    pub goal_state_seq: Arc<AtomicU64>,
    /// Snapshot of the last goal-state payload — just the data
    /// [`lsp_hover_goal_panel`] needs to translate a (panel-line, column)
    /// hover into a Formal Proof document position.
    pub goal_state_cache: Arc<Mutex<GoalStateCache>>,
}

/// Index-parallel snapshot of the goal panel's code-block lines and their
/// mapped Formal Proof source lines (1-indexed, `None` if unmapped).
#[derive(Default)]
pub struct GoalStateCache {
    pub panel_lines: Vec<String>,
    pub panel_line_to_source_line: Vec<Option<u32>>,
}

impl AppState {
    pub(crate) fn doc_uri(&self) -> String {
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
    let token_modifiers = client.token_modifiers.clone();
    let pending = client.pending.clone();
    let writer = client.writer.clone();
    let next_id = client.next_id.clone();
    let app_handle = app.clone();

    std::thread::spawn(move || {
        LspClient::read_messages(stdout, &pending, |msg| {
            handle_lsp_message(
                &app_handle,
                &token_types,
                &token_modifiers,
                &writer,
                &next_id,
                msg,
            );
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

    handle_initialize_response(
        &app,
        &client.token_types,
        &client.token_modifiers,
        &init_result,
    );

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

    // Keep the formal proof source in sync so the LLM can read it via
    // read_lean_source.
    state.proof.lock().await.formal.source = content.clone();

    // Invalidate any in-flight goal-state refresh task spawned before this
    // edit. The subsequent empty-`fileProgress` event will spawn a fresh one.
    state.goal_state_seq.fetch_add(1, Ordering::SeqCst);

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
    // so concurrent `$/lean/plainGoal` calls from the goal-state refresh task
    // are not blocked.
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

    // Mark prose as dirty and kick off a debounced background regeneration.
    state.prose_dirty.store(true, Ordering::SeqCst);
    let seq = state.prose_generation_seq.fetch_add(1, Ordering::SeqCst) + 1;
    spawn_prose_regeneration(app.clone(), seq);

    Ok(())
}

/// Debounce interval: how long to wait after an edit for the source to settle
/// before attempting prose regeneration.
const PROSE_DEBOUNCE: std::time::Duration = std::time::Duration::from_secs(2);

/// Outcome of the pre-flight checks that decide whether to run the LLM.
#[derive(Debug)]
enum ShouldGenerate {
    /// Source is clean, non-empty, and has changed — run the backend.
    Proceed { source: String, hash: String },
    /// Abort this task entirely (stale seq, errors present, unchanged source,
    /// or empty source).
    Abort,
}

/// Decide whether to regenerate prose for sequence number `seq`.
///
/// Performs (in order): staleness check, diagnostics check, source-hash check,
/// and empty-source check. All four are short synchronous operations except
/// the proof clone which needs the async mutex on `proof`.
async fn should_generate_prose(state: &AppState, seq: u64) -> ShouldGenerate {
    // A newer edit has superseded us.
    if state.prose_generation_seq.load(Ordering::SeqCst) != seq {
        return ShouldGenerate::Abort;
    }

    // Don't translate a broken proof.
    let has_errors = {
        let diags = state.current_diagnostics.lock().unwrap();
        diags.iter().any(|d| d.severity == 1)
    };
    if has_errors {
        return ShouldGenerate::Abort;
    }

    let (source, last_hash) = {
        let guard = state.proof.lock().await;
        (guard.formal.source.clone(), guard.prose.source_hash.clone())
    };
    let hash = proof::compute_source_hash(&source);

    // Source hasn't changed since the last prose generation (e.g. type then undo).
    if last_hash == hash {
        return ShouldGenerate::Abort;
    }

    // Nothing to translate.
    if source.trim().is_empty() {
        return ShouldGenerate::Abort;
    }

    ShouldGenerate::Proceed { source, hash }
}

/// Spawn a background task that waits for the editing debounce period, then
/// regenerates the prose proof if the source has settled and compiles cleanly.
fn spawn_prose_regeneration(app: AppHandle, seq: u64) {
    tokio::spawn(async move {
        let state = app.state::<AppState>();

        loop {
            tokio::time::sleep(PROSE_DEBOUNCE).await;

            let (source, hash) = match should_generate_prose(&state, seq).await {
                ShouldGenerate::Abort => return,
                ShouldGenerate::Proceed { source, hash } => (source, hash),
            };

            // Clear dirty flag before starting generation so that any edit
            // arriving during the LLM call re-sets it.
            state.prose_dirty.store(false, Ordering::SeqCst);

            let backend = state.llm.clone();
            let result = proof::translator::run_translator(backend.as_ref(), &source, &app).await;

            // Discard the result if a newer edit superseded us during the
            // (potentially long) LLM call.
            if state.prose_generation_seq.load(Ordering::SeqCst) != seq {
                return;
            }

            if let Ok(prose_text) = result {
                {
                    let mut proof_guard = state.proof.lock().await;
                    proof_guard.prose.text = prose_text.clone();
                    proof_guard.prose.source_hash = hash.clone();
                }

                app.emit(
                    proof::PROSE_UPDATED_EVENT,
                    &proof::ProsePayload {
                        text: prose_text,
                        hash: Some(hash),
                    },
                )
                .ok();

                state.session_dirty.store(true, Ordering::SeqCst);
            }

            // If an edit arrived during generation, loop back to re-debounce
            // and retry; otherwise we're done.
            if !state.prose_dirty.load(Ordering::SeqCst) {
                return;
            }
        }
    });
}

/// Fetch the goal state at the end of the current document.
///
/// This is the "whole proof" goal state — what Lean reports after feeding it
/// the entire Formal Proof. Independent of cursor position.
#[allow(clippy::significant_drop_tightening)] // lock must be held while awaiting on client
async fn fetch_full_proof_goal_state(state: &AppState) -> Result<String, String> {
    let source = state.proof.lock().await.formal.source.clone();
    let (line, col) = end_of_document_position(&source);

    let lock = state.lsp_client.lock().await;
    let Some(client) = lock.as_ref() else {
        return Err(LspError::NotConnected.into());
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

/// Fetch the rendered goal state at the end of every line in the document.
///
/// Returns a `Vec<String>` with one entry per line (same length as the number
/// of lines in the source). Requests are issued sequentially to avoid
/// hammering the LSP.
///
/// `seq` is the sequence number this fetch was spawned for; the loop checks
/// `state.goal_state_seq` between every line and bails out with
/// `Err("stale")` if a newer edit has arrived, so stale per-line data never
/// reaches the UI on long proofs.
#[allow(clippy::significant_drop_tightening)]
async fn fetch_per_line_goal_states(state: &AppState, seq: u64) -> Result<Vec<String>, String> {
    let source = state.proof.lock().await.formal.source.clone();

    let lock = state.lsp_client.lock().await;
    let Some(client) = lock.as_ref() else {
        return Err(LspError::NotConnected.into());
    };
    let doc_uri = state.doc_uri();

    let mut results: Vec<String> = Vec::new();
    for (idx, line_text) in source.split('\n').enumerate() {
        if state.goal_state_seq.load(Ordering::SeqCst) != seq {
            return Err(LspError::Stale.into());
        }
        let line = u32::try_from(idx).map_err(|e| e.to_string())?;
        let col = u32::try_from(line_text.chars().count()).map_err(|e| e.to_string())?;
        let result = client
            .send_request_await(
                "$/lean/plainGoal",
                json!({
                    "textDocument": { "uri": doc_uri },
                    "position": { "line": line, "character": col },
                }),
            )
            .await
            .unwrap_or(serde_json::Value::Null);
        let rendered = result
            .get("rendered")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        results.push(rendered);
    }

    Ok(results)
}

#[derive(serde::Serialize, Clone)]
struct GoalStatePayload {
    full: String,
    panel_line_to_source_line: Vec<Option<u32>>,
}

/// Spawn a background task that, after a short debounce, fetches the
/// whole-proof and per-line goal states and emits a
/// [`proof::GOAL_STATE_UPDATED_EVENT`] to the frontend.
///
/// The task is sequence-guarded: if `state.goal_state_seq` advances before
/// the debounce fires (because another edit or another empty-progress event
/// arrived), this task returns without emitting. This coalesces the burst of
/// empty-`fileProgress` frames Lean emits as elaboration settles, and
/// discards stale results that would overwrite newer ones.
fn spawn_goal_state_refresh(app: AppHandle, seq: u64) {
    // Use Tauri's global async runtime rather than `tokio::spawn`: this
    // function is invoked from the plain `std::thread` that runs the LSP
    // reader loop, which has no Tokio runtime attached to thread-local
    // storage. `tauri::async_runtime::spawn` dispatches via a stored handle
    // and so works from any thread.
    tauri::async_runtime::spawn(async move {
        // Debounce: coalesce bursts of empty-progress frames.
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;

        let state = app.state::<AppState>();
        if state.goal_state_seq.load(Ordering::SeqCst) != seq {
            return; // A newer edit or progress event superseded us.
        }

        let full = match fetch_full_proof_goal_state(&state).await {
            Ok(s) => s,
            Err(e) => {
                log::debug!("goal-state refresh: full fetch failed: {e}");
                return;
            }
        };

        if state.goal_state_seq.load(Ordering::SeqCst) != seq {
            return;
        }

        let per_line = match fetch_per_line_goal_states(&state, seq).await {
            Ok(v) => v,
            Err(e) => {
                log::debug!("goal-state refresh: per-line fetch failed: {e}");
                return;
            }
        };

        let panel_line_to_source_line =
            proof::goal_panel_map::build_panel_line_to_source_line(&full, &per_line);
        let panel_lines = proof::goal_panel_map::flatten_code_block_lines(&full);

        if let Ok(mut cache) = state.goal_state_cache.lock() {
            cache.panel_lines = panel_lines;
            cache.panel_line_to_source_line = panel_line_to_source_line.clone();
        }

        app.emit(
            proof::GOAL_STATE_UPDATED_EVENT,
            &GoalStatePayload {
                full,
                panel_line_to_source_line,
            },
        )
        .ok();
    });
}

/// Compute the (line, character) position at the end of the document.
///
/// `line` is 0-indexed; `character` is the number of characters (UTF-16 code
/// units as counted by `chars().count()` — good enough for Lean's LSP which
/// accepts either as long as it's past the last character).
fn end_of_document_position(source: &str) -> (u32, u32) {
    let last_line_idx = source.split('\n').count().saturating_sub(1);
    let last_line = source.split('\n').next_back().unwrap_or("");
    let col = last_line.chars().count();
    (
        u32::try_from(last_line_idx).unwrap_or(u32::MAX),
        u32::try_from(col).unwrap_or(u32::MAX),
    )
}

/// Send an LSP request and return the raw JSON response.
///
/// Returns `Ok(None)` if the LSP client is not connected — callers should
/// treat this as "no data" and return their type's empty/None default. This
/// consolidates the lock-check-send pattern used by every LSP Tauri command.
#[allow(clippy::significant_drop_tightening)] // lock must be held while awaiting on client
async fn call_lsp_raw(
    state: &AppState,
    method: &str,
    params: serde_json::Value,
) -> Result<Option<serde_json::Value>, String> {
    let lock = state.lsp_client.lock().await;
    let Some(client) = lock.as_ref() else {
        return Ok(None);
    };
    let result = client.send_request_await(method, params).await?;
    Ok(Some(result))
}

/// Build the `{ "textDocument": { "uri": … }, "position": { … } }` params
/// shared by most `textDocument/*` requests.
fn text_document_position_params(doc_uri: &str, line: u32, character: u32) -> serde_json::Value {
    json!({
        "textDocument": { "uri": doc_uri },
        "position": { "line": line, "character": character },
    })
}

#[tauri::command]
async fn get_completions(
    app: AppHandle,
    line: u32,
    col: u32,
) -> Result<Vec<CompletionItem>, String> {
    let state = app.state::<AppState>();
    let params = text_document_position_params(&state.doc_uri(), line, col);
    Ok(call_lsp_raw(&state, "textDocument/completion", params)
        .await?
        .as_ref()
        .map(lsp::parse_completion_items)
        .unwrap_or_default())
}

#[tauri::command]
async fn lsp_hover(app: AppHandle, line: u32, character: u32) -> Result<Option<HoverInfo>, String> {
    let state = app.state::<AppState>();
    let params = text_document_position_params(&state.doc_uri(), line, character);
    Ok(call_lsp_raw(&state, "textDocument/hover", params)
        .await?
        .as_ref()
        .and_then(lsp::parse_hover))
}

/// Translate a goal-panel hover into a Formal Proof document position.
///
/// Given the panel-line text, the source line it maps to, and a UTF-16
/// column within the panel line, extract the word under the cursor and
/// locate it on the source line. Returns the (0-indexed LSP line,
/// UTF-16 column) to send to `textDocument/hover`, or `None` if the
/// cursor isn't on a word or the word isn't present on the source line.
fn resolve_goal_panel_hover_position(
    panel_line: &str,
    source_line: &str,
    source_line_1indexed: u32,
    panel_column: u32,
) -> Option<(u32, u32)> {
    let (word, _, _) = proof::goal_panel_hover::find_word_at(panel_line, panel_column)?;
    let col = proof::goal_panel_hover::locate_in_source(&word, source_line)?;
    Some((source_line_1indexed - 1, col))
}

/// Resolve a hover over an identifier in the Goal State panel by delegating
/// to the Lean LSP at the corresponding position in the Formal Proof.
///
/// `panel_flat_line` is the 0-indexed flat line within the goal panel's
/// concatenated code blocks (see `GoalPanel.codeBlockOffsets` on the
/// frontend); `character` is the UTF-16 column the user is hovering.
/// We look up the mapped Formal Proof line from the cached goal-state
/// snapshot, find the hovered word on the source line, and issue a
/// regular `textDocument/hover` there.
#[tauri::command]
async fn lsp_hover_goal_panel(
    app: AppHandle,
    panel_flat_line: u32,
    character: u32,
) -> Result<Option<HoverInfo>, String> {
    let state = app.state::<AppState>();

    let (panel_line, source_line_1indexed) = {
        let cache = state
            .goal_state_cache
            .lock()
            .map_err(|e| format!("goal-state cache poisoned: {e}"))?;
        let idx = usize::try_from(panel_flat_line).map_err(|e| e.to_string())?;
        let Some(panel_line) = cache.panel_lines.get(idx).cloned() else {
            return Ok(None);
        };
        let Some(src_line) = cache.panel_line_to_source_line.get(idx).copied().flatten() else {
            return Ok(None);
        };
        (panel_line, src_line)
    };

    // Only the target source line needs to leave the `proof` lock.
    let source_line = {
        let proof = state.proof.lock().await;
        proof
            .formal
            .source
            .split('\n')
            .nth((source_line_1indexed - 1) as usize)
            .unwrap_or("")
            .to_string()
    };

    let Some((line, col)) = resolve_goal_panel_hover_position(
        &panel_line,
        &source_line,
        source_line_1indexed,
        character,
    ) else {
        return Ok(None);
    };

    let params = text_document_position_params(&state.doc_uri(), line, col);
    Ok(call_lsp_raw(&state, "textDocument/hover", params)
        .await?
        .as_ref()
        .and_then(lsp::parse_hover))
}

#[tauri::command]
async fn lsp_definition(
    app: AppHandle,
    line: u32,
    character: u32,
) -> Result<Option<DefinitionLocation>, String> {
    let state = app.state::<AppState>();
    let params = text_document_position_params(&state.doc_uri(), line, character);
    Ok(call_lsp_raw(&state, "textDocument/definition", params)
        .await?
        .as_ref()
        .and_then(lsp::parse_definition))
}

#[tauri::command]
async fn lsp_code_actions(
    app: AppHandle,
    start_line: u32,
    start_character: u32,
    end_line: u32,
    end_character: u32,
) -> Result<Vec<CodeActionInfo>, String> {
    let state = app.state::<AppState>();
    let diagnostics_params = {
        let diags = state
            .current_diagnostics
            .lock()
            .map_err(|e| format!("diagnostics lock poisoned: {e}"))?;
        serde_json::to_value(diags.clone()).unwrap_or_else(|_| json!([]))
    };
    let params = json!({
        "textDocument": { "uri": state.doc_uri() },
        "range": {
            "start": { "line": start_line, "character": start_character },
            "end": { "line": end_line, "character": end_character }
        },
        "context": {
            "diagnostics": diagnostics_params,
            "triggerKind": 1
        }
    });
    Ok(call_lsp_raw(&state, "textDocument/codeAction", params)
        .await?
        .as_ref()
        .map(lsp::parse_code_actions)
        .unwrap_or_default())
}

#[tauri::command]
async fn lsp_resolve_code_action(
    app: AppHandle,
    action: serde_json::Value,
) -> Result<Option<WorkspaceEditDto>, String> {
    let state = app.state::<AppState>();
    Ok(call_lsp_raw(&state, "codeAction/resolve", action)
        .await?
        .as_ref()
        .and_then(|v| v.get("edit"))
        .and_then(lsp::parse_workspace_edit))
}

#[tauri::command]
async fn lsp_document_symbols(app: AppHandle) -> Result<Vec<DocumentSymbolInfo>, String> {
    let state = app.state::<AppState>();
    let params = json!({ "textDocument": { "uri": state.doc_uri() } });
    Ok(call_lsp_raw(&state, "textDocument/documentSymbol", params)
        .await?
        .as_ref()
        .map(lsp::parse_document_symbols)
        .unwrap_or_default())
}

fn handle_lsp_message(
    app: &AppHandle,
    token_types: &Arc<Mutex<Vec<String>>>,
    token_modifiers: &Arc<Mutex<Vec<String>>>,
    writer: &Arc<tokio::sync::Mutex<Box<dyn std::io::Write + Send>>>,
    next_id: &Arc<AtomicI64>,
    msg: &serde_json::Value,
) {
    if let Some(result) = msg.get("result") {
        if result.get("capabilities").is_some() {
            handle_initialize_response(app, token_types, token_modifiers, result);
        } else if let Some(data) = result.get("data") {
            handle_semantic_tokens_response(app, token_types, token_modifiers, data);
        }
        return;
    }

    // Server→client requests have both "method" and "id"; ack them with null.
    if let Some(id) = msg.get("id") {
        if let Some(method) = msg.get("method").and_then(|m| m.as_str()) {
            if let Err(e) = lsp::ack_request(writer, id) {
                log::warn!("Failed to ack LSP request: {e}");
            }

            // The server says our cached semantic tokens are stale — re-request.
            if method == "workspace/semanticTokens/refresh" {
                let state = app.state::<AppState>();
                let doc_uri = state.doc_uri();
                if let Err(e) = lsp::send_request_sync(
                    writer,
                    next_id,
                    "textDocument/semanticTokens/full",
                    json!({ "textDocument": { "uri": doc_uri } }),
                ) {
                    log::warn!("Failed to re-request semantic tokens: {e}");
                }
            }

            return;
        }
    }

    // Only messages with a "method" field are notifications we might handle.
    // Everything else (responses without `result.capabilities` or
    // `result.data`) is ignored upstream.
    let Some(method) = msg.get("method").and_then(|m| m.as_str()) else {
        return;
    };

    match serde_json::from_value::<LspNotification>(msg.clone()) {
        Ok(LspNotification::PublishDiagnostics(params)) => {
            let diagnostics = lsp::parse_diagnostics(&params);
            let state = app.state::<AppState>();
            *state.current_diagnostics.lock().unwrap() = diagnostics.clone();
            app.emit("lsp-diagnostics", diagnostics).ok();
        }
        Ok(LspNotification::FileProgress(params)) => {
            let ranges = lsp::parse_file_progress(&params);
            let elaboration_done = ranges.is_empty();
            app.emit("lsp-file-progress", ranges).ok();
            if elaboration_done {
                let state = app.state::<AppState>();
                let seq = state.goal_state_seq.fetch_add(1, Ordering::SeqCst) + 1;
                spawn_goal_state_refresh(app.clone(), seq);
            }
        }
        Ok(LspNotification::LogMessage(p) | LspNotification::ShowMessage(p)) => {
            log::info!("LSP: {}", p.message);
        }
        Err(_) => {
            log::debug!("Unhandled LSP notification: {method}");
        }
    }
}

fn handle_initialize_response(
    app: &AppHandle,
    token_types: &Arc<Mutex<Vec<String>>>,
    token_modifiers: &Arc<Mutex<Vec<String>>>,
    result: &serde_json::Value,
) {
    let type_legend = lsp::parse_token_legend(result);
    let modifier_legend = lsp::parse_modifier_legend(result);
    log::info!("LSP semantic token legend: types={type_legend:?}, modifiers={modifier_legend:?}");
    if let Ok(mut types) = token_types.lock() {
        *types = type_legend;
    }
    if let Ok(mut mods) = token_modifiers.lock() {
        *mods = modifier_legend;
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
    token_modifiers: &Arc<Mutex<Vec<String>>>,
    data: &serde_json::Value,
) {
    let Some(arr) = data.as_array() else { return };
    let data_u32: Vec<u32> = arr
        .iter()
        .filter_map(|v| v.as_u64().and_then(|n| u32::try_from(n).ok()))
        .collect();

    let type_guard = token_types
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mod_guard = token_modifiers
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let tokens = lsp::decode_semantic_tokens(&data_u32, &type_guard, &mod_guard);
    drop(type_guard);
    drop(mod_guard);
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
            let app_data_dir = app
                .path()
                .app_data_dir()
                .expect("Failed to resolve app data directory");

            let project_path = app_data_dir.join("lean-project");

            let initial_settings = settings::load_settings(&app_data_dir);

            #[cfg(feature = "mock-llm")]
            let llm_backend: Arc<dyn llm::Llm> = Arc::new(llm::MockBackend::from_env());

            #[cfg(not(feature = "mock-llm"))]
            let llm_backend: Arc<dyn llm::Llm> = {
                match llm::AnthropicBackend::from_env() {
                    Ok(b) => Arc::new(b),
                    Err(_) => Arc::new(llm::MockBackend::echo()),
                }
            };

            app.manage(AppState {
                lsp_client: Arc::new(tokio::sync::Mutex::new(None)),
                project_path,
                doc_version: AtomicI64::new(2),
                setup_running: Arc::new(AtomicBool::new(false)),
                proof: Arc::new(tokio::sync::Mutex::new(proof::Proof::default())),
                transcript: Arc::new(tokio::sync::Mutex::new(assistant::Transcript::default())),
                llm: llm_backend,
                settings: Arc::new(tokio::sync::Mutex::new(initial_settings)),
                current_session_path: Arc::new(tokio::sync::Mutex::new(None)),
                session_dirty: Arc::new(AtomicBool::new(false)),
                current_diagnostics: Arc::new(Mutex::new(Vec::new())),
                prose_dirty: Arc::new(AtomicBool::new(false)),
                prose_generation_seq: Arc::new(AtomicU64::new(0)),
                goal_state_seq: Arc::new(AtomicU64::new(0)),
                goal_state_cache: Arc::new(Mutex::new(GoalStateCache::default())),
            });

            Ok(())
        })
        .menu(menu::build_menu)
        .on_menu_event(|app, event| {
            app.emit("menu-event", event.id().0.clone()).ok();
        })
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            // A second instance was launched; focus the existing window instead.
            if let Some(window) = app.get_webview_window("main") {
                window.set_focus().ok();
            }
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_window_state::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            get_setup_status,
            start_setup,
            start_lsp,
            update_document,
            get_completions,
            lsp_hover,
            lsp_hover_goal_panel,
            lsp_definition,
            lsp_code_actions,
            lsp_resolve_code_action,
            lsp_document_symbols,
            assistant::send_message,
            assistant::get_transcript,
            assistant::load_transcript,
            proof::translator::generate_prose,
            settings::get_settings,
            settings::save_settings,
            settings::get_default_assistant_prompt,
            settings::get_default_translation_prompt,
            llm::get_available_models,
            session::new_session,
            session::open_session,
            session::save_session,
            session::save_session_as,
            session::auto_save_session,
            session::check_auto_save,
            session::restore_auto_save,
            session::delete_auto_save,
            session::get_last_session,
            session::set_last_session,
            session::set_window_title,
            menu::set_menu_item_enabled,
        ])
        .run(tauri::generate_context!())
        .expect("error while running turnstile");
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
    use std::sync::Mutex;

    use super::{
        assistant, end_of_document_position, llm, lsp, proof, resolve_goal_panel_hover_position,
        should_generate_prose, AppState, ShouldGenerate,
    };
    use std::sync::Arc;

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

    #[test]
    fn resolve_goal_panel_hover_position_finds_word_and_locates_on_source() {
        // Hover at column 1 on "hp : p" hits the word "hp"; on the mapped
        // source line "intro hp hq", "hp" starts at UTF-16 column 6.
        assert_eq!(
            resolve_goal_panel_hover_position("hp : p", "intro hp hq", 3, 1),
            Some((2, 6))
        );
    }

    #[test]
    fn resolve_goal_panel_hover_position_none_when_not_on_word() {
        // Column 5 on "a    b" is whitespace.
        assert_eq!(
            resolve_goal_panel_hover_position("a    b", "let a := b", 1, 3),
            None
        );
    }

    #[test]
    fn resolve_goal_panel_hover_position_none_when_word_absent_from_source() {
        assert_eq!(
            resolve_goal_panel_hover_position("hp : p", "apply or_left", 1, 1),
            None
        );
    }

    #[test]
    fn end_of_document_position_basic() {
        assert_eq!(end_of_document_position(""), (0, 0));
        assert_eq!(end_of_document_position("abc"), (0, 3));
        assert_eq!(end_of_document_position("abc\n"), (1, 0));
        assert_eq!(end_of_document_position("abc\ndef"), (1, 3));
        assert_eq!(end_of_document_position("abc\ndef\n"), (2, 0));
        assert_eq!(end_of_document_position("abc\ndef\nghi"), (2, 3));
    }

    /// Minimal `AppState` suitable for exercising `should_generate_prose`. The
    /// LSP client, transcript, and session fields are not read by the
    /// pre-flight checks.
    fn make_state() -> AppState {
        AppState {
            lsp_client: Arc::new(tokio::sync::Mutex::new(None)),
            project_path: PathBuf::new(),
            doc_version: AtomicI64::new(1),
            setup_running: Arc::new(AtomicBool::new(false)),
            proof: Arc::new(tokio::sync::Mutex::new(proof::Proof::default())),
            transcript: Arc::new(tokio::sync::Mutex::new(assistant::Transcript::default())),
            llm: Arc::new(llm::MockBackend::echo()),
            settings: Arc::new(tokio::sync::Mutex::new(crate::settings::Settings::default())),
            current_session_path: Arc::new(tokio::sync::Mutex::new(None)),
            session_dirty: Arc::new(AtomicBool::new(false)),
            current_diagnostics: Arc::new(Mutex::new(Vec::new())),
            prose_dirty: Arc::new(AtomicBool::new(false)),
            prose_generation_seq: Arc::new(AtomicU64::new(0)),
            goal_state_seq: Arc::new(AtomicU64::new(0)),
            goal_state_cache: Arc::new(Mutex::new(crate::GoalStateCache::default())),
        }
    }

    fn error_diagnostic() -> lsp::DiagnosticInfo {
        lsp::DiagnosticInfo {
            start_line: 0,
            start_col: 0,
            end_line: 0,
            end_col: 1,
            severity: 1,
            message: "boom".to_string(),
        }
    }

    fn warning_diagnostic() -> lsp::DiagnosticInfo {
        lsp::DiagnosticInfo {
            start_line: 0,
            start_col: 0,
            end_line: 0,
            end_col: 1,
            severity: 2,
            message: "heads up".to_string(),
        }
    }

    #[tokio::test]
    async fn should_generate_aborts_when_seq_is_stale() {
        let state = make_state();
        state.prose_generation_seq.store(7, Ordering::SeqCst);
        state.proof.lock().await.formal.source = "theorem foo".to_string();

        // We're task seq=5, but the latest edit bumped seq to 7.
        assert!(matches!(
            should_generate_prose(&state, 5).await,
            ShouldGenerate::Abort
        ));
    }

    #[tokio::test]
    async fn should_generate_aborts_when_diagnostics_have_errors() {
        let state = make_state();
        state.prose_generation_seq.store(1, Ordering::SeqCst);
        state.proof.lock().await.formal.source = "theorem foo".to_string();
        state
            .current_diagnostics
            .lock()
            .unwrap()
            .push(error_diagnostic());

        assert!(matches!(
            should_generate_prose(&state, 1).await,
            ShouldGenerate::Abort
        ));
    }

    #[tokio::test]
    async fn should_generate_proceeds_when_diagnostics_have_only_warnings() {
        let state = make_state();
        state.prose_generation_seq.store(1, Ordering::SeqCst);
        state.proof.lock().await.formal.source = "theorem foo".to_string();
        state
            .current_diagnostics
            .lock()
            .unwrap()
            .push(warning_diagnostic());

        assert!(matches!(
            should_generate_prose(&state, 1).await,
            ShouldGenerate::Proceed { .. }
        ));
    }

    #[tokio::test]
    async fn should_generate_aborts_when_source_hash_unchanged() {
        let state = make_state();
        state.prose_generation_seq.store(1, Ordering::SeqCst);
        let source = "theorem foo : True := trivial".to_string();
        {
            let mut guard = state.proof.lock().await;
            guard.formal.source = source.clone();
            guard.prose.source_hash = proof::compute_source_hash(&source);
        }

        assert!(matches!(
            should_generate_prose(&state, 1).await,
            ShouldGenerate::Abort
        ));
    }

    #[tokio::test]
    async fn should_generate_aborts_when_source_is_whitespace_only() {
        let state = make_state();
        state.prose_generation_seq.store(1, Ordering::SeqCst);
        state.proof.lock().await.formal.source = "   \n\t  ".to_string();

        assert!(matches!(
            should_generate_prose(&state, 1).await,
            ShouldGenerate::Abort
        ));
    }

    #[tokio::test]
    async fn should_generate_proceeds_with_clean_source_and_matching_seq() {
        let state = make_state();
        state.prose_generation_seq.store(3, Ordering::SeqCst);
        let source = "theorem foo : True := trivial".to_string();
        state.proof.lock().await.formal.source = source.clone();

        match should_generate_prose(&state, 3).await {
            ShouldGenerate::Proceed { source: s, hash } => {
                assert_eq!(s, source);
                assert_eq!(hash, proof::compute_source_hash(&source));
            }
            ShouldGenerate::Abort => panic!("expected Proceed"),
        }
    }
}
