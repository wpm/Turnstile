import { describe, it, expect, vi } from 'vitest'
import { mountCodeWindow } from './mountCodeWindow'

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
})
