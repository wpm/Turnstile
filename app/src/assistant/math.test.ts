import { describe, it, expect } from 'vitest'
import { parseMathSegments } from './math'

describe('parseMathSegments', () => {
  it('returns a single text segment for plain text with no math', () => {
    const segments = parseMathSegments('hello world')
    expect(segments).toEqual([{ type: 'text', content: 'hello world' }])
  })

  it('extracts inline math between single dollars', () => {
    const segments = parseMathSegments('prefix $x + 1$ suffix')
    expect(segments).toEqual([
      { type: 'text', content: 'prefix ' },
      { type: 'math', content: 'x + 1', display: false },
      { type: 'text', content: ' suffix' },
    ])
  })

  it('extracts display math between double dollars', () => {
    const segments = parseMathSegments('before $$\\frac{a}{b}$$ after')
    expect(segments).toEqual([
      { type: 'text', content: 'before ' },
      { type: 'math', content: '\\frac{a}{b}', display: true },
      { type: 'text', content: ' after' },
    ])
  })

  it('handles mixed text, inline math, and display math', () => {
    const input = 'Proof: $a = b$ and $$c = d$$ done'
    const segments = parseMathSegments(input)
    expect(segments.length).toBe(5)
    expect(segments[0]).toEqual({ type: 'text', content: 'Proof: ' })
    expect(segments[1]).toEqual({ type: 'math', content: 'a = b', display: false })
    expect(segments[2]).toEqual({ type: 'text', content: ' and ' })
    expect(segments[3]).toEqual({ type: 'math', content: 'c = d', display: true })
    expect(segments[4]).toEqual({ type: 'text', content: ' done' })
  })

  it('handles text with no surrounding whitespace', () => {
    const segments = parseMathSegments('$x$')
    expect(segments).toEqual([{ type: 'math', content: 'x', display: false }])
  })

  it('returns empty array for empty string', () => {
    const segments = parseMathSegments('')
    expect(segments).toEqual([])
  })

  it('display math takes precedence over inline match inside $$', () => {
    // $$...$$ should not be parsed as two $...$ sequences
    const segments = parseMathSegments('$$E = mc^2$$')
    expect(segments).toHaveLength(1)
    expect(segments[0]).toMatchObject({ type: 'math', display: true, content: 'E = mc^2' })
  })
})
