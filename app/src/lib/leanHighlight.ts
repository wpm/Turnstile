/**
 * Lightweight regex-based Lean syntax highlighter for read-only display.
 *
 * Applies the same ``cm-lean-*`` CSS classes used by the CodeMirror editor,
 * so chat messages share the editor's colour scheme without spinning up a
 * full CM6 instance.
 *
 * Coverage: keywords, strings, comments, numbers, operators.  Identifiers
 * are left as plain text.
 */

/** Escape HTML special characters to prevent XSS. */
function escapeHtml(s: string): string {
  return s
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;')
}

/** Wrap content in a span with the given CSS class. */
function span(cls: string, content: string): string {
  return `<span class="${cls}">${content}</span>`
}

// Lean 4 keywords (non-exhaustive but covers the common ones).
const LEAN_KEYWORDS = new Set([
  'abbrev',
  'and',
  'by',
  'calc',
  'class',
  'constructor',
  'def',
  'deriving',
  'do',
  'else',
  'end',
  'example',
  'export',
  'extends',
  'extern',
  'false',
  'for',
  'forall',
  'fun',
  'have',
  'if',
  'import',
  'in',
  'include',
  'inductive',
  'infix',
  'infixl',
  'infixr',
  'instance',
  'let',
  'macro',
  'match',
  'mutual',
  'namespace',
  'noncomputable',
  'notation',
  'of',
  'open',
  'or',
  'partial',
  'postfix',
  'prefix',
  'private',
  'protected',
  'return',
  'section',
  'show',
  'structure',
  'suffices',
  'syntax',
  'termination_by',
  'then',
  'theorem',
  'true',
  'try',
  'type',
  'universe',
  'unless',
  'variable',
  'where',
  'with',
])

/**
 * Tokenise ``code`` and return an HTML string with ``cm-lean-*`` spans.
 *
 * The tokeniser processes the string left-to-right, consuming the longest
 * match at each position in this priority order:
 *
 * 1. Line comment  (-- … end of line)
 * 2. String literal  ("…")
 * 3. Number  (integer or decimal)
 * 4. Operator  (single-character punctuation / symbol)
 * 5. Word token  (keyword or plain identifier)
 * 6. Whitespace / anything else  (passed through as-is)
 */
export function highlightLean(code: string): string {
  if (!code) return ''

  // Single compiled regex with named alternatives tried in order.
  // Groups: comment | string | number | operator | word | other
  // Note: avoid \w inside character classes with the /u flag — use explicit ranges.
  const tokenRe =
    /(--[^\n]*)|("(?:[^"\\]|\\.)*")|(\d+\.?\d*|\.\d+)|([:=<>!+\-*/|&^~?@#$%]+)|([A-Za-z_\u03B1-\u03C9\u0391-\u03A9\u03BB\u2200\u2203'][A-Za-z0-9_'\u03B1-\u03C9\u0391-\u03A9\u03BB\u2200\u2203']*)|(.)/gsu

  let result = ''
  let match: RegExpExecArray | null

  while ((match = tokenRe.exec(code)) !== null) {
    const [, comment, str, num, op, word, other] = match

    if (comment !== undefined) {
      result += span('cm-lean-comment', escapeHtml(comment))
    } else if (str !== undefined) {
      result += span('cm-lean-string', escapeHtml(str))
    } else if (num !== undefined) {
      result += span('cm-lean-number', escapeHtml(num))
    } else if (op !== undefined) {
      result += span('cm-lean-operator', escapeHtml(op))
    } else if (word !== undefined) {
      if (LEAN_KEYWORDS.has(word)) {
        result += span('cm-lean-keyword', escapeHtml(word))
      } else {
        result += escapeHtml(word)
      }
    } else if (other !== undefined) {
      result += escapeHtml(other)
    }
  }

  return result
}
