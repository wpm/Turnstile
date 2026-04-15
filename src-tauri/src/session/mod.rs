//! Session persistence: read/write `.turn` files, plus the Tauri commands
//! that drive new / open / save / autosave.
//!
//! A `.turn` file is a ZIP archive containing:
//!
//! - `meta.json`       — format version, timestamps, UI state
//! - `proof.lean`      — Lean source from the editor buffer
//! - `prose.json`      — prose text and tactic state hash for staleness detection
//! - `transcript.json` — recent window of chat dialog turns
//! - `summary.txt`     — optional LLM-generated conversation summary
//!
//! # Format version
//!
//! `meta.json` carries a `format_version` integer. [`load`] rejects files
//! with an unknown version. The current version is [`FORMAT_VERSION`].
//!
//! # Autosave lifecycle
//!
//! The autosave file (`{app_data_dir}/autosave.turn`) represents
//! uncommitted work from a prior session. It is created by
//! `auto_save_session` only when the session is dirty, and it is deleted
//! whenever that uncommitted work is either promoted to a saved session
//! (via `save_session` or `save_session_as`) or explicitly discarded or
//! restored by the user. Without those deletions, a stale autosave would
//! trigger a false-positive "Restore unsaved session?" prompt on the next
//! app launch.

use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_dialog::{DialogExt, FilePath};

use crate::AppState;

/// Current `.turn` format version. Increment when making breaking changes.
pub const FORMAT_VERSION: u32 = 1;

// ── Data types ────────────────────────────────────────────────────────

/// UI-restoration metadata stored in `meta.json`.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Meta {
    pub format_version: u32,
    /// ISO-8601 creation timestamp (set on first save, never updated).
    pub created_at: String,
    /// ISO-8601 last-saved timestamp.
    pub saved_at: String,
    /// Cursor position: 0-based line number.
    pub cursor_line: u32,
    /// Cursor position: 0-based column number.
    pub cursor_col: u32,
    /// Editor scroll offset in pixels.
    pub editor_scroll_top: f64,
    /// Chat/code split: chat panel width as a percentage (0–100).
    pub chat_width_pct: f64,
    /// Which proof view was active: `"formal"` or `"prose"`. Absent in older files.
    #[serde(default)]
    pub proof_view: Option<String>,
    /// Goal panel height as a percentage of the editor column. Absent in older files.
    #[serde(default)]
    pub goal_panel_pct: Option<f64>,
    /// Per-file word-wrap state for the editor. Absent means the editor default (off).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub word_wrap: Option<bool>,
}

/// Prose content with a tactic state hash for staleness detection.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct ProseData {
    /// LaTeX or plain-text prose content.
    pub text: String,
    /// Hash of the tactic state at the time of writing, for staleness detection.
    pub tactic_state_hash: Option<String>,
}

/// One turn in the chat transcript as serialized in the session file.
///
/// Uses string roles (as opposed to the `Role` enum in
/// [`crate::proof_assistant`]) so older session files remain parseable even
/// as the enum evolves.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct TranscriptTurn {
    /// `"user"` or `"assistant"`.
    pub role: String,
    /// Message content.
    pub content: String,
    /// Unix timestamp in milliseconds.
    pub timestamp: i64,
}

/// Complete session state persisted in a `.turn` file.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SessionState {
    pub meta: Meta,
    /// Lean source text.
    pub proof_lean: String,
    /// Prose annotation data.
    pub prose: ProseData,
    /// Chat turns in the current transcript window (may be empty).
    pub turns: Vec<TranscriptTurn>,
    /// LLM-generated summary of earlier conversation history, if present.
    pub summary: Option<String>,
}

impl SessionState {
    /// Return a blank session with sensible defaults.
    pub fn blank() -> Self {
        let now = current_timestamp();
        SessionState {
            meta: Meta {
                format_version: FORMAT_VERSION,
                created_at: now.clone(),
                saved_at: now,
                ..Default::default()
            },
            proof_lean: String::new(),
            prose: ProseData::default(),
            turns: Vec::new(),
            summary: None,
        }
    }
}

// ── ZIP I/O ───────────────────────────────────────────────────────────

/// Write `state` to a `.turn` ZIP archive at `path`.
///
/// Overwrites any existing file.
pub fn save(state: &SessionState, path: &Path) -> Result<(), String> {
    let buf = build_zip(state)?;
    std::fs::write(path, buf).map_err(|e| format!("Failed to write {}: {e}", path.display()))
}

/// Read a `.turn` ZIP archive from `path` and deserialize it.
///
/// Returns an error if:
/// - the file cannot be read or is not a valid ZIP,
/// - any required entry (`meta.json`, `proof.lean`, `prose.json`,
///   `transcript.json`) is missing or malformed,
/// - `meta.json` carries an unknown `format_version`.
pub fn load(path: &Path) -> Result<SessionState, String> {
    let bytes =
        std::fs::read(path).map_err(|e| format!("Failed to read {}: {e}", path.display()))?;
    parse_zip(&bytes)
}

// ── Internal helpers ──────────────────────────────────────────────────

fn current_timestamp() -> String {
    Utc::now().to_rfc3339()
}

pub(crate) fn build_zip(state: &SessionState) -> Result<Vec<u8>, String> {
    let buf = Vec::new();
    let cursor = Cursor::new(buf);
    let mut zip = zip::ZipWriter::new(cursor);

    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    // meta.json
    let meta_json = serde_json::to_string_pretty(&state.meta)
        .map_err(|e| format!("Failed to serialize meta: {e}"))?;
    zip.start_file("meta.json", options)
        .map_err(|e| format!("ZIP error: {e}"))?;
    zip.write_all(meta_json.as_bytes())
        .map_err(|e| format!("ZIP write error: {e}"))?;

    // proof.lean
    zip.start_file("proof.lean", options)
        .map_err(|e| format!("ZIP error: {e}"))?;
    zip.write_all(state.proof_lean.as_bytes())
        .map_err(|e| format!("ZIP write error: {e}"))?;

    // prose.json
    let prose_json = serde_json::to_string_pretty(&state.prose)
        .map_err(|e| format!("Failed to serialize prose: {e}"))?;
    zip.start_file("prose.json", options)
        .map_err(|e| format!("ZIP error: {e}"))?;
    zip.write_all(prose_json.as_bytes())
        .map_err(|e| format!("ZIP write error: {e}"))?;

    // transcript.json
    let transcript_json = serde_json::to_string_pretty(&state.turns)
        .map_err(|e| format!("Failed to serialize transcript: {e}"))?;
    zip.start_file("transcript.json", options)
        .map_err(|e| format!("ZIP error: {e}"))?;
    zip.write_all(transcript_json.as_bytes())
        .map_err(|e| format!("ZIP write error: {e}"))?;

    // summary.txt (optional)
    if let Some(ref summary) = state.summary {
        zip.start_file("summary.txt", options)
            .map_err(|e| format!("ZIP error: {e}"))?;
        zip.write_all(summary.as_bytes())
            .map_err(|e| format!("ZIP write error: {e}"))?;
    }

    let cursor = zip.finish().map_err(|e| format!("ZIP finish error: {e}"))?;
    Ok(cursor.into_inner())
}

pub(crate) fn parse_zip(bytes: &[u8]) -> Result<SessionState, String> {
    let cursor = Cursor::new(bytes);
    let mut zip =
        zip::ZipArchive::new(cursor).map_err(|e| format!("Not a valid ZIP archive: {e}"))?;

    // meta.json — required
    let meta: Meta = {
        let mut file = zip
            .by_name("meta.json")
            .map_err(|_| "Missing meta.json in archive".to_string())?;
        let mut s = String::new();
        file.read_to_string(&mut s)
            .map_err(|e| format!("Failed to read meta.json: {e}"))?;
        serde_json::from_str(&s).map_err(|e| format!("Failed to parse meta.json: {e}"))?
    };

    if meta.format_version != FORMAT_VERSION {
        return Err(format!(
            "Unknown format version {} (expected {FORMAT_VERSION})",
            meta.format_version
        ));
    }

    // proof.lean — required
    let proof_lean = {
        let mut file = zip
            .by_name("proof.lean")
            .map_err(|_| "Missing proof.lean in archive".to_string())?;
        let mut s = String::new();
        file.read_to_string(&mut s)
            .map_err(|e| format!("Failed to read proof.lean: {e}"))?;
        s
    };

    // prose.json — required
    let prose: ProseData = {
        let mut file = zip
            .by_name("prose.json")
            .map_err(|_| "Missing prose.json in archive".to_string())?;
        let mut s = String::new();
        file.read_to_string(&mut s)
            .map_err(|e| format!("Failed to read prose.json: {e}"))?;
        serde_json::from_str(&s).map_err(|e| format!("Failed to parse prose.json: {e}"))?
    };

    // transcript.json — required (may be empty array)
    let turns: Vec<TranscriptTurn> = {
        let mut file = zip
            .by_name("transcript.json")
            .map_err(|_| "Missing transcript.json in archive".to_string())?;
        let mut s = String::new();
        file.read_to_string(&mut s)
            .map_err(|e| format!("Failed to read transcript.json: {e}"))?;
        serde_json::from_str(&s).map_err(|e| format!("Failed to parse transcript.json: {e}"))?
    };

    // summary.txt — optional
    let summary = match zip.by_name("summary.txt") {
        Ok(mut file) => {
            let mut s = String::new();
            file.read_to_string(&mut s)
                .map_err(|e| format!("Failed to read summary.txt: {e}"))?;
            Some(s)
        }
        Err(_) => None,
    };

    Ok(SessionState {
        meta,
        proof_lean,
        prose,
        turns,
        summary,
    })
}

// ── Command helpers ───────────────────────────────────────────────────

fn app_data_dir(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map_err(|e| format!("Failed to resolve app data directory: {e}"))
}

fn autosave_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app_data_dir(app)?.join("autosave.turn"))
}

/// Delete the autosave file at `path` if it exists. A missing file is not
/// an error — this is idempotent so callers can invoke it unconditionally
/// after any save/open/discard without having to check first.
fn remove_autosave_file(path: &Path) -> Result<(), String> {
    if path.exists() {
        std::fs::remove_file(path).map_err(|e| format!("Failed to delete autosave: {e}"))?;
    }
    Ok(())
}

/// Emit a `session-loaded` event so the frontend can restore UI state.
fn emit_session_loaded(app: &AppHandle, state: &SessionState) {
    app.emit("session-loaded", state).ok();
}

/// Read the last-opened session path from `{dir}/last_session.txt`.
fn read_last_session(dir: &Path) -> Option<String> {
    let path = dir.join("last_session.txt");
    std::fs::read_to_string(&path).ok().and_then(|s| {
        let trimmed = s.trim().to_string();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    })
}

/// Write the last-opened session path to `{dir}/last_session.txt`.
fn write_last_session(dir: &Path, session_path: &Path) -> Result<(), String> {
    let txt_path = dir.join("last_session.txt");
    std::fs::write(&txt_path, session_path.to_string_lossy().as_ref())
        .map_err(|e| format!("Failed to write last_session.txt: {e}"))
}

// ── Commands ──────────────────────────────────────────────────────────

/// Reset all session state and clear the current session path.
#[tauri::command]
pub async fn new_session(app: AppHandle, state: tauri::State<'_, AppState>) -> Result<(), String> {
    // Clear session path and dirty flag.
    *state.current_session_path.lock().await = None;
    state.session_dirty.store(false, Ordering::SeqCst);

    // Reset transcript and proof.
    *state.transcript.lock().await = crate::proof_assistant::Transcript::default();
    *state.proof.lock().await = crate::proof::Proof::default();

    // Emit event so frontend can clear editor / prose.
    let blank = SessionState::blank();
    emit_session_loaded(&app, &blank);

    Ok(())
}

/// Load `session` into app state and emit `session-loaded`. Shared by
/// `open_session` (saved `.turn` file) and `restore_auto_save` (anonymous
/// autosave). `session_path` is the path to record as the current session
/// file, or `None` if the loaded session has no associated file yet.
async fn apply_loaded_session(
    app: &AppHandle,
    state: &AppState,
    session: &SessionState,
    session_path: Option<PathBuf>,
) {
    // Restore transcript.
    {
        let mut transcript = state.transcript.lock().await;
        transcript.summary = session.summary.clone();
        transcript.turns = session
            .turns
            .iter()
            .map(|t| crate::proof_assistant::Turn {
                role: match t.role.as_str() {
                    "assistant" => crate::proof_assistant::Role::Assistant,
                    "system" => crate::proof_assistant::Role::System,
                    _ => crate::proof_assistant::Role::User,
                },
                content: t.content.clone(),
                timestamp: t.timestamp,
            })
            .collect();
    }

    // Restore proof (formal + prose; goal state will refresh from LSP).
    {
        let mut proof = state.proof.lock().await;
        proof.formal.source = session.proof_lean.clone();
        proof.prose.text = session.prose.text.clone();
        proof.prose.source_hash = session.prose.tactic_state_hash.clone().unwrap_or_default();
        proof.goal_state = crate::proof::GoalState::default();
    }

    // Update session path and clear dirty flag.
    *state.current_session_path.lock().await = session_path;
    state.session_dirty.store(false, Ordering::SeqCst);

    // Emit so frontend can restore editor/prose UI.
    emit_session_loaded(app, session);
}

/// Open a `.turn` file. If `path` is `None`, shows a native file picker.
#[tauri::command]
pub async fn open_session(
    app: AppHandle,
    path: Option<String>,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let file_path: PathBuf = match path {
        Some(p) => PathBuf::from(p),
        None => {
            // Show native open-file dialog (blocking on a background thread via oneshot).
            let (tx, rx) = tokio::sync::oneshot::channel::<Option<PathBuf>>();
            app.dialog()
                .file()
                .add_filter("Turnstile session", &["turn"])
                .pick_file(move |result| {
                    let resolved = result.and_then(|fp| match fp {
                        FilePath::Path(p) => Some(p),
                        _ => None,
                    });
                    let _ = tx.send(resolved);
                });
            match rx.await.map_err(|_| "Dialog cancelled".to_string())? {
                Some(p) => p,
                None => return Ok(()), // user cancelled
            }
        }
    };

    let session = load(&file_path)?;
    apply_loaded_session(&app, &state, &session, Some(file_path)).await;

    Ok(())
}

/// Load `autosave.turn` back into the session as an anonymous recovery
/// draft, then delete the autosave file. The restored session has no
/// associated `.turn` path — a subsequent Save will prompt for a filename.
///
/// Returns an error if no autosave file exists or if it fails to parse.
#[tauri::command]
pub async fn restore_auto_save(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let path = autosave_path(&app)?;
    if !path.exists() {
        return Err("No autosave file to restore".to_string());
    }

    let session = load(&path)?;
    apply_loaded_session(&app, &state, &session, None).await;

    // Restoring the autosave consumes it — delete so we never offer the
    // same draft twice.
    remove_autosave_file(&path).ok();

    Ok(())
}

/// Save the current session to the current path. If no path is set, calls save_as.
#[tauri::command]
pub async fn save_session(
    app: AppHandle,
    proof_lean: String,
    prose_text: String,
    prose_hash: Option<String>,
    meta: Meta,
    suggested_name: Option<String>,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let path = state.current_session_path.lock().await.clone();
    match path {
        Some(p) => {
            do_save(&app, &state, &p, proof_lean, prose_text, prose_hash, meta).await?;
            if let Ok(data_dir) = app_data_dir(&app) {
                write_last_session(&data_dir, &p).ok();
            }
            // Saving commits the work — any prior autosave is now stale and
            // would otherwise trigger a false "Restore unsaved session?"
            // prompt on the next launch.
            if let Ok(auto_path) = autosave_path(&app) {
                remove_autosave_file(&auto_path).ok();
            }
        }
        None => {
            save_session_as(
                app,
                proof_lean,
                prose_text,
                prose_hash,
                meta,
                suggested_name,
                state,
            )
            .await?;
        }
    }
    Ok(())
}

/// Show a native save-file dialog and save the session.
#[tauri::command]
pub async fn save_session_as(
    app: AppHandle,
    proof_lean: String,
    prose_text: String,
    prose_hash: Option<String>,
    meta: Meta,
    suggested_name: Option<String>,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let filename = suggested_name
        .filter(|s| !s.is_empty())
        .map(|s| format!("{s}.turn"))
        .unwrap_or_else(|| "session.turn".to_string());

    let (tx, rx) = tokio::sync::oneshot::channel::<Option<PathBuf>>();
    app.dialog()
        .file()
        .add_filter("Turnstile session", &["turn"])
        .set_file_name(&filename)
        .save_file(move |result| {
            let resolved = result.and_then(|fp| match fp {
                FilePath::Path(p) => Some(p),
                _ => None,
            });
            let _ = tx.send(resolved);
        });

    let file_path = match rx.await.map_err(|_| "Dialog cancelled".to_string())? {
        Some(p) => p,
        None => return Ok(()), // user cancelled
    };

    do_save(
        &app, &state, &file_path, proof_lean, prose_text, prose_hash, meta,
    )
    .await?;
    *state.current_session_path.lock().await = Some(file_path.clone());
    if let Ok(data_dir) = app_data_dir(&app) {
        write_last_session(&data_dir, &file_path).ok();
    }
    // Saving commits the work — drop any prior autosave so the next launch
    // does not offer to restore the now-obsolete draft.
    if let Ok(auto_path) = autosave_path(&app) {
        remove_autosave_file(&auto_path).ok();
    }
    Ok(())
}

async fn do_save(
    _app: &AppHandle,
    state: &AppState,
    path: &Path,
    proof_lean: String,
    prose_text: String,
    prose_hash: Option<String>,
    meta: Meta,
) -> Result<(), String> {
    let transcript = state.transcript.lock().await;
    let turns = transcript
        .turns
        .iter()
        .map(|t| TranscriptTurn {
            role: match t.role {
                crate::proof_assistant::Role::User => "user".to_string(),
                crate::proof_assistant::Role::Assistant => "assistant".to_string(),
                crate::proof_assistant::Role::System => "system".to_string(),
            },
            content: t.content.clone(),
            timestamp: t.timestamp,
        })
        .collect();
    let summary = transcript.summary.clone();
    drop(transcript);

    // Preserve created_at from the passed meta; update saved_at.
    let mut final_meta = meta;
    final_meta.saved_at = Utc::now().to_rfc3339();
    if final_meta.created_at.is_empty() {
        final_meta.created_at = final_meta.saved_at.clone();
    }
    final_meta.format_version = FORMAT_VERSION;

    let session = SessionState {
        meta: final_meta,
        proof_lean,
        prose: ProseData {
            text: prose_text,
            tactic_state_hash: prose_hash,
        },
        turns,
        summary,
    };

    save(&session, path)?;
    state.session_dirty.store(false, Ordering::SeqCst);
    Ok(())
}

/// Write an autosave to `{app_data_dir}/autosave.turn` (only if session is dirty).
#[tauri::command]
pub async fn auto_save_session(
    app: AppHandle,
    proof_lean: String,
    prose_text: String,
    prose_hash: Option<String>,
    meta: Meta,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    if !state.session_dirty.load(Ordering::SeqCst) {
        return Ok(());
    }

    let path = autosave_path(&app)?;
    do_save(
        &app, &state, &path, proof_lean, prose_text, prose_hash, meta,
    )
    .await
}

/// Return `true` if `autosave.turn` exists in the app data directory.
#[tauri::command]
pub fn check_auto_save(app: AppHandle) -> Result<bool, String> {
    let path = autosave_path(&app)?;
    Ok(path.exists())
}

/// Delete `autosave.turn` if it exists.
#[tauri::command]
pub fn delete_auto_save(app: AppHandle) -> Result<(), String> {
    let path = autosave_path(&app)?;
    remove_autosave_file(&path)
}

/// Read `{app_data_dir}/last_session.txt` and return the path if it exists.
#[tauri::command]
pub fn get_last_session(app: AppHandle) -> Result<Option<String>, String> {
    let dir = app_data_dir(&app)?;
    Ok(read_last_session(&dir))
}

/// Write `path` to `{app_data_dir}/last_session.txt`.
#[tauri::command]
pub fn set_last_session(app: AppHandle, path: String) -> Result<(), String> {
    let dir = app_data_dir(&app)?;
    write_last_session(&dir, Path::new(&path))
}

/// Set the main window title (called from the frontend when the theorem name changes).
#[tauri::command]
pub fn set_window_title(app: AppHandle, title: String) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("main") {
        window
            .set_title(&title)
            .map_err(|e| format!("Failed to set window title: {e}"))?;
    }
    Ok(())
}

// ── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    fn sample_state() -> SessionState {
        SessionState {
            meta: Meta {
                format_version: FORMAT_VERSION,
                created_at: "2026-01-01T00:00:00Z".to_string(),
                saved_at: "2026-01-01T00:00:01Z".to_string(),
                cursor_line: 3,
                cursor_col: 7,
                editor_scroll_top: 120.5,
                chat_width_pct: 40.0,
                proof_view: None,
                goal_panel_pct: None,
                word_wrap: None,
            },
            proof_lean: "theorem foo : True := by\n  trivial\n".to_string(),
            prose: ProseData {
                text: "This proves True.".to_string(),
                tactic_state_hash: Some("abc123".to_string()),
            },
            turns: vec![
                TranscriptTurn {
                    role: "user".to_string(),
                    content: "What is rfl?".to_string(),
                    timestamp: 1_000_000,
                },
                TranscriptTurn {
                    role: "assistant".to_string(),
                    content: "rfl proves reflexivity.".to_string(),
                    timestamp: 1_000_001,
                },
            ],
            summary: Some("Earlier we discussed rfl.".to_string()),
        }
    }

    #[test]
    fn round_trip_full_state() -> TestResult {
        let original = sample_state();
        let tmp = NamedTempFile::new()?;

        save(&original, tmp.path())?;
        let loaded = load(tmp.path())?;

        assert_eq!(loaded, original);
        Ok(())
    }

    #[test]
    fn round_trip_without_summary() -> TestResult {
        let mut state = sample_state();
        state.summary = None;

        let tmp = NamedTempFile::new()?;
        save(&state, tmp.path())?;
        let loaded = load(tmp.path())?;

        assert_eq!(loaded, state);
        assert!(loaded.summary.is_none());
        Ok(())
    }

    #[test]
    fn round_trip_empty_transcript() -> TestResult {
        let mut state = sample_state();
        state.turns = Vec::new();

        let tmp = NamedTempFile::new()?;
        save(&state, tmp.path())?;
        let loaded = load(tmp.path())?;

        assert_eq!(loaded, state);
        assert!(loaded.turns.is_empty());
        Ok(())
    }

    #[test]
    fn round_trip_empty_proof() -> TestResult {
        let mut state = sample_state();
        state.proof_lean = String::new();

        let tmp = NamedTempFile::new()?;
        save(&state, tmp.path())?;
        let loaded = load(tmp.path())?;

        assert_eq!(loaded.proof_lean, "");
        Ok(())
    }

    #[test]
    fn round_trip_prose_without_hash() -> TestResult {
        let mut state = sample_state();
        state.prose = ProseData {
            text: "Some text".to_string(),
            tactic_state_hash: None,
        };

        let tmp = NamedTempFile::new()?;
        save(&state, tmp.path())?;
        let loaded = load(tmp.path())?;

        assert_eq!(loaded.prose.text, "Some text");
        assert!(loaded.prose.tactic_state_hash.is_none());
        Ok(())
    }

    #[test]
    fn round_trip_transcript_timestamps() -> TestResult {
        let state = sample_state();
        let tmp = NamedTempFile::new()?;

        save(&state, tmp.path())?;
        let loaded = load(tmp.path())?;

        assert_eq!(loaded.turns[0].timestamp, 1_000_000);
        assert_eq!(loaded.turns[1].timestamp, 1_000_001);
        Ok(())
    }

    #[test]
    fn format_version_rejection() -> TestResult {
        let mut state = sample_state();
        state.meta.format_version = 999;

        let bytes = build_zip(&state)?;
        let err = parse_zip(&bytes).unwrap_err();

        assert!(err.contains("Unknown format version 999"), "got: {err}");
        Ok(())
    }

    #[test]
    fn rejects_non_zip_bytes() {
        let garbage = b"this is not a ZIP file";
        let err = parse_zip(garbage).unwrap_err();
        assert!(err.contains("Not a valid ZIP archive"), "got: {err}");
    }

    #[test]
    fn rejects_zip_missing_meta() -> TestResult {
        let buf = Vec::new();
        let cursor = Cursor::new(buf);
        let mut zip = zip::ZipWriter::new(cursor);
        let opts = zip::write::SimpleFileOptions::default();
        zip.start_file("proof.lean", opts)?;
        zip.write_all(b"")?;
        let cursor = zip.finish()?;

        let err = parse_zip(&cursor.into_inner()).unwrap_err();
        assert!(err.contains("Missing meta.json"), "got: {err}");
        Ok(())
    }

    #[test]
    fn rejects_zip_missing_proof_lean() -> TestResult {
        let buf = Vec::new();
        let cursor = Cursor::new(buf);
        let mut zip = zip::ZipWriter::new(cursor);
        let opts = zip::write::SimpleFileOptions::default();
        let meta = Meta {
            format_version: FORMAT_VERSION,
            ..Default::default()
        };
        let meta_json = serde_json::to_string(&meta)?;
        zip.start_file("meta.json", opts)?;
        zip.write_all(meta_json.as_bytes())?;
        let cursor = zip.finish()?;

        let err = parse_zip(&cursor.into_inner()).unwrap_err();
        assert!(err.contains("Missing proof.lean"), "got: {err}");
        Ok(())
    }

    #[test]
    fn blank_session_has_format_version() {
        let state = SessionState::blank();
        assert_eq!(state.meta.format_version, FORMAT_VERSION);
        assert!(state.proof_lean.is_empty());
        assert!(state.prose.text.is_empty());
        assert!(state.prose.tactic_state_hash.is_none());
        assert!(state.turns.is_empty());
        assert!(state.summary.is_none());
    }

    #[test]
    fn proof_view_round_trip() -> TestResult {
        let mut state = sample_state();
        state.meta.proof_view = Some("prose".to_string());

        let bytes = build_zip(&state)?;
        let loaded = parse_zip(&bytes)?;

        assert_eq!(loaded.meta.proof_view, Some("prose".to_string()));
        Ok(())
    }

    #[test]
    fn proof_view_defaults_to_none() -> TestResult {
        let json = r#"{
            "format_version": 1,
            "created_at": "",
            "saved_at": "",
            "cursor_line": 0,
            "cursor_col": 0,
            "editor_scroll_top": 0.0,
            "chat_width_pct": 25.0
        }"#;
        let meta: Meta = serde_json::from_str(json)?;
        assert!(meta.proof_view.is_none());
        Ok(())
    }

    #[test]
    fn word_wrap_round_trip_some_true() -> TestResult {
        let mut state = sample_state();
        state.meta.word_wrap = Some(true);

        let bytes = build_zip(&state)?;
        let loaded = parse_zip(&bytes)?;

        assert_eq!(loaded.meta.word_wrap, Some(true));
        Ok(())
    }

    #[test]
    fn word_wrap_round_trip_some_false() -> TestResult {
        let mut state = sample_state();
        state.meta.word_wrap = Some(false);

        let bytes = build_zip(&state)?;
        let loaded = parse_zip(&bytes)?;

        assert_eq!(loaded.meta.word_wrap, Some(false));
        Ok(())
    }

    #[test]
    fn word_wrap_defaults_to_none() -> TestResult {
        let json = r#"{
            "format_version": 1,
            "created_at": "",
            "saved_at": "",
            "cursor_line": 0,
            "cursor_col": 0,
            "editor_scroll_top": 0.0,
            "chat_width_pct": 25.0
        }"#;
        let meta: Meta = serde_json::from_str(json)?;
        assert!(meta.word_wrap.is_none());
        Ok(())
    }

    #[test]
    fn word_wrap_none_is_not_serialized() -> TestResult {
        let mut state = sample_state();
        state.meta.word_wrap = None;
        let json = serde_json::to_string(&state.meta)?;
        assert!(!json.contains("word_wrap"), "got: {json}");
        Ok(())
    }

    #[test]
    fn meta_fields_preserved() -> TestResult {
        let original = sample_state();
        let bytes = build_zip(&original)?;
        let loaded = parse_zip(&bytes)?;

        assert_eq!(loaded.meta.cursor_line, 3);
        assert_eq!(loaded.meta.cursor_col, 7);
        assert!((loaded.meta.editor_scroll_top - 120.5).abs() < f64::EPSILON);
        assert!((loaded.meta.chat_width_pct - 40.0).abs() < f64::EPSILON);
        Ok(())
    }

    // ── Command helpers ─────────────────────────────────────────────

    #[test]
    fn last_session_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        write_last_session(dir.path(), Path::new("/home/user/proof.turn")).unwrap();
        let result = read_last_session(dir.path());
        assert_eq!(result, Some("/home/user/proof.turn".to_string()));
    }

    #[test]
    fn last_session_returns_none_when_missing() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(read_last_session(dir.path()), None);
    }

    #[test]
    fn last_session_returns_none_when_empty() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("last_session.txt"), "").unwrap();
        assert_eq!(read_last_session(dir.path()), None);
    }

    #[test]
    fn last_session_returns_none_when_whitespace_only() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("last_session.txt"), "  \n  ").unwrap();
        assert_eq!(read_last_session(dir.path()), None);
    }

    #[test]
    fn last_session_round_trip_with_spaces_in_path() {
        let dir = tempfile::tempdir().unwrap();
        let path_with_spaces = Path::new("/home/user/my proofs/session.turn");
        write_last_session(dir.path(), path_with_spaces).unwrap();
        assert_eq!(
            read_last_session(dir.path()),
            Some("/home/user/my proofs/session.turn".to_string())
        );
    }

    #[test]
    fn last_session_overwrite_returns_new_path() {
        let dir = tempfile::tempdir().unwrap();
        write_last_session(dir.path(), Path::new("/old/path.turn")).unwrap();
        write_last_session(dir.path(), Path::new("/new/path.turn")).unwrap();
        assert_eq!(
            read_last_session(dir.path()),
            Some("/new/path.turn".to_string())
        );
    }

    // ── Autosave file lifecycle ────────────────────────────────────

    #[test]
    fn remove_autosave_file_deletes_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("autosave.turn");
        std::fs::write(&path, b"dummy autosave contents").unwrap();
        assert!(path.exists());

        remove_autosave_file(&path).unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn remove_autosave_file_is_idempotent_when_missing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("autosave.turn");
        assert!(!path.exists());

        remove_autosave_file(&path).unwrap();
        remove_autosave_file(&path).unwrap();
    }

    #[test]
    fn remove_autosave_file_leaves_sibling_files_alone() {
        let dir = tempfile::tempdir().unwrap();
        let autosave = dir.path().join("autosave.turn");
        let last_session = dir.path().join("last_session.txt");
        std::fs::write(&autosave, b"autosave").unwrap();
        std::fs::write(&last_session, b"/home/user/proof.turn").unwrap();

        remove_autosave_file(&autosave).unwrap();
        assert!(!autosave.exists());
        assert!(last_session.exists());
    }

    // ── Filesystem error paths ───────────────────────────────────────

    #[test]
    fn write_last_session_errors_when_dir_missing() -> TestResult {
        let dir = tempfile::tempdir()?;
        let missing = dir.path().join("does").join("not").join("exist");
        let result = write_last_session(&missing, Path::new("/home/user/proof.turn"));
        assert!(
            result.is_err(),
            "expected Err when parent directory is missing, got: {result:?}"
        );
        Ok(())
    }

    #[test]
    fn write_last_session_errors_when_target_is_directory() -> TestResult {
        let dir = tempfile::tempdir()?;
        std::fs::create_dir(dir.path().join("last_session.txt"))?;
        let result = write_last_session(dir.path(), Path::new("/home/user/proof.turn"));
        assert!(
            result.is_err(),
            "expected Err when last_session.txt is a directory, got: {result:?}"
        );
        Ok(())
    }

    #[test]
    fn remove_autosave_file_errors_when_path_is_directory() -> TestResult {
        let dir = tempfile::tempdir()?;
        let autosave_as_dir = dir.path().join("autosave.turn");
        std::fs::create_dir(&autosave_as_dir)?;

        let result = remove_autosave_file(&autosave_as_dir);
        assert!(
            result.is_err(),
            "expected Err when autosave path is a directory, got: {result:?}"
        );
        assert!(autosave_as_dir.exists());
        Ok(())
    }

    #[test]
    fn read_last_session_returns_none_on_non_utf8_bytes() -> TestResult {
        let dir = tempfile::tempdir()?;
        std::fs::write(dir.path().join("last_session.txt"), [0xFF, 0xFE, 0xFD])?;
        assert_eq!(read_last_session(dir.path()), None);
        Ok(())
    }

    #[test]
    fn remove_autosave_file_idempotent_after_successful_remove() -> TestResult {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("autosave.turn");
        std::fs::write(&path, b"autosave contents")?;
        assert!(path.exists());

        remove_autosave_file(&path)?;
        remove_autosave_file(&path)?;
        remove_autosave_file(&path)?;
        assert!(!path.exists());
        Ok(())
    }
}
