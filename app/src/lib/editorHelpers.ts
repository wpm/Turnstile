/**
 * Pure data-transformation helpers extracted from editor.ts for testability.
 *
 * These functions convert LSP-style 1-indexed positions (DiagnosticInfo,
 * SemanticToken) into CodeMirror 6 character offsets, and map severity
 * codes to CSS class names. They depend only on a minimal document
 * interface, not on a full EditorView.
 */

import type { DiagnosticInfo, SemanticToken } from './tauri'
import { tokenTypeToCssClass } from './tokenTypes'

// ---------------------------------------------------------------------------
// Minimal document interface — just enough to convert line numbers to offsets.
// Matches the subset of CM6's Text that we actually use.
// ---------------------------------------------------------------------------

export interface DocLike {
  readonly lines: number
  readonly length: number
  line(n: number): { readonly from: number }
}

// ---------------------------------------------------------------------------
// Diagnostic severity → CSS class
// ---------------------------------------------------------------------------

/** Map diagnostic severity (1=error, 2=warning, 3+=info) to a CSS class. */
export function diagnosticSeverityClass(severity: number): string {
  if (severity === 1) return 'cm-diag-error'
  if (severity === 2) return 'cm-diag-warning'
  return 'cm-diag-info'
}

/** Map diagnostic severity to gutter marker CSS class. */
export function diagnosticGutterClass(severity: number): string {
  if (severity === 1) return 'lean-diag-error'
  if (severity === 2) return 'lean-diag-warning'
  return 'lean-diag-info'
}

/** Map diagnostic severity to hover tooltip popup CSS class. */
export function diagnosticPopupClass(severity: number): string {
  if (severity === 1) return 'lean-diag-popup-error'
  if (severity === 2) return 'lean-diag-popup-warning'
  return 'lean-diag-popup-info'
}

// ---------------------------------------------------------------------------
// Diagnostic range conversion
// ---------------------------------------------------------------------------

interface OffsetRange {
  from: number
  to: number
}

/**
 * Convert a DiagnosticInfo (1-indexed lines) into CM6 character offsets.
 *
 * Returns null when the diagnostic references out-of-range lines or produces
 * an invalid/empty span (from >= to, or offsets outside document bounds).
 */
export function diagRange(diag: DiagnosticInfo, doc: DocLike): OffsetRange | null {
  if (
    diag.start_line < 1 ||
    diag.start_line > doc.lines ||
    diag.end_line < 1 ||
    diag.end_line > doc.lines
  )
    return null
  const from = doc.line(diag.start_line).from + diag.start_col
  const to = doc.line(diag.end_line).from + diag.end_col
  if (from < 0 || to > doc.length || from >= to) return null
  return { from, to }
}

// ---------------------------------------------------------------------------
// Semantic token range conversion
// ---------------------------------------------------------------------------

interface TokenRange {
  from: number
  to: number
  cssClass: string
}

/**
 * Convert a SemanticToken (1-indexed line, col offset) into a CM6 mark range
 * with its CSS class. Returns null when the token type is unknown or the
 * resulting span is out of document bounds.
 */
export function semanticTokenRange(token: SemanticToken, doc: DocLike): TokenRange | null {
  const cssClass = tokenTypeToCssClass(token.token_type)
  if (!cssClass) return null

  const lineNum = token.line
  if (lineNum < 1 || lineNum > doc.lines) return null

  const line = doc.line(lineNum)
  const from = line.from + token.col
  const to = line.from + token.col + token.length

  if (from >= 0 && to <= doc.length && from <= to) {
    return { from, to, cssClass }
  }
  return null
}

/**
 * Convert an array of SemanticTokens into sorted mark ranges.
 * Filters out tokens with unknown types or out-of-range positions.
 */
export function buildSemanticTokenRanges(tokens: SemanticToken[], doc: DocLike): TokenRange[] {
  const ranges: TokenRange[] = []
  for (const token of tokens) {
    const r = semanticTokenRange(token, doc)
    if (r) ranges.push(r)
  }
  ranges.sort((a, b) => a.from - b.from)
  return ranges
}

/**
 * Convert an array of DiagnosticInfos into sorted underline ranges with CSS classes.
 * Filters out diagnostics with out-of-range positions.
 */
export function buildDiagnosticRanges(
  diagnostics: DiagnosticInfo[],
  doc: DocLike,
): (OffsetRange & { cssClass: string })[] {
  const ranges: (OffsetRange & { cssClass: string })[] = []
  for (const diag of diagnostics) {
    const r = diagRange(diag, doc)
    if (!r) continue
    ranges.push({ ...r, cssClass: diagnosticSeverityClass(diag.severity) })
  }
  ranges.sort((a, b) => a.from - b.from)
  return ranges
}
