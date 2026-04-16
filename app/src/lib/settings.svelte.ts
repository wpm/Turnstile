/**
 * App settings: persisted preferences stored via Tauri backend (settings.json on disk).
 *
 * Exports a reactive `settings` object (Svelte 5 `$state`) plus
 * pure helpers for parsing/serializing so tests can exercise the
 * logic without touching the singleton store.
 */

import { invoke } from './tauri'
import type { ThemePreference } from './theme'

/** Selectable font sizes offered in the Settings UI. */
export const FONT_SIZE_OPTIONS = [10, 11, 12, 13, 14, 15, 16, 18, 20]

/**
 * Factory defaults — also used by `resetToDefaults`.
 * `model: null` means "use the backend default" (first entry from `get_available_models`).
 */
export const DEFAULT_SETTINGS = {
  editorFontSize: 13,
  proseFontSize: 13,
  assistantFontSize: 13,
  model: null as string | null,
  theme: 'auto' as ThemePreference,
  customPrompt: '',
}

interface SettingsData {
  editorFontSize: number
  proseFontSize: number
  assistantFontSize: number
  model: string | null
  theme: ThemePreference
  customPrompt: string
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
    assistantFontSize:
      typeof raw['assistant_font_size'] === 'number'
        ? raw['assistant_font_size']
        : DEFAULT_SETTINGS.assistantFontSize,
    model: typeof raw['model'] === 'string' ? raw['model'] : DEFAULT_SETTINGS.model,
    theme:
      raw['theme'] === 'dark' || raw['theme'] === 'light' || raw['theme'] === 'auto'
        ? (raw['theme'] as ThemePreference)
        : DEFAULT_SETTINGS.theme,
    customPrompt:
      typeof raw['custom_prompt'] === 'string'
        ? raw['custom_prompt']
        : DEFAULT_SETTINGS.customPrompt,
  }
}

/**
 * Serialize a settings object to the shape expected by the Tauri `save_settings` command.
 */
function serializeSettings(s: SettingsData): Record<string, unknown> {
  return {
    editor_font_size: s.editorFontSize,
    prose_font_size: s.proseFontSize,
    assistant_font_size: s.assistantFontSize,
    model: s.model,
    theme: s.theme,
    custom_prompt: s.customPrompt,
  }
}

// ── Reactive singleton ────────────────────────────────────────────────

let editorFontSize = $state(DEFAULT_SETTINGS.editorFontSize)
let proseFontSize = $state(DEFAULT_SETTINGS.proseFontSize)
let assistantFontSize = $state(DEFAULT_SETTINGS.assistantFontSize)
let model = $state<string | null>(DEFAULT_SETTINGS.model)
let themeValue = $state<ThemePreference>(DEFAULT_SETTINGS.theme)
let customPrompt = $state(DEFAULT_SETTINGS.customPrompt)
let availableModels = $state<ModelInfo[]>([])

export const settings = {
  get editorFontSize() {
    return editorFontSize
  },
  get proseFontSize() {
    return proseFontSize
  },
  get assistantFontSize() {
    return assistantFontSize
  },
  get model() {
    return model
  },
  get theme() {
    return themeValue
  },
  get customPrompt() {
    return customPrompt
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
  assistantFontSize = s.assistantFontSize
  model = s.model
  themeValue = s.theme
  customPrompt = s.customPrompt
}

function currentValues(): SettingsData {
  return {
    editorFontSize,
    proseFontSize,
    assistantFontSize,
    model,
    theme: themeValue,
    customPrompt,
  }
}

export async function updateSetting(
  key: keyof SettingsData,
  value: number | string | null,
): Promise<void> {
  const previous = currentValues()
  if (key === 'editorFontSize' && typeof value === 'number') {
    editorFontSize = value
  } else if (key === 'proseFontSize' && typeof value === 'number') {
    proseFontSize = value
  } else if (key === 'assistantFontSize' && typeof value === 'number') {
    assistantFontSize = value
  } else if (key === 'model') {
    model = typeof value === 'string' ? value : null
  } else if (key === 'theme') {
    themeValue = value === 'light' ? 'light' : value === 'auto' ? 'auto' : 'dark'
  } else if (key === 'customPrompt') {
    customPrompt = typeof value === 'string' ? value : ''
  }
  try {
    const s = serializeSettings(currentValues())
    await invoke('save_settings', { settings: s })
  } catch (err) {
    applySettings(previous)
    throw err
  }
}

export async function resetToDefaults(): Promise<void> {
  const previous = currentValues()
  applySettings({ ...DEFAULT_SETTINGS })
  try {
    const s = serializeSettings(DEFAULT_SETTINGS)
    await invoke('save_settings', { settings: s })
  } catch (err) {
    applySettings(previous)
    throw err
  }
}

// ── Draft state for deferred Apply ───────────────────────────────────

type SettingsKey = keyof SettingsData

interface DraftOptions {
  afterApply?: (values: SettingsData) => Promise<void>
}

// eslint-disable-next-line @typescript-eslint/no-explicit-any -- generic key access on $state fields
type AnyRecord = Record<string, any>

class SettingsDraft {
  #keys: SettingsKey[]
  // `committed` must be reactive: the `dirty` getter compares draft fields
  // against it, and after a successful apply() we reset it so the Apply
  // button disables. Plain private fields are not tracked by Svelte 5, so
  // reassigning a plain field would leave `dirty` stale until another
  // tracked input changes.
  committed = $state<SettingsData>({ ...DEFAULT_SETTINGS })
  #afterApply: ((values: SettingsData) => Promise<void>) | null

  editorFontSize = $state(DEFAULT_SETTINGS.editorFontSize)
  proseFontSize = $state(DEFAULT_SETTINGS.proseFontSize)
  assistantFontSize = $state(DEFAULT_SETTINGS.assistantFontSize)
  model = $state<string | null>(DEFAULT_SETTINGS.model)
  theme = $state<ThemePreference>(DEFAULT_SETTINGS.theme)
  customPrompt = $state(DEFAULT_SETTINGS.customPrompt)

  constructor(keys: SettingsKey[], options?: DraftOptions) {
    this.#keys = keys
    this.#afterApply = options?.afterApply ?? null
    const snapshot = currentValues()
    this.committed = { ...snapshot }
    for (const key of keys) {
      ;(this as AnyRecord)[key] = snapshot[key]
    }
  }

  get dirty(): boolean {
    return this.#keys.some((key) => (this as AnyRecord)[key] !== this.committed[key])
  }

  set(key: SettingsKey, value: number | string | null): void {
    ;(this as AnyRecord)[key] = value
  }

  fillDefaults(): void {
    for (const key of this.#keys) {
      ;(this as AnyRecord)[key] = DEFAULT_SETTINGS[key]
    }
  }

  discard(): void {
    for (const key of this.#keys) {
      ;(this as AnyRecord)[key] = this.committed[key]
    }
  }

  async apply(): Promise<void> {
    const previous = currentValues()
    const merged: SettingsData = { ...previous }
    for (const key of this.#keys) {
      // eslint-disable-next-line @typescript-eslint/no-unsafe-assignment -- dynamic key copy between same-shaped objects
      ;(merged as AnyRecord)[key] = (this as AnyRecord)[key]
    }
    applySettings(merged)
    try {
      const s = serializeSettings(merged)
      await invoke('save_settings', { settings: s })
      if (this.#afterApply) {
        await this.#afterApply(merged)
      }
      this.committed = { ...merged }
    } catch (err) {
      applySettings(previous)
      throw err
    }
  }
}

export function createDraft(keys: SettingsKey[], options?: DraftOptions): SettingsDraft {
  return new SettingsDraft(keys, options)
}
