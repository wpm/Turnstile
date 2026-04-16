/**
 * Minimal Markdown utilities for rendering LSP-returned text.
 *
 * Lean's LSP returns ``$/lean/plainGoal`` responses as Markdown that may contain
 * fenced code blocks (e.g. ```lean ... ```).  These utilities parse that text
 * into typed blocks so components can render the content without showing the
 * raw fence delimiters.
 */

interface CodeBlock {
  type: 'code'
  lang: string
  content: string
}

interface TextBlock {
  type: 'text'
  content: string
}

type Block = CodeBlock | TextBlock

const FENCE_RE = /^```(\w*)\n([\s\S]*?)^```/gm

/**
 * Split a Markdown string into alternating text and fenced-code blocks.
 *
 * Fence delimiters are consumed and not included in any block's ``content``.
 * Empty blocks are omitted from the result.
 */
export function parseBlocks(text: string): Block[] {
  const blocks: Block[] = []
  let last = 0
  let match: RegExpExecArray | null

  FENCE_RE.lastIndex = 0
  while ((match = FENCE_RE.exec(text)) !== null) {
    const pre = text.slice(last, match.index).trim()
    if (pre) blocks.push({ type: 'text', content: pre })
    const lang = match[1] !== undefined && match[1] !== '' ? match[1] : 'lean'
    const content = match[2] ?? ''
    blocks.push({ type: 'code', lang, content })
    last = match.index + match[0].length
  }

  const tail = text.slice(last).trim()
  if (tail) blocks.push({ type: 'text', content: tail })

  return blocks
}
