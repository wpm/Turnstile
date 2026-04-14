import { describe, it, expect, afterEach } from 'vitest'
import { EditorView, type DecorationSet } from '@codemirror/view'
import { EditorState } from '@codemirror/state'
import type { FileProgressRange } from './tauri'
import {
  computeProcessingLines,
  fileProgressExtension,
  setFileProgressEffect,
  type ProgressDocLike,
} from './fileProgress'

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/** Minimal ProgressDocLike — only `lines` is read by computeProcessingLines. */
function makeDoc(lines: number): ProgressDocLike {
  return { lines }
}

function range(startLine: number, endLine: number): FileProgressRange {
  return { start_line: startLine, end_line: endLine }
}

// ---------------------------------------------------------------------------
// computeProcessingLines
// ---------------------------------------------------------------------------

describe('computeProcessingLines', () => {
  it('returns [] when ranges is empty', () => {
    expect(computeProcessingLines(makeDoc(5), [])).toEqual([])
  })

  it('returns a single line for a one-line range', () => {
    expect(computeProcessingLines(makeDoc(5), [range(2, 2)])).toEqual([2])
  })

  it('expands a multi-line range into each line inclusively', () => {
    expect(computeProcessingLines(makeDoc(5), [range(2, 4)])).toEqual([2, 3, 4])
  })

  it('clamps start_line below 1 up to 1', () => {
    expect(computeProcessingLines(makeDoc(5), [range(-5, 2)])).toEqual([1, 2])
    expect(computeProcessingLines(makeDoc(5), [range(0, 2)])).toEqual([1, 2])
  })

  it('clamps end_line above doc.lines down to doc.lines', () => {
    expect(computeProcessingLines(makeDoc(5), [range(4, 999)])).toEqual([4, 5])
  })

  it('dedupes overlapping ranges and returns sorted lines', () => {
    expect(computeProcessingLines(makeDoc(10), [range(1, 3), range(2, 4)])).toEqual([1, 2, 3, 4])
  })

  it('skips inverted ranges where start > end', () => {
    expect(computeProcessingLines(makeDoc(10), [range(5, 2)])).toEqual([])
  })

  it('skips fully out-of-bounds ranges', () => {
    // start beyond doc.lines — after clamping end down to doc.lines, start > end.
    expect(computeProcessingLines(makeDoc(5), [range(100, 200)])).toEqual([])
  })

  it('combines valid and OOB ranges, returning only the valid lines sorted', () => {
    const result = computeProcessingLines(makeDoc(5), [range(100, 200), range(2, 3), range(1, 1)])
    expect(result).toEqual([1, 2, 3])
  })
})

// ---------------------------------------------------------------------------
// fileProgressExtension — StateField integration
//
// Mount a real EditorView in jsdom so the StateField's update logic and the
// EditorView.decorations facet both get exercised. The view provides
// decorations as a live DecorationSet we can iterate.
// ---------------------------------------------------------------------------

/**
 * Track all active views in the current test so we can clean them up even
 * if an assertion throws partway through.
 */
const liveViews: EditorView[] = []

afterEach(() => {
  while (liveViews.length > 0) {
    liveViews.pop()?.destroy()
  }
})

function mountProgressView(docText: string): EditorView {
  const view = new EditorView({
    state: EditorState.create({
      doc: docText,
      extensions: [fileProgressExtension()],
    }),
    parent: document.body,
  })
  liveViews.push(view)
  return view
}

/**
 * Iterate the decorations provided by `EditorView.decorations` and return
 * the `from` offset of each. The fileProgress StateField registers via
 * `EditorView.decorations.from(field)`, so its decorations show up here.
 */
function decoratedLineFroms(view: EditorView): number[] {
  const froms: number[] = []
  const contributors = view.state.facet(EditorView.decorations)
  for (const contributor of contributors) {
    const set: DecorationSet = typeof contributor === 'function' ? contributor(view) : contributor
    set.between(0, view.state.doc.length, (from) => {
      froms.push(from)
    })
  }
  return froms
}

describe('fileProgressExtension StateField', () => {
  // 5 lines, each "lineN" (5 chars) + "\n" (1 char) = 6 chars per line.
  // Line starts: 0, 6, 12, 18, 24.
  const docText = 'line1\nline2\nline3\nline4\nline5'

  it('starts with no decorations before any effect', () => {
    const view = mountProgressView(docText)
    expect(decoratedLineFroms(view)).toEqual([])
  })

  it('decorates each line in a single range', () => {
    const view = mountProgressView(docText)
    view.dispatch({
      effects: setFileProgressEffect.of([{ start_line: 2, end_line: 4 }]),
    })
    expect(decoratedLineFroms(view)).toEqual([6, 12, 18])
  })

  it('clears decorations when the effect is an empty array', () => {
    const view = mountProgressView(docText)
    view.dispatch({
      effects: setFileProgressEffect.of([{ start_line: 1, end_line: 2 }]),
    })
    expect(decoratedLineFroms(view)).toEqual([0, 6])
    view.dispatch({ effects: setFileProgressEffect.of([]) })
    expect(decoratedLineFroms(view)).toEqual([])
  })

  it('maps decorations through subsequent document changes', () => {
    const view = mountProgressView(docText)
    view.dispatch({
      effects: setFileProgressEffect.of([{ start_line: 3, end_line: 3 }]),
    })
    expect(decoratedLineFroms(view)).toEqual([12])

    // Insert "XYZ" at offset 0 — every line shifts by 3.
    view.dispatch({ changes: { from: 0, to: 0, insert: 'XYZ' } })
    expect(decoratedLineFroms(view)).toEqual([15])
  })

  it('clamps out-of-range lines via the shared computeProcessingLines helper', () => {
    const view = mountProgressView(docText)
    // start_line = 0 clamps to 1 (offset 0); end_line = 999 clamps to 5 (offset 24).
    view.dispatch({
      effects: setFileProgressEffect.of([{ start_line: 0, end_line: 999 }]),
    })
    expect(decoratedLineFroms(view)).toEqual([0, 6, 12, 18, 24])
  })

  it('dedupes overlapping ranges in the rendered decoration set', () => {
    const view = mountProgressView(docText)
    view.dispatch({
      effects: setFileProgressEffect.of([
        { start_line: 1, end_line: 3 },
        { start_line: 2, end_line: 4 },
      ]),
    })
    expect(decoratedLineFroms(view)).toEqual([0, 6, 12, 18])
  })
})
