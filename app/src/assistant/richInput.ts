/**
 * Utilities for working with a ``contenteditable`` div as a rich text input.
 *
 * The input can contain:
 *   - Plain text nodes (user-typed text)
 *   - Rendered nodes (``contenteditable="false"`` elements with a
 *     ``data-source`` attribute preserving the original source text)
 *   - ``<br>`` elements (newlines from Shift+Enter)
 *
 * Plain text extraction reconstructs the original source by substituting
 * each rendered node with its ``data-source`` value.
 */

// ── Plain text extraction ──────────────────────────────────────────────

/**
 * Extract the logical plain text from a contenteditable element.
 *
 * Text nodes contribute their ``textContent``.  Rendered nodes (with
 * ``data-source``) contribute their source text.  ``<br>`` elements
 * contribute ``\n``.
 */
export function extractPlainText(el: HTMLElement): string {
  let result = ''
  for (const node of el.childNodes) {
    if (node.nodeType === Node.TEXT_NODE) {
      result += node.textContent ?? ''
    } else if (node.nodeType === Node.ELEMENT_NODE) {
      const elem = node as HTMLElement
      const source = elem.getAttribute('data-source')
      if (source !== null) {
        result += source
      } else if (elem.tagName === 'BR') {
        result += '\n'
      } else {
        // Recurse into child elements (e.g. divs from Enter key)
        result += extractPlainText(elem)
      }
    }
  }
  return result
}

// ── Cursor offset mapping ──────────────────────────────────────────────

/**
 * Get the current cursor offset within the plain-text representation
 * of the contenteditable content.
 */
export function getCursorOffset(el: HTMLElement): number {
  const sel = el.ownerDocument.getSelection()
  if (!sel || sel.rangeCount === 0) return 0

  const range = sel.getRangeAt(0)
  // Create a range from start of el to the cursor position
  const preRange = el.ownerDocument.createRange()
  preRange.selectNodeContents(el)
  preRange.setEnd(range.startContainer, range.startOffset)

  // Walk the nodes in preRange to compute the plain-text offset
  return computePlainTextLength(el, range.startContainer, range.startOffset)
}

/**
 * Set the cursor position within the contenteditable element,
 * mapped from a plain-text offset.
 */
export function setCursorOffset(el: HTMLElement, offset: number): void {
  const pos = findDomPosition(el, offset)
  if (!pos) return

  const sel = el.ownerDocument.getSelection()
  if (!sel) return
  const range = el.ownerDocument.createRange()
  range.setStart(pos.node, pos.offset)
  range.collapse(true)
  sel.removeAllRanges()
  sel.addRange(range)
}

// ── Range replacement ──────────────────────────────────────────────────

/**
 * Replace a range of plain text in the contenteditable element with new
 * plain text.  Used for Lean abbreviation replacement.
 */
export function replaceRangeWithText(
  el: HTMLElement,
  from: number,
  to: number,
  newText: string,
): void {
  const startPos = findDomPosition(el, from)
  const endPos = findDomPosition(el, to)
  if (!startPos || !endPos) return

  const range = el.ownerDocument.createRange()
  range.setStart(startPos.node, startPos.offset)
  range.setEnd(endPos.node, endPos.offset)
  range.deleteContents()

  const textNode = el.ownerDocument.createTextNode(newText)
  range.insertNode(textNode)

  // Normalize to merge adjacent text nodes
  el.normalize()
}

/**
 * Replace a range of plain text in the contenteditable element with an
 * HTML node.  Used for inline rendering of math and code.
 */
export function replaceRangeWithNode(
  el: HTMLElement,
  from: number,
  to: number,
  node: HTMLElement,
): void {
  const startPos = findDomPosition(el, from)
  const endPos = findDomPosition(el, to)
  if (!startPos || !endPos) return

  const range = el.ownerDocument.createRange()
  range.setStart(startPos.node, startPos.offset)
  range.setEnd(endPos.node, endPos.offset)
  range.deleteContents()
  range.insertNode(node)

  // Normalize to merge adjacent text nodes
  el.normalize()
  placeCursorAfterNode(el, node)
}

// ── Rendered-node helpers ──────────────────────────────────────────────

/**
 * Returns ``true`` if ``node`` is an element carrying a ``data-source``
 * attribute — i.e. a rendered inline math or code node.
 */
export function isRenderedNode(node: Node): boolean {
  return node.nodeType === Node.ELEMENT_NODE && (node as HTMLElement).hasAttribute('data-source')
}

/**
 * Place the cursor immediately after ``node`` inside a concrete ``Text``
 * node.  If no text node follows ``node`` in ``el``, an empty ``Text``
 * node is inserted so the cursor has somewhere to land.
 *
 * Using a child-index position on ``el`` (the alternative) is ambiguous
 * for the browser's insertion logic when the adjacent sibling is
 * ``contenteditable="false"``; this function avoids that ambiguity.
 */
export function placeCursorAfterNode(el: HTMLElement, node: HTMLElement): void {
  const sel = el.ownerDocument.defaultView?.getSelection()
  if (!sel) return

  let textNode: Text

  if (node.nextSibling?.nodeType === Node.TEXT_NODE) {
    textNode = node.nextSibling as Text
  } else {
    textNode = el.ownerDocument.createTextNode('')
    node.after(textNode)
  }

  const range = el.ownerDocument.createRange()
  range.setStart(textNode, 0)
  range.collapse(true)
  sel.removeAllRanges()
  sel.addRange(range)
}

/**
 * Remove a rendered node from ``el``, merge adjacent text nodes, and
 * return the plain-text offset at which the node started.
 */
export function removeRenderedNode(el: HTMLElement, node: HTMLElement): number {
  const startOffset = computeNodeStartOffset(el, node)
  node.remove()
  el.normalize()
  return startOffset
}

/**
 * Inspect the current ``Selection`` to determine whether the cursor is
 * immediately adjacent to a rendered node inside ``el``.
 *
 * Returns the adjacent rendered node and which side the cursor is on
 * (``'before'`` = cursor is just before the node; ``'after'`` = cursor is
 * just after the node), or ``null`` if the cursor is not adjacent to any
 * rendered node.
 */
export function getRenderedNodeAtCursor(
  el: HTMLElement,
): { node: HTMLElement; side: 'before' | 'after' } | null {
  const sel = el.ownerDocument.defaultView?.getSelection()
  if (!sel || sel.rangeCount === 0) return null

  const range = sel.getRangeAt(0)
  const container = range.startContainer
  const offset = range.startOffset

  if (container === el) {
    if (offset > 0) {
      const prev = el.childNodes[offset - 1]
      if (prev && isRenderedNode(prev)) {
        return { node: prev as HTMLElement, side: 'after' }
      }
    }
    const next = el.childNodes[offset]
    if (next && isRenderedNode(next)) {
      return { node: next as HTMLElement, side: 'before' }
    }
    return null
  }

  if (container.nodeType === Node.TEXT_NODE) {
    if (offset === 0) {
      const prev = container.previousSibling
      if (prev && isRenderedNode(prev)) {
        return { node: prev as HTMLElement, side: 'after' }
      }
    }
    if (offset === (container.textContent ?? '').length) {
      const next = container.nextSibling
      if (next && isRenderedNode(next)) {
        return { node: next as HTMLElement, side: 'before' }
      }
    }
  }

  return null
}

// ── Internal helpers ───────────────────────────────────────────────────

/**
 * Compute the plain-text offset of the start of ``node`` within ``el``.
 */
function computeNodeStartOffset(el: HTMLElement, node: HTMLElement): number {
  let offset = 0
  for (const child of el.childNodes) {
    if (child === node) return offset
    if (child.nodeType === Node.TEXT_NODE) {
      offset += (child.textContent ?? '').length
    } else if (child.nodeType === Node.ELEMENT_NODE) {
      const elem = child as HTMLElement
      const source = elem.getAttribute('data-source')
      if (source !== null) {
        offset += source.length
      } else if (elem.tagName === 'BR') {
        offset += 1
      } else {
        offset += plainTextLength(elem)
      }
    }
  }
  return offset
}

interface DomPosition {
  node: Node
  offset: number
}

/**
 * Find the DOM node and offset corresponding to a plain-text offset
 * within the contenteditable element.
 */
function findDomPosition(el: HTMLElement, targetOffset: number): DomPosition | null {
  let remaining = targetOffset

  for (const node of el.childNodes) {
    if (node.nodeType === Node.TEXT_NODE) {
      const len = (node.textContent ?? '').length
      if (remaining <= len) {
        return { node, offset: remaining }
      }
      remaining -= len
    } else if (node.nodeType === Node.ELEMENT_NODE) {
      const elem = node as HTMLElement
      const source = elem.getAttribute('data-source')
      if (source !== null) {
        const len = source.length
        if (remaining <= len) {
          // Position within a rendered node — place cursor before or after
          const parent = node.parentNode ?? el
          const index = Array.from(parent.childNodes).indexOf(node)
          return remaining === 0
            ? { node: parent, offset: index }
            : { node: parent, offset: index + 1 }
        }
        remaining -= len
      } else if (elem.tagName === 'BR') {
        if (remaining === 0) {
          const parent = node.parentNode ?? el
          const index = Array.from(parent.childNodes).indexOf(node)
          return { node: parent, offset: index }
        }
        remaining -= 1
      } else {
        // Recurse
        const result = findDomPosition(elem, remaining)
        if (result) return result
        remaining -= plainTextLength(elem)
      }
    }
  }

  // Offset at the very end
  return { node: el, offset: el.childNodes.length }
}

/** Compute the plain-text length of a DOM subtree. */
function plainTextLength(el: HTMLElement): number {
  let len = 0
  for (const node of el.childNodes) {
    if (node.nodeType === Node.TEXT_NODE) {
      len += (node.textContent ?? '').length
    } else if (node.nodeType === Node.ELEMENT_NODE) {
      const elem = node as HTMLElement
      const source = elem.getAttribute('data-source')
      if (source !== null) {
        len += source.length
      } else if (elem.tagName === 'BR') {
        len += 1
      } else {
        len += plainTextLength(elem)
      }
    }
  }
  return len
}

/**
 * Compute the plain-text length from the start of ``el`` up to the given
 * DOM position (container + offset).
 */
function computePlainTextLength(el: HTMLElement, container: Node, offset: number): number {
  // When the container IS el, offset is a child index — sum children up to that index.
  if (container === el) {
    let len = 0
    for (let i = 0; i < offset; i++) {
      const child: ChildNode | undefined = el.childNodes[i]
      if (!child) break
      if (child.nodeType === Node.TEXT_NODE) {
        len += (child.textContent ?? '').length
      } else if (child.nodeType === Node.ELEMENT_NODE) {
        const source = (child as HTMLElement).getAttribute('data-source')
        if (source !== null) {
          len += source.length
        } else if ((child as HTMLElement).tagName === 'BR') {
          len += 1
        } else {
          len += plainTextLength(child as HTMLElement)
        }
      }
    }
    return len
  }

  let len = 0

  for (const node of el.childNodes) {
    if (node === container) {
      // The cursor is in this text node
      if (node.nodeType === Node.TEXT_NODE) {
        return len + offset
      }
      // container is a child element — offset refers to its child index
      const elem = node as HTMLElement
      for (let i = 0; i < offset; i++) {
        const child: ChildNode | undefined = elem.childNodes[i]
        if (!child) break
        if (child.nodeType === Node.TEXT_NODE) {
          len += (child.textContent ?? '').length
        } else if (child.nodeType === Node.ELEMENT_NODE) {
          const source = (child as HTMLElement).getAttribute('data-source')
          len += source !== null ? source.length : plainTextLength(child as HTMLElement)
        }
      }
      return len
    }

    if (node.contains(container)) {
      // Recurse into this node
      if (node.nodeType === Node.ELEMENT_NODE) {
        return len + computePlainTextLength(node as HTMLElement, container, offset)
      }
    }

    // Node is before the cursor — add its full length
    if (node.nodeType === Node.TEXT_NODE) {
      len += (node.textContent ?? '').length
    } else if (node.nodeType === Node.ELEMENT_NODE) {
      const elem = node as HTMLElement
      const source = elem.getAttribute('data-source')
      if (source !== null) {
        len += source.length
      } else if (elem.tagName === 'BR') {
        len += 1
      } else {
        len += plainTextLength(elem)
      }
    }
  }

  return len
}
