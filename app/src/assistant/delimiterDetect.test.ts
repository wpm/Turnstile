import { describe, it, expect } from 'vitest'
import { detectCompletedDelimiter, isInsideOpenDelimiter } from './delimiterDetect'
import type { DelimitedSpan } from './delimiterDetect'

/** Assert that result is non-null and return it with narrowed type. */
function expectNonNull(result: DelimitedSpan | null): DelimitedSpan {
  if (result === null) throw new Error('Expected non-null result')
  return result
}

describe('detectCompletedDelimiter', () => {
  // ── Inline code (backticks) ─────────────────────────────────────────

  describe('inline code', () => {
    it('detects completed backtick pair', () => {
      const r = expectNonNull(detectCompletedDelimiter('`code`', 6))
      expect(r.kind).toBe('inline-code')
      expect(r.content).toBe('code')
      expect(r.from).toBe(0)
      expect(r.to).toBe(6)
    })

    it('detects backtick pair with surrounding text', () => {
      const r = expectNonNull(detectCompletedDelimiter('hello `theorem` world', 15))
      expect(r.kind).toBe('inline-code')
      expect(r.content).toBe('theorem')
      expect(r.from).toBe(6)
      expect(r.to).toBe(15)
    })

    it('returns null for lone backtick with no opener', () => {
      expect(detectCompletedDelimiter('hello`', 6)).toBeNull()
    })

    it('returns null for empty backtick pair', () => {
      expect(detectCompletedDelimiter('``', 2)).toBeNull()
    })

    it('does not match when last char is not a backtick', () => {
      expect(detectCompletedDelimiter('`code', 5)).toBeNull()
    })
  })

  // ── Inline math ($...$) ─────────────────────────────────────────────

  describe('inline math', () => {
    it('detects completed single-dollar pair', () => {
      const r = expectNonNull(detectCompletedDelimiter('$x^2$', 5))
      expect(r.kind).toBe('inline-math')
      expect(r.content).toBe('x^2')
      expect(r.from).toBe(0)
      expect(r.to).toBe(5)
    })

    it('detects inline math with surrounding text', () => {
      const r = expectNonNull(detectCompletedDelimiter('proof: $a + b$ end', 14))
      expect(r.kind).toBe('inline-math')
      expect(r.content).toBe('a + b')
      expect(r.from).toBe(7)
      expect(r.to).toBe(14)
    })

    it('returns null for lone dollar with no opener', () => {
      expect(detectCompletedDelimiter('hello$', 6)).toBeNull()
    })

    it('returns null for empty dollar pair', () => {
      // $$ is display math opener, not an empty inline math
      expect(detectCompletedDelimiter('$$', 2)).toBeNull()
    })
  })

  // ── Display math ($$...$$) ──────────────────────────────────────────

  describe('display math', () => {
    it('detects completed double-dollar pair', () => {
      const r = expectNonNull(detectCompletedDelimiter('$$\\frac{a}{b}$$', 15))
      expect(r.kind).toBe('display-math')
      expect(r.content).toBe('\\frac{a}{b}')
      expect(r.from).toBe(0)
      expect(r.to).toBe(15)
    })

    it('detects display math with surrounding text', () => {
      const r = expectNonNull(detectCompletedDelimiter('See: $$x = y$$ done', 14))
      expect(r.kind).toBe('display-math')
      expect(r.content).toBe('x = y')
      expect(r.from).toBe(5)
      expect(r.to).toBe(14)
    })

    it('returns null for $$ with no closing pair', () => {
      expect(detectCompletedDelimiter('$$hello', 7)).toBeNull()
    })

    it('returns null for empty display math', () => {
      expect(detectCompletedDelimiter('$$$$', 4)).toBeNull()
    })
  })

  // ── Edge cases ──────────────────────────────────────────────────────

  describe('edge cases', () => {
    it('returns null for cursor at position 0', () => {
      expect(detectCompletedDelimiter('', 0)).toBeNull()
    })

    it('returns null when no delimiter at cursor', () => {
      expect(detectCompletedDelimiter('hello world', 5)).toBeNull()
    })

    it('does not match escaped dollar', () => {
      expect(detectCompletedDelimiter('\\$x\\$', 5)).toBeNull()
    })

    it('display math takes priority over inline math', () => {
      // When we type the second $ of $$...$$ closing, it should match display
      const r = expectNonNull(detectCompletedDelimiter('$$x$$', 5))
      expect(r.kind).toBe('display-math')
      expect(r.content).toBe('x')
    })
  })
})

// ── isInsideOpenDelimiter ─────────────────────────────────────────────

describe('isInsideOpenDelimiter', () => {
  describe('returns true when cursor is inside an open delimiter', () => {
    it('open inline math: $\\alpha', () => {
      expect(isInsideOpenDelimiter('$\\alpha', 7)).toBe(true)
    })

    it('just after opening $', () => {
      expect(isInsideOpenDelimiter('$', 1)).toBe(true)
    })

    it('open backtick: `code ', () => {
      expect(isInsideOpenDelimiter('`code ', 6)).toBe(true)
    })

    it('open display math: $$\\frac{', () => {
      expect(isInsideOpenDelimiter('$$\\frac{', 8)).toBe(true)
    })

    it('just after opening $$', () => {
      expect(isInsideOpenDelimiter('$$', 2)).toBe(true)
    })

    it('closed then reopened: $a$ $b', () => {
      expect(isInsideOpenDelimiter('$a$ $b', 6)).toBe(true)
    })

    it('text before open math: text $x + ', () => {
      expect(isInsideOpenDelimiter('text $x + ', 10)).toBe(true)
    })
  })

  describe('returns false when cursor is not inside any delimiter', () => {
    it('empty input', () => {
      expect(isInsideOpenDelimiter('', 0)).toBe(false)
    })

    it('plain text', () => {
      expect(isInsideOpenDelimiter('hello', 5)).toBe(false)
    })

    it('closed inline math: $x$', () => {
      expect(isInsideOpenDelimiter('$x$', 3)).toBe(false)
    })

    it('closed backtick: `code`', () => {
      expect(isInsideOpenDelimiter('`code`', 6)).toBe(false)
    })

    it('closed display math: $$x$$', () => {
      expect(isInsideOpenDelimiter('$$x$$', 5)).toBe(false)
    })

    it('backslash sequence without delimiter: \\alpha', () => {
      expect(isInsideOpenDelimiter('\\alpha', 6)).toBe(false)
    })

    it('escaped dollar is not an opener: \\$x', () => {
      expect(isInsideOpenDelimiter('\\$x', 3)).toBe(false)
    })

    it('after closed math with trailing text: $a$ text', () => {
      expect(isInsideOpenDelimiter('$a$ text', 8)).toBe(false)
    })
  })
})
