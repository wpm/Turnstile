/**
 * Rich content rendering pipeline for assistant messages.
 *
 * Supports full Markdown (via ``marked``), fenced Lean code blocks with
 * syntax highlighting, inline code, and LaTeX math (``$...$`` / ``$$...$$``
 * via KaTeX).
 *
 * Pipeline:
 *   1. Protect fenced code blocks from math extraction
 *   2. Convert LaTeX environments (theorem, proof, …) to markdown
 *   3. Extract math delimiters as placeholders
 *   4. Restore code fences (so ``marked`` processes them)
 *   5. Run ``marked.parse`` with a custom renderer for Lean highlighting
 *   6. Replace math placeholders with KaTeX-rendered HTML
 */

import { Marked } from 'marked'
import { highlightLean, escapeHtml } from '../formal-proof/leanHighlight'
import { renderMath } from './math'

// ── Placeholder helpers ─────────────────────────────────────────────────

interface MathEntry {
  latex: string
  display: boolean
}

const CODE_PREFIX = '%%CODE_'
const MATH_PREFIX = '%%MATH_'
const PLACEHOLDER_SUFFIX = '%%'

function codePlaceholder(i: number): string {
  return CODE_PREFIX + String(i) + PLACEHOLDER_SUFFIX
}

function mathPlaceholder(i: number): string {
  return MATH_PREFIX + String(i) + PLACEHOLDER_SUFFIX
}

// ── Step 1 & 3: Protect / restore fenced code blocks ────────────────────

const FENCE_RE = /^```[\s\S]*?^```/gm

function protectCodeFences(text: string): { cleaned: string; fences: Map<string, string> } {
  const fences = new Map<string, string>()
  let i = 0
  const cleaned = text.replace(FENCE_RE, (match) => {
    const ph = codePlaceholder(i++)
    fences.set(ph, match)
    return ph
  })
  return { cleaned, fences }
}

function restoreCodeFences(text: string, fences: Map<string, string>): string {
  let result = text
  for (const [ph, original] of fences) {
    result = result.replace(ph, original)
  }
  return result
}

// ── Step 2: Convert LaTeX environments to markdown ─────────────────────

const SUPPORTED_ENVS = [
  'theorem',
  'lemma',
  'proposition',
  'corollary',
  'definition',
  'remark',
  'example',
  'proof',
]

const ENV_RE = new RegExp(
  `\\\\begin\\{(${SUPPORTED_ENVS.join('|')})\\}` +
    `(?:\\[([^\\]]*)\\])?` +
    `([\\s\\S]*?)` +
    `\\\\end\\{\\1\\}`,
  'g',
)

function capitalize(s: string): string {
  return s.charAt(0).toUpperCase() + s.slice(1)
}

function convertLatexEnvironments(text: string): string {
  return text.replace(ENV_RE, (_, env: string, title: string | undefined, body: string) => {
    const trimmed = body.trim()
    if (env === 'proof') {
      const label = title ? `*Proof (${title}).*` : '*Proof.*'
      return `${label} ${trimmed} $\\square$`
    }
    const name = capitalize(env)
    const label = title ? `**${name} (${title}).**` : `**${name}.**`
    return `${label} ${trimmed}`
  })
}

// ── Step 3: Extract math delimiters ─────────────────────────────────────

// Match $$...$$ (display) first, then $...$ (inline).
const MATH_RE = /\$\$([\s\S]*?)\$\$|\$((?:[^$\\]|\\.)+?)\$/g

function extractMath(text: string): { cleaned: string; segments: Map<string, MathEntry> } {
  const segments = new Map<string, MathEntry>()
  let i = 0
  const cleaned = text.replace(
    MATH_RE,
    (_, display: string | undefined, inline: string | undefined) => {
      const ph = mathPlaceholder(i++)
      if (display !== undefined) {
        segments.set(ph, { latex: display, display: true })
      } else if (inline !== undefined) {
        segments.set(ph, { latex: inline, display: false })
      }
      return ph
    },
  )
  return { cleaned, segments }
}

// ── Step 6: Restore math placeholders with KaTeX HTML ──────────────────

function restoreMath(html: string, segments: Map<string, MathEntry>): string {
  let result = html
  for (const [ph, entry] of segments) {
    result = result.replace(ph, renderMath(entry.latex, entry.display))
  }
  return result
}

// ── Step 5: Configure marked with custom renderer ──────────────────────

const marked = new Marked({
  gfm: true,
  breaks: true,
  async: false,
  renderer: {
    code({ text, lang }: { text: string; lang?: string }): string {
      const isLean = !lang || lang === 'lean'
      if (isLean) {
        return `<pre><code class="assistant-lean-code">${highlightLean(text)}</code></pre>`
      }
      return `<pre><code>${escapeHtml(text)}</code></pre>`
    },
    codespan({ text }: { text: string }): string {
      return `<code class="assistant-lean-code">${highlightLean(text)}</code>`
    },
    html({ text }: { text: string }): string {
      return escapeHtml(text)
    },
  },
})

// ── Public API ──────────────────────────────────────────────────────────

export function renderContent(content: string): string {
  if (!content) return ''

  // 1. Protect code fences from math extraction
  const { cleaned: noFences, fences } = protectCodeFences(content)

  // 2. Convert LaTeX environments to markdown
  const noEnvs = convertLatexEnvironments(noFences)

  // 3. Extract math as placeholders
  const { cleaned: noMath, segments } = extractMath(noEnvs)

  // 4. Restore code fences for marked to process
  const restored = restoreCodeFences(noMath, fences)

  // 5. Parse markdown
  const html = marked.parse(restored) as string

  // 6. Replace math placeholders with KaTeX output
  return restoreMath(html, segments)
}
