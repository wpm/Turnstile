/**
 * Code-action support — `textDocument/codeAction` with a gutter lightbulb
 * and Cmd/Ctrl+. keybinding.
 *
 * The lightbulb appears on the cursor line when one or more actions are
 * available. Pressing Enter on the popup, or clicking an entry, applies the
 * workspace edit as a single CodeMirror transaction (so undo rolls it back
 * in one step).
 */

import {
  GutterMarker,
  gutter,
  keymap,
  ViewPlugin,
  type EditorView,
  type PluginValue,
  type ViewUpdate,
} from '@codemirror/view'
import { StateField, StateEffect, Transaction, type Extension, type Text } from '@codemirror/state'
import {
  lspCodeActions,
  lspResolveCodeAction,
  type CodeActionInfo,
  type WorkspaceEditDto,
} from './lspRequests'

// ---------------------------------------------------------------------------
// State: actions available at the current cursor line
// ---------------------------------------------------------------------------

/** Payload: mapping from 1-indexed line number → list of available actions. */
const setCodeActionsEffect = StateEffect.define<Map<number, CodeActionInfo[]>>()

const codeActionsField = StateField.define<Map<number, CodeActionInfo[]>>({
  create() {
    return new Map()
  },
  update(map, tr) {
    for (const effect of tr.effects) {
      if (effect.is(setCodeActionsEffect)) {
        return effect.value
      }
    }
    // Clear stale actions when the document changes.
    if (tr.docChanged) return new Map()
    return map
  },
})

// ---------------------------------------------------------------------------
// Workspace edit application
// ---------------------------------------------------------------------------

/**
 * Convert LSP `TextEditDto` entries (0-indexed line/character) into CM6
 * changes, filtering to edits targeting `docUri`.
 *
 * Returns `null` if no edits apply to the current document.
 */
export function workspaceEditToChanges(
  edit: WorkspaceEditDto,
  docUri: string,
  doc: Text,
): { from: number; to: number; insert: string }[] | null {
  const matching = edit.changes.filter(([uri]) => uri === docUri).flatMap(([, edits]) => edits)
  if (matching.length === 0) return null

  const changes: { from: number; to: number; insert: string }[] = []
  for (const te of matching) {
    const startLineNum = te.start_line + 1
    const endLineNum = te.end_line + 1
    if (startLineNum < 1 || startLineNum > doc.lines) continue
    if (endLineNum < 1 || endLineNum > doc.lines) continue
    const startLine = doc.line(startLineNum)
    const endLine = doc.line(endLineNum)
    const from = Math.min(startLine.from + te.start_character, startLine.to)
    const to = Math.min(endLine.from + te.end_character, endLine.to)
    changes.push({ from, to, insert: te.new_text })
  }
  return changes.length > 0 ? changes : null
}

/**
 * Apply a workspace edit to the current editor view as a single transaction.
 *
 * Returns true on success. Edits targeting other files are ignored (current-
 * file-only constraint, mirroring go-to-definition).
 */
export function applyWorkspaceEdit(
  view: EditorView,
  edit: WorkspaceEditDto,
  docUri: string,
): boolean {
  const changes = workspaceEditToChanges(edit, docUri, view.state.doc)
  if (!changes) return false
  view.dispatch({
    changes,
    userEvent: 'lsp.codeAction',
    annotations: Transaction.userEvent.of('lsp.codeAction'),
  })
  return true
}

// ---------------------------------------------------------------------------
// Gutter lightbulb marker
// ---------------------------------------------------------------------------

class LightbulbMarker extends GutterMarker {
  override toDOM(): HTMLElement {
    const el = document.createElement('span')
    el.className = 'lean-code-action-lightbulb'
    el.setAttribute('aria-label', 'Code actions available')
    el.title = 'Code actions available (⌘.)'
    // Simple light-bulb SVG — themed via CSS (currentColor).
    el.innerHTML =
      '<svg width="12" height="12" viewBox="0 0 16 16" fill="none" aria-hidden="true">' +
      '<path fill="currentColor" d="M8 1.5a4 4 0 0 0-2.4 7.2c.5.4.8 1 .9 1.6H9.5c0-.6.4-1.2.9-1.6A4 4 0 0 0 8 1.5zM6 12h4v1H6v-1zm1 2h2v1H7v-1z"/>' +
      '</svg>'
    return el
  }
}

function codeActionGutter(onClick: (view: EditorView, lineNum: number) => void): Extension {
  return gutter({
    class: 'lean-code-action-gutter',
    lineMarker(view, line) {
      const lineNum = view.state.doc.lineAt(line.from).number // 1-indexed
      const actions = view.state.field(codeActionsField).get(lineNum)
      if (!actions || actions.length === 0) return null
      return new LightbulbMarker()
    },
    domEventHandlers: {
      mousedown(view, line, event) {
        const lineNum = view.state.doc.lineAt(line.from).number
        const actions = view.state.field(codeActionsField).get(lineNum)
        if (!actions || actions.length === 0) return false
        ;(event as MouseEvent).preventDefault()
        onClick(view, lineNum)
        return true
      },
    },
    initialSpacer: () => new LightbulbMarker(),
  })
}

// ---------------------------------------------------------------------------
// Popup menu
// ---------------------------------------------------------------------------

interface Popup {
  dom: HTMLElement
  destroy: () => void
}

export function showActionsPopup(
  view: EditorView,
  actions: CodeActionInfo[],
  onApply: (action: CodeActionInfo) => Promise<boolean>,
): Popup {
  const dom = document.createElement('div')
  dom.className = 'lean-code-action-popup'
  dom.setAttribute('role', 'listbox')
  dom.tabIndex = 0

  let selected = 0
  const items: HTMLElement[] = []
  actions.forEach((action, i) => {
    const item = document.createElement('div')
    item.className = 'lean-code-action-item'
    item.setAttribute('role', 'option')
    item.textContent = action.title
    item.addEventListener('mouseenter', () => {
      setSelected(i)
    })
    item.addEventListener('mousedown', (e) => {
      e.preventDefault()
      void apply(i)
    })
    dom.appendChild(item)
    items.push(item)
  })

  function setSelected(i: number): void {
    if (i < 0 || i >= items.length) return
    items[selected]?.classList.remove('lean-code-action-item-selected')
    selected = i
    items[selected]?.classList.add('lean-code-action-item-selected')
  }
  setSelected(0)

  async function apply(i: number): Promise<void> {
    const action = actions[i]
    if (!action) return
    const ok = await onApply(action)
    if (ok) destroy()
  }

  function onKey(event: KeyboardEvent): void {
    if (event.key === 'Escape') {
      destroy()
      event.preventDefault()
    } else if (event.key === 'ArrowDown') {
      setSelected((selected + 1) % items.length)
      event.preventDefault()
    } else if (event.key === 'ArrowUp') {
      setSelected((selected - 1 + items.length) % items.length)
      event.preventDefault()
    } else if (event.key === 'Enter') {
      void apply(selected)
      event.preventDefault()
    }
  }

  // Anchor above the cursor. `coordsAtPos` can throw in environments without
  // layout (jsdom in unit tests); the popup still functions unanchored.
  let cursorRect: { top: number; left: number } | null = null
  try {
    cursorRect = view.coordsAtPos(view.state.selection.main.head)
  } catch {
    cursorRect = null
  }
  if (cursorRect) {
    dom.style.position = 'fixed'
    dom.style.top = `${String(cursorRect.top - 4)}px`
    dom.style.left = `${String(cursorRect.left)}px`
    dom.style.transform = 'translateY(-100%)'
  }

  dom.addEventListener('keydown', onKey)
  document.body.appendChild(dom)
  dom.focus()

  const destroy = (): void => {
    dom.removeEventListener('keydown', onKey)
    dom.remove()
  }
  return { dom, destroy }
}

// ---------------------------------------------------------------------------
// Public extension
// ---------------------------------------------------------------------------

interface CodeActionsOptions {
  /** URI of the currently-open document — used to filter workspace edits. */
  currentUri: () => string
  /** Override used by tests. */
  fetchActions?: typeof lspCodeActions
  /** Override used by tests. */
  resolveAction?: typeof lspResolveCodeAction
}

/**
 * Pure helper: look up the applicable edit for an action, resolving lazily if
 * needed. Exposed so tests can exercise the resolve path without the editor.
 */
export async function resolveActionEdit(
  action: CodeActionInfo,
  resolveAction: typeof lspResolveCodeAction = lspResolveCodeAction,
): Promise<WorkspaceEditDto | null> {
  if (action.edit) return action.edit
  if (action.resolve_data === null || action.resolve_data === undefined) return null
  try {
    return await resolveAction(action)
  } catch {
    return null
  }
}

/** Public re-export for tests that need to assemble test fixtures. */
export { codeActionsField, setCodeActionsEffect }
export type { CodeActionInfo, WorkspaceEditDto }

/**
 * Build the code-actions CodeMirror extension (gutter + keymap + state).
 *
 * Requests actions on cursor-line changes, debounced to avoid spamming the
 * LSP during rapid movement. Ctrl+. / Cmd+. opens the popup at the cursor.
 */
export function codeActionsExtension(options: CodeActionsOptions): Extension {
  const fetchActions = options.fetchActions ?? lspCodeActions
  const resolveAction = options.resolveAction ?? lspResolveCodeAction

  const debounceMs = 250

  // Refresh the action cache for the current cursor line.
  const refreshPlugin = ViewPlugin.fromClass(
    class implements PluginValue {
      timer: ReturnType<typeof setTimeout> | null = null
      lastLine = -1

      constructor(view: EditorView) {
        this.schedule(view)
      }

      update(update: ViewUpdate): void {
        if (update.docChanged || update.selectionSet) {
          this.schedule(update.view)
        }
      }

      schedule(view: EditorView): void {
        const head = view.state.selection.main.head
        const line = view.state.doc.lineAt(head)
        if (line.number === this.lastLine && !view.state.field(codeActionsField).size) {
          // Same line and no stale entries — no refresh needed.
        }
        this.lastLine = line.number

        if (this.timer) clearTimeout(this.timer)
        this.timer = setTimeout(() => {
          void this.fetch(view, line.number, line.from, line.to)
        }, debounceMs)
      }

      async fetch(
        view: EditorView,
        lineNum: number,
        lineFrom: number,
        lineTo: number,
      ): Promise<void> {
        const startCol = 0
        const endCol = lineTo - lineFrom
        let actions: CodeActionInfo[]
        try {
          actions = await fetchActions(lineNum - 1, startCol, lineNum - 1, endCol)
        } catch {
          return
        }
        // Ensure view is still mounted.
        if (!view.dom.isConnected) return
        // Avoid updating state unless something changed.
        const existing = view.state.field(codeActionsField, false)
        if (actions.length === 0 && !existing?.has(lineNum)) return

        const map = new Map<number, CodeActionInfo[]>()
        if (actions.length > 0) {
          map.set(lineNum, actions)
        }
        view.dispatch({ effects: setCodeActionsEffect.of(map) })
      }

      destroy(): void {
        if (this.timer) clearTimeout(this.timer)
      }
    },
  )

  const onGutterClick = (view: EditorView, lineNum: number): void => {
    openActionsPopup(view, lineNum)
  }

  function openActionsPopup(view: EditorView, lineNum: number): void {
    const actions = view.state.field(codeActionsField).get(lineNum)
    if (!actions || actions.length === 0) return
    showActionsPopup(view, actions, async (action) => {
      const edit = await resolveActionEdit(action, resolveAction)
      if (!edit) return false
      return applyWorkspaceEdit(view, edit, options.currentUri())
    })
  }

  const keymapExt = keymap.of([
    {
      key: 'Mod-.',
      run(view) {
        const head = view.state.selection.main.head
        const lineNum = view.state.doc.lineAt(head).number
        openActionsPopup(view, lineNum)
        return true
      },
    },
  ])

  return [codeActionsField, refreshPlugin, codeActionGutter(onGutterClick), keymapExt]
}
