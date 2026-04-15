/**
 * Shared CodeMirror 6 extensions used by both the editable `Editor` and the
 * read-only `CodeWindow`.
 *
 * Factoring these out keeps both mount sites aligned on one source of truth
 * for semantic-token decoration, active-line highlighting, and the app's
 * dark/light theme.
 */

import { EditorView, Decoration, type DecorationSet } from '@codemirror/view'
import { StateField, StateEffect, RangeSet, type Extension, type Range } from '@codemirror/state'
import type { SemanticToken } from './tauri'
import { buildSemanticTokenRanges, computeGoalLines } from './editorHelpers'
import type { ResolvedTheme } from './theme'

// ---------------------------------------------------------------------------
// Semantic token highlighting
// ---------------------------------------------------------------------------

export const setSemanticTokensEffect = StateEffect.define<SemanticToken[]>()

export const semanticTokensField = StateField.define<DecorationSet>({
  create() {
    return Decoration.none
  },
  update(decorations, tr) {
    decorations = decorations.map(tr.changes)
    for (const effect of tr.effects) {
      if (effect.is(setSemanticTokensEffect)) {
        const ranges = buildSemanticTokenRanges(effect.value, tr.state.doc).map((r) =>
          Decoration.mark({ class: r.cssClass }).range(r.from, r.to),
        )
        decorations = RangeSet.of(ranges)
      }
    }
    return decorations
  },
  provide(field) {
    return EditorView.decorations.from(field)
  },
})

// ---------------------------------------------------------------------------
// Active-line decoration — one or more CodeMirror lines marked with the
// `cm-goal-line` class. Used for goal-panel cross-highlighting.
// ---------------------------------------------------------------------------

export const setActiveLineEffect = StateEffect.define<number[]>()

const activeLineDeco = Decoration.line({ class: 'cm-goal-line' })

export const activeLineField = StateField.define<DecorationSet>({
  create() {
    return Decoration.none
  },
  update(decorations, tr) {
    for (const effect of tr.effects) {
      if (effect.is(setActiveLineEffect)) {
        const lines = computeGoalLines(tr.state.doc, effect.value)
        if (lines.length === 0) return Decoration.none
        const doc = tr.state.doc
        const lineDecos: Range<Decoration>[] = lines.map((n) =>
          activeLineDeco.range(doc.line(n).from),
        )
        return RangeSet.of(lineDecos)
      }
    }
    return decorations.map(tr.changes)
  },
  provide(field) {
    return EditorView.decorations.from(field)
  },
})

// ---------------------------------------------------------------------------
// Theme
// ---------------------------------------------------------------------------
//
// Typography is color-independent and always applied. The dark/light
// selectors use CSS custom properties from app.css; we only toggle CM6's
// `dark` flag so scrollbars and its own highlight-style fallback pick up the
// correct palette.

// `.cm-scroller` intentionally omits `font-size` so app.css's
// `--editor-font-size` knob takes effect (#127). Consumers that want a fixed
// size (e.g. CodeWindow) pin it with their own component-scoped style.
export const baseTheme = EditorView.baseTheme({
  '.cm-scroller': {
    overflow: 'auto',
    fontFamily: '"JetBrains Mono", ui-monospace, "SF Mono", "Cascadia Mono", monospace',
    lineHeight: '1.5',
  },
})

const darkTheme = EditorView.theme(
  {
    '&': { height: '100%', backgroundColor: 'var(--bg-primary)', color: 'var(--text-primary)' },
    '.cm-content': { caretColor: 'var(--accent)' },
    '.cm-cursor': { borderLeftColor: 'var(--accent)' },
    '.cm-activeLine': { backgroundColor: 'var(--bg-tertiary)' },
    '.cm-gutters': {
      backgroundColor: 'var(--bg-secondary)',
      color: 'var(--text-secondary)',
      borderRight: '1px solid var(--border)',
      paddingLeft: '8px',
    },
    '.cm-activeLineGutter': { backgroundColor: 'var(--bg-tertiary)' },
  },
  { dark: true },
)

const lightTheme = EditorView.theme(
  {
    '&': { height: '100%', backgroundColor: 'var(--bg-primary)', color: 'var(--text-primary)' },
    '.cm-content': { caretColor: 'var(--accent)' },
    '.cm-cursor': { borderLeftColor: 'var(--accent)' },
    '.cm-activeLine': { backgroundColor: 'var(--bg-secondary)' },
    '.cm-gutters': {
      backgroundColor: 'var(--bg-secondary)',
      color: 'var(--text-secondary)',
      borderRight: '1px solid var(--border)',
      paddingLeft: '8px',
    },
    '.cm-activeLineGutter': { backgroundColor: 'var(--bg-tertiary)' },
  },
  { dark: false },
)

export function themeExtension(t: ResolvedTheme): Extension {
  return t === 'dark' ? darkTheme : lightTheme
}
