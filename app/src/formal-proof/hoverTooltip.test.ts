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
  it('returns a plaintext tooltip when the LSP responds with plaintext contents', async () => {
    const fetchHover = vi.fn().mockResolvedValue({ contents: 'Nat → Nat → Nat', kind: 'plaintext' })
    const view = makeView('def foo : Nat := 42\n')
    const tooltip = await hoverTypeSource(view, 5, fetchHover)
    view.destroy()

    expect(tooltip).not.toBeNull()
    expect(tooltip?.pos).toBe(5)
    expect(tooltip?.above).toBe(true)
    if (!tooltip) throw new Error('expected tooltip')
    const { dom } = tooltip.create(view)
    expect(dom.className).toBe('lean-hover-popup')
    // Plaintext branch: rendered inside <pre> with textContent only — no HTML.
    const pre = dom.querySelector('pre')
    expect(pre).not.toBeNull()
    expect(pre?.textContent).toBe('Nat → Nat → Nat')
    expect(pre?.innerHTML).toBe('Nat → Nat → Nat')
  })

  it('renders markdown hover contents as rich HTML', async () => {
    const contents = '```lean\nfoo : Nat\n```\n\n**Docs** for `foo`.'
    const fetchHover = vi.fn().mockResolvedValue({ contents, kind: 'markdown' })
    const view = makeView('def foo : Nat := 42\n')
    const tooltip = await hoverTypeSource(view, 5, fetchHover)
    view.destroy()
    if (!tooltip) throw new Error('expected tooltip')

    const { dom } = tooltip.create(view)
    expect(dom.className).toBe('lean-hover-popup')
    // Markdown branch: inner container renders HTML via renderContent.
    const body = dom.querySelector('.lean-hover-popup-md')
    expect(body).not.toBeNull()
    // Bold emphasis from **Docs**.
    expect(body?.querySelector('strong')?.textContent).toBe('Docs')
    // Lean fenced code uses the shared highlighter class.
    expect(body?.querySelector('pre code.assistant-lean-code')).not.toBeNull()
  })

  it('converts CM6 position to 0-indexed line and character', async () => {
    const fetchHover = vi.fn().mockResolvedValue({ contents: 'T', kind: 'plaintext' })
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
    const fetchHover = vi.fn().mockResolvedValue({ contents: '   ', kind: 'plaintext' })
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
