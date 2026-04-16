import { describe, it, expect } from 'vitest'
import { parseGoalLine, type GoalLineSegment } from './goalLineParse'

/** Helper: concatenate segment texts to verify round-trip fidelity. */
function joinSegments(segments: GoalLineSegment[]): string {
  return segments.map((s) => s.text).join('')
}

describe('parseGoalLine', () => {
  it('parses a simple hypothesis', () => {
    expect(parseGoalLine('hp : p')).toEqual([
      { text: 'hp', kind: 'name' },
      { text: ' : p', kind: 'plain' },
    ])
  })

  it('parses a hypothesis with prime in name', () => {
    expect(parseGoalLine("h' : Nat")).toEqual([
      { text: "h'", kind: 'name' },
      { text: ' : Nat', kind: 'plain' },
    ])
  })

  it('parses a hypothesis with underscore', () => {
    expect(parseGoalLine('h_left : p ∧ q')).toEqual([
      { text: 'h_left', kind: 'name' },
      { text: ' : p ∧ q', kind: 'plain' },
    ])
  })

  it('parses a hypothesis with Greek letters', () => {
    expect(parseGoalLine('hα : α → β')).toEqual([
      { text: 'hα', kind: 'name' },
      { text: ' : α → β', kind: 'plain' },
    ])
  })

  it('parses an indented hypothesis', () => {
    expect(parseGoalLine('  hp : p')).toEqual([
      { text: '  ', kind: 'plain' },
      { text: 'hp', kind: 'name' },
      { text: ' : p', kind: 'plain' },
    ])
  })

  it('parses a turnstile line', () => {
    expect(parseGoalLine('⊢ p')).toEqual([
      { text: '⊢', kind: 'turnstile' },
      { text: ' p', kind: 'plain' },
    ])
  })

  it('parses an indented turnstile', () => {
    expect(parseGoalLine('  ⊢ p ∧ q')).toEqual([
      { text: '  ', kind: 'plain' },
      { text: '⊢', kind: 'turnstile' },
      { text: ' p ∧ q', kind: 'plain' },
    ])
  })

  it('returns plain for a case label', () => {
    expect(parseGoalLine('case left')).toEqual([{ text: 'case left', kind: 'plain' }])
  })

  it('returns plain for an empty string', () => {
    expect(parseGoalLine('')).toEqual([{ text: '', kind: 'plain' }])
  })

  it('round-trips all segments back to the original line', () => {
    const lines = ['hp : p', '⊢ p', 'case left', "  h' : Nat", 'hα : α → β', '', '  ⊢ True']
    for (const line of lines) {
      expect(joinSegments(parseGoalLine(line))).toBe(line)
    }
  })
})
