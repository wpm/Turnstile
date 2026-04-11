import { describe, it, expect } from 'vitest'
import { highlightLean } from './leanHighlight'

describe('highlightLean', () => {
  it('wraps keywords in cm-lean-keyword spans', () => {
    const result = highlightLean('def foo := 42')
    expect(result).toContain('cm-lean-keyword')
    expect(result).toContain('def')
  })

  it('wraps string literals in cm-lean-string spans', () => {
    const result = highlightLean('def s := "hello"')
    expect(result).toContain('cm-lean-string')
    // The string content is HTML-escaped inside the span
    expect(result).toContain('&quot;hello&quot;')
  })

  it('wraps line comments in cm-lean-comment spans', () => {
    const result = highlightLean('-- this is a comment')
    expect(result).toContain('cm-lean-comment')
    expect(result).toContain('-- this is a comment')
  })

  it('wraps numbers in cm-lean-number spans', () => {
    const result = highlightLean('def n := 42')
    expect(result).toContain('cm-lean-number')
    expect(result).toContain('42')
  })

  it('does not wrap plain identifiers', () => {
    const result = highlightLean('myIdentifier')
    // No span wrapping — plain identifiers returned as text
    expect(result).not.toContain('<span')
    expect(result).toContain('myIdentifier')
  })

  it('escapes HTML angle brackets in code content', () => {
    const result = highlightLean('-- <script>')
    expect(result).not.toContain('<script>')
    expect(result).toContain('&lt;script&gt;')
  })

  it('escapes ampersands in code content', () => {
    const result = highlightLean('-- a & b')
    expect(result).toContain('&amp;')
  })

  it('handles multiple keywords on one line', () => {
    const result = highlightLean('theorem foo : True := by')
    const matches = result.match(/cm-lean-keyword/g)
    expect(matches).not.toBeNull()
    // 'theorem' and 'by' are both keywords
    if (matches) expect(matches.length).toBeGreaterThanOrEqual(2)
  })

  it('returns empty string for empty input', () => {
    expect(highlightLean('')).toBe('')
  })
})
