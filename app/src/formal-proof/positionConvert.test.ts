import { describe, expect, it } from 'vitest'
import { Text } from '@codemirror/state'
import { cmLineToLsp, cmPosToLsp, lspLineToCm, lspPosToCmOffset } from './positionConvert'

describe('cmLineToLsp / lspLineToCm', () => {
  it('converts round-trip cleanly', () => {
    for (const lsp of [0, 1, 17, 9999]) {
      expect(cmLineToLsp(lspLineToCm(lsp))).toBe(lsp)
    }
  })

  it('maps CM 1 → LSP 0 (first line)', () => {
    expect(cmLineToLsp(1)).toBe(0)
    expect(lspLineToCm(0)).toBe(1)
  })
})

describe('cmPosToLsp', () => {
  it('maps the start of a document to (0, 0)', () => {
    const doc = Text.of(['hello', 'world'])
    expect(cmPosToLsp(doc, 0)).toEqual({ line: 0, character: 0 })
  })

  it('maps a mid-line position', () => {
    const doc = Text.of(['hello', 'world'])
    // offset 3 = "hel|lo" on line 1 → LSP (0, 3)
    expect(cmPosToLsp(doc, 3)).toEqual({ line: 0, character: 3 })
  })

  it('maps a position on the second line', () => {
    const doc = Text.of(['hello', 'world'])
    // "hello\n" is 6 chars; offset 8 = "wo|rld" → LSP (1, 2)
    expect(cmPosToLsp(doc, 8)).toEqual({ line: 1, character: 2 })
  })
})

describe('lspPosToCmOffset', () => {
  it('returns null for a line beyond the document', () => {
    const doc = Text.of(['hello'])
    expect(lspPosToCmOffset(doc, 5, 0)).toBeNull()
  })

  it('returns null for a negative line', () => {
    const doc = Text.of(['hello'])
    expect(lspPosToCmOffset(doc, -1, 0)).toBeNull()
  })

  it('maps LSP (0, 0) to the start of the first line', () => {
    const doc = Text.of(['hello', 'world'])
    const result = lspPosToCmOffset(doc, 0, 0)
    expect(result?.offset).toBe(0)
    expect(result?.line.number).toBe(1)
  })

  it('maps LSP (1, 2) to the third char of the second line', () => {
    const doc = Text.of(['hello', 'world'])
    const result = lspPosToCmOffset(doc, 1, 2)
    // "hello\n" = 6 chars, "wo" = 2 more → offset 8.
    expect(result?.offset).toBe(8)
    expect(result?.line.number).toBe(2)
  })

  it('clamps past-end-of-line characters to the line end', () => {
    const doc = Text.of(['hi', 'there'])
    // Line 1 ("hi") has length 2; asking for char 99 should clamp to 2.
    const result = lspPosToCmOffset(doc, 0, 99)
    expect(result?.offset).toBe(2)
  })
})
