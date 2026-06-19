//! App settings: persisted to `{app_data_dir}/settings.json`.
//!
//! `load_settings` reads the file at startup; `save_settings_to_disk` writes it
//! on any change.  The Tauri commands delegate to these functions so the UI can
//! read and write settings via `invoke`.

use std::path::Path;

use serde::{Deserialize, Serialize};
use tauri::Manager;

/// Persisted application settings.
///
/// Each `*_model` field is `None` when the user has not chosen an override;
/// callers fall back to `crate::llm::models::default_model_id()`. Each
/// `*_prompt` field is `None` when the user has not written their own prompt;
/// callers fall back to the baked-in default constant
/// (`DEFAULT_ASSISTANT_PROMPT`, `DEFAULT_TRANSLATION_PROMPT`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Settings {
    /// Font size for the code editor panel (pt).
    pub editor_font_size: u8,
    /// Font size for the Goal State panel (pt).
    pub goal_state_font_size: u8,
    /// Font size for the Prose Proof panel (pt).
    #[serde(alias = "prose_font_size")]
    pub prose_proof_font_size: u8,
    /// Font size for the assistant panel (pt).
    #[serde(alias = "chat_font_size")]
    pub assistant_font_size: u8,
    /// Selected model ID for the assistant conversation.  `None` means "use
    /// the backend default".
    #[serde(alias = "model")]
    pub assistant_model: Option<String>,
    /// Selected model ID for the Proof translator.  `None` means "use the
    /// backend default".
    pub translation_model: Option<String>,
    /// User override for the assistant system prompt.  `None` means "use the
    /// built-in `DEFAULT_ASSISTANT_PROMPT`".
    pub assistant_prompt: Option<String>,
    /// User override for the translation system prompt.  `None` means "use
    /// the built-in `DEFAULT_TRANSLATION_PROMPT`".
    pub translation_prompt: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            editor_font_size: 13,
            goal_state_font_size: 13,
            prose_proof_font_size: 13,
            assistant_font_size: 13,
            assistant_model: None,
            translation_model: None,
            assistant_prompt: None,
            translation_prompt: None,
        }
    }
}

/// Path to the settings file within the app-data directory.
fn settings_path(app_data_dir: &Path) -> std::path::PathBuf {
    app_data_dir.join("settings.json")
}

/// Outcome of loading settings from disk.
///
/// Distinguishes a normal load (or first-run absence) from recovery after a
/// corrupt file, so the caller can warn the user that their settings were
/// reset rather than silently discarding their models, prompts, and font
/// sizes.
pub struct SettingsLoad {
    /// The settings to use — the loaded values, or defaults on absence or
    /// corruption.
    pub settings: Settings,
    /// `Some(message)` when the file was present but unparseable and has been
    /// reset to defaults (and the bad file backed up). `None` on a clean load
    /// or a normal first-run absence. Surfaced to the user as a warning toast.
    pub warning: Option<String>,
}

/// User-facing warning shown when `settings.json` was present but unreadable.
const CORRUPT_SETTINGS_WARNING: &str =
    "Your settings file was unreadable and has been reset to defaults.";

/// Load settings from `{app_data_dir}/settings.json`.
///
/// Distinguishes an absent file (normal first run — stay silent) from a
/// present-but-unparseable file (reset to defaults, back up the bad file, and
/// warn). A missing or unreadable file falls back to defaults silently; only a
/// file that exists but contains malformed JSON produces a warning.
pub fn load_settings_checked(app_data_dir: &Path) -> SettingsLoad {
    let path = settings_path(app_data_dir);
    let Ok(raw) = std::fs::read_to_string(&path) else {
        // Absent (or unreadable) file: normal first run, stay silent.
        return SettingsLoad {
            settings: Settings::default(),
            warning: None,
        };
    };
    match serde_json::from_str::<Settings>(&raw) {
        Ok(settings) => SettingsLoad {
            settings,
            warning: None,
        },
        Err(e) => {
            // Present but malformed: back up the bad file so it can be
            // inspected/recovered, then reset to defaults and warn. Moving it
            // aside also means the next launch sees an absent file and stays
            // silent, so the warning fires once per corruption — not forever.
            match backup_corrupt_settings(&path) {
                Some(backup) => tracing::warn!(
                    "settings.json was unparseable ({e}); reset to defaults, backed up to {}",
                    backup.display()
                ),
                None => {
                    tracing::warn!("settings.json was unparseable ({e}); reset to defaults");
                }
            }
            SettingsLoad {
                settings: Settings::default(),
                warning: Some(CORRUPT_SETTINGS_WARNING.to_string()),
            }
        }
    }
}

/// Move a corrupt `settings.json` aside to `settings.json.bak` so it isn't
/// silently lost. Returns the backup path on success; logs and returns `None`
/// on failure (the reset still proceeds).
fn backup_corrupt_settings(path: &Path) -> Option<std::path::PathBuf> {
    let backup = path.with_extension("json.bak");
    match std::fs::rename(path, &backup) {
        Ok(()) => Some(backup),
        Err(e) => {
            tracing::warn!("failed to back up corrupt settings.json: {e}");
            None
        }
    }
}

/// Load settings from `{app_data_dir}/settings.json`.
///
/// Gracefully falls back to `Settings::default()` if the file is missing,
/// unreadable, or contains malformed JSON. Callers that need to distinguish a
/// corrupt file (to warn the user) should use [`load_settings_checked`].
#[must_use]
pub fn load_settings(app_data_dir: &Path) -> Settings {
    load_settings_checked(app_data_dir).settings
}

/// Write `settings` to `{app_data_dir}/settings.json`, creating the directory
/// if it does not yet exist.
///
/// # Errors
///
/// Returns an error if the directory cannot be created or the file cannot be written.
pub fn save_settings_to_disk(settings: &Settings, app_data_dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(app_data_dir)
        .map_err(|e| format!("Failed to create app data directory: {e}"))?;

    let json = serde_json::to_string_pretty(settings)
        .map_err(|e| format!("Failed to serialize settings: {e}"))?;

    std::fs::write(settings_path(app_data_dir), json)
        .map_err(|e| format!("Failed to write settings.json: {e}"))
}

// ── Tauri commands ────────────────────────────────────────────────────

/// Return the current settings from the app state.
///
/// # Errors
///
/// Returns an error if the settings mutex is poisoned.
#[tauri::command]
#[tracing::instrument(level = "debug", skip_all)]
pub async fn get_settings(app: tauri::AppHandle) -> Result<Settings, String> {
    let state = app.state::<crate::AppState>();
    let s = state.settings.lock().await.clone();
    Ok(s)
}

/// Persist new settings to disk and update the app state.
///
/// # Errors
///
/// Returns an error if the app data directory cannot be resolved or the file cannot be written.
#[tauri::command]
#[tracing::instrument(level = "debug", skip_all)]
pub async fn save_settings(app: tauri::AppHandle, settings: Settings) -> Result<(), String> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to resolve app data directory: {e}"))?;

    save_settings_to_disk(&settings, &app_data_dir)?;

    let state = app.state::<crate::AppState>();
    let mut lock = state.settings.lock().await;
    *lock = settings;
    drop(lock);
    Ok(())
}

/// Return the baked-in default assistant prompt so the Settings UI can show
/// it when the user hasn't set their own.
#[tauri::command]
#[must_use]
pub const fn get_default_assistant_prompt() -> &'static str {
    crate::assistant::DEFAULT_ASSISTANT_PROMPT
}

/// Return the baked-in default translation prompt so the Settings UI can show
/// it when the user hasn't set their own.
#[tauri::command]
#[must_use]
pub const fn get_default_translation_prompt() -> &'static str {
    crate::proof::translator::DEFAULT_TRANSLATION_PROMPT
}

// ── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_prompt_getters_return_baked_in_constants() {
        assert_eq!(
            get_default_assistant_prompt(),
            crate::assistant::DEFAULT_ASSISTANT_PROMPT
        );
        assert!(!get_default_assistant_prompt().is_empty());
        assert_eq!(
            get_default_translation_prompt(),
            crate::proof::translator::DEFAULT_TRANSLATION_PROMPT
        );
        assert!(!get_default_translation_prompt().is_empty());
    }

    #[test]
    fn default_values_are_correct() {
        let s = Settings::default();
        assert_eq!(s.editor_font_size, 13);
        assert_eq!(s.goal_state_font_size, 13);
        assert_eq!(s.prose_proof_font_size, 13);
        assert_eq!(s.assistant_font_size, 13);
        assert_eq!(s.assistant_model, None);
        assert_eq!(s.translation_model, None);
        assert_eq!(s.assistant_prompt, None);
        assert_eq!(s.translation_prompt, None);
    }

    #[test]
    fn graceful_fallback_on_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let s = load_settings(dir.path());
        assert_eq!(s, Settings::default());
    }

    #[test]
    fn graceful_fallback_on_malformed_json() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("settings.json"), b"not valid json").unwrap();
        let s = load_settings(dir.path());
        assert_eq!(s, Settings::default());
    }

    #[test]
    fn checked_load_is_silent_when_file_absent() {
        let dir = tempfile::tempdir().unwrap();
        let out = load_settings_checked(dir.path());
        assert_eq!(out.settings, Settings::default());
        assert!(out.warning.is_none());
    }

    #[test]
    fn checked_load_is_silent_on_clean_load() {
        let dir = tempfile::tempdir().unwrap();
        let original = Settings {
            editor_font_size: 18,
            ..Settings::default()
        };
        save_settings_to_disk(&original, dir.path()).unwrap();
        let out = load_settings_checked(dir.path());
        assert_eq!(out.settings, original);
        assert!(out.warning.is_none());
    }

    #[test]
    fn checked_load_warns_and_backs_up_corrupt_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, b"not valid json").unwrap();

        let out = load_settings_checked(dir.path());

        assert_eq!(out.settings, Settings::default());
        assert_eq!(out.warning.as_deref(), Some(CORRUPT_SETTINGS_WARNING));
        // The bad file is moved aside, not silently discarded.
        assert!(!path.exists(), "corrupt file should be moved aside");
        let backup = dir.path().join("settings.json.bak");
        assert!(backup.exists(), "corrupt file should be backed up");
        assert_eq!(std::fs::read(&backup).unwrap(), b"not valid json");
    }

    #[test]
    fn checked_load_after_corruption_is_silent_next_launch() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("settings.json"), b"not valid json").unwrap();

        // First load recovers and warns.
        assert!(load_settings_checked(dir.path()).warning.is_some());
        // The file was moved aside, so the next launch sees no file and stays
        // silent — the warning fires once per corruption, not every launch.
        assert!(load_settings_checked(dir.path()).warning.is_none());
    }

    #[test]
    fn round_trip_save_and_load() {
        let dir = tempfile::tempdir().unwrap();
        let original = Settings {
            editor_font_size: 16,
            goal_state_font_size: 12,
            prose_proof_font_size: 14,
            assistant_font_size: 12,
            assistant_model: Some("claude-sonnet-4-6".to_string()),
            translation_model: Some("claude-haiku-4-5-20251001".to_string()),
            assistant_prompt: Some("Prefer tactic-mode proofs.".to_string()),
            translation_prompt: Some("Translate concisely.".to_string()),
        };

        save_settings_to_disk(&original, dir.path()).expect("save should succeed");
        let loaded = load_settings(dir.path());
        assert_eq!(loaded, original);
    }

    #[test]
    fn save_creates_directory_if_missing() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("nested").join("path");
        let s = Settings::default();
        save_settings_to_disk(&s, &nested).expect("save should create missing directories");
        assert!(nested.join("settings.json").exists());
    }

    #[test]
    fn load_returns_defaults_for_empty_json_object() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("settings.json"), b"{}").unwrap();
        let s = load_settings(dir.path());
        assert_eq!(s, Settings::default());
    }

    #[test]
    fn partial_json_loads_specified_fields_defaults_rest() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("settings.json"),
            br#"{"editor_font_size": 20}"#,
        )
        .unwrap();
        let s = load_settings(dir.path());
        assert_eq!(s.editor_font_size, 20);
        assert_eq!(s.goal_state_font_size, 13);
        assert_eq!(s.prose_proof_font_size, 13);
        assert_eq!(s.assistant_font_size, 13);
        assert_eq!(s.assistant_model, None);
    }

    #[test]
    fn extra_unknown_keys_ignored() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("settings.json"),
            br#"{"editor_font_size": 15, "unknown_future_field": true, "theme": "light", "custom_prompt": "legacy"}"#,
        )
        .unwrap();
        let s = load_settings(dir.path());
        assert_eq!(s.editor_font_size, 15);
    }

    #[test]
    fn legacy_prose_font_size_alias_is_accepted() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("settings.json"),
            br#"{"prose_font_size": 17}"#,
        )
        .unwrap();
        let s = load_settings(dir.path());
        assert_eq!(s.prose_proof_font_size, 17);
    }

    #[test]
    fn legacy_model_alias_is_accepted() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("settings.json"),
            br#"{"model": "claude-haiku-4-5-20251001"}"#,
        )
        .unwrap();
        let s = load_settings(dir.path());
        assert_eq!(
            s.assistant_model.as_deref(),
            Some("claude-haiku-4-5-20251001")
        );
    }

    #[test]
    fn translation_model_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let original = Settings {
            translation_model: Some("claude-opus-4-6".to_string()),
            ..Settings::default()
        };
        save_settings_to_disk(&original, dir.path()).unwrap();
        let loaded = load_settings(dir.path());
        assert_eq!(loaded.translation_model.as_deref(), Some("claude-opus-4-6"));
    }

    #[test]
    fn assistant_prompt_none_means_use_default() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("settings.json"), b"{}").unwrap();
        let s = load_settings(dir.path());
        assert!(s.assistant_prompt.is_none());
    }

    #[test]
    fn assistant_prompt_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let original = Settings {
            assistant_prompt: Some("be terse".to_string()),
            ..Settings::default()
        };
        save_settings_to_disk(&original, dir.path()).unwrap();
        let loaded = load_settings(dir.path());
        assert_eq!(loaded.assistant_prompt.as_deref(), Some("be terse"));
    }
}
