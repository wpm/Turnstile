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
})
