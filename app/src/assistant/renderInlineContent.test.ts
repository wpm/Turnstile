import { describe, it, expect } from 'vitest'
import { createMathElement, createCodeElement } from './renderInlineContent'

describe('createMathElement', () => {
  it('creates a span with KaTeX-rendered content', () => {
    const el = createMathElement('x^2', false, '$x^2$')
    expect(el.tagName).toBe('SPAN')
    expect(el.getAttribute('contenteditable')).toBe('false')
    expect(el.getAttribute('data-source')).toBe('$x^2$')
    expect(el.getAttribute('aria-label')).toBe('$x^2$')
    expect(el.className).toContain('assistant-rendered-inline')
    expect(el.className).toContain('assistant-rendered-math')
    expect(el.innerHTML).toContain('katex')
  })

  it('creates display math element', () => {
    const el = createMathElement('\\frac{a}{b}', true, '$$\\frac{a}{b}$$')
    expect(el.innerHTML).toContain('katex')
    expect(el.getAttribute('data-source')).toBe('$$\\frac{a}{b}$$')
  })
})

describe('createCodeElement', () => {
  it('creates a code element with Lean highlighting', () => {
    const el = createCodeElement('theorem', '`theorem`')
    expect(el.tagName).toBe('CODE')
    expect(el.getAttribute('contenteditable')).toBe('false')
    expect(el.getAttribute('data-source')).toBe('`theorem`')
    expect(el.getAttribute('aria-label')).toBe('`theorem`')
    expect(el.className).toContain('assistant-rendered-inline')
    expect(el.className).toContain('assistant-rendered-code')
    expect(el.className).toContain('assistant-lean-code')
    // 'theorem' is a Lean keyword, so it should be highlighted
    expect(el.innerHTML).toContain('cm-lean-keyword')
  })

  it('creates code element for non-keyword text', () => {
    const el = createCodeElement('myVar', '`myVar`')
    expect(el.innerHTML).toContain('myVar')
    expect(el.innerHTML).not.toContain('cm-lean-keyword')
  })
})
