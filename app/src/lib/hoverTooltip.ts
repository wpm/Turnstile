/**
 * CodeMirror hover-tooltip extension showing Lean type info at the cursor.
 *
 * Delegates to the Rust `lsp_hover` Tauri command (which calls
 * `textDocument/hover`). The response includes the LSP `MarkupKind`; we
 * render markdown contents through the shared `renderContent` pipeline
 * (fenced Lean highlighting + LaTeX) and plaintext as preformatted text.
 */

import { hoverTooltip, type Tooltip, type EditorView } from '@codemirror/view'
import type { Extension } from '@codemirror/state'
import { lspHover, type HoverInfo } from './lspRequests'
import { cmPosToLsp } from './positionConvert'
import { renderContent } from './renderContent'

/** Hover delay in ms — matches VS Code. Exported for tests. */
export const HOVER_TYPE_DELAY_MS = 300

/**
 * Inner hover-source function used by the `hoverTooltip` extension.
 *
 * Split out for direct unit testing without booting a full EditorView.
 *
 * @param view CodeMirror view — only `state.doc.lineAt` is used.
 * @param pos Document offset the user is hovering over.
 * @param fetchHover Override used by tests. Defaults to `lspHover`.
 */
export async function hoverTypeSource(
  view: EditorView,
  pos: number,
  fetchHover: (line: number, character: number) => Promise<HoverInfo | null> = lspHover,
): Promise<Tooltip | null> {
  const { line: lspLine, character: col } = cmPosToLsp(view.state.doc, pos)

  let hover: HoverInfo | null
  try {
    hover = await fetchHover(lspLine, col)
  } catch {
    return null
  }
  if (!hover || hover.contents.trim() === '') return null

  const { contents, kind } = hover

  return {
    pos,
    above: true,
    create() {
      const dom = document.createElement('div')
      dom.className = 'lean-hover-popup'
      if (kind === 'markdown') {
        // Rich markdown: fenced Lean blocks highlighted, LaTeX rendered,
        // docstrings formatted. `renderContent` escapes raw HTML.
        const body = document.createElement('div')
        body.className = 'lean-hover-popup-content lean-hover-popup-md'
        body.innerHTML = renderContent(contents)
        dom.appendChild(body)
      } else {
        // Plaintext: preserve spacing exactly, no HTML interpretation.
        const pre = document.createElement('pre')
        pre.className = 'lean-hover-popup-content'
        pre.textContent = contents
        dom.appendChild(pre)
      }
      return { dom }
    },
  }
}

/**
 * Build the hover-type CodeMirror extension.
 *
 * @param fetchHover Override used by tests. Defaults to `lspHover`.
 */
export function hoverTypeExtension(fetchHover: typeof lspHover = lspHover): Extension {
  return hoverTooltip((view, pos) => hoverTypeSource(view, pos, fetchHover), {
    hoverTime: HOVER_TYPE_DELAY_MS,
  })
}
