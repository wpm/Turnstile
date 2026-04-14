import { describe, it, expect } from 'vitest'
import {
  extractTitleFromProse,
  extractLeanTheoremName,
  humanizeLeanName,
  getTheoremTitle,
  titleToFilename,
} from './theoremName'

describe('extractTitleFromProse', () => {
  it('extracts title from \\begin{theorem}[...]', () => {
    const prose = '\\begin{theorem}[Irrationality of √2]\nStatement.\n\\end{theorem}'
    expect(extractTitleFromProse(prose)).toBe('Irrationality of √2')
  })

  it('extracts title from \\begin{lemma}[...]', () => {
    const prose = '\\begin{lemma}[Pumping Lemma]\nBody.\n\\end{lemma}'
    expect(extractTitleFromProse(prose)).toBe('Pumping Lemma')
  })

  it('extracts title from \\begin{proposition}[...]', () => {
    const prose = '\\begin{proposition}[Unique Factorization]\nBody.\n\\end{proposition}'
    expect(extractTitleFromProse(prose)).toBe('Unique Factorization')
  })

  it('extracts title from \\begin{corollary}[...]', () => {
    const prose = '\\begin{corollary}[Infinitude of Primes]\nBody.\n\\end{corollary}'
    expect(extractTitleFromProse(prose)).toBe('Infinitude of Primes')
  })

  it('returns null when no title bracket exists', () => {
    const prose = '\\begin{theorem}\nStatement.\n\\end{theorem}'
    expect(extractTitleFromProse(prose)).toBeNull()
  })

  it('returns null for empty string', () => {
    expect(extractTitleFromProse('')).toBeNull()
  })

  it('returns first match when multiple environments exist', () => {
    const prose = [
      '\\begin{theorem}[First Title]',
      'Body.',
      '\\end{theorem}',
      '\\begin{lemma}[Second Title]',
      'Body.',
      '\\end{lemma}',
    ].join('\n')
    expect(extractTitleFromProse(prose)).toBe('First Title')
  })
})

describe('extractLeanTheoremName', () => {
  it('extracts theorem name', () => {
    expect(extractLeanTheoremName('theorem sqrt_two_irrational : ...')).toBe('sqrt_two_irrational')
  })

  it('extracts lemma name', () => {
    expect(extractLeanTheoremName('lemma nat_add_comm (n m : ℕ) : ...')).toBe('nat_add_comm')
  })

  it('extracts from multiline source', () => {
    const source = 'import Mathlib\n\ntheorem foo_bar : True := by\n  trivial'
    expect(extractLeanTheoremName(source)).toBe('foo_bar')
  })

  it('returns first match when multiple declarations exist', () => {
    const source = 'theorem first_thm : True := trivial\nlemma second_lem : True := trivial'
    expect(extractLeanTheoremName(source)).toBe('first_thm')
  })

  it('returns null for empty string', () => {
    expect(extractLeanTheoremName('')).toBeNull()
  })

  it('returns null when no theorem/lemma exists', () => {
    expect(extractLeanTheoremName('def foo := 42')).toBeNull()
  })
})

describe('humanizeLeanName', () => {
  it('splits underscores and capitalizes', () => {
    expect(humanizeLeanName('nat_add_comm')).toBe('Nat Add Comm')
  })

  it('handles single word', () => {
    expect(humanizeLeanName('trivial')).toBe('Trivial')
  })

  it('handles already-capitalized segments', () => {
    expect(humanizeLeanName('Nat_add')).toBe('Nat Add')
  })
})

describe('getTheoremTitle', () => {
  it('prefers prose title when available', () => {
    const prose = '\\begin{theorem}[Irrationality of √2]\n...\n\\end{theorem}'
    const lean = 'theorem sqrt_two_irrational : ...'
    expect(getTheoremTitle(prose, lean)).toBe('Irrationality of √2')
  })

  it('falls back to humanized Lean name when no prose title', () => {
    expect(getTheoremTitle('', 'theorem nat_add_comm : ...')).toBe('Nat Add Comm')
  })

  it('falls back to humanized Lean name when prose has no bracket title', () => {
    const prose = '\\begin{theorem}\nStatement.\n\\end{theorem}'
    expect(getTheoremTitle(prose, 'theorem foo_bar : ...')).toBe('Foo Bar')
  })

  it('returns "New Theorem" when neither source is available', () => {
    expect(getTheoremTitle('', '')).toBe('New Theorem')
  })

  it('returns "New Theorem" when Lean has no theorem declaration', () => {
    expect(getTheoremTitle('', 'def foo := 42')).toBe('New Theorem')
  })
})

describe('titleToFilename', () => {
  it('lowercases and hyphenates', () => {
    expect(titleToFilename('Irrationality of √2')).toBe('irrationality-of-2')
  })

  it('handles simple title', () => {
    expect(titleToFilename('Nat Add Comm')).toBe('nat-add-comm')
  })

  it('strips leading/trailing hyphens', () => {
    expect(titleToFilename('  Hello World  ')).toBe('hello-world')
  })

  it('collapses multiple special chars', () => {
    expect(titleToFilename('foo---bar')).toBe('foo-bar')
  })

  it('handles empty string', () => {
    expect(titleToFilename('')).toBe('')
  })
})
