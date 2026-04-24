#![warn(unused_qualifications)]
//! Tauri app glue: command handlers, app state, and LSP message dispatch.
//!
//! # Data flows
//!
//! **Edit → diagnostics:**
//! Frontend change → `update_document` → `textDocument/didChange` → LSP emits
//! `textDocument/publishDiagnostics` → `lsp-diagnostics` Tauri event → frontend.
//!

pub mod assistant;
pub mod format;
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

use tauri::{AppHandle, Emitter, Manager};

use lsp::{
    CodeActionInfo, CompletionItem, DefinitionLocation, DocumentSymbolInfo, HoverInfo, LeanClient,
    LspStatus, WorkspaceEditDto,
};

pub struct AppState {
    pub lsp_client: Arc<tokio::sync::Mutex<Option<LeanClient>>>,
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
    /// Semantic token type legend from the server's `InitializeResult`.
    pub token_types: Arc<Mutex<Vec<String>>>,
    /// Semantic token modifier legend from the server's `InitializeResult`.
    pub token_modifiers: Arc<Mutex<Vec<String>>>,
}

impl AppState {
    fn doc_uri(&self) -> Result<lsp_types::Url, String> {
        lsp_types::Url::from_file_path(self.project_path.join("Proof.lean"))
            .map_err(|()| "invalid doc path".to_string())
    }
}

#[derive(serde::Serialize)]
struct SetupStatusResponse {
    complete: bool,
    project_path: String,
}

/// An LSP `Position` as sent by the frontend.
#[derive(serde::Deserialize)]
struct LspPosition {
    line: u32,
    character: u32,
}

/// An LSP `Range` as sent by the frontend.
#[derive(serde::Deserialize)]
struct LspRange {
    start: LspPosition,
    end: LspPosition,
}

/// One entry in a `textDocument/didChange` `contentChanges` array.
#[derive(serde::Deserialize)]
struct ContentChange {
    range: LspRange,
    text: String,
}

/// Convert an LSP (line, character) position to a byte offset in `source`.
/// Both line and character are 0-indexed; character counts UTF-16 code units.
fn lsp_pos_to_offset(source: &str, target_line: u32, target_character: u32) -> usize {
    let mut current_line = 0u32;
    let mut utf16_col = 0u32;
    let mut offset = 0;

    for ch in source.chars() {
        if current_line == target_line {
            if utf16_col >= target_character {
                return offset;
            }
            utf16_col += u32::try_from(ch.len_utf16()).unwrap_or(2);
        } else if ch == '\n' {
            current_line += 1;
        }
        offset += ch.len_utf8();
    }
    offset.min(source.len())
}

/// Apply a batch of incremental LSP content changes to `source` in order.
/// Changes must be applied last-to-first so earlier offsets stay valid.
fn apply_content_changes(source: &mut String, changes: &[ContentChange]) {
    if changes.is_empty() {
        return;
    }
    if changes.len() == 1 {
        let c = &changes[0];
        let start = lsp_pos_to_offset(source, c.range.start.line, c.range.start.character);
        let end = lsp_pos_to_offset(source, c.range.end.line, c.range.end.character);
        source.replace_range(start..end, &c.text);
        return;
    }
    let mut edits: Vec<(usize, usize, &str)> = changes
        .iter()
        .map(|c| {
            let start = lsp_pos_to_offset(source, c.range.start.line, c.range.start.character);
            let end = lsp_pos_to_offset(source, c.range.end.line, c.range.end.character);
            (start, end, c.text.as_str())
        })
        .collect();
    edits.sort_by_key(|e| std::cmp::Reverse(e.0));
    for (start, end, text) in edits {
        source.replace_range(start..end, text);
    }
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn parse_formatted_input(text: String) -> Vec<format::Span> {
    format::parse(&text)
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
async fn get_lsp_ready(app: AppHandle) -> bool {
    let state = app.state::<AppState>();
    let ready = state.lsp_client.lock().await.is_some();
    ready
}

#[tauri::command]
async fn start_lsp(app: AppHandle) -> Result<(), String> {
    use lsp_types::{DidOpenTextDocumentParams, TextDocumentItem, Url};
    use tokio::sync::mpsc;

    let state = app.state::<AppState>();

    let lean_bin = setup::lean_bin();
    let root_uri = Url::from_directory_path(&state.project_path)
        .map_err(|()| "invalid project path".to_string())?;

    app.emit(
        "lsp-status",
        LspStatus {
            state: String::new(),
            message: format!("initializing ({})...", lean_bin.display()),
        },
    )
    .ok();

    let (event_tx, event_rx) = mpsc::channel(64);

    tauri::async_runtime::spawn(lsp::events::forward_lsp_events(event_rx, app.clone()));

    let client = LeanClient::start(&lean_bin, &state.project_path, root_uri, event_tx)
        .await
        .map_err(|e| format!("LSP start failed: {e}"))?;

    let (types, modifiers) = client.token_legend();
    tracing::debug!("LSP token legend: types={types:?}");
    {
        *state.token_types.lock().unwrap() = types;
        *state.token_modifiers.lock().unwrap() = modifiers;
    }

    app.emit(
        "lsp-status",
        LspStatus {
            state: "connected".to_string(),
            message: "connected".to_string(),
        },
    )
    .ok();

    let doc_uri = state.doc_uri()?;

    client
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: doc_uri,
                language_id: "lean4".to_string(),
                version: 1,
                text: String::new(),
            },
        })
        .map_err(|e| e.to_string())?;

    *state.lsp_client.lock().await = Some(client);

    Ok(())
}

/// Decode semantic tokens and spawn a background task to update proof annotations.
fn apply_semantic_tokens(app: &AppHandle, tokens: &lsp_types::SemanticTokens) {
    let state = app.state::<AppState>();
    let type_guard = state
        .token_types
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mod_guard = state
        .token_modifiers
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let decoded = lsp::decode_semantic_tokens(&tokens.data, &type_guard, &mod_guard);
    drop(type_guard);
    drop(mod_guard);
    let proof = state.proof.clone();
    let app_handle = app.clone();
    tauri::async_runtime::spawn(async move {
        let items = {
            let mut guard = proof.lock().await;
            guard.annotations.set_tokens(&decoded);
            guard.annotations.items.clone()
        };
        app_handle
            .emit(proof::ANNOTATIONS_UPDATED_EVENT, &items)
            .ok();
    });
}

#[tauri::command]
async fn update_document(app: AppHandle, changes: Vec<ContentChange>) -> Result<(), String> {
    use lsp_types::{
        DidChangeTextDocumentParams, SemanticTokensParams, TextDocumentContentChangeEvent,
        TextDocumentIdentifier, VersionedTextDocumentIdentifier,
    };

    let state = app.state::<AppState>();
    let version =
        i32::try_from(state.doc_version.fetch_add(1, Ordering::SeqCst)).unwrap_or(i32::MAX);

    // Apply incremental changes to the stored source so `read_lean_source` stays in sync.
    {
        let mut proof = state.proof.lock().await;
        apply_content_changes(&mut proof.formal.source, &changes);
    }

    // Invalidate any in-flight goal-state refresh task spawned before this edit.
    state.goal_state_seq.fetch_add(1, Ordering::SeqCst);

    let client_arc = {
        let lock = state.lsp_client.lock().await;
        lock.as_ref().map(|_| state.lsp_client.clone())
    };

    let Some(client_arc) = client_arc else {
        tracing::warn!(
            "update_document: LSP client not connected; edit will not be sent to server"
        );
        return Ok(());
    };

    let doc_uri = state.doc_uri()?;

    let content_changes: Vec<TextDocumentContentChangeEvent> = changes
        .iter()
        .map(|c| TextDocumentContentChangeEvent {
            range: Some(lsp_types::Range {
                start: lsp_types::Position {
                    line: c.range.start.line,
                    character: c.range.start.character,
                },
                end: lsp_types::Position {
                    line: c.range.end.line,
                    character: c.range.end.character,
                },
            }),
            range_length: None,
            text: c.text.clone(),
        })
        .collect();

    {
        let lock = client_arc.lock().await;
        if let Some(client) = lock.as_ref() {
            client
                .did_change(DidChangeTextDocumentParams {
                    text_document: VersionedTextDocumentIdentifier {
                        uri: doc_uri.clone(),
                        version,
                    },
                    content_changes,
                })
                .map_err(|e| e.to_string())?;
        }
    }

    // Request semantic tokens outside the didChange lock so concurrent plainGoal
    // calls from the goal-state refresh task are not blocked.
    let tokens_result = {
        let lock = client_arc.lock().await;
        if let Some(client) = lock.as_ref() {
            client
                .semantic_tokens_full(SemanticTokensParams {
                    text_document: TextDocumentIdentifier { uri: doc_uri },
                    work_done_progress_params: lsp_types::WorkDoneProgressParams::default(),
                    partial_result_params: lsp_types::PartialResultParams::default(),
                })
                .await
                .ok()
                .flatten()
        } else {
            None
        }
    };

    if let Some(lsp_types::SemanticTokensResult::Tokens(tokens)) = tokens_result {
        apply_semantic_tokens(&app, &tokens);
    }

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
/// Performs (in order): staleness check, diagnostics check, goal-state-hash
/// check, and empty-goal-state check. All four are short synchronous
/// operations except the proof clone which needs the async mutex on `proof`.
async fn should_generate_prose(state: &AppState, seq: u64) -> ShouldGenerate {
    // A newer goal-state change has superseded us.
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

    let (source, goal_state_text, last_hash) = {
        let guard = state.proof.lock().await;
        (
            guard.formal.source.clone(),
            guard.goal_state.full.clone(),
            guard.prose.goal_state_hash.clone(),
        )
    };
    let hash = proof::compute_source_hash(&goal_state_text);

    // Goal state hasn't changed since the last prose generation.
    if last_hash == hash {
        return ShouldGenerate::Abort;
    }

    // Nothing to translate.
    if goal_state_text.trim().is_empty() {
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

            if let Ok(raw) = result {
                let prose_text = proof::translator::render_katex(&raw);
                {
                    let mut proof_guard = state.proof.lock().await;
                    proof_guard.prose.text.clone_from(&prose_text);
                    proof_guard.prose.goal_state_hash.clone_from(&hash);
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
    use lsp_types::{Position, TextDocumentIdentifier, TextDocumentPositionParams};

    let source = state.proof.lock().await.formal.source.clone();
    let (line, col) = end_of_document_position(&source);

    let lock = state.lsp_client.lock().await;
    let Some(client) = lock.as_ref() else {
        return Err("LSP not connected".to_string());
    };

    let doc_uri = state.doc_uri()?;

    let result = client
        .plain_goal(TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: doc_uri },
            position: Position {
                line,
                character: col,
            },
        })
        .await
        .map_err(|e| e.to_string())?;

    Ok(result.map(|g| g.rendered).unwrap_or_default())
}

/// Spawn a background task that, after a short debounce, fetches the
/// whole-proof goal state and emits a [`proof::GOAL_STATE_UPDATED_EVENT`] to
/// the frontend.
///
/// The task is sequence-guarded: if `state.goal_state_seq` advances before
/// the debounce fires, this task returns without emitting.
fn spawn_goal_state_refresh(app: AppHandle, seq: u64) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;

        let state = app.state::<AppState>();
        if state.goal_state_seq.load(Ordering::SeqCst) != seq {
            return;
        }

        let full = match fetch_full_proof_goal_state(&state).await {
            Ok(s) => s,
            Err(e) => {
                log::debug!("goal-state refresh: fetch failed: {e}");
                return;
            }
        };

        // Persist goal state and check whether it's changed since the last prose
        // generation — both in one lock scope to avoid a TOCTOU race.
        let (last_hash, new_hash) = {
            let mut guard = state.proof.lock().await;
            let last = guard.prose.goal_state_hash.clone();
            guard.goal_state.full.clone_from(&full);
            drop(guard);
            let new = proof::compute_source_hash(&full);
            (last, new)
        };

        app.emit(proof::GOAL_STATE_UPDATED_EVENT, &full).ok();

        if last_hash != new_hash && !full.trim().is_empty() {
            state.prose_dirty.store(true, Ordering::SeqCst);
            let prose_seq = state.prose_generation_seq.fetch_add(1, Ordering::SeqCst) + 1;
            spawn_prose_regeneration(app.clone(), prose_seq);
        }
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

#[tauri::command]
async fn get_completions(
    app: AppHandle,
    line: u32,
    col: u32,
) -> Result<Vec<CompletionItem>, String> {
    use lsp_types::{
        CompletionContext, CompletionParams, CompletionTriggerKind, Position,
        TextDocumentIdentifier,
    };

    let state = app.state::<AppState>();
    let lock = state.lsp_client.lock().await;
    let Some(client) = lock.as_ref() else {
        return Ok(Vec::new());
    };
    let doc_uri = state.doc_uri()?;
    let raw = client
        .completion(CompletionParams {
            text_document_position: lsp_types::TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: doc_uri },
                position: Position {
                    line,
                    character: col,
                },
            },
            work_done_progress_params: lsp_types::WorkDoneProgressParams::default(),
            partial_result_params: lsp_types::PartialResultParams::default(),
            context: Some(CompletionContext {
                trigger_kind: CompletionTriggerKind::INVOKED,
                trigger_character: None,
            }),
        })
        .await
        .map_err(|e| e.to_string())?;
    drop(lock);
    Ok(lsp::parse_completion_items(raw))
}

#[tauri::command]
async fn lsp_hover(app: AppHandle, line: u32, character: u32) -> Result<Option<HoverInfo>, String> {
    use lsp_types::{HoverParams, Position, TextDocumentIdentifier};

    let state = app.state::<AppState>();
    let lock = state.lsp_client.lock().await;
    let Some(client) = lock.as_ref() else {
        return Ok(None);
    };
    let doc_uri = state.doc_uri()?;
    let raw = client
        .hover(HoverParams {
            text_document_position_params: lsp_types::TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: doc_uri },
                position: Position { line, character },
            },
            work_done_progress_params: lsp_types::WorkDoneProgressParams::default(),
        })
        .await
        .map_err(|e| e.to_string())?;
    drop(lock);
    Ok(lsp::parse_hover(raw))
}

#[tauri::command]
async fn lsp_definition(
    app: AppHandle,
    line: u32,
    character: u32,
) -> Result<Option<DefinitionLocation>, String> {
    use lsp_types::{GotoDefinitionParams, Position, TextDocumentIdentifier};

    let state = app.state::<AppState>();
    let lock = state.lsp_client.lock().await;
    let Some(client) = lock.as_ref() else {
        return Ok(None);
    };
    let doc_uri = state.doc_uri()?;
    let raw = client
        .definition(GotoDefinitionParams {
            text_document_position_params: lsp_types::TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: doc_uri },
                position: Position { line, character },
            },
            work_done_progress_params: lsp_types::WorkDoneProgressParams::default(),
            partial_result_params: lsp_types::PartialResultParams::default(),
        })
        .await
        .map_err(|e| e.to_string())?;
    drop(lock);
    Ok(lsp::parse_definition(raw))
}

#[tauri::command]
async fn lsp_code_actions(
    app: AppHandle,
    start_line: u32,
    start_character: u32,
    end_line: u32,
    end_character: u32,
) -> Result<Vec<CodeActionInfo>, String> {
    use lsp_types::{
        CodeActionContext, CodeActionParams, Diagnostic, Position, Range, TextDocumentIdentifier,
    };

    let state = app.state::<AppState>();
    let lock = state.lsp_client.lock().await;
    let Some(client) = lock.as_ref() else {
        return Ok(Vec::new());
    };
    let doc_uri = state.doc_uri()?;
    let diagnostics: Vec<Diagnostic> = {
        let diags = state
            .current_diagnostics
            .lock()
            .map_err(|e| format!("diagnostics lock poisoned: {e}"))?;
        diags
            .iter()
            .filter_map(|d| serde_json::from_value(serde_json::to_value(d).ok()?).ok())
            .collect()
    };
    let raw = client
        .code_action(CodeActionParams {
            text_document: TextDocumentIdentifier { uri: doc_uri },
            range: Range {
                start: Position {
                    line: start_line,
                    character: start_character,
                },
                end: Position {
                    line: end_line,
                    character: end_character,
                },
            },
            context: CodeActionContext {
                diagnostics,
                only: None,
                trigger_kind: None,
            },
            work_done_progress_params: lsp_types::WorkDoneProgressParams::default(),
            partial_result_params: lsp_types::PartialResultParams::default(),
        })
        .await
        .map_err(|e| e.to_string())?;
    drop(lock);
    Ok(lsp::parse_code_actions(raw))
}

#[tauri::command]
async fn lsp_resolve_code_action(
    app: AppHandle,
    action: serde_json::Value,
) -> Result<Option<WorkspaceEditDto>, String> {
    use lsp_types::CodeAction;

    let state = app.state::<AppState>();
    let lock = state.lsp_client.lock().await;
    let Some(client) = lock.as_ref() else {
        return Ok(None);
    };
    let code_action: CodeAction =
        serde_json::from_value(action).map_err(|e| format!("invalid code action: {e}"))?;
    let resolved = client
        .resolve_code_action(code_action)
        .await
        .map_err(|e| e.to_string())?;
    drop(lock);
    Ok(resolved.edit.and_then(lsp::parse_workspace_edit))
}

#[tauri::command]
async fn lsp_document_symbols(app: AppHandle) -> Result<Vec<DocumentSymbolInfo>, String> {
    use lsp_types::{DocumentSymbolParams, TextDocumentIdentifier};

    let state = app.state::<AppState>();
    let lock = state.lsp_client.lock().await;
    let Some(client) = lock.as_ref() else {
        return Ok(Vec::new());
    };
    let doc_uri = state.doc_uri()?;
    let raw = client
        .document_symbol(DocumentSymbolParams {
            text_document: TextDocumentIdentifier { uri: doc_uri },
            work_done_progress_params: lsp_types::WorkDoneProgressParams::default(),
            partial_result_params: lsp_types::PartialResultParams::default(),
        })
        .await
        .map_err(|e| e.to_string())?;
    drop(lock);
    Ok(lsp::parse_document_symbols(raw))
}

/// Send `textDocument/didSave` to the LSP server with the current source text.
/// Called after session writes to disk. Fire-and-forget; errors are logged, not propagated.
pub(crate) async fn send_did_save(state: &AppState) {
    use lsp_types::{DidSaveTextDocumentParams, TextDocumentIdentifier};

    let source = state.proof.lock().await.formal.source.clone();
    let lock = state.lsp_client.lock().await;
    if let Some(client) = lock.as_ref() {
        let Ok(doc_uri) = state.doc_uri() else {
            return;
        };
        client
            .did_save(DidSaveTextDocumentParams {
                text_document: TextDocumentIdentifier { uri: doc_uri },
                text: Some(source),
            })
            .ok();
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
/// # Panics
///
/// Panics if the Tauri application fails to build or run.
#[allow(clippy::too_many_lines)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    tracing_log::LogTracer::init().ok();

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

            let setup_running = Arc::new(AtomicBool::new(false));

            app.manage(AppState {
                lsp_client: Arc::new(tokio::sync::Mutex::new(None)),
                project_path: project_path.clone(),
                doc_version: AtomicI64::new(2),
                setup_running: setup_running.clone(),
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
                token_types: Arc::new(Mutex::new(Vec::new())),
                token_modifiers: Arc::new(Mutex::new(Vec::new())),
            });

            // On startup: run setup if needed, then start the LSP.
            let app_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                if !setup::check_setup_complete(&project_path)
                    && !setup_running.swap(true, Ordering::SeqCst)
                {
                    setup::run_setup(app_handle.clone(), project_path.clone(), setup_running).await;
                }
                // Start the LSP regardless of whether setup just ran or was already done.
                // `start_lsp` is idempotent — if setup failed, lean_bin won't exist and
                // it will emit an lsp-status error that the frontend can surface.
                if let Err(e) = start_lsp(app_handle.clone()).await {
                    log::warn!("Auto-start LSP failed: {e}");
                }
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
        .on_window_event(|window, event| {
            if matches!(event, tauri::WindowEvent::Destroyed) {
                use lsp_types::{DidCloseTextDocumentParams, TextDocumentIdentifier};
                let app = window.app_handle().clone();
                tauri::async_runtime::block_on(async move {
                    let state = app.state::<AppState>();
                    let lock = state.lsp_client.lock().await;
                    if let Some(client) = lock.as_ref() {
                        if let Ok(uri) = state.doc_uri() {
                            client
                                .did_close(DidCloseTextDocumentParams {
                                    text_document: TextDocumentIdentifier { uri },
                                })
                                .ok();
                        }
                    }
                });
            }
        })
        .invoke_handler(tauri::generate_handler![
            parse_formatted_input,
            get_setup_status,
            start_setup,
            get_lsp_ready,
            start_lsp,
            update_document,
            get_completions,
            lsp_hover,
            lsp_definition,
            lsp_code_actions,
            lsp_resolve_code_action,
            lsp_document_symbols,
            assistant::send_message,
            assistant::get_transcript,
            assistant::load_transcript,
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
        apply_content_changes, assistant, end_of_document_position, llm, lsp, lsp_pos_to_offset,
        proof, should_generate_prose, AppState, ContentChange, LspPosition, LspRange,
        ShouldGenerate,
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
    fn end_of_document_position_basic() {
        assert_eq!(end_of_document_position(""), (0, 0));
        assert_eq!(end_of_document_position("abc"), (0, 3));
        assert_eq!(end_of_document_position("abc\n"), (1, 0));
        assert_eq!(end_of_document_position("abc\ndef"), (1, 3));
        assert_eq!(end_of_document_position("abc\ndef\n"), (2, 0));
        assert_eq!(end_of_document_position("abc\ndef\nghi"), (2, 3));
    }

    // -- lsp_pos_to_offset --------------------------------------------------

    fn pos(line: u32, character: u32) -> LspPosition {
        LspPosition { line, character }
    }

    fn change(sl: u32, sc: u32, el: u32, ec: u32, text: &str) -> ContentChange {
        ContentChange {
            range: LspRange {
                start: pos(sl, sc),
                end: pos(el, ec),
            },
            text: text.to_string(),
        }
    }

    #[test]
    fn lsp_pos_to_offset_start_of_doc() {
        assert_eq!(lsp_pos_to_offset("hello\nworld", 0, 0), 0);
    }

    #[test]
    fn lsp_pos_to_offset_mid_first_line() {
        assert_eq!(lsp_pos_to_offset("hello\nworld", 0, 3), 3);
    }

    #[test]
    fn lsp_pos_to_offset_start_of_second_line() {
        assert_eq!(lsp_pos_to_offset("hello\nworld", 1, 0), 6);
    }

    #[test]
    fn lsp_pos_to_offset_mid_second_line() {
        assert_eq!(lsp_pos_to_offset("hello\nworld", 1, 3), 9);
    }

    #[test]
    fn lsp_pos_to_offset_empty_doc() {
        assert_eq!(lsp_pos_to_offset("", 0, 0), 0);
    }

    #[test]
    fn lsp_pos_to_offset_clamps_to_doc_length() {
        assert_eq!(lsp_pos_to_offset("abc", 0, 100), 3);
    }

    #[test]
    fn lsp_pos_to_offset_clamps_out_of_bounds_line() {
        assert_eq!(lsp_pos_to_offset("hello\nworld", 5, 0), 11);
    }

    // -- apply_content_changes ----------------------------------------------

    #[test]
    fn apply_changes_single_insertion() {
        let mut s = "hello world".to_string();
        apply_content_changes(&mut s, &[change(0, 5, 0, 5, ",")]);
        assert_eq!(s, "hello, world");
    }

    #[test]
    fn apply_changes_single_deletion() {
        let mut s = "hello world".to_string();
        apply_content_changes(&mut s, &[change(0, 5, 0, 6, "")]);
        assert_eq!(s, "helloworld");
    }

    #[test]
    fn apply_changes_single_replacement() {
        let mut s = "hello world".to_string();
        apply_content_changes(&mut s, &[change(0, 6, 0, 11, "Lean")]);
        assert_eq!(s, "hello Lean");
    }

    #[test]
    fn apply_changes_append_newline_and_text() {
        let mut s = "theorem foo".to_string();
        apply_content_changes(&mut s, &[change(0, 11, 0, 11, " : True")]);
        assert_eq!(s, "theorem foo : True");
    }

    #[test]
    fn apply_changes_multiline_replacement() {
        let mut s = "line0\nline1\nline2".to_string();
        // Replace "line1" (line 1, chars 0–5) with "replaced"
        apply_content_changes(&mut s, &[change(1, 0, 1, 5, "replaced")]);
        assert_eq!(s, "line0\nreplaced\nline2");
    }

    #[test]
    fn apply_changes_multiple_non_overlapping() {
        // Two insertions on the same line at different positions.
        // CodeMirror sends them in document order; we must apply last-first.
        let mut s = "abcdef".to_string();
        let changes = vec![
            change(0, 2, 0, 2, "X"), // insert X at pos 2
            change(0, 4, 0, 4, "Y"), // insert Y at pos 4 (in old doc)
        ];
        apply_content_changes(&mut s, &changes);
        assert_eq!(s, "abXcdYef");
    }

    #[test]
    fn apply_changes_empty_change_list() {
        let mut s = "unchanged".to_string();
        apply_content_changes(&mut s, &[]);
        assert_eq!(s, "unchanged");
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
            token_types: Arc::new(Mutex::new(Vec::new())),
            token_modifiers: Arc::new(Mutex::new(Vec::new())),
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
        {
            let mut guard = state.proof.lock().await;
            guard.formal.source = "theorem foo".to_string();
            guard.goal_state.full = "⊢ True".to_string();
        }
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
    async fn should_generate_aborts_when_goal_state_hash_unchanged() {
        let state = make_state();
        state.prose_generation_seq.store(1, Ordering::SeqCst);
        let goal_state = "⊢ True".to_string();
        {
            let mut guard = state.proof.lock().await;
            guard.formal.source = "theorem foo : True := trivial".to_string();
            guard.goal_state.full = goal_state.clone();
            guard.prose.goal_state_hash = proof::compute_source_hash(&goal_state);
        }

        assert!(matches!(
            should_generate_prose(&state, 1).await,
            ShouldGenerate::Abort
        ));
    }

    #[tokio::test]
    async fn should_generate_aborts_when_goal_state_is_whitespace_only() {
        let state = make_state();
        state.prose_generation_seq.store(1, Ordering::SeqCst);
        {
            let mut guard = state.proof.lock().await;
            guard.formal.source = "theorem foo : True := trivial".to_string();
            guard.goal_state.full = "   \n\t  ".to_string();
        }

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
        let goal_state = "⊢ True".to_string();
        {
            let mut guard = state.proof.lock().await;
            guard.formal.source = source.clone();
            guard.goal_state.full = goal_state.clone();
        }

        match should_generate_prose(&state, 3).await {
            ShouldGenerate::Proceed { source: s, hash } => {
                assert_eq!(s, source);
                assert_eq!(hash, proof::compute_source_hash(&goal_state));
            }
            ShouldGenerate::Abort => panic!("expected Proceed"),
        }
    }

    #[tokio::test]
    async fn should_generate_proceeds_with_empty_source_but_non_empty_goal_state() {
        // Formal source may be empty while the goal state is non-empty (e.g. the
        // LLM cleared the editor but LSP hasn't caught up). We still proceed
        // so the translator can handle it; the LLM call itself may return empty.
        let state = make_state();
        state.prose_generation_seq.store(1, Ordering::SeqCst);
        {
            let mut guard = state.proof.lock().await;
            guard.formal.source = String::new();
            guard.goal_state.full = "⊢ True".to_string();
        }

        assert!(matches!(
            should_generate_prose(&state, 1).await,
            ShouldGenerate::Proceed { source, .. } if source.is_empty()
        ));
    }

    #[tokio::test]
    async fn stale_prose_seq_aborts_even_with_changed_goal_state() {
        // Verifies that a prose regen task with a stale seq number aborts
        // immediately even if the goal state hash has changed — ensures that
        // rapid goal-state changes don't result in concurrent LLM calls.
        let state = make_state();
        state.prose_generation_seq.store(5, Ordering::SeqCst);

        assert!(matches!(
            should_generate_prose(&state, 3).await,
            ShouldGenerate::Abort
        ));
    }
}
