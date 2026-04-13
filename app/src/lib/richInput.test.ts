import { describe, it, expect, beforeEach, afterEach } from 'vitest'
import {
  extractPlainText,
  getCursorOffset,
  setCursorOffset,
  replaceRangeWithText,
  replaceRangeWithNode,
} from './richInput'

describe('richInput', () => {
  let el: HTMLDivElement

  beforeEach(() => {
    el = document.createElement('div')
    el.setAttribute('contenteditable', 'true')
    document.body.appendChild(el)
  })

  afterEach(() => {
    el.remove()
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

    it('recurses into nested elements without data-source', () => {
      el.innerHTML = '<div>line one</div><div>line two</div>'
      expect(extractPlainText(el)).toBe('line oneline two')
    })

    it('handles mixed rendered nodes and br elements', () => {
      el.innerHTML = 'text<br><span data-source="$a$" contenteditable="false">a</span><br>end'
      expect(extractPlainText(el)).toBe('text\n$a$\nend')
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

    it('replaces in the middle of text', () => {
      el.textContent = 'a \\to b'
      replaceRangeWithText(el, 2, 5, '→')
      expect(el.textContent).toBe('a → b')
    })

    it('replaces inside a nested div element', () => {
      el.innerHTML = '<div>hello \\alpha</div>'
      replaceRangeWithText(el, 6, 12, 'α')
      expect(extractPlainText(el)).toBe('hello α')
    })

    it('replaces across text adjacent to a rendered node', () => {
      el.innerHTML = '<span data-source="$x$" contenteditable="false">x</span> \\to end'
      // The \\to starts at offset 4 ($x$ is 3 chars + space = 4)
      replaceRangeWithText(el, 4, 7, '→')
      expect(extractPlainText(el)).toBe('$x$ → end')
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

    it('replaces at end of text', () => {
      el.textContent = 'prefix $y$'
      const node = document.createElement('span')
      node.setAttribute('data-source', '$y$')
      node.setAttribute('contenteditable', 'false')
      node.textContent = 'y'
      replaceRangeWithNode(el, 7, 10, node)
      expect(extractPlainText(el)).toBe('prefix $y$')
    })

    it('handles replacing adjacent to br elements', () => {
      el.innerHTML = 'first<br>$x$ end'
      const node = document.createElement('span')
      node.setAttribute('data-source', '$x$')
      node.setAttribute('contenteditable', 'false')
      node.textContent = 'x'
      // After first\n, offset 6 is start of $x$, offset 9 is end
      replaceRangeWithNode(el, 6, 9, node)
      expect(extractPlainText(el)).toBe('first\n$x$ end')
    })
  })

  // ── getCursorOffset / setCursorOffset ───────────────────────────────

  describe('cursor offset', () => {
    it('getCursorOffset returns 0 when no selection', () => {
      el.textContent = 'hello'
      expect(getCursorOffset(el)).toBe(0)
    })

    it('setCursorOffset and getCursorOffset round-trip on plain text', () => {
      el.textContent = 'hello world'
      el.focus()
      setCursorOffset(el, 5)
      expect(getCursorOffset(el)).toBe(5)
    })

    it('setCursorOffset at start', () => {
      el.textContent = 'hello'
      el.focus()
      setCursorOffset(el, 0)
      expect(getCursorOffset(el)).toBe(0)
    })

    it('setCursorOffset at end', () => {
      el.textContent = 'hello'
      el.focus()
      setCursorOffset(el, 5)
      expect(getCursorOffset(el)).toBe(5)
    })

    it('setCursorOffset with rendered node skips over it', () => {
      el.innerHTML = 'ab<span data-source="$x$" contenteditable="false">x</span>cd'
      el.focus()
      // Offset 5 = 'ab' (2) + '$x$' (3) = after the rendered node
      setCursorOffset(el, 5)
      expect(getCursorOffset(el)).toBe(5)
    })

    it('setCursorOffset before rendered node', () => {
      el.innerHTML = 'ab<span data-source="$x$" contenteditable="false">x</span>cd'
      el.focus()
      setCursorOffset(el, 2)
      expect(getCursorOffset(el)).toBe(2)
    })

    it('setCursorOffset after br element', () => {
      el.innerHTML = 'hello<br>world'
      el.focus()
      // 'hello' (5) + '\n' (1) = offset 6 is start of 'world'
      setCursorOffset(el, 6)
      expect(getCursorOffset(el)).toBe(6)
    })

    it('getCursorOffset after rendered node followed by text', () => {
      el.innerHTML = '<span data-source="$a$" contenteditable="false">a</span>bc'
      el.focus()
      // Offset 4 = '$a$' (3) + 'b' (1) = in the 'bc' text node
      setCursorOffset(el, 4)
      expect(getCursorOffset(el)).toBe(4)
    })

    it('getCursorOffset in nested div', () => {
      el.innerHTML = '<div>nested text</div>'
      el.focus()
      setCursorOffset(el, 6)
      expect(getCursorOffset(el)).toBe(6)
    })

    it('getCursorOffset with cursor in nested div element', () => {
      el.innerHTML = '<div>abcdef</div>'
      el.focus()
      setCursorOffset(el, 3)
      expect(getCursorOffset(el)).toBe(3)
    })

    it('getCursorOffset at very end of complex content', () => {
      el.innerHTML = 'a<span data-source="$x$" contenteditable="false">x</span>b<br>c'
      el.focus()
      // 'a' (1) + '$x$' (3) + 'b' (1) + '\n' (1) + 'c' (1) = 7 total
      setCursorOffset(el, 7)
      expect(getCursorOffset(el)).toBe(7)
    })
  })
})
