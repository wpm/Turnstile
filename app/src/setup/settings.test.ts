import { describe, it, expect, vi, beforeEach } from 'vitest'
import {
  parseSettings,
  applySettings,
  settings,
  updateSetting,
  resetToDefaults,
  createDraft,
  DEFAULT_SETTINGS,
} from './settings.svelte'

// Mock the tauri invoke function
vi.mock('../session/tauri', () => ({
  invoke: vi.fn(),
}))

describe('parseSettings basic shape', () => {
  it('returns defaults when raw is null', () => {
    expect(parseSettings(null)).toEqual(DEFAULT_SETTINGS)
  })

  it('returns defaults when raw is undefined', () => {
    expect(parseSettings(undefined)).toEqual(DEFAULT_SETTINGS)
  })

  it('parses all font size fields', () => {
    const s = parseSettings({
      editor_font_size: 15,
      goal_state_font_size: 14,
      prose_proof_font_size: 16,
      assistant_font_size: 18,
    })
    expect(s.editorFontSize).toBe(15)
    expect(s.goalStateFontSize).toBe(14)
    expect(s.proseProofFontSize).toBe(16)
    expect(s.assistantFontSize).toBe(18)
  })

  it('parses model fields', () => {
    const s = parseSettings({
      assistant_model: 'claude-opus-4-6',
      translation_model: 'claude-haiku-4-5-20251001',
    })
    expect(s.assistantModel).toBe('claude-opus-4-6')
    expect(s.translationModel).toBe('claude-haiku-4-5-20251001')
  })

  it('parses prompt fields', () => {
    const s = parseSettings({
      assistant_prompt: 'be terse',
      translation_prompt: 'translate precisely',
    })
    expect(s.assistantPrompt).toBe('be terse')
    expect(s.translationPrompt).toBe('translate precisely')
  })

  it('defaults prompt fields to null when missing', () => {
    const s = parseSettings({})
    expect(s.assistantPrompt).toBeNull()
    expect(s.translationPrompt).toBeNull()
  })

  it('defaults prompt fields to null when non-string', () => {
    const s = parseSettings({ assistant_prompt: 42, translation_prompt: true })
    expect(s.assistantPrompt).toBeNull()
    expect(s.translationPrompt).toBeNull()
  })
})

describe('parseSettings legacy aliases', () => {
  it('falls back to prose_font_size when prose_proof_font_size is absent', () => {
    const s = parseSettings({ prose_font_size: 17 })
    expect(s.proseProofFontSize).toBe(17)
  })

  it('prefers prose_proof_font_size when both are present', () => {
    const s = parseSettings({ prose_font_size: 17, prose_proof_font_size: 19 })
    expect(s.proseProofFontSize).toBe(19)
  })

  it('falls back to model when assistant_model is absent', () => {
    const s = parseSettings({ model: 'claude-sonnet-4-6' })
    expect(s.assistantModel).toBe('claude-sonnet-4-6')
  })

  it('prefers assistant_model when both are present', () => {
    const s = parseSettings({ model: 'claude-sonnet-4-6', assistant_model: 'claude-opus-4-6' })
    expect(s.assistantModel).toBe('claude-opus-4-6')
  })
})

describe('updateSetting', () => {
  beforeEach(async () => {
    // Reset to known state
    applySettings(parseSettings({}))
    const { invoke } = await import('../session/tauri')
    vi.mocked(invoke).mockReset()
  })

  it('persists a setting to the backend on success', async () => {
    const { invoke } = await import('../session/tauri')
    vi.mocked(invoke).mockResolvedValueOnce(undefined)

    await updateSetting('editorFontSize', 18)
    expect(settings.editorFontSize).toBe(18)
    expect(invoke).toHaveBeenCalledWith('save_settings', {
      settings: expect.objectContaining({ editor_font_size: 18 }) as unknown,
    })
  })

  it('rolls back local state when backend save fails', async () => {
    const { invoke } = await import('../session/tauri')
    vi.mocked(invoke).mockRejectedValueOnce(new Error('disk full'))

    const originalSize = settings.editorFontSize
    await expect(updateSetting('editorFontSize', 20)).rejects.toThrow('disk full')
    expect(settings.editorFontSize).toBe(originalSize)
  })

  it('serializes the new key shape', async () => {
    const { invoke } = await import('../session/tauri')
    vi.mocked(invoke).mockResolvedValueOnce(undefined)

    await updateSetting('goalStateFontSize', 15)
    expect(invoke).toHaveBeenCalledWith('save_settings', {
      settings: expect.objectContaining({
        goal_state_font_size: 15,
        prose_proof_font_size: expect.any(Number) as unknown,
        assistant_model: null,
        translation_model: null,
        assistant_prompt: null,
        translation_prompt: null,
      }) as unknown,
    })
  })
})

describe('resetToDefaults', () => {
  beforeEach(async () => {
    applySettings({
      editorFontSize: 20,
      goalStateFontSize: 18,
      proseProofFontSize: 20,
      assistantFontSize: 20,
      assistantModel: 'claude-opus-4-6',
      translationModel: 'claude-sonnet-4-6',
      assistantPrompt: 'be terse',
      translationPrompt: 'translate loosely',
    })
    const { invoke } = await import('../session/tauri')
    vi.mocked(invoke).mockReset()
  })

  it('resets all settings and persists to backend', async () => {
    const { invoke } = await import('../session/tauri')
    vi.mocked(invoke).mockResolvedValueOnce(undefined)

    await resetToDefaults()
    expect(settings.editorFontSize).toBe(13)
    expect(settings.goalStateFontSize).toBe(13)
    expect(settings.proseProofFontSize).toBe(13)
    expect(settings.assistantFontSize).toBe(13)
    expect(settings.assistantModel).toBeNull()
    expect(settings.translationModel).toBeNull()
    expect(settings.assistantPrompt).toBeNull()
    expect(settings.translationPrompt).toBeNull()
  })

  it('rolls back when backend save fails', async () => {
    const { invoke } = await import('../session/tauri')
    vi.mocked(invoke).mockRejectedValueOnce(new Error('save error'))

    await expect(resetToDefaults()).rejects.toThrow('save error')
    // Should have rolled back to the values set in beforeEach
    expect(settings.editorFontSize).toBe(20)
    expect(settings.assistantModel).toBe('claude-opus-4-6')
    expect(settings.assistantPrompt).toBe('be terse')
  })
})

describe('SettingsDraft', () => {
  beforeEach(async () => {
    applySettings(parseSettings({}))
    const { invoke } = await import('../session/tauri')
    vi.mocked(invoke).mockReset()
  })

  it('snapshots current committed values on creation', () => {
    applySettings({ ...DEFAULT_SETTINGS, editorFontSize: 16 })
    const draft = createDraft(['editorFontSize', 'goalStateFontSize'])
    expect(draft.editorFontSize).toBe(16)
    expect(draft.goalStateFontSize).toBe(DEFAULT_SETTINGS.goalStateFontSize)
  })

  it('set() updates a draft field', () => {
    const draft = createDraft(['editorFontSize'])
    draft.set('editorFontSize', 20)
    expect(draft.editorFontSize).toBe(20)
  })

  it('dirty is false initially', () => {
    const draft = createDraft(['editorFontSize'])
    expect(draft.dirty).toBe(false)
  })

  it('dirty becomes true after set()', () => {
    const draft = createDraft(['editorFontSize'])
    draft.set('editorFontSize', 20)
    expect(draft.dirty).toBe(true)
  })

  it('dirty returns to false after discard()', () => {
    const draft = createDraft(['editorFontSize'])
    draft.set('editorFontSize', 20)
    draft.discard()
    expect(draft.dirty).toBe(false)
    expect(draft.editorFontSize).toBe(DEFAULT_SETTINGS.editorFontSize)
  })

  it('apply() persists to backend and updates singleton', async () => {
    const { invoke } = await import('../session/tauri')
    vi.mocked(invoke).mockResolvedValueOnce(undefined)

    const draft = createDraft(['editorFontSize'])
    draft.set('editorFontSize', 18)
    await draft.apply()

    expect(settings.editorFontSize).toBe(18)
    expect(invoke).toHaveBeenCalledWith('save_settings', {
      settings: expect.objectContaining({ editor_font_size: 18 }) as unknown,
    })
  })

  it('apply() rolls back singleton on failure', async () => {
    const { invoke } = await import('../session/tauri')
    vi.mocked(invoke).mockRejectedValueOnce(new Error('disk full'))

    const draft = createDraft(['editorFontSize'])
    draft.set('editorFontSize', 20)
    await expect(draft.apply()).rejects.toThrow('disk full')

    expect(settings.editorFontSize).toBe(DEFAULT_SETTINGS.editorFontSize)
  })

  it('apply() resets dirty to false', async () => {
    const { invoke } = await import('../session/tauri')
    vi.mocked(invoke).mockResolvedValueOnce(undefined)

    const draft = createDraft(['editorFontSize'])
    draft.set('editorFontSize', 18)
    await draft.apply()
    expect(draft.dirty).toBe(false)
  })

  it('fillDefaults() sets fields to defaults and marks dirty', () => {
    applySettings({ ...DEFAULT_SETTINGS, editorFontSize: 20, goalStateFontSize: 18 })
    const draft = createDraft(['editorFontSize', 'goalStateFontSize'])
    expect(draft.dirty).toBe(false)

    draft.fillDefaults()
    expect(draft.editorFontSize).toBe(DEFAULT_SETTINGS.editorFontSize)
    expect(draft.goalStateFontSize).toBe(DEFAULT_SETTINGS.goalStateFontSize)
    expect(draft.dirty).toBe(true)
  })

  it('afterApply hook runs after successful persist', async () => {
    const { invoke } = await import('../session/tauri')
    vi.mocked(invoke).mockResolvedValueOnce(undefined)

    const afterApply = vi.fn()
    const draft = createDraft(['assistantModel'], { afterApply })
    draft.set('assistantModel', 'claude-opus-4-6')
    await draft.apply()

    expect(afterApply).toHaveBeenCalledWith(
      expect.objectContaining({ assistantModel: 'claude-opus-4-6' }) as unknown,
    )
  })

  it('afterApply hook does NOT run on failure', async () => {
    const { invoke } = await import('../session/tauri')
    vi.mocked(invoke).mockRejectedValueOnce(new Error('fail'))

    const afterApply = vi.fn()
    const draft = createDraft(['assistantModel'], { afterApply })
    draft.set('assistantModel', 'claude-opus-4-6')
    await expect(draft.apply()).rejects.toThrow('fail')

    expect(afterApply).not.toHaveBeenCalled()
  })

  it('draft only affects its scoped keys', async () => {
    const { invoke } = await import('../session/tauri')
    vi.mocked(invoke).mockResolvedValueOnce(undefined)

    applySettings({ ...DEFAULT_SETTINGS, editorFontSize: 16, goalStateFontSize: 18 })
    const draft = createDraft(['editorFontSize'])
    draft.set('editorFontSize', 20)
    await draft.apply()

    expect(settings.editorFontSize).toBe(20)
    expect(settings.goalStateFontSize).toBe(18) // unchanged
  })

  it('supports prompt draft with null round trip', () => {
    applySettings({ ...DEFAULT_SETTINGS, assistantPrompt: null })
    const draft = createDraft(['assistantPrompt'])
    expect(draft.assistantPrompt).toBeNull()
    draft.set('assistantPrompt', 'custom')
    expect(draft.dirty).toBe(true)
    draft.set('assistantPrompt', null)
    expect(draft.dirty).toBe(false)
  })
})
