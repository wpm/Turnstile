import { describe, it, expect } from 'vitest'
import type { DiagnosticInfo, SemanticToken } from './tauri'
import {
  type DocLike,
  diagRange,
  diagnosticSeverityClass,
  diagnosticGutterClass,
  diagnosticPopupClass,
  semanticTokenRange,
  buildSemanticTokenRanges,
  buildDiagnosticRanges,
} from './editorHelpers'

// ---------------------------------------------------------------------------
// Helpers — minimal DocLike that mirrors a simple multi-line document.
// ---------------------------------------------------------------------------

/**
 * Build a DocLike from raw text. Each line's `from` is its character offset
 * in the full document string.
 */
function makeDoc(text: string): DocLike {
  const rawLines = text.split('\n')
  const lineStarts: number[] = []
  let offset = 0
  for (const raw of rawLines) {
    lineStarts.push(offset)
    offset += raw.length + 1 // +1 for '\n'
  }
  return {
    lines: rawLines.length,
    length: text.length,
    line(n: number): { from: number } {
      const start = lineStarts[n - 1]
      if (start === undefined) throw new Error(`line ${String(n)} out of range`)
      return { from: start }
    },
  }
}

// ---------------------------------------------------------------------------
// diagnosticSeverityClass
// ---------------------------------------------------------------------------

describe('diagnosticSeverityClass', () => {
  it('returns cm-diag-error for severity 1', () => {
    expect(diagnosticSeverityClass(1)).toBe('cm-diag-error')
  })

  it('returns cm-diag-warning for severity 2', () => {
    expect(diagnosticSeverityClass(2)).toBe('cm-diag-warning')
  })

  it('returns cm-diag-info for severity 3 (info)', () => {
    expect(diagnosticSeverityClass(3)).toBe('cm-diag-info')
  })

  it('returns cm-diag-info for severity 4 (hint)', () => {
    expect(diagnosticSeverityClass(4)).toBe('cm-diag-info')
  })
})

// ---------------------------------------------------------------------------
// diagnosticGutterClass
// ---------------------------------------------------------------------------

describe('diagnosticGutterClass', () => {
  it('maps severity 1 to lean-diag-error', () => {
    expect(diagnosticGutterClass(1)).toBe('lean-diag-error')
  })

  it('maps severity 2 to lean-diag-warning', () => {
    expect(diagnosticGutterClass(2)).toBe('lean-diag-warning')
  })

  it('maps severity 3+ to lean-diag-info', () => {
    expect(diagnosticGutterClass(3)).toBe('lean-diag-info')
    expect(diagnosticGutterClass(4)).toBe('lean-diag-info')
  })
})

// ---------------------------------------------------------------------------
// diagnosticPopupClass
// ---------------------------------------------------------------------------

describe('diagnosticPopupClass', () => {
  it('maps severity 1 to lean-diag-popup-error', () => {
    expect(diagnosticPopupClass(1)).toBe('lean-diag-popup-error')
  })

  it('maps severity 2 to lean-diag-popup-warning', () => {
    expect(diagnosticPopupClass(2)).toBe('lean-diag-popup-warning')
  })

  it('maps severity 3+ to lean-diag-popup-info', () => {
    expect(diagnosticPopupClass(3)).toBe('lean-diag-popup-info')
  })
})

// ---------------------------------------------------------------------------
// diagRange
// ---------------------------------------------------------------------------

describe('diagRange', () => {
  // "hello\nworld" — line 1 starts at 0, line 2 starts at 6
  const doc = makeDoc('hello\nworld')

  it('converts a single-line diagnostic to character offsets', () => {
    const diag: DiagnosticInfo = {
      start_line: 1,
      start_col: 1,
      end_line: 1,
      end_col: 4,
      severity: 1,
      message: 'err',
    }
    expect(diagRange(diag, doc)).toEqual({ from: 1, to: 4 })
  })

  it('converts a multi-line diagnostic to character offsets', () => {
    const diag: DiagnosticInfo = {
      start_line: 1,
      start_col: 3,
      end_line: 2,
      end_col: 2,
      severity: 2,
      message: 'warn',
    }
    expect(diagRange(diag, doc)).toEqual({ from: 3, to: 8 })
  })

  it('returns null when start_line is 0 (below lower bound)', () => {
    const diag: DiagnosticInfo = {
      start_line: 0,
      start_col: 0,
      end_line: 1,
      end_col: 3,
      severity: 1,
      message: 'err',
    }
    expect(diagRange(diag, doc)).toBeNull()
  })

  it('returns null when end_line exceeds document lines', () => {
    const diag: DiagnosticInfo = {
      start_line: 1,
      start_col: 0,
      end_line: 99,
      end_col: 1,
      severity: 1,
      message: 'err',
    }
    expect(diagRange(diag, doc)).toBeNull()
  })

  it('returns null when from >= to (empty span)', () => {
    const diag: DiagnosticInfo = {
      start_line: 1,
      start_col: 3,
      end_line: 1,
      end_col: 3,
      severity: 1,
      message: 'err',
    }
    expect(diagRange(diag, doc)).toBeNull()
  })

  it('returns null when to exceeds document length', () => {
    const diag: DiagnosticInfo = {
      start_line: 2,
      start_col: 0,
      end_line: 2,
      end_col: 100,
      severity: 1,
      message: 'err',
    }
    expect(diagRange(diag, doc)).toBeNull()
  })
})

// ---------------------------------------------------------------------------
// semanticTokenRange
// ---------------------------------------------------------------------------

describe('semanticTokenRange', () => {
  const doc = makeDoc('def foo := 42\ntheorem bar')

  it('maps a known token type to a range with CSS class', () => {
    const token: SemanticToken = { line: 1, col: 0, length: 3, token_type: 'keyword' }
    expect(semanticTokenRange(token, doc)).toEqual({
      from: 0,
      to: 3,
      cssClass: 'cm-lean-keyword',
    })
  })

  it('returns null for an unknown token type', () => {
    const token: SemanticToken = { line: 1, col: 0, length: 3, token_type: 'unknownType' }
    expect(semanticTokenRange(token, doc)).toBeNull()
  })

  it('returns null when line is out of range (0)', () => {
    const token: SemanticToken = { line: 0, col: 0, length: 3, token_type: 'keyword' }
    expect(semanticTokenRange(token, doc)).toBeNull()
  })

  it('returns null when line exceeds document lines', () => {
    const token: SemanticToken = { line: 99, col: 0, length: 3, token_type: 'keyword' }
    expect(semanticTokenRange(token, doc)).toBeNull()
  })

  it('returns null when token extends beyond document length', () => {
    const token: SemanticToken = { line: 2, col: 5, length: 500, token_type: 'function' }
    expect(semanticTokenRange(token, doc)).toBeNull()
  })

  it('handles a zero-length token (from === to)', () => {
    // from <= to is the condition, so from === to is valid
    const token: SemanticToken = { line: 1, col: 3, length: 0, token_type: 'variable' }
    expect(semanticTokenRange(token, doc)).toEqual({
      from: 3,
      to: 3,
      cssClass: 'cm-lean-variable',
    })
  })

  it('maps token on second line correctly', () => {
    // "theorem" starts at col 0 of line 2 (offset 14)
    const token: SemanticToken = { line: 2, col: 0, length: 7, token_type: 'keyword' }
    expect(semanticTokenRange(token, doc)).toEqual({
      from: 14,
      to: 21,
      cssClass: 'cm-lean-keyword',
    })
  })
})

// ---------------------------------------------------------------------------
// buildSemanticTokenRanges
// ---------------------------------------------------------------------------

describe('buildSemanticTokenRanges', () => {
  const doc = makeDoc('hello world')

  it('returns an empty array for an empty token list', () => {
    expect(buildSemanticTokenRanges([], doc)).toEqual([])
  })

  it('filters out unknown token types and sorts by from', () => {
    const tokens: SemanticToken[] = [
      { line: 1, col: 6, length: 5, token_type: 'variable' },
      { line: 1, col: 0, length: 5, token_type: 'keyword' },
      { line: 1, col: 3, length: 2, token_type: 'bogus' },
    ]
    const result = buildSemanticTokenRanges(tokens, doc)
    expect(result).toHaveLength(2)
    expect(result[0]?.from).toBe(0)
    expect(result[1]?.from).toBe(6)
  })
})

// ---------------------------------------------------------------------------
// buildDiagnosticRanges
// ---------------------------------------------------------------------------

describe('buildDiagnosticRanges', () => {
  const doc = makeDoc('line one\nline two')

  it('returns an empty array for no diagnostics', () => {
    expect(buildDiagnosticRanges([], doc)).toEqual([])
  })

  it('converts diagnostics and attaches severity class', () => {
    const diags: DiagnosticInfo[] = [
      { start_line: 1, start_col: 0, end_line: 1, end_col: 4, severity: 1, message: 'e' },
      { start_line: 2, start_col: 0, end_line: 2, end_col: 4, severity: 2, message: 'w' },
    ]
    const result = buildDiagnosticRanges(diags, doc)
    expect(result).toHaveLength(2)
    expect(result[0]).toEqual({ from: 0, to: 4, cssClass: 'cm-diag-error' })
    expect(result[1]).toEqual({ from: 9, to: 13, cssClass: 'cm-diag-warning' })
  })

  it('filters out diagnostics with out-of-range positions', () => {
    const diags: DiagnosticInfo[] = [
      { start_line: 99, start_col: 0, end_line: 99, end_col: 1, severity: 1, message: 'bad' },
      { start_line: 1, start_col: 0, end_line: 1, end_col: 4, severity: 3, message: 'ok' },
    ]
    const result = buildDiagnosticRanges(diags, doc)
    expect(result).toHaveLength(1)
    expect(result[0]?.cssClass).toBe('cm-diag-info')
  })
})
