import { describe, it, expect } from 'vitest'
import { findAbbrevReplacement, applyAbbrevReplacement } from './leanAbbrev'
import type { AbbrevReplacement } from './leanAbbrev'

/** Assert that result is non-null and return it with narrowed type. */
function expectNonNull(result: AbbrevReplacement | null): AbbrevReplacement {
  if (result === null) throw new Error('Expected non-null result')
  return result
}

describe('findAbbrevReplacement', () => {
  // ── Unambiguous abbreviations (eager replacement) ───────────────────

  describe('eager replacement for unambiguous abbreviations', () => {
    it('replaces \\alpha immediately (no other abbreviation starts with "alpha")', () => {
      const r = expectNonNull(findAbbrevReplacement('\\alpha', 6))
      expect(r.replacement).toBe('α')
      expect(r.from).toBe(0)
      expect(r.to).toBe(6)
    })

    it('replaces \\forall immediately', () => {
      const r = expectNonNull(findAbbrevReplacement('\\forall', 7))
      expect(r.replacement).toBe('∀')
    })
  })

  // ── Disambiguation by non-abbreviation character ────────────────────

  describe('replacement when followed by non-abbreviation character', () => {
    it('replaces \\to when followed by space', () => {
      const r = expectNonNull(findAbbrevReplacement('\\to ', 4))
      expect(r.replacement).toBe('→')
      expect(r.from).toBe(0)
      expect(r.to).toBe(3) // does not consume the trailing space
    })

    it('replaces \\lam when followed by space', () => {
      const r = expectNonNull(findAbbrevReplacement('\\lam ', 5))
      expect(r.replacement).toBe('λ')
      expect(r.from).toBe(0)
      expect(r.to).toBe(4)
    })

    it('replaces \\to when followed by period', () => {
      const r = expectNonNull(findAbbrevReplacement('\\to.', 4))
      expect(r.replacement).toBe('→')
    })
  })

  // ── Auto-closing pairs with $CURSOR ─────────────────────────────────

  describe('auto-closing pairs', () => {
    it('replaces \\< with ⟨ and sets cursorOffset', () => {
      const r = expectNonNull(findAbbrevReplacement('\\< ', 3))
      expect(r.replacement).toBe('⟨')
    })

    it('replaces \\<> with ⟨⟩ and sets cursorOffset between', () => {
      const r = expectNonNull(findAbbrevReplacement('\\<>', 3))
      expect(r.replacement).toBe('⟨⟩')
      expect(r.cursorOffset).toBe(1) // cursor between ⟨ and ⟩
    })
  })

  // ── Backslash escape ────────────────────────────────────────────────

  describe('backslash escape', () => {
    it('replaces \\\\ with \\', () => {
      const r = expectNonNull(findAbbrevReplacement('\\\\', 2))
      expect(r.replacement).toBe('\\')
    })
  })

  // ── Mid-string replacement ──────────────────────────────────────────

  describe('mid-string context', () => {
    it('finds abbreviation at end of longer text', () => {
      const r = expectNonNull(findAbbrevReplacement('hello \\alpha', 12))
      expect(r.from).toBe(6)
      expect(r.to).toBe(12)
      expect(r.replacement).toBe('α')
    })

    it('finds abbreviation with trailing char after longer prefix', () => {
      // 'x \to ' — cursor at 6, just after typing the space that disambiguates
      const r = expectNonNull(findAbbrevReplacement('x \\to ', 6))
      expect(r.from).toBe(2)
      expect(r.to).toBe(5)
      expect(r.replacement).toBe('→')
    })
  })

  // ── No match cases ──────────────────────────────────────────────────

  describe('no match', () => {
    it('returns null when no backslash present', () => {
      expect(findAbbrevReplacement('hello', 5)).toBeNull()
    })

    it('returns null for ambiguous abbreviation with no trailing char', () => {
      // \to is ambiguous (top, toa, etc.) — should not replace without trailing char
      expect(findAbbrevReplacement('\\to', 3)).toBeNull()
    })

    it('returns null for ambiguous prefix \\t', () => {
      expect(findAbbrevReplacement('\\t', 2)).toBeNull()
    })

    it('returns null for unknown abbreviation', () => {
      expect(findAbbrevReplacement('\\zzzzz ', 7)).toBeNull()
    })

    it('returns null when cursor is not at end of abbreviation', () => {
      expect(findAbbrevReplacement('\\alpha hello', 6)).not.toBeNull()
      // cursor at position 8 is in 'hello', not at end of abbreviation
      expect(findAbbrevReplacement('\\alpha hello', 8)).toBeNull()
    })
  })
})

describe('applyAbbrevReplacement', () => {
  it('splices replacement into text', () => {
    const result = applyAbbrevReplacement('hello \\alpha', {
      from: 6,
      to: 12,
      replacement: 'α',
      cursorOffset: null,
    })
    expect(result.newText).toBe('hello α')
    expect(result.newCursorPos).toBe(7) // after α
  })

  it('handles cursorOffset for auto-closing pairs', () => {
    const result = applyAbbrevReplacement('\\<>', {
      from: 0,
      to: 3,
      replacement: '⟨⟩',
      cursorOffset: 1,
    })
    expect(result.newText).toBe('⟨⟩')
    expect(result.newCursorPos).toBe(1) // between ⟨ and ⟩
  })

  it('preserves text after the replacement', () => {
    const result = applyAbbrevReplacement('x \\to ', {
      from: 2,
      to: 5,
      replacement: '→',
      cursorOffset: null,
    })
    expect(result.newText).toBe('x → ')
    expect(result.newCursorPos).toBe(3) // after →
  })
})
