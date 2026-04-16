import { describe, it, expect } from 'vitest'
import { tokenTypeToCssClass } from './tokenTypes'

describe('tokenTypeToCssClass', () => {
  it('maps keyword to cm-lean-keyword', () => {
    expect(tokenTypeToCssClass('keyword')).toBe('cm-lean-keyword')
  })

  it('maps all type variants to cm-lean-type', () => {
    for (const t of ['type', 'class', 'struct', 'enum', 'interface', 'typeParameter']) {
      expect(tokenTypeToCssClass(t)).not.toBeNull()
    }
  })

  it('returns null for unknown token types', () => {
    expect(tokenTypeToCssClass('unknown_future_type')).toBeNull()
  })

  it('maps variable with declaration modifier to cm-lean-function', () => {
    expect(tokenTypeToCssClass('variable', ['declaration'])).toBe('cm-lean-function')
  })

  it('maps plain variable to cm-lean-variable', () => {
    expect(tokenTypeToCssClass('variable', [])).toBe('cm-lean-variable')
    expect(tokenTypeToCssClass('variable')).toBe('cm-lean-variable')
  })

  it('maps property to cm-lean-property', () => {
    expect(tokenTypeToCssClass('property')).toBe('cm-lean-property')
  })
})
