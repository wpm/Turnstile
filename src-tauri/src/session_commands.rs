//! Tauri command handlers for session save/open/autosave.
//!
//! These commands implement the full session lifecycle:
//! - `new_session` — reset all session state
//! - `open_session` — load a `.turn` file (with optional native file picker)
//! - `save_session` — save to current path or prompt for one
//! - `save_session_as` — save with native file picker
//! - `auto_save_session` — write autosave.turn to app data dir
//! - `check_auto_save` — return true if autosave.turn exists
//! - `delete_auto_save` — delete autosave.turn
//! - `get_last_session` — read last_session.txt from app data dir
//! - `set_last_session` — write path to last_session.txt

use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_dialog::{DialogExt, FilePath};

use crate::session::{self, SessionState};
use crate::AppState;

// ── Helpers ───────────────────────────────────────────────────────────

fn app_data_dir(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map_err(|e| format!("Failed to resolve app data directory: {e}"))
}

fn autosave_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app_data_dir(app)?.join("autosave.turn"))
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

    // Reset chat state.
    *state.chat_state.lock().await = crate::chat::ChatState::default();

    // Emit event so frontend can clear editor / prose.
    let blank = SessionState::blank();
    emit_session_loaded(&app, &blank);

    Ok(())
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

    let session = session::load(&file_path)?;

    // Restore chat state.
    {
        let mut chat = state.chat_state.lock().await;
        chat.summary = session.summary.clone();
        chat.transcript = session
            .transcript
            .iter()
            .map(|t| crate::chat::Turn {
                role: match t.role.as_str() {
                    "assistant" => crate::chat::Role::Assistant,
                    "system" => crate::chat::Role::System,
                    _ => crate::chat::Role::User,
                },
                content: t.content.clone(),
                timestamp: t.timestamp,
            })
            .collect();
    }

    // Update session path and clear dirty flag.
    *state.current_session_path.lock().await = Some(file_path);
    state.session_dirty.store(false, Ordering::SeqCst);

    // Emit so frontend can restore editor/prose UI.
    emit_session_loaded(&app, &session);

    Ok(())
}

/// Save the current session to the current path. If no path is set, calls save_as.
#[tauri::command]
pub async fn save_session(
    app: AppHandle,
    proof_lean: String,
    prose_text: String,
    prose_hash: Option<String>,
    meta: session::Meta,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let path = state.current_session_path.lock().await.clone();
    match path {
        Some(p) => {
            do_save(&app, &state, &p, proof_lean, prose_text, prose_hash, meta).await?;
            if let Ok(data_dir) = app_data_dir(&app) {
                write_last_session(&data_dir, &p).ok();
            }
        }
        None => {
            save_session_as(app, proof_lean, prose_text, prose_hash, meta, state).await?;
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
    meta: session::Meta,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let (tx, rx) = tokio::sync::oneshot::channel::<Option<PathBuf>>();
    app.dialog()
        .file()
        .add_filter("Turnstile session", &["turn"])
        .set_file_name("session.turn")
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
    Ok(())
}

async fn do_save(
    _app: &AppHandle,
    state: &AppState,
    path: &Path,
    proof_lean: String,
    prose_text: String,
    prose_hash: Option<String>,
    meta: session::Meta,
) -> Result<(), String> {
    use chrono::Utc;

    let chat = state.chat_state.lock().await;
    let transcript = chat
        .transcript
        .iter()
        .map(|t| session::TranscriptTurn {
            role: match t.role {
                crate::chat::Role::User => "user".to_string(),
                crate::chat::Role::Assistant => "assistant".to_string(),
                crate::chat::Role::System => "system".to_string(),
            },
            content: t.content.clone(),
            timestamp: t.timestamp,
        })
        .collect();
    let summary = chat.summary.clone();
    drop(chat);

    // Preserve created_at from the passed meta; update saved_at.
    let mut final_meta = meta;
    final_meta.saved_at = Utc::now().to_rfc3339();
    if final_meta.created_at.is_empty() {
        final_meta.created_at = final_meta.saved_at.clone();
    }
    final_meta.format_version = session::FORMAT_VERSION;

    let session = SessionState {
        meta: final_meta,
        proof_lean,
        prose: session::ProseData {
            text: prose_text,
            tactic_state_hash: prose_hash,
        },
        transcript,
        summary,
    };

    session::save(&session, path)?;
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
    meta: session::Meta,
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
    if path.exists() {
        std::fs::remove_file(&path).map_err(|e| format!("Failed to delete autosave: {e}"))?;
    }
    Ok(())
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

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

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
}
