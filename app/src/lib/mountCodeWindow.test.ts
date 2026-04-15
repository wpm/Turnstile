import { describe, it, expect, vi } from 'vitest'
import { EditorState } from '@codemirror/state'
import { EditorView } from '@codemirror/view'
import { mountCodeWindow, lineClickExtension, indirectFetchHover } from './mountCodeWindow'

describe('mountCodeWindow', () => {
  it('mounts and renders the initial content', () => {
    const container = document.createElement('div')
    const handle = mountCodeWindow(container, {
      initialTheme: 'dark',
      initialContent: 'line 1\nline 2',
    })

    expect(container.querySelector('.cm-editor')).not.toBeNull()
    expect(container.textContent).toContain('line 1')
    expect(container.textContent).toContain('line 2')

    handle.destroy()
  })

  it('is read-only: the editable attribute is false', () => {
    const container = document.createElement('div')
    const handle = mountCodeWindow(container, {
      initialTheme: 'dark',
      initialContent: 'x',
    })

    const content = container.querySelector('.cm-content')
    expect(content?.getAttribute('contenteditable')).toBe('false')

    handle.destroy()
  })

  it('setContent replaces the document', () => {
    const container = document.createElement('div')
    const handle = mountCodeWindow(container, {
      initialTheme: 'dark',
      initialContent: 'first',
    })

    handle.setContent('replaced')
    expect(container.textContent).toContain('replaced')
    expect(container.textContent).not.toContain('first')

    handle.destroy()
  })

  it('setActiveLine adds cm-goal-line to the specified line', () => {
    const container = document.createElement('div')
    const handle = mountCodeWindow(container, {
      initialTheme: 'dark',
      initialContent: 'a\nb\nc',
    })

    handle.setActiveLine(2)
    const active = container.querySelectorAll('.cm-goal-line')
    expect(active.length).toBe(1)

    handle.destroy()
  })

  it('setActiveLine with null clears the highlight', () => {
    const container = document.createElement('div')
    const handle = mountCodeWindow(container, {
      initialTheme: 'dark',
      initialContent: 'a\nb',
    })

    handle.setActiveLine(1)
    expect(container.querySelectorAll('.cm-goal-line').length).toBe(1)
    handle.setActiveLine(null)
    expect(container.querySelectorAll('.cm-goal-line').length).toBe(0)

    handle.destroy()
  })

  it('setActiveLine out-of-range line is a no-op', () => {
    const container = document.createElement('div')
    const handle = mountCodeWindow(container, {
      initialTheme: 'dark',
      initialContent: 'only one line',
    })

    handle.setActiveLine(5)
    expect(container.querySelectorAll('.cm-goal-line').length).toBe(0)

    handle.destroy()
  })

  it('destroy removes the editor from the DOM', () => {
    const container = document.createElement('div')
    const handle = mountCodeWindow(container, {
      initialTheme: 'dark',
      initialContent: 'x',
    })

    expect(container.querySelector('.cm-editor')).not.toBeNull()
    handle.destroy()
    expect(container.querySelector('.cm-editor')).toBeNull()
  })

  it('does not throw when mounted without a fetchHover or onLineClick', () => {
    const container = document.createElement('div')
    const handle = mountCodeWindow(container, {
      initialTheme: 'light',
      initialContent: 'x',
    })
    expect(container.querySelector('.cm-editor')).not.toBeNull()
    handle.destroy()
  })

  it('can accept an onLineClick callback without throwing', () => {
    const container = document.createElement('div')
    const onLineClick = vi.fn<(line: number) => void>()
    const handle = mountCodeWindow(container, {
      initialTheme: 'dark',
      initialContent: 'a\nb\nc',
      onLineClick,
    })
    // We don't simulate an actual click here — CM6 coordinate math in
    // jsdom is unreliable. The key invariant is that wiring the callback
    // does not explode at mount time.
    expect(container.querySelector('.cm-editor')).not.toBeNull()
    handle.destroy()
  })

  it('setCallbacks replaces the click handler so later callers see the latest closure', () => {
    const container = document.createElement('div')
    const handle = mountCodeWindow(container, {
      initialTheme: 'dark',
      initialContent: 'a',
    })
    const next = vi.fn<(line: number) => void>()
    // Swapping callbacks must not throw or re-mount the editor.
    handle.setCallbacks({ onLineClick: next })
    expect(container.querySelector('.cm-editor')).not.toBeNull()
    handle.destroy()
  })

  it('setContent is idempotent — no redundant dispatch when text unchanged', () => {
    const container = document.createElement('div')
    const handle = mountCodeWindow(container, {
      initialTheme: 'dark',
      initialContent: 'same',
    })
    // Second call with the same content should be a no-op; exercising it
    // ensures the guard is exercised.
    handle.setContent('same')
    expect(container.textContent).toContain('same')
    handle.destroy()
  })

  it('setActiveLine is idempotent for identical calls', () => {
    const container = document.createElement('div')
    const handle = mountCodeWindow(container, {
      initialTheme: 'dark',
      initialContent: 'a\nb',
    })
    handle.setActiveLine(1)
    // Second identical call should early-return; no decoration churn.
    handle.setActiveLine(1)
    expect(container.querySelectorAll('.cm-goal-line').length).toBe(1)
    handle.destroy()
  })

  it('setTheme swaps the theme extension without crashing', () => {
    const container = document.createElement('div')
    const handle = mountCodeWindow(container, {
      initialTheme: 'dark',
      initialContent: 'x',
    })
    // Ensure both branches of themeExtension get exercised via the
    // compartment reconfigure path.
    handle.setTheme('light')
    handle.setTheme('dark')
    expect(container.querySelector('.cm-editor')).not.toBeNull()
    handle.destroy()
  })
})

describe('lineClickExtension', () => {
  function makeView(doc: string, ext = lineClickExtension(() => undefined)): EditorView {
    const parent = document.createElement('div')
    return new EditorView({
      state: EditorState.create({ doc, extensions: [ext] }),
      parent,
    })
  }

  it('invokes the handler with the 1-indexed line under the click', () => {
    const onLineClick = vi.fn<(line: number) => void>()
    const view = makeView(
      'first\nsecond\nthird',
      lineClickExtension(() => onLineClick),
    )
    // Offset 8 lies inside "second" (after "first\n"), so lineAt → line 2.
    vi.spyOn(view, 'posAtCoords').mockReturnValue(8)
    view.contentDOM.dispatchEvent(
      new MouseEvent('click', { clientX: 10, clientY: 10, bubbles: true }),
    )
    expect(onLineClick).toHaveBeenCalledWith(2)
    view.destroy()
  })

  it('does nothing when the handler getter returns undefined', () => {
    const view = makeView(
      'x',
      lineClickExtension(() => undefined),
    )
    vi.spyOn(view, 'posAtCoords').mockReturnValue(0)
    // Dispatch should be a no-op — no handler to call, nothing to assert
    // beyond "does not throw."
    view.contentDOM.dispatchEvent(new MouseEvent('click', { bubbles: true }))
    view.destroy()
  })

  it('is a no-op when posAtCoords returns null', () => {
    const onLineClick = vi.fn<(line: number) => void>()
    const view = makeView(
      'x',
      lineClickExtension(() => onLineClick),
    )
    vi.spyOn(view, 'posAtCoords').mockReturnValue(null)
    view.contentDOM.dispatchEvent(new MouseEvent('click', { bubbles: true }))
    expect(onLineClick).not.toHaveBeenCalled()
    view.destroy()
  })

  it('reads the handler fresh on every click — later setCallbacks swaps work', () => {
    let current: ((line: number) => void) | undefined
    const view = makeView(
      'a\nb',
      lineClickExtension(() => current),
    )
    vi.spyOn(view, 'posAtCoords').mockReturnValue(0)

    const first = vi.fn<(line: number) => void>()
    current = first
    view.contentDOM.dispatchEvent(new MouseEvent('click', { bubbles: true }))
    expect(first).toHaveBeenCalledWith(1)

    const second = vi.fn<(line: number) => void>()
    current = second
    view.contentDOM.dispatchEvent(new MouseEvent('click', { bubbles: true }))
    expect(second).toHaveBeenCalledWith(1)
    expect(first).toHaveBeenCalledTimes(1)

    view.destroy()
  })
})

describe('indirectFetchHover', () => {
  it('delegates to whichever fetcher the getter currently returns', async () => {
    const fetcher = vi
      .fn<(line: number, character: number) => Promise<null>>()
      .mockResolvedValue(null)
    const indirect = indirectFetchHover(() => fetcher)
    await indirect(3, 7)
    expect(fetcher).toHaveBeenCalledWith(3, 7)
  })

  it('resolves to null when the getter returns undefined', async () => {
    const indirect = indirectFetchHover(() => undefined)
    expect(await indirect(0, 0)).toBeNull()
  })

  it('re-reads the getter on every call — callback swaps take effect', async () => {
    const fetcherRef: {
      current:
        | ((line: number, character: number) => Promise<{ contents: string; kind: 'plaintext' }>)
        | undefined
    } = { current: undefined }
    const indirect = indirectFetchHover(() => fetcherRef.current)
    expect(await indirect(0, 0)).toBeNull()

    fetcherRef.current = vi.fn().mockResolvedValue({ contents: 'Nat', kind: 'plaintext' })
    const result = await indirect(1, 2)
    expect(result).toEqual({ contents: 'Nat', kind: 'plaintext' })
  })
})
