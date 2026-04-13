import { invoke } from './tauri'
import { MENU_IDS } from './menu'

/** Enable or disable the Save Session menu item to match the dirty state. */
export function syncSaveMenuState(dirty: boolean): Promise<void> {
  return invoke('set_menu_item_enabled', {
    id: MENU_IDS.SAVE_SESSION,
    enabled: dirty,
  })
}
