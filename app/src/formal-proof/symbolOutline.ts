/**
 * Helpers for the symbol-outline command palette.
 *
 * Rendering is a Svelte component (`SymbolOutline.svelte`); this module
 * houses the pure logic (flattening the symbol tree, fuzzy filtering, kind
 * icons) so it can be tested without a DOM.
 */

import type { DocumentSymbolInfo } from './lspRequests'

export interface FlatSymbol {
  symbol: DocumentSymbolInfo
  depth: number
}

/** Depth-first flatten of a symbol tree, preserving pre-order. */
export function flattenSymbols(symbols: DocumentSymbolInfo[], depth = 0): FlatSymbol[] {
  const out: FlatSymbol[] = []
  for (const sym of symbols) {
    out.push({ symbol: sym, depth })
    if (sym.children.length > 0) {
      out.push(...flattenSymbols(sym.children, depth + 1))
    }
  }
  return out
}

/**
 * Subsequence fuzzy match. Returns a score (higher is better) or null when
 * `needle` is not a subsequence of `haystack`. Contiguous runs earn a bonus,
 * and earlier matches score higher.
 *
 * Empty needles always match with score 0.
 */
export function fuzzyScore(needle: string, haystack: string): number | null {
  if (needle === '') return 0
  const n = needle.toLowerCase()
  const h = haystack.toLowerCase()
  let score = 0
  let last = -1
  let run = 0
  let ni = 0
  for (let i = 0; i < h.length && ni < n.length; i++) {
    if (h[i] === n[ni]) {
      const contiguous = i === last + 1
      run = contiguous ? run + 1 : 1
      // Earlier matches: tiny bonus; contiguous runs: larger bonus.
      score += 1 + run * 2 + Math.max(0, 5 - i)
      last = i
      ni++
    }
  }
  if (ni < n.length) return null
  return score
}

/**
 * Filter and rank flat symbols by the query. An empty query returns the list
 * as-is. Ties are broken by original order (stable sort).
 */
export function filterSymbols(flat: FlatSymbol[], query: string): FlatSymbol[] {
  if (query.trim() === '') return flat.slice()
  const scored: { entry: FlatSymbol; score: number; index: number }[] = []
  flat.forEach((entry, index) => {
    const s = fuzzyScore(query, entry.symbol.name)
    if (s !== null) scored.push({ entry, score: s, index })
  })
  scored.sort((a, b) => b.score - a.score || a.index - b.index)
  return scored.map((s) => s.entry)
}

/**
 * Map an LSP `SymbolKind` numeric code to a short textual tag.
 * See <https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/#symbolKind>.
 */
export function symbolKindTag(kind: number): string {
  // Only include kinds Lean actually emits; everything else falls through.
  switch (kind) {
    case 2:
      return 'module'
    case 3:
      return 'namespace'
    case 5:
      return 'class'
    case 6:
      return 'method'
    case 7:
      return 'property'
    case 10:
      return 'enum'
    case 11:
      return 'interface'
    case 12:
      return 'function'
    case 13:
      return 'variable'
    case 14:
      return 'constant'
    case 22:
      return 'struct'
    case 23:
      return 'event'
    case 25:
      return 'type'
    default:
      return 'symbol'
  }
}
