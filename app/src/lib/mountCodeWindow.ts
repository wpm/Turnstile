/**
 * Mount a read-only CodeMirror 6 instance for short snippets of Lean source
 * — e.g. the code blocks inside the Goal State panel. Hover is optional and
 * supplied by the caller: the CodeWindow's document isn't known to the LSP,
 * so different mount sites resolve hover against different real documents.
 */

import { EditorState, Compartment, type Extension } from '@codemirror/state'
import { EditorView, tooltips } from '@codemirror/view'
import {
  baseTheme,
  themeExtension,
  setActiveLineEffect,
  activeLineField,
} from './codeWindowExtensions'
import { hoverTypeExtension } from './hoverTooltip'
import type { HoverInfo } from './lspRequests'
import type { ResolvedTheme } from './theme'

/** Callback used by CodeWindow to resolve hover info. */
export type CodeWindowHoverFn = (line: number, character: number) => Promise<HoverInfo | null>

/**
 * Build the click extension that translates a DOM click into a 1-indexed
 * CodeMirror line number. Exported for direct unit testing — production
 * callers get this wired up through `mountCodeWindow`.
 */
export function lineClickExtension(
  getHandler: () => ((line: number) => void) | undefined,
): Extension {
  return EditorView.domEventHandlers({
    click(event, view) {
      const handler = getHandler()
      if (!handler) return false
      const pos = view.posAtCoords({ x: event.clientX, y: event.clientY })
      if (pos === null) return false
      handler(view.state.doc.lineAt(pos).number)
      return false
    },
  })
}

/**
 * Indirect hover-fetch trampoline: defers to whatever callback the getter
 * returns at call time, falling back to a resolved-null when no callback
 * is currently registered. Exported for direct unit testing.
 */
export function indirectFetchHover(
  getFetcher: () => CodeWindowHoverFn | undefined,
): CodeWindowHoverFn {
  return (line, character) => getFetcher()?.(line, character) ?? Promise.resolve(null)
}

interface CodeWindowOptions {
  initialTheme: ResolvedTheme
  initialContent?: string
  /**
   * Called when the user clicks anywhere on a line. `line` is 1-indexed
   * within this CodeWindow's own document — callers are responsible for
   * translating to any surrounding coordinate system.
   */
  onLineClick?: ((line: number) => void) | undefined
  /** Resolve hover info at a CodeWindow-local 0-indexed position. */
  fetchHover?: CodeWindowHoverFn | undefined
}

export interface CodeWindowHandle {
  setContent(text: string): void
  setActiveLine(line: number | null): void
  setTheme(theme: ResolvedTheme): void
  /** Update the callbacks after mount so prop changes propagate. */
  setCallbacks(opts: {
    onLineClick?: ((line: number) => void) | undefined
    fetchHover?: CodeWindowHoverFn | undefined
  }): void
  destroy(): void
}

export function mountCodeWindow(
  container: HTMLElement,
  options: CodeWindowOptions,
): CodeWindowHandle {
  const themeCompartment = new Compartment()

  // Svelte re-creates callback closures on every render; trap the latest
  // ones here so CodeMirror's long-lived extensions can always dispatch to
  // the current prop value rather than the one captured at mount.
  const callbacks: {
    onLineClick?: ((line: number) => void) | undefined
    fetchHover?: CodeWindowHoverFn | undefined
  } = {
    onLineClick: options.onLineClick,
    fetchHover: options.fetchHover,
  }

  const clickHandler = lineClickExtension(() => callbacks.onLineClick)
  const hoverExtension = hoverTypeExtension(indirectFetchHover(() => callbacks.fetchHover))

  const view = new EditorView({
    state: EditorState.create({
      doc: options.initialContent ?? '',
      extensions: [
        EditorState.readOnly.of(true),
        EditorView.editable.of(false),
        EditorView.lineWrapping,
        activeLineField,
        hoverExtension,
        tooltips({ parent: document.body }),
        clickHandler,
        baseTheme,
        themeCompartment.of(themeExtension(options.initialTheme)),
      ],
    }),
    parent: container,
  })

  let lastActiveLine: number | null = null

  return {
    setContent(text: string) {
      if (view.state.doc.toString() === text) return
      view.dispatch({
        changes: { from: 0, to: view.state.doc.length, insert: text },
      })
    },
    setActiveLine(line: number | null) {
      if (line === lastActiveLine) return
      lastActiveLine = line
      view.dispatch({
        effects: setActiveLineEffect.of(line === null ? [] : [line]),
      })
    },
    setTheme(t: ResolvedTheme) {
      view.dispatch({ effects: themeCompartment.reconfigure(themeExtension(t)) })
    },
    setCallbacks(opts) {
      callbacks.onLineClick = opts.onLineClick
      callbacks.fetchHover = opts.fetchHover
    },
    destroy() {
      view.destroy()
    },
  }
}
