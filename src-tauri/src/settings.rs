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
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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

/// Load settings from `{app_data_dir}/settings.json`.
///
/// Gracefully falls back to `Settings::default()` if the file is missing,
/// unreadable, or contains malformed JSON.
pub fn load_settings(app_data_dir: &Path) -> Settings {
    let path = settings_path(app_data_dir);
    let raw = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(_) => return Settings::default(),
    };
    serde_json::from_str::<Settings>(&raw).unwrap_or_default()
}

/// Write `settings` to `{app_data_dir}/settings.json`, creating the directory
/// if it does not yet exist.
pub fn save_settings_to_disk(settings: &Settings, app_data_dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(app_data_dir)
        .map_err(|e| format!("Failed to create app data directory: {e}"))?;

    let json = serde_json::to_string_pretty(settings)
        .map_err(|e| format!("Failed to serialize settings: {e}"))?;

    std::fs::write(settings_path(app_data_dir), json)
        .map_err(|e| format!("Failed to write settings.json: {e}"))
}

// ── Tauri commands ────────────────────────────────────────────────────

/// Return the current settings from app state.
#[tauri::command]
pub async fn get_settings(app: tauri::AppHandle) -> Result<Settings, String> {
    let state = app.state::<crate::AppState>();
    let lock = state.settings.lock().await;
    let s: Settings = lock.clone();
    Ok(s)
}

/// Persist new settings to disk and update app state.
#[tauri::command]
pub async fn save_settings(app: tauri::AppHandle, settings: Settings) -> Result<(), String> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to resolve app data directory: {e}"))?;

    save_settings_to_disk(&settings, &app_data_dir)?;

    let state = app.state::<crate::AppState>();
    let mut lock = state.settings.lock().await;
    *lock = settings;
    Ok(())
}

/// Return the baked-in default assistant prompt so the Settings UI can show
/// it when the user hasn't set their own.
#[tauri::command]
pub fn get_default_assistant_prompt() -> &'static str {
    crate::assistant::DEFAULT_ASSISTANT_PROMPT
}

/// Return the baked-in default translation prompt so the Settings UI can show
/// it when the user hasn't set their own.
#[tauri::command]
pub fn get_default_translation_prompt() -> &'static str {
    crate::proof::translator::DEFAULT_TRANSLATION_PROMPT
}

// ── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

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
