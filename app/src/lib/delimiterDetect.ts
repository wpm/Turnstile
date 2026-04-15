/**
 * Detects when a closing delimiter completes a delimited span in the input.
 *
 * Supports:
 *   - Inline code:   `` `...` ``
 *   - Inline math:   ``$...$``
 *   - Display math:  ``$$...$$``
 *
 * Detection fires only when the cursor is immediately after the closing
 * delimiter.  Display math (``$$``) is checked before inline math (``$``)
 * to avoid false positives.
 */

export interface DelimitedSpan {
  /** Offset of the opening delimiter in plain text. */
  from: number
  /** Offset after the closing delimiter. */
  to: number
  /** The content between delimiters (excluding delimiters). */
  content: string
  /** The kind of delimited span. */
  kind: 'inline-code' | 'inline-math' | 'display-math'
}

/**
 * Given the full plain text and the cursor position (where a delimiter
 * character was just typed), detect if a delimited span is now complete.
 *
 * Returns ``null`` if no complete delimited span is found.
 */
export function detectCompletedDelimiter(text: string, cursorPos: number): DelimitedSpan | null {
  if (cursorPos < 2) return null

  const last = text[cursorPos - 1]

  // ── Display math: $$...$$ ───────────────────────────────────────────
  if (last === '$' && cursorPos >= 4 && text[cursorPos - 2] === '$') {
    // Closing $$ at cursorPos-2..cursorPos. Find opening $$.
    const searchEnd = cursorPos - 2
    for (let i = searchEnd - 1; i >= 1; i--) {
      if (text[i] === '$' && text[i - 1] === '$') {
        // Check not escaped
        if (i >= 2 && text[i - 2] === '\\') continue
        const content = text.slice(i + 1, searchEnd)
        if (content.length === 0) return null
        return { from: i - 1, to: cursorPos, content, kind: 'display-math' }
      }
    }
  }

  // ── Inline math: $...$ ──────────────────────────────────────────────
  if (last === '$') {
    // Check not escaped
    if (cursorPos >= 2 && text[cursorPos - 2] === '\\') return null

    // Find opening $ scanning backwards
    for (let i = cursorPos - 2; i >= 0; i--) {
      if (text[i] === '$') {
        // Check not escaped
        if (i >= 1 && text[i - 1] === '\\') continue
        // Don't match $$ as opening for inline math
        if (i >= 1 && text[i - 1] === '$') continue
        const content = text.slice(i + 1, cursorPos - 1)
        if (content.length === 0) return null
        return { from: i, to: cursorPos, content, kind: 'inline-math' }
      }
    }
  }

  // ── Inline code: `...` ──────────────────────────────────────────────
  if (last === '`') {
    for (let i = cursorPos - 2; i >= 0; i--) {
      if (text[i] === '`') {
        const content = text.slice(i + 1, cursorPos - 1)
        if (content.length === 0) return null
        return { from: i, to: cursorPos, content, kind: 'inline-code' }
      }
    }
  }

  return null
}

// ── Open-delimiter detection ──────────────────────────────────────────

const enum State {
  Normal,
  Backtick,
  InlineMath,
  DisplayMath,
}

/**
 * Returns ``true`` when ``cursorPos`` falls inside an unclosed delimiter
 * (`` ` ``, ``$``, or ``$$``).  Used to suppress abbreviation replacement
 * while the user is typing the contents of a delimited span.
 *
 * The scan runs forward from position 0 so that nested/sequential
 * delimiters are tracked correctly.  Escape handling mirrors
 * {@link detectCompletedDelimiter}.
 */
export function isInsideOpenDelimiter(text: string, cursorPos: number): boolean {
  let state: State = State.Normal
  let i = 0
  const end = Math.min(cursorPos, text.length)

  while (i < end) {
    const ch = text[i]

    switch (state) {
      case State.Normal:
        if (ch === '`') {
          state = State.Backtick
          i++
        } else if (ch === '$') {
          // Check not escaped
          if (i >= 1 && text[i - 1] === '\\') {
            i++
            break
          }
          if (i + 1 < end && text[i + 1] === '$') {
            state = State.DisplayMath
            i += 2
          } else {
            state = State.InlineMath
            i++
          }
        } else {
          i++
        }
        break

      case State.Backtick:
        if (ch === '`') {
          state = State.Normal
        }
        i++
        break

      case State.InlineMath:
        if (ch === '$' && !(i >= 1 && text[i - 1] === '\\')) {
          state = State.Normal
        }
        i++
        break

      case State.DisplayMath:
        if (ch === '$' && i + 1 < end && text[i + 1] === '$' && !(i >= 1 && text[i - 1] === '\\')) {
          state = State.Normal
          i += 2
        } else {
          i++
        }
        break
    }
  }

  return state !== State.Normal
}
