import { describe, it, expect, vi } from 'vitest'
import { EditorState } from '@codemirror/state'
import { EditorView } from '@codemirror/view'
import { handleGotoDefinition } from './gotoDefinition'

function makeView(doc: string): EditorView {
  const parent = document.createElement('div')
  return new EditorView({
    state: EditorState.create({ doc }),
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
