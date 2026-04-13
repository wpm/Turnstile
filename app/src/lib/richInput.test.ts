import { describe, it, expect, beforeEach } from 'vitest'
import { extractPlainText, replaceRangeWithText, replaceRangeWithNode } from './richInput'

describe('richInput', () => {
  let el: HTMLDivElement

  beforeEach(() => {
    el = document.createElement('div')
    el.setAttribute('contenteditable', 'true')
    document.body.appendChild(el)
  })

  // ── extractPlainText ────────────────────────────────────────────────

  describe('extractPlainText', () => {
    it('extracts text from plain text nodes', () => {
      el.textContent = 'hello world'
      expect(extractPlainText(el)).toBe('hello world')
    })

    it('extracts empty string from empty element', () => {
      expect(extractPlainText(el)).toBe('')
    })

    it('uses data-source from rendered nodes', () => {
      el.innerHTML =
        'before <span data-source="$x^2$" contenteditable="false">rendered</span> after'
      expect(extractPlainText(el)).toBe('before $x^2$ after')
    })

    it('handles multiple rendered nodes', () => {
      el.innerHTML =
        '<span data-source="`code`" contenteditable="false">c</span> and <span data-source="$y$" contenteditable="false">y</span>'
      expect(extractPlainText(el)).toBe('`code` and $y$')
    })

    it('handles br elements as newlines', () => {
      el.innerHTML = 'line1<br>line2'
      expect(extractPlainText(el)).toBe('line1\nline2')
    })
  })

  // ── replaceRangeWithText ────────────────────────────────────────────

  describe('replaceRangeWithText', () => {
    it('replaces a range of text with new text', () => {
      el.textContent = 'hello \\alpha'
      replaceRangeWithText(el, 6, 12, 'α')
      expect(el.textContent).toBe('hello α')
    })

    it('replaces at start of text', () => {
      el.textContent = '\\to rest'
      replaceRangeWithText(el, 0, 3, '→')
      expect(el.textContent).toBe('→ rest')
    })

    it('replaces at end of text', () => {
      el.textContent = 'prefix \\lam'
      replaceRangeWithText(el, 7, 11, 'λ')
      expect(el.textContent).toBe('prefix λ')
    })
  })

  // ── replaceRangeWithNode ────────────────────────────────────────────

  describe('replaceRangeWithNode', () => {
    it('replaces a range of text with an HTML node', () => {
      el.textContent = 'see $x^2$ here'
      const node = document.createElement('span')
      node.setAttribute('data-source', '$x^2$')
      node.setAttribute('contenteditable', 'false')
      node.textContent = 'rendered'
      replaceRangeWithNode(el, 4, 9, node)
      expect(extractPlainText(el)).toBe('see $x^2$ here')
      expect(el.querySelector('[data-source]')).not.toBeNull()
    })

    it('replaces at start of text', () => {
      el.textContent = '`code` rest'
      const node = document.createElement('code')
      node.setAttribute('data-source', '`code`')
      node.setAttribute('contenteditable', 'false')
      node.textContent = 'code'
      replaceRangeWithNode(el, 0, 6, node)
      expect(extractPlainText(el)).toBe('`code` rest')
    })
  })
})
