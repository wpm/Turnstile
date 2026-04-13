import { describe, it, expect } from 'vitest'
import { toggleTheme, resolveTheme } from './theme'

describe('toggleTheme', () => {
  it('returns light when given dark', () => {
    expect(toggleTheme('dark')).toBe('light')
  })

  it('returns dark when given light', () => {
    expect(toggleTheme('light')).toBe('dark')
  })
})

describe('resolveTheme', () => {
  it('returns dark when preference is dark', () => {
    expect(resolveTheme('dark', 'light')).toBe('dark')
  })

  it('returns light when preference is light', () => {
    expect(resolveTheme('light', 'dark')).toBe('light')
  })

  it('returns system theme when preference is auto and system is dark', () => {
    expect(resolveTheme('auto', 'dark')).toBe('dark')
  })

  it('returns system theme when preference is auto and system is light', () => {
    expect(resolveTheme('auto', 'light')).toBe('light')
  })
})
