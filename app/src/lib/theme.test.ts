import { describe, it, expect } from 'vitest'
import { toggleTheme } from './theme'

describe('toggleTheme', () => {
  it('returns light when given dark', () => {
    expect(toggleTheme('dark')).toBe('light')
  })

  it('returns dark when given light', () => {
    expect(toggleTheme('light')).toBe('dark')
  })
})
