/**
 * Go-to-definition support — F12 + Cmd/Ctrl-click — for the CodeMirror editor.
 *
 * Current-file only: if the LSP resolves the target to a different file we
 * call `onExternalDef` (the app wires this to an informational toast) and do
 * nothing to the editor.
 */

import { EditorView, keymap } from '@codemirror/view'
import type { Extension } from '@codemirror/state'
import { lspDefinition, type DefinitionLocation } from './lspRequests'
import { cmPosToLsp, lspPosToCmOffset } from './positionConvert'

interface GotoDefinitionOptions {
  /** Called when the definition is resolved in a different file. */
  onExternalDef: (uri: string) => void
  /** Returns the URI of the currently-open document. */
  currentUri: () => string
  /** Override used by tests. Defaults to `lspDefinition`. */
  fetchDefinition?: typeof lspDefinition
}

/**
 * Resolve the definition at `pos` and, if it's in the current file, move the
 * cursor there and scroll into view. Exposed for tests.
 */
export async function handleGotoDefinition(
  view: EditorView,
  pos: number,
  options: GotoDefinitionOptions,
): Promise<'jumped' | 'external' | 'none'> {
  const doc = view.state.doc
  const { line: lspLine, character: col } = cmPosToLsp(doc, pos)

  const fetchDefinition = options.fetchDefinition ?? lspDefinition

  let def: DefinitionLocation | null
  try {
    def = await fetchDefinition(lspLine, col)
  } catch {
    return 'none'
  }
  if (!def) return 'none'

  if (def.uri !== options.currentUri()) {
    options.onExternalDef(def.uri)
    return 'external'
  }

  const target = lspPosToCmOffset(doc, def.line, def.character)
  if (!target) return 'none'

  view.dispatch({
    selection: { anchor: target.offset },
    effects: EditorView.scrollIntoView(target.offset, { y: 'center' }),
  })
  view.focus()
  return 'jumped'
}

/** Build the go-to-definition extension — F12 key + Cmd/Ctrl-click. */
export function gotoDefinitionExtension(options: GotoDefinitionOptions): Extension {
  return [
    keymap.of([
      {
        key: 'F12',
        run: (view) => {
          const pos = view.state.selection.main.head
          void handleGotoDefinition(view, pos, options)
          return true
        },
      },
    ]),
    EditorView.domEventHandlers({
      mousedown(event, view) {
        if (!event.metaKey && !event.ctrlKey) return false
        const pos = view.posAtCoords({ x: event.clientX, y: event.clientY })
        if (pos === null) return false
        // Prevent the default cursor placement from stealing focus before the jump.
        event.preventDefault()
        void handleGotoDefinition(view, pos, options)
        return true
      },
    }),
  ]
}
