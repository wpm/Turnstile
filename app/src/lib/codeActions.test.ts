import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { EditorState } from '@codemirror/state'
import { EditorView } from '@codemirror/view'
import {
  applyWorkspaceEdit,
  workspaceEditToChanges,
  resolveActionEdit,
  codeActionsExtension,
  codeActionsField,
  setCodeActionsEffect,
  type WorkspaceEditDto,
  type CodeActionInfo,
} from './codeActions'

const DOC_URI = 'file:///proof.lean'

function makeView(doc: string): EditorView {
  const parent = document.createElement('div')
  return new EditorView({
    state: EditorState.create({ doc }),
    parent,
  })
}

describe('workspaceEditToChanges', () => {
  it('converts a single-line replace to a CM6 change', () => {
    const view = makeView('def foo : Nat := sorry\n')
    const edit: WorkspaceEditDto = {
      changes: [
        [
          DOC_URI,
          [
            {
              start_line: 0,
              start_character: 17,
              end_line: 0,
              end_character: 22,
              new_text: 'exact rfl',
            },
          ],
        ],
      ],
    }
    const changes = workspaceEditToChanges(edit, DOC_URI, view.state.doc)
    expect(changes).not.toBeNull()
    expect(changes).toEqual([{ from: 17, to: 22, insert: 'exact rfl' }])
    view.destroy()
  })

  it('converts a multi-line replace across lines', () => {
    const view = makeView('line 0\nline 1\nline 2\n')
    const edit: WorkspaceEditDto = {
      changes: [
        [
          DOC_URI,
          [
            {
              start_line: 0,
              start_character: 2,
              end_line: 2,
              end_character: 4,
              new_text: 'REPLACED',
            },
          ],
        ],
      ],
    }
    const changes = workspaceEditToChanges(edit, DOC_URI, view.state.doc)
    expect(changes).toEqual([{ from: 2, to: 18, insert: 'REPLACED' }])
    view.destroy()
  })

  it('filters out edits targeting other files', () => {
    const view = makeView('def foo : Nat := 42\n')
    const edit: WorkspaceEditDto = {
      changes: [
        [
          'file:///other.lean',
          [
            {
              start_line: 0,
              start_character: 0,
              end_line: 0,
              end_character: 3,
              new_text: 'xxx',
            },
          ],
        ],
      ],
    }
    expect(workspaceEditToChanges(edit, DOC_URI, view.state.doc)).toBeNull()
    view.destroy()
  })

  it('merges edits from matching URI and ignores others', () => {
    const view = makeView('def foo : Nat := 42\n')
    const edit: WorkspaceEditDto = {
      changes: [
        [
          DOC_URI,
          [
            {
              start_line: 0,
              start_character: 0,
              end_line: 0,
              end_character: 3,
              new_text: 'DEF',
            },
          ],
        ],
        [
          'file:///other.lean',
          [
            {
              start_line: 0,
              start_character: 0,
              end_line: 0,
              end_character: 1,
              new_text: 'X',
            },
          ],
        ],
      ],
    }
    const changes = workspaceEditToChanges(edit, DOC_URI, view.state.doc)
    expect(changes).toEqual([{ from: 0, to: 3, insert: 'DEF' }])
    view.destroy()
  })

  it('skips edits whose lines are outside the document', () => {
    const view = makeView('abc\n')
    const edit: WorkspaceEditDto = {
      changes: [
        [
          DOC_URI,
          [
            {
              start_line: 99,
              start_character: 0,
              end_line: 99,
              end_character: 0,
              new_text: 'zzz',
            },
          ],
        ],
      ],
    }
    expect(workspaceEditToChanges(edit, DOC_URI, view.state.doc)).toBeNull()
    view.destroy()
  })

  it('clamps character offsets past end of line', () => {
    const view = makeView('abc\n')
    const edit: WorkspaceEditDto = {
      changes: [
        [
          DOC_URI,
          [
            {
              start_line: 0,
              start_character: 0,
              end_line: 0,
              end_character: 999,
              new_text: 'x',
            },
          ],
        ],
      ],
    }
    const changes = workspaceEditToChanges(edit, DOC_URI, view.state.doc)
    expect(changes).toEqual([{ from: 0, to: 3, insert: 'x' }])
    view.destroy()
  })
})

describe('applyWorkspaceEdit', () => {
  it('dispatches a single transaction and returns true on success', () => {
    const view = makeView('sorry\n')
    const edit: WorkspaceEditDto = {
      changes: [
        [
          DOC_URI,
          [
            {
              start_line: 0,
              start_character: 0,
              end_line: 0,
              end_character: 5,
              new_text: 'rfl',
            },
          ],
        ],
      ],
    }
    const result = applyWorkspaceEdit(view, edit, DOC_URI)
    expect(result).toBe(true)
    expect(view.state.doc.toString()).toBe('rfl\n')
    view.destroy()
  })

  it('returns false when no edits target the current file', () => {
    const view = makeView('def foo := 42\n')
    const edit: WorkspaceEditDto = {
      changes: [
        [
          'file:///other.lean',
          [
            {
              start_line: 0,
              start_character: 0,
              end_line: 0,
              end_character: 1,
              new_text: 'x',
            },
          ],
        ],
      ],
    }
    const result = applyWorkspaceEdit(view, edit, DOC_URI)
    expect(result).toBe(false)
    expect(view.state.doc.toString()).toBe('def foo := 42\n')
    view.destroy()
  })
})

describe('resolveActionEdit', () => {
  it('returns the inline edit when present', async () => {
    const edit: WorkspaceEditDto = { changes: [] }
    const action: CodeActionInfo = {
      title: 'Try this',
      kind: null,
      edit,
      resolve_data: null,
    }
    const resolveAction = vi.fn()
    const result = await resolveActionEdit(action, resolveAction)
    expect(result).toBe(edit)
    expect(resolveAction).not.toHaveBeenCalled()
  })

  it('resolves via codeAction/resolve when only data is present', async () => {
    const resolved: WorkspaceEditDto = { changes: [] }
    const resolveAction = vi.fn().mockResolvedValue(resolved)
    const action: CodeActionInfo = {
      title: 'Lazy',
      kind: null,
      edit: null,
      resolve_data: { token: 42 },
    }
    const result = await resolveActionEdit(action, resolveAction)
    expect(resolveAction).toHaveBeenCalledWith(action)
    expect(result).toBe(resolved)
  })

  it('returns null when resolve fails', async () => {
    const resolveAction = vi.fn().mockRejectedValue(new Error('no'))
    const action: CodeActionInfo = {
      title: 'Lazy',
      kind: null,
      edit: null,
      resolve_data: { token: 42 },
    }
    const result = await resolveActionEdit(action, resolveAction)
    expect(result).toBeNull()
  })

  it('returns null when neither edit nor data are available', async () => {
    const action: CodeActionInfo = {
      title: 'Bare',
      kind: null,
      edit: null,
      resolve_data: null,
    }
    const result = await resolveActionEdit(action)
    expect(result).toBeNull()
  })
})

// Helper: build a view with the code-actions extension wired up with a
// mocked fetcher. Useful for exercising the ViewPlugin + gutter + popup.
function makeViewWithExt(opts: {
  doc: string
  fetchActions: (...args: number[]) => Promise<CodeActionInfo[]>
  resolveAction?: (action: unknown) => Promise<WorkspaceEditDto | null>
  currentUri?: string
}): EditorView {
  const parent = document.createElement('div')
  document.body.appendChild(parent)
  return new EditorView({
    state: EditorState.create({
      doc: opts.doc,
      extensions: [
        codeActionsExtension({
          currentUri: () => opts.currentUri ?? DOC_URI,
          fetchActions: opts.fetchActions,
          ...(opts.resolveAction ? { resolveAction: opts.resolveAction } : {}),
        }),
      ],
    }),
    parent,
  })
}

describe('codeActionsExtension', () => {
  beforeEach(() => {
    vi.useFakeTimers()
  })
  afterEach(() => {
    vi.useRealTimers()
  })

  it('populates the actions field after debounce when LSP returns actions', async () => {
    const action: CodeActionInfo = {
      title: 'Try this',
      kind: null,
      edit: {
        changes: [
          [
            DOC_URI,
            [
              {
                start_line: 0,
                start_character: 0,
                end_line: 0,
                end_character: 5,
                new_text: 'fixed',
              },
            ],
          ],
        ],
      },
      resolve_data: null,
    }
    const fetchActions = vi.fn().mockResolvedValue([action])
    const view = makeViewWithExt({ doc: 'sorry\n', fetchActions })

    // Debounce is 250ms; advance past it and flush microtasks.
    await vi.advanceTimersByTimeAsync(300)

    expect(fetchActions).toHaveBeenCalled()
    const map = view.state.field(codeActionsField)
    // Cursor is on line 1 (1-indexed).
    expect(map.get(1)).toEqual([action])
    view.destroy()
  })

  it('renders a lightbulb marker in the gutter on lines with actions', async () => {
    const action: CodeActionInfo = {
      title: 'Try this',
      kind: null,
      edit: { changes: [] },
      resolve_data: null,
    }
    const fetchActions = vi.fn().mockResolvedValue([action])
    const view = makeViewWithExt({ doc: 'sorry\n', fetchActions })

    await vi.advanceTimersByTimeAsync(300)

    // The gutter class we set on the code-action gutter.
    const gutterEl = view.dom.querySelector('.lean-code-action-gutter')
    expect(gutterEl).not.toBeNull()
    // The marker inside — our LightbulbMarker.toDOM class name.
    const bulb = view.dom.querySelector('.lean-code-action-lightbulb')
    expect(bulb).not.toBeNull()
    expect(bulb?.getAttribute('aria-label')).toBe('Code actions available')
    view.destroy()
  })

  it('does not fetch synchronously — the initial schedule is debounced', () => {
    const fetchActions = vi.fn().mockResolvedValue([])
    const view = makeViewWithExt({ doc: 'abc\n', fetchActions })
    // Still inside the debounce window.
    expect(fetchActions).not.toHaveBeenCalled()
    view.destroy()
  })

  it('ignores LSP errors silently and leaves the field empty', async () => {
    const fetchActions = vi.fn().mockRejectedValue(new Error('LSP down'))
    const view = makeViewWithExt({ doc: 'abc\n', fetchActions })

    await vi.advanceTimersByTimeAsync(300)

    expect(fetchActions).toHaveBeenCalled()
    expect(view.state.field(codeActionsField).size).toBe(0)
    view.destroy()
  })

  it('clears stale actions when the document changes', () => {
    // Seed the field via the effect to avoid timing.
    const action: CodeActionInfo = {
      title: 'X',
      kind: null,
      edit: { changes: [] },
      resolve_data: null,
    }
    const fetchActions = vi.fn().mockResolvedValue([])
    const view = makeViewWithExt({ doc: 'abc\n', fetchActions })
    view.dispatch({ effects: setCodeActionsEffect.of(new Map([[1, [action]]])) })
    expect(view.state.field(codeActionsField).size).toBe(1)

    view.dispatch({ changes: { from: 0, insert: 'X' } })
    expect(view.state.field(codeActionsField).size).toBe(0)
    view.destroy()
  })
})
