/**
 * App settings: persisted preferences stored via Tauri backend (settings.json on disk).
 *
 * Exports a reactive `settings` object (Svelte 5 `$state`) plus
 * pure helpers for parsing/serializing so tests can exercise the
 * logic without touching the singleton store.
 */

import { invoke } from './tauri'
import type { Theme } from './theme'

/** Selectable font sizes offered in the Settings UI. */
export const FONT_SIZE_OPTIONS = [10, 11, 12, 13, 14, 15, 16, 18, 20]

/**
 * Factory defaults — also used by `resetToDefaults`.
 * `model: null` means "use the backend default" (first entry from `get_available_models`).
 */
const DEFAULT_SETTINGS = {
  editorFontSize: 13,
  proseFontSize: 13,
  chatFontSize: 13,
  model: null as string | null,
  theme: 'dark' as Theme,
}

interface SettingsData {
  editorFontSize: number
  proseFontSize: number
  chatFontSize: number
  model: string | null
  theme: Theme
}

export interface ModelInfo {
  id: string
  display_name: string
}

/**
 * Parse a raw settings object (from Tauri backend) into a settings object.
 * Missing or invalid keys fall back to `DEFAULT_SETTINGS`.
 */
export function parseSettings(raw: Record<string, unknown> | null | undefined): SettingsData {
  if (!raw || typeof raw !== 'object') {
    return { ...DEFAULT_SETTINGS }
  }
  return {
    editorFontSize:
      typeof raw['editor_font_size'] === 'number'
        ? raw['editor_font_size']
        : DEFAULT_SETTINGS.editorFontSize,
    proseFontSize:
      typeof raw['prose_font_size'] === 'number'
        ? raw['prose_font_size']
        : DEFAULT_SETTINGS.proseFontSize,
    chatFontSize:
      typeof raw['chat_font_size'] === 'number'
        ? raw['chat_font_size']
        : DEFAULT_SETTINGS.chatFontSize,
    model: typeof raw['model'] === 'string' ? raw['model'] : DEFAULT_SETTINGS.model,
    theme:
      raw['theme'] === 'dark' || raw['theme'] === 'light'
        ? (raw['theme'] as Theme)
        : DEFAULT_SETTINGS.theme,
  }
}

/**
 * Serialize a settings object to the shape expected by the Tauri `save_settings` command.
 */
function serializeSettings(s: SettingsData): Record<string, unknown> {
  return {
    editor_font_size: s.editorFontSize,
    prose_font_size: s.proseFontSize,
    chat_font_size: s.chatFontSize,
    model: s.model,
    theme: s.theme,
  }
}

// ── Reactive singleton ────────────────────────────────────────────────

let editorFontSize = $state(DEFAULT_SETTINGS.editorFontSize)
let proseFontSize = $state(DEFAULT_SETTINGS.proseFontSize)
let chatFontSize = $state(DEFAULT_SETTINGS.chatFontSize)
let model = $state<string | null>(DEFAULT_SETTINGS.model)
let themeValue = $state<Theme>(DEFAULT_SETTINGS.theme)
let availableModels = $state<ModelInfo[]>([])

export const settings = {
  get editorFontSize() {
    return editorFontSize
  },
  get proseFontSize() {
    return proseFontSize
  },
  get chatFontSize() {
    return chatFontSize
  },
  get model() {
    return model
  },
  get theme() {
    return themeValue
  },
  get availableModels() {
    return availableModels
  },
}

export function setAvailableModels(models: ModelInfo[]): void {
  availableModels = models
}

/**
 * Apply a parsed settings object to the reactive singleton.
 * Called on startup after loading settings from the Tauri backend.
 */
export function applySettings(s: SettingsData): void {
  editorFontSize = s.editorFontSize
  proseFontSize = s.proseFontSize
  chatFontSize = s.chatFontSize
  model = s.model
  themeValue = s.theme
}

function currentValues(): SettingsData {
  return { editorFontSize, proseFontSize, chatFontSize, model, theme: themeValue }
}

export function updateSetting(key: keyof SettingsData, value: number | string | null): void {
  if (key === 'editorFontSize' && typeof value === 'number') {
    editorFontSize = value
  } else if (key === 'proseFontSize' && typeof value === 'number') {
    proseFontSize = value
  } else if (key === 'chatFontSize' && typeof value === 'number') {
    chatFontSize = value
  } else if (key === 'model') {
    model = typeof value === 'string' ? value : null
  } else if (key === 'theme') {
    themeValue = value === 'light' ? 'light' : 'dark'
  }
  const s = serializeSettings(currentValues())
  invoke('save_settings', { settings: s }).catch((err: unknown) => {
    console.error('save_settings failed:', err)
  })
}

export function resetToDefaults(): void {
  editorFontSize = DEFAULT_SETTINGS.editorFontSize
  proseFontSize = DEFAULT_SETTINGS.proseFontSize
  chatFontSize = DEFAULT_SETTINGS.chatFontSize
  model = DEFAULT_SETTINGS.model
  themeValue = DEFAULT_SETTINGS.theme
  const s = serializeSettings(DEFAULT_SETTINGS)
  invoke('save_settings', { settings: s }).catch((err: unknown) => {
    console.error('save_settings failed:', err)
  })
}
