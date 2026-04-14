import { describe, it, expect, vi } from 'vitest'
import { EditorState } from '@codemirror/state'
import { EditorView } from '@codemirror/view'
import { hoverTypeSource, HOVER_TYPE_DELAY_MS } from './hoverTooltip'

function makeView(doc: string): EditorView {
  const parent = document.createElement('div')
  return new EditorView({
    state: EditorState.create({ doc }),
    parent,
  })
}

describe('hoverTypeSource', () => {
  it('returns a tooltip when the LSP responds with contents', async () => {
    const fetchHover = vi.fn().mockResolvedValue({ contents: 'Nat → Nat → Nat' })
    const view = makeView('def foo : Nat := 42\n')
    const tooltip = await hoverTypeSource(view, 5, fetchHover)
    view.destroy()

    expect(tooltip).not.toBeNull()
    expect(tooltip?.pos).toBe(5)
    expect(tooltip?.above).toBe(true)
    if (!tooltip) throw new Error('expected tooltip')
    const { dom } = tooltip.create(view)
    expect(dom.className).toBe('lean-hover-popup')
    expect(dom.textContent).toContain('Nat → Nat → Nat')
  })

  it('converts CM6 position to 0-indexed line and character', async () => {
    const fetchHover = vi.fn().mockResolvedValue({ contents: 'T' })
    const view = makeView('line 0\nline 1\nline 2\n')
    // Position 10: inside "line 1" — 0-indexed line=1, char=3
    await hoverTypeSource(view, 10, fetchHover)
    view.destroy()

    expect(fetchHover).toHaveBeenCalledWith(1, 3)
  })

  it('returns null when the LSP responds with null', async () => {
    const fetchHover = vi.fn().mockResolvedValue(null)
    const view = makeView('def foo : Nat := 42\n')
    const tooltip = await hoverTypeSource(view, 5, fetchHover)
    view.destroy()
    expect(tooltip).toBeNull()
  })

  it('returns null when the LSP responds with empty contents', async () => {
    const fetchHover = vi.fn().mockResolvedValue({ contents: '   ' })
    const view = makeView('def foo : Nat := 42\n')
    const tooltip = await hoverTypeSource(view, 5, fetchHover)
    view.destroy()
    expect(tooltip).toBeNull()
  })

  it('returns null when the LSP request throws', async () => {
    const fetchHover = vi.fn().mockRejectedValue(new Error('LSP not connected'))
    const view = makeView('def foo : Nat := 42\n')
    const tooltip = await hoverTypeSource(view, 5, fetchHover)
    view.destroy()
    expect(tooltip).toBeNull()
  })

  it('HOVER_TYPE_DELAY_MS matches VS Code convention (300 ms)', () => {
    expect(HOVER_TYPE_DELAY_MS).toBe(300)
  })
})
