/**
 * CodeMirror 6 extension for Lean elaboration progress highlighting.
 *
 * Shows a subtle background tint on lines currently being processed by the
 * Lean elaborator.  The visual treatment is isolated here so it can be
 * changed independently of the protocol plumbing in editor.ts / App.svelte.
 */

import { StateEffect, StateField, RangeSet, type Extension, type Range } from '@codemirror/state'
import { EditorView, Decoration, type DecorationSet } from '@codemirror/view'
import type { FileProgressRange } from './tauri'

// ---------------------------------------------------------------------------
// Effect — dispatched from EditorHandle.applyFileProgress()
// ---------------------------------------------------------------------------

export const setFileProgressEffect = StateEffect.define<FileProgressRange[]>()

// ---------------------------------------------------------------------------
// Pure helper — testable without a real CodeMirror EditorView
// ---------------------------------------------------------------------------

/**
 * Minimal slice of a CM6 `Text` that `computeProcessingLines` needs. Only
 * the total line count matters for deciding which lines are in-bounds.
 */
export interface ProgressDocLike {
  readonly lines: number
}

/**
 * Convert a list of LSP file-progress ranges into the 1-indexed line numbers
 * that should be decorated.
 *
 * - `start_line` is clamped up to 1 (no lines below 1).
 * - `end_line` is clamped down to `doc.lines` (no lines past end-of-doc).
 * - Ranges where the clamped `start > end` (inverted or fully past EOF) are
 *   dropped.
 * - The returned list is sorted ascending and deduplicated, so overlapping
 *   ranges produce a clean, stable input to `RangeSet.of`.
 * - An empty input yields an empty output.
 */
export function computeProcessingLines(
  doc: ProgressDocLike,
  ranges: FileProgressRange[],
): number[] {
  if (ranges.length === 0) return []

  const seen = new Set<number>()
  for (const r of ranges) {
    const start = Math.max(1, r.start_line)
    const end = Math.min(doc.lines, r.end_line)
    if (start > end) continue
    for (let line = start; line <= end; line++) {
      seen.add(line)
    }
  }

  return Array.from(seen).sort((a, b) => a - b)
}

// ---------------------------------------------------------------------------
// StateField — maps FileProgressRange[] to line decorations
// ---------------------------------------------------------------------------

const processingLineDeco = Decoration.line({ class: 'cm-lean-processing' })

const fileProgressField = StateField.define<DecorationSet>({
  create() {
    return Decoration.none
  },
  update(decorations, tr) {
    for (const effect of tr.effects) {
      if (effect.is(setFileProgressEffect)) {
        const lines = computeProcessingLines(tr.state.doc, effect.value)
        if (lines.length === 0) return Decoration.none

        const doc = tr.state.doc
        const lineDecos: Range<Decoration>[] = lines.map((lineNum) =>
          processingLineDeco.range(doc.line(lineNum).from),
        )
        return RangeSet.of(lineDecos)
      }
    }
    // Between progress notifications, map positions through doc changes.
    return decorations.map(tr.changes)
  },
  provide(field) {
    return EditorView.decorations.from(field)
  },
})

// ---------------------------------------------------------------------------
// Public extension bundle
// ---------------------------------------------------------------------------

export function fileProgressExtension(): Extension {
  return fileProgressField
}
