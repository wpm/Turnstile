/**
 * Theorem name extraction and conversion utilities.
 *
 * Two-tier strategy:
 *   1. Extract the friendly title from LLM-generated prose (`\begin{theorem}[Title]`)
 *   2. Fall back to parsing the Lean source for `theorem foo_bar` and humanizing
 */

// Match the first \begin{theorem}[...] or \begin{lemma}[...] bracket title.
const PROSE_TITLE_RE = /\\begin\{(?:theorem|lemma|proposition|corollary)\}\[([^\]]+)\]/

// Match the first `theorem` or `lemma` declaration in Lean source.
const LEAN_DECL_RE = /\b(?:theorem|lemma)\s+(\w+)/

/**
 * Extract the theorem title from LLM-generated prose.
 *
 * Looks for `\begin{theorem}[Title]` (or lemma/proposition/corollary) and
 * returns the bracket content, or `null` if not found.
 */
export function extractTitleFromProse(proseText: string): string | null {
  if (!proseText) return null
  const m = PROSE_TITLE_RE.exec(proseText)
  return m?.[1] ?? null
}

/**
 * Extract the first theorem/lemma identifier from Lean source code.
 *
 * Returns the raw identifier (e.g. `"sqrt_two_irrational"`), or `null`.
 */
export function extractLeanTheoremName(leanSource: string): string | null {
  if (!leanSource) return null
  const m = LEAN_DECL_RE.exec(leanSource)
  return m?.[1] ?? null
}

/**
 * Convert a Lean identifier to a human-readable title.
 *
 * Splits on underscores and capitalizes each word:
 * `"nat_add_comm"` → `"Nat Add Comm"`.
 */
export function humanizeLeanName(ident: string): string {
  return ident
    .split('_')
    .map((w) => w.charAt(0).toUpperCase() + w.slice(1))
    .join(' ')
}

/**
 * Return the best available theorem title.
 *
 * Priority: prose title → humanized Lean name → `"New Theorem"`.
 */
export function getTheoremTitle(proseText: string, leanSource: string): string {
  const fromProse = extractTitleFromProse(proseText)
  if (fromProse) return fromProse

  const leanName = extractLeanTheoremName(leanSource)
  if (leanName) return humanizeLeanName(leanName)

  return 'New Theorem'
}

/**
 * Convert a theorem title to a safe filename stem.
 *
 * Lowercases, replaces whitespace/special chars with hyphens, strips
 * non-alphanumeric characters, and collapses consecutive hyphens.
 */
export function titleToFilename(title: string): string {
  return title
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '')
}
