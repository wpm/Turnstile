/** Menu item IDs emitted by the Rust backend (must match `src-tauri/src/menu.rs`). */
export const MENU_IDS = {
  NEW_SESSION: 'new_session',
  OPEN_SESSION: 'open_session',
  SAVE_SESSION: 'save_session',
  SAVE_SESSION_AS: 'save_session_as',
  SETTINGS: 'settings',
  TOGGLE_WORD_WRAP: 'toggle_word_wrap',
} as const

export interface MenuActions {
  newSession: () => void
  openSession: () => void
  saveSession: () => void
  saveSessionAs: () => void
  openSettings: () => void
  toggleWordWrap: () => void
}

/** Dispatch a menu-event ID to the appropriate action.  Returns true if handled. */
export function handleMenuEvent(id: string, actions: MenuActions): boolean {
  switch (id) {
    case MENU_IDS.NEW_SESSION:
      actions.newSession()
      return true
    case MENU_IDS.OPEN_SESSION:
      actions.openSession()
      return true
    case MENU_IDS.SAVE_SESSION:
      actions.saveSession()
      return true
    case MENU_IDS.SAVE_SESSION_AS:
      actions.saveSessionAs()
      return true
    case MENU_IDS.SETTINGS:
      actions.openSettings()
      return true
    case MENU_IDS.TOGGLE_WORD_WRAP:
      actions.toggleWordWrap()
      return true
    default:
      return false
  }
}
