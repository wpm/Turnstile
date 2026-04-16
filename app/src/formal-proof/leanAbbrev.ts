/**
 * Lean 4 unicode abbreviation engine for the assistant input.
 *
 * Wraps {@link @leanprover/unicode-input}'s {@link AbbreviationProvider} to
 * detect and replace backslash-prefixed abbreviations (e.g. ``\to`` → ``→``)
 * as the user types.
 *
 * Replacement triggers:
 *   - **Eager**: immediately when the abbreviation is unambiguous (no other
 *     abbreviation shares the prefix).
 *   - **Disambiguated**: when the character after a valid abbreviation is not
 *     a continuation of any longer abbreviation.
 */

import { AbbreviationProvider } from '@leanprover/unicode-input'

// ── Types ──────────────────────────────────────────────────────────────

export interface AbbrevReplacement {
  /** Start offset of the ``\`` leader in the input string. */
  from: number
  /** End offset (exclusive) of the abbreviation text (not including trailing char). */
  to: number
  /** The Unicode replacement string. */
  replacement: string
  /** Cursor offset within the replacement for ``$CURSOR`` pairs, or null. */
  cursorOffset: number | null
}

// ── Singleton provider ─────────────────────────────────────────────────

const CURSOR_MARKER = '$CURSOR'

const abbreviationProvider = new AbbreviationProvider({
  abbreviationCharacter: '\\',
  customTranslations: {},
  eagerReplacementEnabled: true,
})

// ── Helpers ────────────────────────────────────────────────────────────

/** Characters that can appear in an abbreviation identifier. */
function isAbbrevChar(ch: string): boolean {
  // Abbreviation identifiers use alphanumeric, punctuation, and symbol chars.
  // Spaces, newlines, and control characters are NOT valid.
  return ch.length === 1 && ch > ' ' && ch !== '\\'
}

/**
 * Parse a ``$CURSOR``-bearing replacement string into the actual replacement
 * and the cursor offset within it.
 */
function parseCursor(raw: string): { text: string; cursorOffset: number | null } {
  const idx = raw.indexOf(CURSOR_MARKER)
  if (idx === -1) return { text: raw, cursorOffset: null }
  return {
    text: raw.slice(0, idx) + raw.slice(idx + CURSOR_MARKER.length),
    cursorOffset: idx,
  }
}

// ── Public API ─────────────────────────────────────────────────────────

/**
 * Given the full input text and the current cursor position, determine if
 * there is a completed abbreviation ending at or just before the cursor.
 *
 * Returns ``null`` if no replacement should be made.
 */
export function findAbbrevReplacement(text: string, cursorPos: number): AbbrevReplacement | null {
  if (cursorPos <= 0) return null

  // Try two strategies:
  // 1. Full span from `\` to cursor as an unambiguous abbreviation.
  // 2. Span from `\` to cursor-1 as a valid abbreviation, where the last
  //    character disambiguates (the full span is not a prefix of any abbrev).

  // --- Find the backslash scanning from cursorPos-1 ---
  // Special case: `\\` — the second `\` is the abbreviation body.
  if (cursorPos >= 2 && text[cursorPos - 1] === '\\' && text[cursorPos - 2] === '\\') {
    const symbol = abbreviationProvider.getSymbolForAbbreviation('\\')
    if (symbol !== undefined) {
      const { text: replacement, cursorOffset } = parseCursor(symbol)
      return { from: cursorPos - 2, to: cursorPos, replacement, cursorOffset }
    }
  }

  // Determine if the last character is a trailing disambiguator (not part of
  // any abbreviation). If so, the abbreviation body ends before it.
  const lastChar = text[cursorPos - 1] ?? ''
  const trailing = !isAbbrevChar(lastChar) && lastChar !== '\\'
  const scanStart = trailing ? cursorPos - 2 : cursorPos - 1

  if (scanStart < 0) return null

  // Find the backslash by scanning backwards over abbreviation characters.
  let backslashPos = -1
  for (let i = scanStart; i >= 0; i--) {
    if (text[i] === '\\') {
      backslashPos = i
      break
    }
    if (!isAbbrevChar(text[i] ?? '')) return null
  }
  if (backslashPos === -1) return null

  const abbrevEnd = trailing ? cursorPos - 1 : cursorPos
  const abbrevBody = text.slice(backslashPos + 1, abbrevEnd)
  if (abbrevBody.length === 0) return null

  const allAbbrevs = abbreviationProvider.getSymbolsByAbbreviation()
  const exactSymbol = abbreviationProvider.getSymbolForAbbreviation(abbrevBody)

  if (trailing && exactSymbol !== undefined) {
    // Trailing disambiguator confirms the abbreviation.
    const { text: replacement, cursorOffset } = parseCursor(exactSymbol)
    return { from: backslashPos, to: abbrevEnd, replacement, cursorOffset }
  }

  if (!trailing && exactSymbol !== undefined) {
    // Strategy 1: the full body is unambiguous (no longer abbreviation shares prefix).
    const hasLonger = Object.keys(allAbbrevs).some(
      (k) => k.startsWith(abbrevBody) && k !== abbrevBody,
    )
    if (!hasLonger) {
      const { text: replacement, cursorOffset } = parseCursor(exactSymbol)
      return { from: backslashPos, to: abbrevEnd, replacement, cursorOffset }
    }
  }

  // Strategy 2: body without last char is a valid abbreviation, and the
  // full body (with last char) is NOT a prefix of any abbreviation.
  // Handles both: trailing non-abbrev chars (e.g. `\to `) and abbrev chars
  // that don't continue the abbreviation (e.g. `\to.` where `to.` has no match).
  if (abbrevBody.length >= 2) {
    const bodyWithoutLast = abbrevBody.slice(0, -1)
    const shortSymbol = abbreviationProvider.getSymbolForAbbreviation(bodyWithoutLast)
    if (shortSymbol !== undefined) {
      const hasContinuation = Object.keys(allAbbrevs).some((k) => k.startsWith(abbrevBody))
      if (!hasContinuation) {
        const { text: replacement, cursorOffset } = parseCursor(shortSymbol)
        return {
          from: backslashPos,
          to: backslashPos + 1 + bodyWithoutLast.length,
          replacement,
          cursorOffset,
        }
      }
    }
  }

  return null
}

/**
 * Apply a replacement to the text, returning the new text and cursor position.
 */
export function applyAbbrevReplacement(
  text: string,
  replacement: AbbrevReplacement,
): { newText: string; newCursorPos: number } {
  const newText =
    text.slice(0, replacement.from) + replacement.replacement + text.slice(replacement.to)
  const newCursorPos =
    replacement.cursorOffset !== null
      ? replacement.from + replacement.cursorOffset
      : replacement.from + replacement.replacement.length
  return { newText, newCursorPos }
}
