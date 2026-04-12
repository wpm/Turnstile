//! Native menu bar construction for all platforms.
//!
//! Builds a File menu with session commands and an Edit menu with standard
//! clipboard items.  On macOS an additional app-name submenu provides About,
//! Settings, Hide/Show, and Quit — following Apple HIG.  On Windows/Linux
//! Settings and Quit live inside the File menu instead.

use tauri::menu::{AboutMetadata, MenuBuilder, MenuItem, SubmenuBuilder};
use tauri::{include_image, AppHandle, Runtime};

// ── Menu item IDs (public so tests can assert on them) ──────────────────────

pub const NEW_SESSION: &str = "new_session";
pub const OPEN_SESSION: &str = "open_session";
pub const SAVE_SESSION: &str = "save_session";
pub const SAVE_SESSION_AS: &str = "save_session_as";
pub const SETTINGS: &str = "settings";

/// Every custom (non-predefined) menu-item ID that the frontend must handle.
pub const ALL_CUSTOM_IDS: &[&str] = &[
    NEW_SESSION,
    OPEN_SESSION,
    SAVE_SESSION,
    SAVE_SESSION_AS,
    SETTINGS,
];

// ── Menu construction ───────────────────────────────────────────────────────

/// Build the application menu bar.
pub fn build_menu<R: Runtime>(handle: &AppHandle<R>) -> tauri::Result<tauri::menu::Menu<R>> {
    let mut menu = MenuBuilder::new(handle);

    // macOS: app-name submenu (About, Settings, Services, Hide, Quit)
    #[cfg(target_os = "macos")]
    {
        let settings_item =
            MenuItem::with_id(handle, SETTINGS, "Settings...", true, Some("CmdOrCtrl+,"))?;

        let app_submenu = SubmenuBuilder::new(handle, "Turnstile")
            .about(Some(AboutMetadata {
                icon: Some(include_image!("./icons/32x32.png")),
                ..Default::default()
            }))
            .separator()
            .item(&settings_item)
            .separator()
            .services()
            .separator()
            .hide()
            .hide_others()
            .show_all()
            .separator()
            .quit()
            .build()?;

        menu = menu.item(&app_submenu);
    }

    // ── File submenu (all platforms) ────────────────────────────────────────

    let new_item = MenuItem::with_id(
        handle,
        NEW_SESSION,
        "New Session",
        true,
        Some("CmdOrCtrl+N"),
    )?;
    let open_item = MenuItem::with_id(
        handle,
        OPEN_SESSION,
        "Open Session...",
        true,
        Some("CmdOrCtrl+O"),
    )?;
    let save_item = MenuItem::with_id(
        handle,
        SAVE_SESSION,
        "Save Session",
        true,
        Some("CmdOrCtrl+S"),
    )?;
    let save_as_item = MenuItem::with_id(
        handle,
        SAVE_SESSION_AS,
        "Save Session As...",
        true,
        Some("CmdOrCtrl+Shift+S"),
    )?;

    #[allow(unused_mut)]
    let mut file_submenu = SubmenuBuilder::new(handle, "File")
        .item(&new_item)
        .item(&open_item)
        .separator()
        .item(&save_item)
        .item(&save_as_item);

    // Windows/Linux: Settings and Quit go in File (macOS has them in the app submenu)
    #[cfg(not(target_os = "macos"))]
    {
        let settings_item =
            MenuItem::with_id(handle, SETTINGS, "Settings...", true, Some("CmdOrCtrl+,"))?;
        file_submenu = file_submenu
            .separator()
            .item(&settings_item)
            .separator()
            .quit();
    }

    menu = menu.item(&file_submenu.build()?);

    // ── Edit submenu (all platforms — clipboard shortcuts for the webview) ──

    let edit_submenu = SubmenuBuilder::new(handle, "Edit")
        .undo()
        .redo()
        .separator()
        .cut()
        .copy()
        .paste()
        .select_all()
        .build()?;

    menu = menu.item(&edit_submenu);

    menu.build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_custom_ids_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for id in ALL_CUSTOM_IDS {
            assert!(seen.insert(id), "duplicate menu ID: {id}");
        }
    }

    #[test]
    fn all_custom_ids_contains_required_items() {
        let required = [
            NEW_SESSION,
            OPEN_SESSION,
            SAVE_SESSION,
            SAVE_SESSION_AS,
            SETTINGS,
        ];
        for id in &required {
            assert!(
                ALL_CUSTOM_IDS.contains(id),
                "ALL_CUSTOM_IDS is missing required ID: {id}"
            );
        }
    }

    #[test]
    fn id_constants_match_expected_strings() {
        assert_eq!(NEW_SESSION, "new_session");
        assert_eq!(OPEN_SESSION, "open_session");
        assert_eq!(SAVE_SESSION, "save_session");
        assert_eq!(SAVE_SESSION_AS, "save_session_as");
        assert_eq!(SETTINGS, "settings");
    }
}
