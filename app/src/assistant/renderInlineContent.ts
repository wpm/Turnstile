/**
 * Create rendered DOM elements for inline insertion into the
 * contenteditable assistant input.
 *
 * Each element is ``contenteditable="false"`` (atomic — cannot be partially
 * edited) and carries a ``data-source`` attribute preserving the original
 * delimited source text so {@link extractPlainText} can reconstruct it.
 */

import { renderMath } from './math'
import { highlightLean } from '../formal-proof/leanHighlight'

/**
 * Create a rendered inline math element.
 *
 * @param latex     The LaTeX content (without delimiters).
 * @param display   Whether this is display math (``$$``).
 * @param sourceText The original delimited text (e.g. ``$x^2$``).
 */
export function createMathElement(
  latex: string,
  display: boolean,
  sourceText: string,
): HTMLElement {
  const el = document.createElement('span')
  el.className = 'assistant-rendered-inline assistant-rendered-math'
  el.setAttribute('contenteditable', 'false')
  el.setAttribute('data-source', sourceText)
  el.setAttribute('aria-label', sourceText)
  el.innerHTML = renderMath(latex, display)
  return el
}

/**
 * Create a rendered inline code element with Lean syntax highlighting.
 *
 * @param code       The code content (without backtick delimiters).
 * @param sourceText The original delimited text (e.g. `` `theorem` ``).
 */
export function createCodeElement(code: string, sourceText: string): HTMLElement {
  const el = document.createElement('code')
  el.className = 'assistant-rendered-inline assistant-rendered-code assistant-lean-code'
  el.setAttribute('contenteditable', 'false')
  el.setAttribute('data-source', sourceText)
  el.setAttribute('aria-label', sourceText)
  el.innerHTML = highlightLean(code)
  return el
}
