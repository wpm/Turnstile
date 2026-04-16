import katex from 'katex'

interface TextSegment {
  type: 'text'
  content: string
}

interface MathSegment {
  type: 'math'
  content: string
  display: boolean
}

type Segment = TextSegment | MathSegment

/**
 * Split ``text`` into alternating text and math segments.
 *
 * ``$$...$$`` is matched first (display math), then ``$...$`` (inline math).
 * The remaining pieces become text segments.
 */
export function parseMathSegments(text: string): Segment[] {
  if (!text) return []

  const segments: Segment[] = []
  // Match $$...$$ (display) or $...$ (inline); $$ must be tried first.
  const mathRe = /\$\$([\s\S]*?)\$\$|\$((?:[^$]|\\.)*?)\$/g

  let lastIndex = 0
  let match: RegExpExecArray | null

  while ((match = mathRe.exec(text)) !== null) {
    // Text before this math segment
    if (match.index > lastIndex) {
      segments.push({ type: 'text', content: text.slice(lastIndex, match.index) })
    }

    if (match[1] !== undefined) {
      // $$...$$ — display math
      segments.push({ type: 'math', content: match[1], display: true })
    } else if (match[2] !== undefined) {
      // $...$ — inline math
      segments.push({ type: 'math', content: match[2], display: false })
    }

    lastIndex = match.index + match[0].length
  }

  // Trailing text
  if (lastIndex < text.length) {
    segments.push({ type: 'text', content: text.slice(lastIndex) })
  }

  return segments
}

/**
 * Render a LaTeX string to an HTML string via KaTeX.
 * Never throws: ``throwOnError: false`` renders a red error span instead.
 */
export function renderMath(latex: string, display: boolean): string {
  return katex.renderToString(latex, {
    throwOnError: false,
    displayMode: display,
  })
}
