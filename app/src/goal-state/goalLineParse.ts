/**
 * Segment a goal-state line into typed spans for styled rendering.
 *
 * Patterns: `hp : p` (hypothesis), `⊢ p` (turnstile), `case left` (plain).
 * Concatenating segment `.text` values reproduces the original line exactly.
 */

export interface GoalLineSegment {
  text: string
  kind: 'name' | 'turnstile' | 'plain'
}

const turnstileRe = /^(\s*)(⊢)(.*)$/
const hypRe = /^(\s*)([\w\u0370-\u03FF\u2070-\u209F\u2100-\u214F'_]+)( : )(.*)$/

export function parseGoalLine(line: string): GoalLineSegment[] {
  if (line === '') return [{ text: '', kind: 'plain' }]

  const turnstileMatch = turnstileRe.exec(line)
  if (turnstileMatch) {
    const [, leading = '', symbol = '⊢', rest = ''] = turnstileMatch
    const segments: GoalLineSegment[] = []
    if (leading) segments.push({ text: leading, kind: 'plain' })
    segments.push({ text: symbol, kind: 'turnstile' })
    if (rest) segments.push({ text: rest, kind: 'plain' })
    return segments
  }

  const hypMatch = hypRe.exec(line)
  if (hypMatch) {
    const [, leading = '', name = '', sep = ' : ', type = ''] = hypMatch
    const segments: GoalLineSegment[] = []
    if (leading) segments.push({ text: leading, kind: 'plain' })
    segments.push({ text: name, kind: 'name' })
    segments.push({ text: `${sep}${type}`, kind: 'plain' })
    return segments
  }

  return [{ text: line, kind: 'plain' }]
}
