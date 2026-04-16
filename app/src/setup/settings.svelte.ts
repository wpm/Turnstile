/**
 * App settings: persisted preferences stored via Tauri backend (settings.json on disk).
 *
 * Exports a reactive `settings` object (Svelte 5 `$state`) plus
 * pure helpers for parsing/serializing so tests can exercise the
 * logic without touching the singleton store.
 */

import { invoke } from '../session/tauri'

/** Selectable font sizes offered in the Settings UI. */
export const FONT_SIZE_OPTIONS = [10, 11, 12, 13, 14, 15, 16, 18, 20]

/**
 * Factory defaults — also used by `resetToDefaults`.
 * `*Model: null` means "use the backend default" (first entry from `get_available_models`).
 * `*Prompt: null` means "use the built-in default prompt baked into the binary".
 */
export const DEFAULT_SETTINGS = {
  editorFontSize: 13,
  goalStateFontSize: 13,
  proseProofFontSize: 13,
  assistantFontSize: 13,
  assistantModel: null as string | null,
  translationModel: null as string | null,
  assistantPrompt: null as string | null,
  translationPrompt: null as string | null,
}

interface SettingsData {
  editorFontSize: number
  goalStateFontSize: number
  proseProofFontSize: number
  assistantFontSize: number
  assistantModel: string | null
  translationModel: string | null
  assistantPrompt: string | null
  translationPrompt: string | null
}

export interface ModelInfo {
  id: string
  display_name: string
}

function numberOr(raw: Record<string, unknown>, key: string, fallback: number): number {
  const v = raw[key]
  return typeof v === 'number' ? v : fallback
}

function stringOrNull(raw: Record<string, unknown>, key: string): string | null {
  const v = raw[key]
  return typeof v === 'string' ? v : null
}

/**
 * Parse a raw settings object (from Tauri backend) into a settings object.
 * Missing or invalid keys fall back to `DEFAULT_SETTINGS`.
 *
 * Accepts legacy keys (`prose_font_size`, `model`) for one release so
 * migrating from pre-reorg settings.json doesn't lose the user's values.
 */
export function parseSettings(raw: Record<string, unknown> | null | undefined): SettingsData {
  if (!raw || typeof raw !== 'object') {
    return { ...DEFAULT_SETTINGS }
  }
  const proseKey = 'prose_proof_font_size' in raw ? 'prose_proof_font_size' : 'prose_font_size'
  const assistantModelKey = 'assistant_model' in raw ? 'assistant_model' : 'model'
  return {
    editorFontSize: numberOr(raw, 'editor_font_size', DEFAULT_SETTINGS.editorFontSize),
    goalStateFontSize: numberOr(raw, 'goal_state_font_size', DEFAULT_SETTINGS.goalStateFontSize),
    proseProofFontSize: numberOr(raw, proseKey, DEFAULT_SETTINGS.proseProofFontSize),
    assistantFontSize: numberOr(raw, 'assistant_font_size', DEFAULT_SETTINGS.assistantFontSize),
    assistantModel: stringOrNull(raw, assistantModelKey),
    translationModel: stringOrNull(raw, 'translation_model'),
    assistantPrompt: stringOrNull(raw, 'assistant_prompt'),
    translationPrompt: stringOrNull(raw, 'translation_prompt'),
  }
}

/**
 * Serialize a settings object to the shape expected by the Tauri `save_settings` command.
 */
function serializeSettings(s: SettingsData): Record<string, unknown> {
  return {
    editor_font_size: s.editorFontSize,
    goal_state_font_size: s.goalStateFontSize,
    prose_proof_font_size: s.proseProofFontSize,
    assistant_font_size: s.assistantFontSize,
    assistant_model: s.assistantModel,
    translation_model: s.translationModel,
    assistant_prompt: s.assistantPrompt,
    translation_prompt: s.translationPrompt,
  }
}

// ── Reactive singleton ────────────────────────────────────────────────

let editorFontSize = $state(DEFAULT_SETTINGS.editorFontSize)
let goalStateFontSize = $state(DEFAULT_SETTINGS.goalStateFontSize)
let proseProofFontSize = $state(DEFAULT_SETTINGS.proseProofFontSize)
let assistantFontSize = $state(DEFAULT_SETTINGS.assistantFontSize)
let assistantModel = $state<string | null>(DEFAULT_SETTINGS.assistantModel)
let translationModel = $state<string | null>(DEFAULT_SETTINGS.translationModel)
let assistantPrompt = $state<string | null>(DEFAULT_SETTINGS.assistantPrompt)
let translationPrompt = $state<string | null>(DEFAULT_SETTINGS.translationPrompt)
let availableModels = $state<ModelInfo[]>([])

export const settings = {
  get editorFontSize() {
    return editorFontSize
  },
  get goalStateFontSize() {
    return goalStateFontSize
  },
  get proseProofFontSize() {
    return proseProofFontSize
  },
  get assistantFontSize() {
    return assistantFontSize
  },
  get assistantModel() {
    return assistantModel
  },
  get translationModel() {
    return translationModel
  },
  get assistantPrompt() {
    return assistantPrompt
  },
  get translationPrompt() {
    return translationPrompt
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
  goalStateFontSize = s.goalStateFontSize
  proseProofFontSize = s.proseProofFontSize
  assistantFontSize = s.assistantFontSize
  assistantModel = s.assistantModel
  translationModel = s.translationModel
  assistantPrompt = s.assistantPrompt
  translationPrompt = s.translationPrompt
}

function currentValues(): SettingsData {
  return {
    editorFontSize,
    goalStateFontSize,
    proseProofFontSize,
    assistantFontSize,
    assistantModel,
    translationModel,
    assistantPrompt,
    translationPrompt,
  }
}

type SettingsKey = keyof SettingsData
type SettingsValue = SettingsData[SettingsKey]

function assignField(key: SettingsKey, value: SettingsValue): void {
  if (key === 'editorFontSize' && typeof value === 'number') editorFontSize = value
  else if (key === 'goalStateFontSize' && typeof value === 'number') goalStateFontSize = value
  else if (key === 'proseProofFontSize' && typeof value === 'number') proseProofFontSize = value
  else if (key === 'assistantFontSize' && typeof value === 'number') assistantFontSize = value
  else if (key === 'assistantModel') assistantModel = typeof value === 'string' ? value : null
  else if (key === 'translationModel') translationModel = typeof value === 'string' ? value : null
  else if (key === 'assistantPrompt') assistantPrompt = typeof value === 'string' ? value : null
  else if (key === 'translationPrompt') translationPrompt = typeof value === 'string' ? value : null
}

export async function updateSetting(key: SettingsKey, value: SettingsValue): Promise<void> {
  const previous = currentValues()
  assignField(key, value)
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

// ── Default prompt fetchers ──────────────────────────────────────────

/**
 * Fetch the baked-in default assistant prompt from the Rust backend.
 * Used by the Settings UI to pre-fill the textarea when the user hasn't
 * set their own prompt.
 */
export async function getDefaultAssistantPrompt(): Promise<string> {
  return invoke<string>('get_default_assistant_prompt')
}

/**
 * Fetch the baked-in default translation prompt from the Rust backend.
 */
export async function getDefaultTranslationPrompt(): Promise<string> {
  return invoke<string>('get_default_translation_prompt')
}

// ── Draft state for deferred Apply ───────────────────────────────────

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
  goalStateFontSize = $state(DEFAULT_SETTINGS.goalStateFontSize)
  proseProofFontSize = $state(DEFAULT_SETTINGS.proseProofFontSize)
  assistantFontSize = $state(DEFAULT_SETTINGS.assistantFontSize)
  assistantModel = $state<string | null>(DEFAULT_SETTINGS.assistantModel)
  translationModel = $state<string | null>(DEFAULT_SETTINGS.translationModel)
  assistantPrompt = $state<string | null>(DEFAULT_SETTINGS.assistantPrompt)
  translationPrompt = $state<string | null>(DEFAULT_SETTINGS.translationPrompt)

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

  set(key: SettingsKey, value: SettingsValue): void {
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

export type { SettingsDraft }
