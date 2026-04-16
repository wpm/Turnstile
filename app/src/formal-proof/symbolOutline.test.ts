import { describe, it, expect } from 'vitest'
import {
  flattenSymbols,
  fuzzyScore,
  filterSymbols,
  symbolKindTag,
  type FlatSymbol,
} from './symbolOutline'
import type { DocumentSymbolInfo } from './lspRequests'

function sym(name: string, kind: number, children: DocumentSymbolInfo[] = []): DocumentSymbolInfo {
  return {
    name,
    kind,
    start_line: 0,
    start_character: 0,
    end_line: 0,
    end_character: name.length,
    children,
  }
}

describe('flattenSymbols', () => {
  it('preserves preorder and tracks depth', () => {
    const tree: DocumentSymbolInfo[] = [
      sym('Ns', 3, [sym('inner1', 12), sym('inner2', 12, [sym('inner_inner', 13)])]),
      sym('top', 12),
    ]
    const flat = flattenSymbols(tree)
    expect(flat.map((f) => `${String(f.depth)}:${f.symbol.name}`)).toEqual([
      '0:Ns',
      '1:inner1',
      '1:inner2',
      '2:inner_inner',
      '0:top',
    ])
  })

  it('returns empty for an empty tree', () => {
    expect(flattenSymbols([])).toEqual([])
  })
})

describe('fuzzyScore', () => {
  it('matches a subsequence', () => {
    expect(fuzzyScore('tc', 'testCase')).not.toBeNull()
  })

  it('returns null when characters are not in order', () => {
    expect(fuzzyScore('tc', 'cat')).toBeNull()
  })

  it('prefers contiguous matches over scattered ones', () => {
    const contiguous = fuzzyScore('test', 'testSuite')
    const scattered = fuzzyScore('test', 'tXeXsXt')
    expect(contiguous).not.toBeNull()
    expect(scattered).not.toBeNull()
    if (contiguous === null || scattered === null) return
    expect(contiguous).toBeGreaterThan(scattered)
  })

  it('is case insensitive', () => {
    expect(fuzzyScore('FOO', 'foobar')).not.toBeNull()
    expect(fuzzyScore('foo', 'FOOBAR')).not.toBeNull()
  })

  it('returns 0 for an empty query', () => {
    expect(fuzzyScore('', 'anything')).toBe(0)
  })
})

describe('filterSymbols', () => {
  const tree: DocumentSymbolInfo[] = [
    sym('foobar', 12),
    sym('foo', 12),
    sym('bar', 12),
    sym('baz', 12),
  ]
  const flat: FlatSymbol[] = flattenSymbols(tree)

  it('returns the unfiltered list when query is empty', () => {
    expect(filterSymbols(flat, '').map((f) => f.symbol.name)).toEqual([
      'foobar',
      'foo',
      'bar',
      'baz',
    ])
  })

  it('filters by subsequence match', () => {
    const result = filterSymbols(flat, 'fo').map((f) => f.symbol.name)
    expect(result).toContain('foo')
    expect(result).toContain('foobar')
    expect(result).not.toContain('bar')
    expect(result).not.toContain('baz')
  })

  it('returns an empty list when nothing matches', () => {
    expect(filterSymbols(flat, 'xyz')).toEqual([])
  })

  it('ranks higher-scoring matches first (contiguous beats scattered)', () => {
    const result = filterSymbols(flat, 'foo').map((f) => f.symbol.name)
    expect(result[0]).toMatch(/^foo/)
  })
})

describe('symbolKindTag', () => {
  it('maps common Lean symbol kinds', () => {
    expect(symbolKindTag(3)).toBe('namespace')
    expect(symbolKindTag(12)).toBe('function')
    expect(symbolKindTag(22)).toBe('struct')
  })

  it('falls back to "symbol" for unknown kinds', () => {
    expect(symbolKindTag(999)).toBe('symbol')
  })
})
