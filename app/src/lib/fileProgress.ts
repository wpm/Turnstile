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
        const ranges = effect.value
        // Empty array = elaboration complete, clear all highlights.
        if (ranges.length === 0) return Decoration.none

        const lineDecos: Range<Decoration>[] = []
        const doc = tr.state.doc

        for (const range of ranges) {
          const startLine = Math.max(1, range.start_line)
          const endLine = Math.min(doc.lines, range.end_line)

          for (let lineNum = startLine; lineNum <= endLine; lineNum++) {
            lineDecos.push(processingLineDeco.range(doc.line(lineNum).from))
          }
        }

        // Deduplicate in case ranges overlap.
        lineDecos.sort((a, b) => a.from - b.from)
        let prevFrom = -1
        const unique = lineDecos.filter((d) => {
          const dominated = d.from === prevFrom
          prevFrom = d.from
          return !dominated
        })
        return RangeSet.of(unique)
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
