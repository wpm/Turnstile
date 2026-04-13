//! App settings: persisted to `{app_data_dir}/settings.json`.
//!
//! `load_settings` reads the file at startup; `save_settings_to_disk` writes it
//! on any change.  The Tauri commands delegate to these functions so the UI can
//! read and write settings via `invoke`.

use std::path::Path;

use serde::{Deserialize, Serialize};
use tauri::Manager;

use crate::models;

fn default_theme() -> String {
    "auto".to_string()
}

/// Persisted application settings.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Settings {
    /// Font size for the code editor panel (pt).
    pub editor_font_size: u8,
    /// Font size for the prose / goal-state panel (pt).
    pub prose_font_size: u8,
    /// Font size for the chat panel (pt).
    pub chat_font_size: u8,
    /// Selected model ID.  `None` means "use the backend default".
    pub model: Option<String>,
    /// UI theme: `"dark"` or `"light"`.
    #[serde(default = "default_theme")]
    pub theme: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            editor_font_size: 13,
            prose_font_size: 13,
            chat_font_size: 13,
            model: None,
            theme: default_theme(),
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

/// Return the list of available models from `models.rs`.
#[tauri::command]
pub fn get_available_models() -> Vec<models::ModelInfo> {
    models::MODELS.to_vec()
}

/// Update the selected model in settings and persist to disk.
#[tauri::command]
pub async fn set_model(app: tauri::AppHandle, model_id: String) -> Result<(), String> {
    if !models::is_valid_model_id(&model_id) {
        return Err(format!("Unknown model ID: {model_id}"));
    }

    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to resolve app data directory: {e}"))?;

    let state = app.state::<crate::AppState>();
    let mut lock = state.settings.lock().await;
    lock.model = Some(model_id);
    let updated = lock.clone();
    drop(lock);

    save_settings_to_disk(&updated, &app_data_dir)
}

// ── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_values_are_correct() {
        let s = Settings::default();
        assert_eq!(s.editor_font_size, 13);
        assert_eq!(s.prose_font_size, 13);
        assert_eq!(s.chat_font_size, 13);
        assert_eq!(s.model, None);
    }

    #[test]
    fn graceful_fallback_on_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        // No settings.json in the temp dir — should return defaults.
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
            prose_font_size: 14,
            chat_font_size: 12,
            model: Some("claude-sonnet-4-6".to_string()),
            theme: "light".to_string(),
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
        // serde's default field values kick in for missing keys.
        let s = load_settings(dir.path());
        assert_eq!(s, Settings::default());
    }

    #[test]
    fn theme_defaults_to_auto() {
        assert_eq!(Settings::default().theme, "auto");
    }

    #[test]
    fn theme_round_trip_light() {
        let dir = tempfile::tempdir().unwrap();
        let original = Settings {
            theme: "light".to_string(),
            ..Settings::default()
        };
        save_settings_to_disk(&original, dir.path()).unwrap();
        let loaded = load_settings(dir.path());
        assert_eq!(loaded.theme, "light");
    }

    #[test]
    fn theme_round_trip_auto() {
        let dir = tempfile::tempdir().unwrap();
        let original = Settings {
            theme: "auto".to_string(),
            ..Settings::default()
        };
        save_settings_to_disk(&original, dir.path()).unwrap();
        let loaded = load_settings(dir.path());
        assert_eq!(loaded.theme, "auto");
    }

    #[test]
    fn theme_falls_back_on_missing_key() {
        let dir = tempfile::tempdir().unwrap();
        // JSON without a "theme" key — serde field default kicks in.
        std::fs::write(
            dir.path().join("settings.json"),
            br#"{"editor_font_size": 14}"#,
        )
        .unwrap();
        let s = load_settings(dir.path());
        assert_eq!(s.theme, "auto");
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
        assert_eq!(s.prose_font_size, 13); // default
        assert_eq!(s.chat_font_size, 13); // default
        assert_eq!(s.model, None); // default
    }

    #[test]
    fn extra_unknown_keys_ignored() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("settings.json"),
            br#"{"editor_font_size": 15, "unknown_future_field": true}"#,
        )
        .unwrap();
        let s = load_settings(dir.path());
        assert_eq!(s.editor_font_size, 15);
    }
}
