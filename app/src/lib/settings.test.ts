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
vi.mock('./tauri', () => ({
  invoke: vi.fn(),
}))

describe('parseSettings theme field', () => {
  it('parses theme "light" from raw settings', () => {
    const s = parseSettings({ theme: 'light' })
    expect(s.theme).toBe('light')
  })

  it('parses theme "dark" from raw settings', () => {
    const s = parseSettings({ theme: 'dark' })
    expect(s.theme).toBe('dark')
  })

  it('parses theme "auto" from raw settings', () => {
    const s = parseSettings({ theme: 'auto' })
    expect(s.theme).toBe('auto')
  })

  it('defaults theme to auto when missing', () => {
    const s = parseSettings({})
    expect(s.theme).toBe('auto')
  })

  it('defaults theme to auto when invalid type', () => {
    const s = parseSettings({ theme: 123 })
    expect(s.theme).toBe('auto')
  })

  it('defaults theme to auto when unrecognized value', () => {
    const s = parseSettings({ theme: 'purple' })
    expect(s.theme).toBe('auto')
  })
})

describe('parseSettings customPrompt field', () => {
  it('parses custom_prompt string from raw settings', () => {
    const s = parseSettings({ custom_prompt: 'Prefer tactic mode.' })
    expect(s.customPrompt).toBe('Prefer tactic mode.')
  })

  it('defaults customPrompt to empty string when missing', () => {
    const s = parseSettings({})
    expect(s.customPrompt).toBe('')
  })

  it('defaults customPrompt to empty string when non-string', () => {
    const s = parseSettings({ custom_prompt: 42 })
    expect(s.customPrompt).toBe('')
  })

  it('round-trips customPrompt through applySettings', () => {
    applySettings(parseSettings({ custom_prompt: 'Be terse.' }))
    expect(settings.customPrompt).toBe('Be terse.')
  })
})

describe('updateSetting', () => {
  beforeEach(async () => {
    // Reset to known state
    applySettings(parseSettings({}))
    const { invoke } = await import('./tauri')
    vi.mocked(invoke).mockReset()
  })

  it('persists a setting to the backend on success', async () => {
    const { invoke } = await import('./tauri')
    vi.mocked(invoke).mockResolvedValueOnce(undefined)

    await updateSetting('editorFontSize', 18)
    expect(settings.editorFontSize).toBe(18)
    expect(invoke).toHaveBeenCalledWith('save_settings', {
      settings: expect.objectContaining({ editor_font_size: 18 }) as unknown,
    })
  })

  it('rolls back local state when backend save fails', async () => {
    const { invoke } = await import('./tauri')
    vi.mocked(invoke).mockRejectedValueOnce(new Error('disk full'))

    const originalSize = settings.editorFontSize
    await expect(updateSetting('editorFontSize', 20)).rejects.toThrow('disk full')
    expect(settings.editorFontSize).toBe(originalSize)
  })
})

describe('resetToDefaults', () => {
  beforeEach(async () => {
    applySettings({
      editorFontSize: 20,
      proseFontSize: 20,
      chatFontSize: 20,
      model: 'gpt-4',
      theme: 'light',
      customPrompt: 'noise',
    })
    const { invoke } = await import('./tauri')
    vi.mocked(invoke).mockReset()
  })

  it('resets all settings and persists to backend', async () => {
    const { invoke } = await import('./tauri')
    vi.mocked(invoke).mockResolvedValueOnce(undefined)

    await resetToDefaults()
    expect(settings.editorFontSize).toBe(13)
    expect(settings.model).toBeNull()
    expect(settings.theme).toBe('auto')
    expect(settings.customPrompt).toBe('')
  })

  it('rolls back when backend save fails', async () => {
    const { invoke } = await import('./tauri')
    vi.mocked(invoke).mockRejectedValueOnce(new Error('save error'))

    await expect(resetToDefaults()).rejects.toThrow('save error')
    // Should have rolled back to the values set in beforeEach
    expect(settings.editorFontSize).toBe(20)
    expect(settings.model).toBe('gpt-4')
    expect(settings.theme).toBe('light')
    expect(settings.customPrompt).toBe('noise')
  })
})

describe('SettingsDraft', () => {
  beforeEach(async () => {
    applySettings(parseSettings({}))
    const { invoke } = await import('./tauri')
    vi.mocked(invoke).mockReset()
  })

  it('snapshots current committed values on creation', () => {
    applySettings({ ...DEFAULT_SETTINGS, editorFontSize: 16 })
    const draft = createDraft(['editorFontSize', 'proseFontSize'])
    expect(draft.editorFontSize).toBe(16)
    expect(draft.proseFontSize).toBe(DEFAULT_SETTINGS.proseFontSize)
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
    const { invoke } = await import('./tauri')
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
    const { invoke } = await import('./tauri')
    vi.mocked(invoke).mockRejectedValueOnce(new Error('disk full'))

    const draft = createDraft(['editorFontSize'])
    draft.set('editorFontSize', 20)
    await expect(draft.apply()).rejects.toThrow('disk full')

    expect(settings.editorFontSize).toBe(DEFAULT_SETTINGS.editorFontSize)
  })

  it('apply() resets dirty to false', async () => {
    const { invoke } = await import('./tauri')
    vi.mocked(invoke).mockResolvedValueOnce(undefined)

    const draft = createDraft(['editorFontSize'])
    draft.set('editorFontSize', 18)
    await draft.apply()
    expect(draft.dirty).toBe(false)
  })

  it('fillDefaults() sets fields to defaults and marks dirty', () => {
    applySettings({ ...DEFAULT_SETTINGS, editorFontSize: 20, proseFontSize: 18 })
    const draft = createDraft(['editorFontSize', 'proseFontSize'])
    expect(draft.dirty).toBe(false)

    draft.fillDefaults()
    expect(draft.editorFontSize).toBe(DEFAULT_SETTINGS.editorFontSize)
    expect(draft.proseFontSize).toBe(DEFAULT_SETTINGS.proseFontSize)
    expect(draft.dirty).toBe(true)
  })

  it('afterApply hook runs after successful persist', async () => {
    const { invoke } = await import('./tauri')
    vi.mocked(invoke).mockResolvedValueOnce(undefined)

    const afterApply = vi.fn()
    const draft = createDraft(['model'], { afterApply })
    draft.set('model', 'gpt-4')
    await draft.apply()

    expect(afterApply).toHaveBeenCalledWith(expect.objectContaining({ model: 'gpt-4' }) as unknown)
  })

  it('afterApply hook does NOT run on failure', async () => {
    const { invoke } = await import('./tauri')
    vi.mocked(invoke).mockRejectedValueOnce(new Error('fail'))

    const afterApply = vi.fn()
    const draft = createDraft(['model'], { afterApply })
    draft.set('model', 'gpt-4')
    await expect(draft.apply()).rejects.toThrow('fail')

    expect(afterApply).not.toHaveBeenCalled()
  })

  it('draft only affects its scoped keys', async () => {
    const { invoke } = await import('./tauri')
    vi.mocked(invoke).mockResolvedValueOnce(undefined)

    applySettings({ ...DEFAULT_SETTINGS, editorFontSize: 16, proseFontSize: 18 })
    const draft = createDraft(['editorFontSize'])
    draft.set('editorFontSize', 20)
    await draft.apply()

    expect(settings.editorFontSize).toBe(20)
    expect(settings.proseFontSize).toBe(18) // unchanged
  })
})
