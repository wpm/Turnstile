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
mod lean;
pub mod llm;
pub mod menu;
pub mod proof;
pub mod secret;
pub mod session;
pub mod settings;
mod setup;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use lean::client::Client as LeanClient;
use lean::messages::turnstile::{
    emit_turnstile, AssistantStatus, DisconnectReason, GoalStateInfo, HoverInfo, LspStatus,
    TurnstileMessage,
};
use lean::messages::DisplayLspParams;
use tauri::{AppHandle, Emitter, Manager};
use tracing::debug;

pub struct AppState {
    pub lsp_client: Arc<tokio::sync::Mutex<Option<LeanClient>>>,
    /// The proof currently being developed — formal + prose + goal state.
    pub proof: Arc<tokio::sync::Mutex<proof::Proof>>,
    /// Assistant conversation state.
    pub transcript: Arc<tokio::sync::Mutex<assistant::Transcript>>,
    /// LLM backend (the Anthropic API client).
    pub llm: Arc<dyn llm::Llm>,
    /// Persisted user settings.
    pub settings: Arc<tokio::sync::Mutex<settings::Settings>>,
    /// Path of the currently open `.turn` file (None if unsaved).
    pub current_session_path: Arc<tokio::sync::Mutex<Option<PathBuf>>>,
    /// Whether the session has unsaved changes.
    pub session_dirty: Arc<AtomicBool>,
    /// Latest LSP diagnostics for the Lean source file.
    pub current_diagnostics: Arc<Mutex<Vec<lean::messages::turnstile::DiagnosticInfo>>>,
    /// Latest LSP status emitted to the frontend. Recorded on every emit so a
    /// late-subscribing webview can recover it via `get_lsp_status`, closing the
    /// startup race where `start_lsp` emits "connected" before the frontend's
    /// `turnstile-message` listener is registered.
    pub last_lsp_status: Arc<Mutex<Option<LspStatus>>>,
    /// Latest Proof Assistant connection status emitted to the frontend.
    /// Recorded on every emit so a late-subscribing webview can recover it via
    /// `get_assistant_status`, mirroring `last_lsp_status`.
    pub last_assistant_status: Arc<Mutex<Option<AssistantStatus>>>,
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

fn dispatch_turnstile_message(app: &AppHandle, msg: TurnstileMessage) {
    use std::sync::atomic::Ordering;

    match msg {
        TurnstileMessage::Diagnostics { items: diagnostics } => {
            let state = app.state::<AppState>();
            state
                .current_diagnostics
                .lock()
                .unwrap()
                .clone_from(&diagnostics);
            let proof = state.proof.clone();
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                let items = {
                    let mut guard = proof.lock().await;
                    guard.annotations.set_diagnostics(&diagnostics);
                    guard.annotations.items.clone()
                };
                emit_turnstile(&app, &TurnstileMessage::AnnotationsUpdated { items });
            });
        }
        TurnstileMessage::ElaborationDone => {
            let state = app.state::<AppState>();
            let seq = state.goal_state_seq.fetch_add(1, Ordering::SeqCst) + 1;
            spawn_goal_state_refresh(app.clone(), seq);
            spawn_semantic_token_refresh(app.clone());
            emit_turnstile(app, &TurnstileMessage::ElaborationDone);
        }
        TurnstileMessage::SemanticTokenRefresh => {
            spawn_semantic_token_refresh(app.clone());
        }
        TurnstileMessage::LspStatus(status) => {
            emit_lsp_status(app, status);
        }
        TurnstileMessage::AssistantStatus(status) => {
            emit_assistant_status(app, status);
        }
        msg @ (TurnstileMessage::FileProgress { .. }
        | TurnstileMessage::ShowMessage { .. }
        | TurnstileMessage::AnnotationsUpdated { .. }
        | TurnstileMessage::SemanticTokens { .. }
        | TurnstileMessage::GoalStateUpdated(_)) => {
            emit_turnstile(app, &msg);
        }
    }
}

/// Record the latest LSP status in app state, then emit it to the frontend.
/// Recording lets a late-subscribing webview recover the status via the
/// `get_lsp_status` command, so a "connected" emitted before the frontend's
/// listener is registered is not lost.
fn emit_lsp_status(app: &AppHandle, status: LspStatus) {
    app.state::<AppState>()
        .last_lsp_status
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .replace(status.clone());
    emit_turnstile(app, &TurnstileMessage::LspStatus(status));
}

/// Record the latest Proof Assistant status in app state, then emit it to the
/// frontend. Recording lets a late-subscribing webview recover the status via
/// `get_assistant_status`, mirroring [`emit_lsp_status`].
fn emit_assistant_status(app: &AppHandle, status: AssistantStatus) {
    app.state::<AppState>()
        .last_assistant_status
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .replace(status.clone());
    emit_turnstile(app, &TurnstileMessage::AssistantStatus(status));
}

/// Flip the Proof Assistant to `Disconnected` with the given reason and emit it.
/// Called from the send path when a backend failure implicates the key.
pub fn set_assistant_disconnected(app: &AppHandle, reason: DisconnectReason) {
    emit_assistant_status(app, AssistantStatus::Disconnected { reason });
}

async fn start_lsp(app: AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();

    let lean_bin = setup::lean_bin();

    emit_lsp_status(
        &app,
        LspStatus {
            state: String::new(),
            message: format!("initializing ({})...", lean_bin.display()),
        },
    );

    // Open the document with whatever source the app already holds (e.g. a
    // session restored before the LSP came up) so server and editor agree.
    let initial_source = state.proof.lock().await.formal.source.clone();

    let app_for_dispatch = app.clone();
    let client = LeanClient::start(&initial_source, move |msg| {
        dispatch_turnstile_message(&app_for_dispatch, msg);
    })
    .await
    .map_err(|e| format!("LSP start failed: {e}"))?;

    let (types, modifiers) = client.token_legend();
    debug!("LSP token legend: types={types:?}");
    {
        *state.token_types.lock().unwrap() = types;
        *state.token_modifiers.lock().unwrap() = modifiers;
    }

    *state.lsp_client.lock().await = Some(client);

    emit_lsp_status(
        &app,
        LspStatus {
            state: "connected".to_string(),
            message: "connected".to_string(),
        },
    );

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
    let decoded =
        lean::messages::turnstile::decode_semantic_tokens(&tokens.data, &type_guard, &mod_guard);
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
        emit_turnstile(&app_handle, &TurnstileMessage::AnnotationsUpdated { items });
    });
}

#[tauri::command]
#[tracing::instrument(level = "debug", skip_all)]
async fn update_document(app: AppHandle, changes: Vec<ContentChange>) -> Result<(), String> {
    use lsp_types::TextDocumentContentChangeEvent;

    let state = app.state::<AppState>();

    // Apply incremental changes to the stored source so `read_lean_source` stays in sync.
    let source_is_empty = {
        let mut proof = state.proof.lock().await;
        apply_content_changes(&mut proof.formal.source, &changes);
        proof.formal.source.trim().is_empty()
    };

    // Bump the goal-state sequence so any in-flight refresh from a prior edit
    // is invalidated and won't repopulate the panels.
    let goal_seq = state.goal_state_seq.fetch_add(1, Ordering::SeqCst) + 1;

    if source_is_empty {
        // The proof was deleted: the goal state and prose have nothing to
        // describe. Elaborating an empty document produces no goal state, and
        // the refresh paths deliberately don't overwrite with an empty value
        // (they can't tell "still elaborating" from "empty"), so they would
        // leave the stale panels on screen. Clear both explicitly instead of
        // scheduling a refresh. The didChange below still goes out so the
        // server's view stays in sync.
        clear_goal_state_and_prose(&app, &state).await;
    } else {
        // Schedule a fresh goal-state refresh. The fileProgress-based refresh
        // handles the post-elaboration case; this also covers edits where Lean
        // sends no fileProgress (e.g. a paste into an idle, empty editor).
        spawn_goal_state_refresh(app.clone(), goal_seq);
    }

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
            // change_document bumps the Protocol version counter — the same
            // counter the staleness filters compare against — and sends the
            // didChange. Routing the version through Protocol is what makes
            // stale fileProgress/diagnostics notifications detectable.
            client
                .change_document(content_changes)
                .map_err(|e| e.to_string())?;
        }
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
    let cur_seq = state.prose_generation_seq.load(Ordering::SeqCst);
    if cur_seq != seq {
        debug!("prose: abort — stale seq (have {seq}, current {cur_seq})");
        return ShouldGenerate::Abort;
    }

    // Don't translate a broken proof.
    let has_errors = {
        let diags = state.current_diagnostics.lock().unwrap();
        diags.iter().any(|d| d.severity == 1)
    };
    if has_errors {
        debug!("prose: abort — diagnostics contain an error");
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
        debug!("prose: abort — goal-state hash unchanged ({hash})");
        return ShouldGenerate::Abort;
    }

    // Nothing to translate.
    if goal_state_text.trim().is_empty() {
        debug!("prose: abort — goal state is empty");
        return ShouldGenerate::Abort;
    }

    debug!("prose: proceed — goal state changed, generating");
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
            debug!("prose: calling run_translator (seq {seq})");
            // Drive the Prose Proof busy indicator for the duration of the LLM
            // call. The `false` below fires on every outcome (Ok, Err, or a
            // newer edit superseding this task) because it sits before any
            // early return.
            app.emit(proof::PROSE_GENERATING_EVENT, true).ok();
            let result = proof::translator::run_translator(backend.as_ref(), &source, &app).await;
            app.emit(proof::PROSE_GENERATING_EVENT, false).ok();
            match &result {
                Ok(raw) => debug!("prose: run_translator returned Ok ({} chars)", raw.len()),
                Err(e) => debug!("prose: run_translator returned Err: {e}"),
            }

            // Discard the result if a newer edit superseded us during the
            // (potentially long) LLM call.
            if state.prose_generation_seq.load(Ordering::SeqCst) != seq {
                debug!("prose: superseded after generation (seq {seq}), discarding");
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

/// Fetch the goal state for the current document.
///
/// Tries `$/lean/plainGoal` at end-of-document first (works for open tactic
/// goals). If that returns nothing (closed or term-mode proof), it falls back to
/// `$/lean/plainTermGoal` at the last non-whitespace character, which returns
/// the expected type at that position.
#[allow(clippy::significant_drop_tightening)] // lock must be held while awaiting on client
async fn fetch_full_proof_goal_state(state: &AppState) -> Result<String, String> {
    use lsp_types::{Position, TextDocumentIdentifier, TextDocumentPositionParams};

    let source = state.proof.lock().await.formal.source.clone();
    let (end_line, end_col) = end_of_document_position(&source);

    // Clone a lock-free handle and drop the mutex guard before issuing any
    // request. Holding `lsp_client` across `plain_goal`/`plain_term_goal` would
    // deadlock every other LSP call (e.g. a paste's didChange) if the query
    // stalls mid-elaboration.
    let handle = {
        let lock = state.lsp_client.lock().await;
        let Some(client) = lock.as_ref() else {
            return Err("LSP not connected".to_string());
        };
        client.handle()
    };

    let doc_uri = handle.proof_uri.clone();

    let end_params = TextDocumentPositionParams {
        text_document: TextDocumentIdentifier {
            uri: doc_uri.clone(),
        },
        position: Position {
            line: end_line,
            character: end_col,
        },
    };

    let tactic = handle
        .plain_goal(end_params)
        .await
        .map_err(|e| e.to_string())?;

    if let Some(g) = tactic {
        return Ok(g.rendered);
    }

    // plainGoal returns nothing for closed/term-mode proofs. Try plainTermGoal
    // at the last non-whitespace character — that position is inside the proof
    // body where Lean can report the expected type.
    let (term_line, term_col) = last_non_whitespace_position(&source);
    let term_params = TextDocumentPositionParams {
        text_document: TextDocumentIdentifier { uri: doc_uri },
        position: Position {
            line: term_line,
            character: term_col,
        },
    };

    let term = handle
        .plain_term_goal(term_params)
        .await
        .map_err(|e| e.to_string())?;

    Ok(term.map(|g| g.goal).unwrap_or_default())
}

/// Return the (line, character) LSP position of the last non-whitespace
/// character in `source`, or (0, 0) if the source is empty/whitespace-only.
/// Both values are 0-indexed.
fn last_non_whitespace_position(source: &str) -> (u32, u32) {
    let lines: Vec<&str> = source.split('\n').collect();
    for (line_idx, line) in lines.iter().enumerate().rev() {
        let trimmed = line.trim_end();
        if !trimmed.is_empty() {
            let col = trimmed.chars().count().saturating_sub(1);
            return (
                u32::try_from(line_idx).unwrap_or(u32::MAX),
                u32::try_from(col).unwrap_or(u32::MAX),
            );
        }
    }
    (0, 0)
}

/// Clear the goal state and prose proof, persist the cleared values, and emit
/// both to the frontend so the panels empty out. Called when the formal proof
/// is deleted — there is nothing left to describe.
///
/// Bumps `prose_generation_seq` first so any in-flight prose generation is
/// discarded and cannot repopulate the panel after this clear.
async fn clear_goal_state_and_prose(app: &AppHandle, state: &AppState) {
    state.prose_generation_seq.fetch_add(1, Ordering::SeqCst);

    state.proof.lock().await.clear_derived();
    state.prose_dirty.store(false, Ordering::SeqCst);

    emit_turnstile(
        app,
        &TurnstileMessage::GoalStateUpdated(GoalStateInfo {
            full: String::new(),
        }),
    );
    app.emit(
        proof::PROSE_UPDATED_EVENT,
        &proof::ProsePayload {
            text: String::new(),
            hash: None,
        },
    )
    .ok();

    state.session_dirty.store(true, Ordering::SeqCst);
}

/// Spawn a background task that, after a short debounce, fetches the
/// whole-proof goal state and emits a [`TurnstileMessage::GoalStateUpdated`] to
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

        // If Lean returned nothing (e.g. still elaborating or idle timeout),
        // don't overwrite the existing goal state with an empty string.
        if full.trim().is_empty() {
            return;
        }

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

        let full_nonempty = !full.trim().is_empty();
        emit_turnstile(
            &app,
            &TurnstileMessage::GoalStateUpdated(GoalStateInfo { full }),
        );

        debug!(
            "goal-state refresh: full={:?} last_hash={last_hash} new_hash={new_hash} \
             trigger_prose={}",
            full_nonempty,
            last_hash != new_hash && full_nonempty
        );
        if last_hash != new_hash && full_nonempty {
            state.prose_dirty.store(true, Ordering::SeqCst);
            let prose_seq = state.prose_generation_seq.fetch_add(1, Ordering::SeqCst) + 1;
            debug!("prose: scheduling regeneration with seq {prose_seq}");
            spawn_prose_regeneration(app.clone(), prose_seq);
        }
    });
}

/// Spawn a background task that requests a fresh set of semantic tokens from
/// Lean and applies them to the editor via [`apply_semantic_tokens`].
///
/// Called after `$/lean/fileProgress` signals elaboration is complete, at which
/// point Lean has a full token set for the file.
fn spawn_semantic_token_refresh(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        use lsp_types::{SemanticTokensParams, TextDocumentIdentifier};

        let state = app.state::<AppState>();
        // Clone a lock-free handle and drop the guard before requesting tokens.
        // Holding `lsp_client` across the request would block didChange/hover.
        let handle = {
            let lock = state.lsp_client.lock().await;
            match lock.as_ref() {
                Some(c) => c.handle(),
                None => return,
            }
        };
        let doc_uri = handle.proof_uri.clone();
        debug!("semantic token refresh: requesting tokens");
        let tokens_result = handle
            .semantic_tokens_full(SemanticTokensParams {
                text_document: TextDocumentIdentifier { uri: doc_uri },
                work_done_progress_params: lsp_types::WorkDoneProgressParams::default(),
                partial_result_params: lsp_types::PartialResultParams::default(),
            })
            .await
            .ok()
            .flatten();
        match tokens_result {
            Some(lsp_types::SemanticTokensResult::Tokens(tokens)) => {
                apply_semantic_tokens(&app, &tokens);
            }
            Some(other) => debug!(?other, "semantic tokens: unexpected result variant"),
            None => debug!("semantic tokens: no result"),
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
#[tracing::instrument(level = "debug", skip_all)]
async fn lsp_hover(app: AppHandle, line: u32, character: u32) -> Result<Option<HoverInfo>, String> {
    use lsp_types::{HoverParams, Position, TextDocumentIdentifier};

    let state = app.state::<AppState>();
    // Clone a lock-free handle and drop the guard before the hover request.
    let handle = {
        let lock = state.lsp_client.lock().await;
        match lock.as_ref() {
            Some(client) => client.handle(),
            None => return Ok(None),
        }
    };
    let doc_uri = handle.proof_uri.clone();
    let raw = handle
        .hover(HoverParams {
            text_document_position_params: lsp_types::TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: doc_uri },
                position: Position { line, character },
            },
            work_done_progress_params: lsp_types::WorkDoneProgressParams::default(),
        })
        .await
        .map_err(|e| e.to_string())?;
    Ok(lean::messages::turnstile::parse_hover(raw))
}

/// Return the most recently emitted LSP status, if any. The frontend calls
/// this once after registering its `turnstile-message` listener to recover a
/// status it may have missed during startup.
// Tauri commands must take `State` by value; clippy can't see that.
#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
fn get_lsp_status(state: tauri::State<'_, AppState>) -> Option<LspStatus> {
    state
        .last_lsp_status
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
}

/// Return the most recently emitted Proof Assistant status, if any. The
/// frontend calls this on mount to recover a status it may have missed during
/// startup, mirroring `get_lsp_status`.
// Tauri commands must take `State` by value; clippy can't see that.
#[allow(clippy::needless_pass_by_value)]
#[tauri::command]
fn get_assistant_status(state: tauri::State<'_, AppState>) -> Option<AssistantStatus> {
    state
        .last_assistant_status
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
}

/// Resolve the API key for the backend and the initial [`AssistantStatus`].
///
/// Resolution order matches [`secret::resolve_api_key`]: Turnstile's keychain
/// entry first, then the `ANTHROPIC_API_KEY` env var. The returned key string is
/// empty when none is available (the backend then refuses to call the API). The
/// status distinguishes a missing key from an unavailable secret store so the
/// toast (#60) can name the right fix. A present key is reported `Connected`
/// optimistically; an actual API auth rejection downgrades it to
/// `Disconnected { KeyRejected }` at send time.
fn resolve_key_and_status() -> (String, AssistantStatus) {
    // Probe the secret store directly so we can tell "no key" from "store
    // unavailable". A store error doesn't abort — we still honor the env var.
    let store_unavailable = matches!(
        secret::read_api_key(),
        Err(secret::SecretError::StoreUnavailable(_))
    );

    if let Some(key) = secret::resolve_api_key() {
        if !key.is_empty() {
            return (key.expose().to_string(), AssistantStatus::Connected);
        }
    }

    let reason = if store_unavailable {
        DisconnectReason::StoreUnavailable
    } else {
        DisconnectReason::NoKey
    };
    (String::new(), AssistantStatus::Disconnected { reason })
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
/// # Panics
///
/// Panics if the Tauri application fails to build or run.
#[allow(clippy::too_many_lines)]
pub fn run() {
    use assistant::{get_transcript, load_transcript, send_message};
    use llm::get_available_models;
    use menu::set_menu_item_enabled;
    use session::{
        auto_save_session, check_auto_save, delete_auto_save, get_last_session, new_session,
        open_session, restore_auto_save, save_session, save_session_as, set_last_session,
        set_window_title,
    };
    use settings::{
        get_default_assistant_prompt, get_default_translation_prompt, get_settings, save_settings,
    };

    init_tracing();
    tracing_log::LogTracer::init().ok();

    // GUI launches from Finder/Dock don't inherit the login shell's PATH, so the
    // elan/lake/lean tooling we spawn during setup and for the LSP can't be
    // found. Pull the real PATH from the user's shell config before anything is
    // spawned. Log and continue on failure — a missing PATH fix is recoverable
    // (setup still prepends elan's bin dir to whatever PATH is present).
    if let Err(e) = fix_path_env::fix() {
        log::warn!("fix_path_env::fix() failed; PATH may be incomplete for GUI launches: {e}");
    }

    tauri::Builder::default()
        .setup(|app| {
            let app_data_dir = app
                .path()
                .app_data_dir()
                .expect("Failed to resolve app data directory");

            let project_path = app_data_dir.join("lean-project");

            let initial_settings = settings::load_settings(&app_data_dir);

            // Resolve the API key from the OS secret store (keychain first, then
            // the ANTHROPIC_API_KEY env var) and derive the initial assistant
            // status. When no key is available the backend is built with an
            // empty key, which reports the missing-key error on use rather than
            // echoing — and we mark the assistant Disconnected.
            let (api_key, initial_assistant_status) = resolve_key_and_status();
            let llm_backend: Arc<dyn llm::Llm> = Arc::new(llm::AnthropicBackend::new(api_key));

            let setup_running = Arc::new(AtomicBool::new(false));

            app.manage(AppState {
                lsp_client: Arc::new(tokio::sync::Mutex::new(None)),
                proof: Arc::new(tokio::sync::Mutex::new(proof::Proof::default())),
                transcript: Arc::new(tokio::sync::Mutex::new(assistant::Transcript::default())),
                llm: llm_backend,
                settings: Arc::new(tokio::sync::Mutex::new(initial_settings)),
                current_session_path: Arc::new(tokio::sync::Mutex::new(None)),
                session_dirty: Arc::new(AtomicBool::new(false)),
                current_diagnostics: Arc::new(Mutex::new(Vec::new())),
                last_lsp_status: Arc::new(Mutex::new(None)),
                last_assistant_status: Arc::new(Mutex::new(None)),
                prose_dirty: Arc::new(AtomicBool::new(false)),
                prose_generation_seq: Arc::new(AtomicU64::new(0)),
                goal_state_seq: Arc::new(AtomicU64::new(0)),
                token_types: Arc::new(Mutex::new(Vec::new())),
                token_modifiers: Arc::new(Mutex::new(Vec::new())),
            });

            // Emit the assistant status once at startup so the indicator (#59)
            // and toast (#60) reflect reality before the user does anything.
            emit_assistant_status(app.handle(), initial_assistant_status);

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
                        let params = DidCloseTextDocumentParams {
                            text_document: TextDocumentIdentifier {
                                uri: client.proof_uri.clone(),
                            },
                        };
                        debug!("{}", params.display());
                        client.did_close(params).ok();
                    }
                });
            }
        })
        .invoke_handler(tauri::generate_handler![
            update_document,
            lsp_hover,
            get_lsp_status,
            get_assistant_status,
            send_message,
            get_transcript,
            load_transcript,
            get_settings,
            save_settings,
            get_default_assistant_prompt,
            get_default_translation_prompt,
            get_available_models,
            new_session,
            open_session,
            save_session,
            save_session_as,
            auto_save_session,
            check_auto_save,
            restore_auto_save,
            delete_auto_save,
            get_last_session,
            set_last_session,
            set_window_title,
            set_menu_item_enabled,
            secret::set_api_key,
            secret::has_api_key,
            secret::clear_api_key,
        ])
        .run(tauri::generate_context!())
        .expect("error while running turnstile");
}

pub fn init_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,async_lsp=off")),
        )
        .with_span_events(tracing_subscriber::fmt::format::FmtSpan::NEW)
        .with_target(false)
        .with_file(true)
        .with_line_number(true)
        .with_writer(std::io::stderr)
        .try_init();
}

#[macro_export]
macro_rules! init_tracing {
    () => {
        $crate::init_tracing()
    };
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::Mutex;

    use super::{
        apply_content_changes, assistant, end_of_document_position, last_non_whitespace_position,
        llm, lsp_pos_to_offset, proof, should_generate_prose, AppState, ContentChange, LspPosition,
        LspRange, ShouldGenerate,
    };
    use crate::lean;
    use std::sync::Arc;

    #[test]
    fn end_of_document_position_basic() {
        assert_eq!(end_of_document_position(""), (0, 0));
        assert_eq!(end_of_document_position("abc"), (0, 3));
        assert_eq!(end_of_document_position("abc\n"), (1, 0));
        assert_eq!(end_of_document_position("abc\ndef"), (1, 3));
        assert_eq!(end_of_document_position("abc\ndef\n"), (2, 0));
        assert_eq!(end_of_document_position("abc\ndef\nghi"), (2, 3));
    }

    #[test]
    fn last_non_whitespace_position_finds_last_glyph() {
        // Empty / whitespace-only docs report the origin.
        assert_eq!(last_non_whitespace_position(""), (0, 0));
        assert_eq!(last_non_whitespace_position("   \n\t  "), (0, 0));
        // Single line: column of the last non-space char (0-indexed).
        assert_eq!(last_non_whitespace_position("abc"), (0, 2));
        // Trailing spaces on the line are ignored.
        assert_eq!(last_non_whitespace_position("abc   "), (0, 2));
        // Trailing blank lines are skipped; we land on the last content line.
        assert_eq!(last_non_whitespace_position("abc\ndef\n\n  "), (1, 2));
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
            proof: Arc::new(tokio::sync::Mutex::new(proof::Proof::default())),
            transcript: Arc::new(tokio::sync::Mutex::new(assistant::Transcript::default())),
            // The pre-flight checks under test never call the backend, so an
            // empty-key Anthropic backend is sufficient.
            llm: Arc::new(llm::AnthropicBackend::new(String::new())),
            settings: Arc::new(tokio::sync::Mutex::new(crate::settings::Settings::default())),
            current_session_path: Arc::new(tokio::sync::Mutex::new(None)),
            session_dirty: Arc::new(AtomicBool::new(false)),
            current_diagnostics: Arc::new(Mutex::new(Vec::new())),
            last_lsp_status: Arc::new(Mutex::new(None)),
            last_assistant_status: Arc::new(Mutex::new(None)),
            prose_dirty: Arc::new(AtomicBool::new(false)),
            prose_generation_seq: Arc::new(AtomicU64::new(0)),
            goal_state_seq: Arc::new(AtomicU64::new(0)),
            token_types: Arc::new(Mutex::new(Vec::new())),
            token_modifiers: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn error_diagnostic() -> lean::messages::turnstile::DiagnosticInfo {
        lean::messages::turnstile::DiagnosticInfo {
            start_line: 0,
            start_col: 0,
            end_line: 0,
            end_col: 1,
            severity: 1,
            message: "boom".to_string(),
        }
    }

    fn warning_diagnostic() -> lean::messages::turnstile::DiagnosticInfo {
        lean::messages::turnstile::DiagnosticInfo {
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
        // Verifies that a prose regeneration task with a stale seq number aborts
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
