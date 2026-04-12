import { describe, it, expect } from 'vitest'
import { parseSettings } from './settings.svelte'

describe('parseSettings theme field', () => {
  it('parses theme from raw settings', () => {
    const s = parseSettings({ theme: 'light' })
    expect(s.theme).toBe('light')
  })

  it('defaults theme to dark when missing', () => {
    const s = parseSettings({})
    expect(s.theme).toBe('dark')
  })

  it('defaults theme to dark when invalid type', () => {
    const s = parseSettings({ theme: 123 })
    expect(s.theme).toBe('dark')
  })

  it('defaults theme to dark when unrecognized value', () => {
    const s = parseSettings({ theme: 'purple' })
    expect(s.theme).toBe('dark')
  })
})
