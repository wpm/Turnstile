/**
 * Position-conversion utilities for the CodeMirror 6 ↔ LSP boundary.
 *
 * CodeMirror numbers lines from 1; LSP numbers them from 0. Characters are
 * 0-indexed in both. These helpers keep the conversions in one place so we
 * can't silently drift off by one.
 */

import type { Line, Text } from '@codemirror/state'

/** Convert a CodeMirror 1-indexed line number to a 0-indexed LSP line. */
export function cmLineToLsp(cmLine: number): number {
  return cmLine - 1
}

/** Convert a 0-indexed LSP line to a CodeMirror 1-indexed line number. */
export function lspLineToCm(lspLine: number): number {
  return lspLine + 1
}

/**
 * Translate a CodeMirror document offset into an LSP position.
 *
 * Returns `{ line, character }` where `line` is 0-indexed and `character` is
 * a UTF-16 code-unit offset from the start of the line — matching the shape
 * the LSP Tauri commands expect.
 */
export function cmPosToLsp(doc: Text, pos: number): { line: number; character: number } {
  const line = doc.lineAt(pos)
  return { line: cmLineToLsp(line.number), character: pos - line.from }
}

/**
 * Translate an LSP position into a CodeMirror document offset, clamped to the
 * bounds of `doc`.
 *
 * Returns `null` if the LSP line is outside the document.
 */
export function lspPosToCmOffset(
  doc: Text,
  lspLine: number,
  lspCharacter: number,
): { line: Line; offset: number } | null {
  const cmLineNum = lspLineToCm(lspLine)
  if (cmLineNum < 1 || cmLineNum > doc.lines) return null
  const line = doc.line(cmLineNum)
  const offset = Math.min(line.from + lspCharacter, line.to)
  return { line, offset }
}
