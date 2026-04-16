import { describe, it, expect, vi, beforeAll } from 'vitest'
import { EditorState } from '@codemirror/state'
import { EditorView } from '@codemirror/view'
import { gotoDefinitionExtension, handleGotoDefinition } from './gotoDefinition'
import type { DefinitionLocation } from './lspRequests'

// jsdom lacks layout, so Range.getClientRects is undefined. CM6's default
// mousedown handler calls it internally and crashes. Stub it with a single
// zero-sized rect so dispatched mousedown events don't raise unhandled
// exceptions.
beforeAll(() => {
  const zeroRect = {
    top: 0,
    left: 0,
    right: 0,
    bottom: 0,
    width: 0,
    height: 0,
    x: 0,
    y: 0,
    toJSON() {
      return this
    },
  } as DOMRect
  if (typeof Range.prototype.getClientRects !== 'function') {
    Range.prototype.getClientRects = function () {
      return [zeroRect] as unknown as DOMRectList
    }
    Range.prototype.getBoundingClientRect = function () {
      return zeroRect
    }
  }
})

function makeView(doc: string): EditorView {
  const parent = document.createElement('div')
  return new EditorView({
    state: EditorState.create({ doc }),
    parent,
  })
}

function makeViewWithExt(opts: {
  doc: string
  fetchDefinition: (line: number, character: number) => Promise<DefinitionLocation | null>
  currentUri?: string
  onExternalDef?: (uri: string) => void
}): EditorView {
  const parent = document.createElement('div')
  document.body.appendChild(parent)
  return new EditorView({
    state: EditorState.create({
      doc: opts.doc,
      extensions: [
        gotoDefinitionExtension({
          currentUri: () => opts.currentUri ?? 'file:///proof.lean',
          fetchDefinition: opts.fetchDefinition,
          onExternalDef:
            opts.onExternalDef ??
            (() => {
              /* noop */
            }),
        }),
      ],
    }),
    parent,
  })
}

describe('handleGotoDefinition', () => {
  it('moves the cursor when the definition is in the current file', async () => {
    const fetchDefinition = vi.fn().mockResolvedValue({
      uri: 'file:///proof.lean',
      line: 0,
      character: 4,
      end_line: 0,
      end_character: 7,
    })
    const onExternalDef = vi.fn()
    const view = makeView('def foo : Nat := 42\nfoo\n')
    // Starting at position 10 (inside the 'foo' reference line)
    const result = await handleGotoDefinition(view, 10, {
      fetchDefinition,
      onExternalDef,
      currentUri: () => 'file:///proof.lean',
    })

    expect(result).toBe('jumped')
    expect(onExternalDef).not.toHaveBeenCalled()
    // Cursor should be on line 0 (`def foo`) at character 4
    expect(view.state.selection.main.head).toBe(4)
    view.destroy()
  })

  it('calls onExternalDef when the definition is in a different file', async () => {
    const fetchDefinition = vi.fn().mockResolvedValue({
      uri: 'file:///mathlib/Nat.lean',
      line: 0,
      character: 0,
      end_line: 0,
      end_character: 3,
    })
    const onExternalDef = vi.fn()
    const view = makeView('import Mathlib\nNat.succ 3\n')

    const initialHead = view.state.selection.main.head
    const result = await handleGotoDefinition(view, 20, {
      fetchDefinition,
      onExternalDef,
      currentUri: () => 'file:///proof.lean',
    })

    expect(result).toBe('external')
    expect(onExternalDef).toHaveBeenCalledWith('file:///mathlib/Nat.lean')
    // Cursor should not have moved
    expect(view.state.selection.main.head).toBe(initialHead)
    view.destroy()
  })

  it('returns "none" when the LSP returns null', async () => {
    const fetchDefinition = vi.fn().mockResolvedValue(null)
    const onExternalDef = vi.fn()
    const view = makeView('def foo : Nat := 42\n')

    const result = await handleGotoDefinition(view, 5, {
      fetchDefinition,
      onExternalDef,
      currentUri: () => 'file:///proof.lean',
    })

    expect(result).toBe('none')
    expect(onExternalDef).not.toHaveBeenCalled()
    view.destroy()
  })

  it('returns "none" when the LSP request throws', async () => {
    const fetchDefinition = vi.fn().mockRejectedValue(new Error('LSP not connected'))
    const onExternalDef = vi.fn()
    const view = makeView('def foo : Nat := 42\n')

    const result = await handleGotoDefinition(view, 5, {
      fetchDefinition,
      onExternalDef,
      currentUri: () => 'file:///proof.lean',
    })

    expect(result).toBe('none')
    view.destroy()
  })

  it('clamps cursor to the target line when character is past line end', async () => {
    const fetchDefinition = vi.fn().mockResolvedValue({
      uri: 'file:///proof.lean',
      line: 0,
      character: 999, // past end of "abc"
      end_line: 0,
      end_character: 999,
    })
    const onExternalDef = vi.fn()
    const view = makeView('abc\nxyz\n')

    await handleGotoDefinition(view, 5, {
      fetchDefinition,
      onExternalDef,
      currentUri: () => 'file:///proof.lean',
    })

    // Cursor should be at end of line 0, not past it
    expect(view.state.selection.main.head).toBe(3)
    view.destroy()
  })

  it('returns "none" when target line is out of document range', async () => {
    const fetchDefinition = vi.fn().mockResolvedValue({
      uri: 'file:///proof.lean',
      line: 99, // way past end
      character: 0,
      end_line: 99,
      end_character: 0,
    })
    const onExternalDef = vi.fn()
    const view = makeView('abc\n')

    const result = await handleGotoDefinition(view, 0, {
      fetchDefinition,
      onExternalDef,
      currentUri: () => 'file:///proof.lean',
    })

    expect(result).toBe('none')
    view.destroy()
  })
})

describe('gotoDefinitionExtension', () => {
  it('F12 key triggers a definition fetch at the cursor', async () => {
    const fetchDefinition = vi.fn().mockResolvedValue(null)
    const view = makeViewWithExt({ doc: 'def foo := 42\n', fetchDefinition })
    // Move cursor to offset 4 ("foo")
    view.dispatch({ selection: { anchor: 4 } })

    const event = new KeyboardEvent('keydown', { key: 'F12', bubbles: true })
    view.contentDOM.dispatchEvent(event)
    await Promise.resolve()
    await Promise.resolve()

    expect(fetchDefinition).toHaveBeenCalled()
    view.destroy()
  })

  it('Cmd-click triggers a definition fetch at the click position', async () => {
    const fetchDefinition = vi.fn().mockResolvedValue(null)
    const view = makeViewWithExt({ doc: 'def foo := 42\n', fetchDefinition })
    // Force posAtCoords to return a concrete offset regardless of layout.
    const posAtCoordsSpy = vi.spyOn(view, 'posAtCoords').mockReturnValue(4)

    const event = new MouseEvent('mousedown', {
      bubbles: true,
      cancelable: true,
      metaKey: true,
      clientX: 10,
      clientY: 10,
    })
    view.contentDOM.dispatchEvent(event)
    await Promise.resolve()
    await Promise.resolve()

    expect(event.defaultPrevented).toBe(true)
    expect(fetchDefinition).toHaveBeenCalled()
    posAtCoordsSpy.mockRestore()
    view.destroy()
  })

  it('plain mousedown (no modifier) is ignored by the handler', () => {
    const fetchDefinition = vi.fn().mockResolvedValue(null)
    const view = makeViewWithExt({ doc: 'def foo := 42\n', fetchDefinition })

    const event = new MouseEvent('mousedown', {
      bubbles: true,
      cancelable: true,
      clientX: 10,
      clientY: 10,
    })
    view.contentDOM.dispatchEvent(event)

    expect(fetchDefinition).not.toHaveBeenCalled()
    view.destroy()
  })

  it('Cmd-click with no position resolved is a no-op', () => {
    const fetchDefinition = vi.fn().mockResolvedValue(null)
    const view = makeViewWithExt({ doc: 'def foo := 42\n', fetchDefinition })
    const posAtCoordsSpy = vi.spyOn(view, 'posAtCoords').mockReturnValue(null)

    const event = new MouseEvent('mousedown', {
      bubbles: true,
      cancelable: true,
      metaKey: true,
      clientX: 10,
      clientY: 10,
    })
    view.contentDOM.dispatchEvent(event)

    // Our handler should not have acted; fetchDefinition is the authoritative
    // signal (defaultPrevented can flip due to CM6's default mousedown).
    expect(fetchDefinition).not.toHaveBeenCalled()
    posAtCoordsSpy.mockRestore()
    view.destroy()
  })
})
