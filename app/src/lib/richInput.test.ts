import { describe, it, expect, beforeEach, afterEach } from 'vitest'
import {
  extractPlainText,
  getCursorOffset,
  setCursorOffset,
  replaceRangeWithText,
  replaceRangeWithNode,
  isRenderedNode,
  placeCursorAfterNode,
  removeRenderedNode,
  getRenderedNodeAtCursor,
} from './richInput'

describe('richInput', () => {
  let el: HTMLDivElement

  beforeEach(() => {
    el = document.createElement('div')
    el.setAttribute('contenteditable', 'true')
    document.body.appendChild(el)
    document.getSelection()?.removeAllRanges()
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

    it('getCursorOffset accumulates past a nested div to reach trailing text', () => {
      // Exercises computePlainTextLength summing past a non-BR non-rendered element
      // and the final return len path.
      el.innerHTML = '<div>abc</div>xyz'
      el.focus()
      // Place cursor manually at offset 1 in the trailing 'xyz' text node.
      const trailingText = el.childNodes[1] // 'xyz' text node
      if (trailingText) {
        const range = document.createRange()
        range.setStart(trailingText, 1) // after 'x'
        range.collapse(true)
        document.getSelection()?.removeAllRanges()
        document.getSelection()?.addRange(range)
      }
      // 'abc' (3 from the div) + 'x' (1) = 4
      expect(getCursorOffset(el)).toBe(4)
    })

    it('getCursorOffset when cursor is at child-index position inside a nested element', () => {
      // Exercises the computePlainTextLength branch where container is an
      // element node and offset is a child-index within it.
      el.innerHTML = '<div><span data-source="`a`" contenteditable="false">a</span>text</div>'
      el.focus()
      const innerDiv = el.querySelector('div')
      if (!innerDiv) throw new Error('div not found')
      // Place cursor at child-index 1 inside the div (after the rendered span)
      const range = document.createRange()
      range.setStart(innerDiv, 1)
      range.collapse(true)
      document.getSelection()?.removeAllRanges()
      document.getSelection()?.addRange(range)
      // '`a`' is 3 chars — cursor is after it at offset 3
      expect(getCursorOffset(el)).toBe(3)
    })
  })

  // ── Test helpers for new functions ─────────────────────────────────

  /** Get the rendered span from el, failing fast if absent. */
  function getSpan(): HTMLElement {
    const span = el.querySelector<HTMLElement>('[data-source]')
    if (!span) throw new Error('Expected rendered span not found')
    return span
  }

  function setSelectionAt(container: Node, offset: number): void {
    const range = document.createRange()
    range.setStart(container, offset)
    range.collapse(true)
    const sel = document.getSelection()
    sel?.removeAllRanges()
    sel?.addRange(range)
  }

  // ── isRenderedNode ──────────────────────────────────────────────────

  describe('isRenderedNode', () => {
    it('returns true for element with data-source attribute', () => {
      const span = document.createElement('span')
      span.setAttribute('data-source', '$x$')
      span.setAttribute('contenteditable', 'false')
      expect(isRenderedNode(span)).toBe(true)
    })

    it('returns false for plain text node', () => {
      const text = document.createTextNode('hello')
      expect(isRenderedNode(text)).toBe(false)
    })

    it('returns false for element without data-source', () => {
      const div = document.createElement('div')
      expect(isRenderedNode(div)).toBe(false)
    })

    it('returns false for BR element', () => {
      const br = document.createElement('br')
      expect(isRenderedNode(br)).toBe(false)
    })
  })

  // ── placeCursorAfterNode ────────────────────────────────────────────

  describe('placeCursorAfterNode', () => {
    it('places cursor in existing next text node at offset 0', () => {
      el.innerHTML = '<span data-source="$x$" contenteditable="false">x</span>after'
      el.focus()
      placeCursorAfterNode(el, getSpan())
      const range = document.getSelection()?.getRangeAt(0)
      expect(range?.startContainer.nodeType).toBe(Node.TEXT_NODE)
      expect(range?.startOffset).toBe(0)
      expect(range?.startContainer.textContent).toBe('after')
    })

    it('creates empty text node when no sibling follows, cursor lands there', () => {
      el.innerHTML = '<span data-source="$x$" contenteditable="false">x</span>'
      el.focus()
      placeCursorAfterNode(el, getSpan())
      const range = document.getSelection()?.getRangeAt(0)
      expect(range?.startContainer.nodeType).toBe(Node.TEXT_NODE)
      expect(range?.startOffset).toBe(0)
    })

    it('getCursorOffset returns source length after placement at start', () => {
      el.innerHTML = '<span data-source="$x$" contenteditable="false">x</span>'
      el.focus()
      placeCursorAfterNode(el, getSpan())
      expect(getCursorOffset(el)).toBe(3) // '$x$' is 3 chars
    })

    it('getCursorOffset correct when rendered node is preceded by text', () => {
      el.innerHTML = 'ab<span data-source="$x$" contenteditable="false">x</span>'
      el.focus()
      placeCursorAfterNode(el, getSpan())
      expect(getCursorOffset(el)).toBe(5) // 'ab' (2) + '$x$' (3)
    })
  })

  // ── removeRenderedNode ──────────────────────────────────────────────

  describe('removeRenderedNode', () => {
    it('removes the rendered node from the DOM', () => {
      el.innerHTML = 'before <span data-source="$x$" contenteditable="false">x</span> after'
      removeRenderedNode(el, getSpan())
      expect(el.querySelector('[data-source]')).toBeNull()
    })

    it('merges adjacent text nodes after removal', () => {
      el.innerHTML = 'before<span data-source="$x$" contenteditable="false">x</span>after'
      removeRenderedNode(el, getSpan())
      // After normalize, should be a single text node
      const first = el.childNodes[0]
      expect(el.childNodes.length).toBe(1)
      expect(first?.nodeType).toBe(Node.TEXT_NODE)
      expect(first?.textContent).toBe('beforeafter')
    })

    it('returns the plain-text offset where the node started (rendered node at start)', () => {
      el.innerHTML = '<span data-source="$x$" contenteditable="false">x</span>after'
      const offset = removeRenderedNode(el, getSpan())
      expect(offset).toBe(0)
    })

    it('returns the plain-text offset where the node started (rendered node after text)', () => {
      el.innerHTML = 'hello <span data-source="$x$" contenteditable="false">x</span> world'
      const offset = removeRenderedNode(el, getSpan())
      expect(offset).toBe(6) // 'hello ' is 6 chars
    })

    it('preserves surrounding plain text after removal', () => {
      el.innerHTML = 'hello <span data-source="$x$" contenteditable="false">x</span> world'
      removeRenderedNode(el, getSpan())
      expect(extractPlainText(el)).toBe('hello  world')
    })
  })

  // ── getRenderedNodeAtCursor ─────────────────────────────────────────

  describe('getRenderedNodeAtCursor', () => {
    it('returns null when no selection exists', () => {
      el.textContent = 'hello'
      // No focus, no selection
      document.getSelection()?.removeAllRanges()
      expect(getRenderedNodeAtCursor(el)).toBeNull()
    })

    it('returns null when cursor is in the middle of a text node', () => {
      el.textContent = 'hello world'
      el.focus()
      setCursorOffset(el, 5)
      expect(getRenderedNodeAtCursor(el)).toBeNull()
    })

    it('returns null when cursor is between two plain text nodes (no rendered node adjacent)', () => {
      el.innerHTML = 'hello world'
      el.focus()
      setCursorOffset(el, 0)
      expect(getRenderedNodeAtCursor(el)).toBeNull()
    })

    it('returns { node, side: "after" } when cursor is at child-index position after rendered node', () => {
      el.innerHTML = '<span data-source="$x$" contenteditable="false">x</span>after'
      el.focus()
      // Place cursor at child-index 1 (right after the span, before "after")
      setSelectionAt(el, 1)
      const result = getRenderedNodeAtCursor(el)
      expect(result?.side).toBe('after')
      expect(result?.node.getAttribute('data-source')).toBe('$x$')
    })

    it('returns { node, side: "before" } when cursor is at child-index position before rendered node', () => {
      el.innerHTML = 'before<span data-source="$x$" contenteditable="false">x</span>'
      el.focus()
      // Place cursor at child-index 1 (right before the span, after "before")
      setSelectionAt(el, 1)
      const result = getRenderedNodeAtCursor(el)
      expect(result?.side).toBe('before')
      expect(result?.node.getAttribute('data-source')).toBe('$x$')
    })

    it('returns { node, side: "after" } when cursor is at offset 0 of text node after rendered node', () => {
      el.innerHTML = '<span data-source="$x$" contenteditable="false">x</span>after'
      el.focus()
      const textNode = el.childNodes[1] // "after" text node
      if (textNode) setSelectionAt(textNode, 0)
      const result = getRenderedNodeAtCursor(el)
      expect(result?.side).toBe('after')
    })

    it('returns { node, side: "before" } when cursor is at end of text node before rendered node', () => {
      el.innerHTML = 'before<span data-source="$x$" contenteditable="false">x</span>'
      el.focus()
      const textNode = el.childNodes[0] // "before" text node
      if (textNode) setSelectionAt(textNode, 6) // "before".length === 6
      const result = getRenderedNodeAtCursor(el)
      expect(result?.side).toBe('before')
    })
  })

  // ── replaceRangeWithNode regression: cursor in Text node ────────────

  describe('replaceRangeWithNode cursor placement', () => {
    it('places cursor in a Text node (not child-index on el) after replacement', () => {
      el.textContent = 'see $x^2$ here'
      el.focus()
      const node = document.createElement('span')
      node.setAttribute('data-source', '$x^2$')
      node.setAttribute('contenteditable', 'false')
      node.textContent = 'rendered'
      replaceRangeWithNode(el, 4, 9, node)
      const range = document.getSelection()?.getRangeAt(0)
      expect(range?.startContainer.nodeType).toBe(Node.TEXT_NODE)
      expect(range?.startContainer).not.toBe(el)
    })
  })
})
