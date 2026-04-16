import { describe, it, expect } from 'vitest'
import { parseBlocks } from './markdown'

describe('parseBlocks', () => {
  it('returns a single code block for a bare fence', () => {
    const input = '```lean\n⊢ Nat\n```'
    expect(parseBlocks(input)).toEqual([{ type: 'code', lang: 'lean', content: '⊢ Nat\n' }])
  })

  it('defaults lang to "lean" when fence has no hint', () => {
    const input = '```\nfoo\n```'
    expect(parseBlocks(input)).toEqual([{ type: 'code', lang: 'lean', content: 'foo\n' }])
  })

  it('returns a single text block for plain prose', () => {
    expect(parseBlocks('no goals')).toEqual([{ type: 'text', content: 'no goals' }])
  })

  it('splits prose and code blocks', () => {
    const input = 'case foo\n```lean\n⊢ True\n```'
    expect(parseBlocks(input)).toEqual([
      { type: 'text', content: 'case foo' },
      { type: 'code', lang: 'lean', content: '⊢ True\n' },
    ])
  })

  it('handles multiple fenced blocks', () => {
    const input = '```lean\na\n```\ncase bar\n```lean\nb\n```'
    expect(parseBlocks(input)).toEqual([
      { type: 'code', lang: 'lean', content: 'a\n' },
      { type: 'text', content: 'case bar' },
      { type: 'code', lang: 'lean', content: 'b\n' },
    ])
  })

  it('returns empty array for empty string', () => {
    expect(parseBlocks('')).toEqual([])
  })

  it('omits whitespace-only text blocks between fences', () => {
    const input = '```lean\na\n```\n\n```lean\nb\n```'
    const blocks = parseBlocks(input)
    expect(blocks.every((b) => b.content.trim() !== '')).toBe(true)
  })
})
