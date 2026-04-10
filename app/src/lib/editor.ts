import {
  EditorState,
  StateField,
  StateEffect,
  RangeSet,
  Transaction,
} from '@codemirror/state'
import {
  EditorView,
  Decoration,
  type DecorationSet,
  gutter,
  GutterMarker,
  type ViewUpdate,
} from '@codemirror/view'
import { basicSetup } from 'codemirror'
import type { DiagnosticInfo, SemanticToken } from './tauri'
import { tokenTypeToCssClass } from './tokenTypes'

// ---------------------------------------------------------------------------
// Effects — used to dispatch state changes from outside the editor
// ---------------------------------------------------------------------------

const setSemanticTokensEffect = StateEffect.define<SemanticToken[]>()
const setDiagnosticsEffect = StateEffect.define<DiagnosticInfo[]>()

// ---------------------------------------------------------------------------
// Semantic token highlighting — StateField<DecorationSet>
// ---------------------------------------------------------------------------

const semanticTokensField = StateField.define<DecorationSet>({
  create() {
    return Decoration.none
  },
  update(decorations, tr) {
    decorations = decorations.map(tr.changes)
    for (const effect of tr.effects) {
      if (effect.is(setSemanticTokensEffect)) {
        const ranges: ReturnType<typeof Decoration.mark>[] = []
        for (const token of effect.value) {
          const cssClass = tokenTypeToCssClass(token.token_type)
          if (!cssClass) continue

          // Tokens are 1-indexed from backend; CM6 lines are also 1-indexed
          // via doc.line(n) but positions are character offsets from 0.
          const lineNum = token.line // 1-indexed
          if (lineNum < 1 || lineNum > tr.state.doc.lines) continue

          const line = tr.state.doc.line(lineNum)
          const from = line.from + token.col
          const to = line.from + token.col + token.length

          if (from >= 0 && to <= tr.state.doc.length && from <= to) {
            ranges.push(Decoration.mark({ class: cssClass }).range(from, to))
          }
        }
        // RangeSet requires ranges sorted by `from`
        ranges.sort((a, b) => a.from - b.from)
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
// Diagnostic gutter markers
// ---------------------------------------------------------------------------

class DiagnosticMarker extends GutterMarker {
  constructor(
    private readonly cssClass: string,
    private readonly msg: string,
  ) {
    super()
  }

  toDOM() {
    const el = document.createElement('div')
    el.className = this.cssClass
    el.title = this.msg
    el.textContent = '●'
    return el
  }
}

// Map from 1-indexed line number to DiagnosticInfo
const diagnosticsField = StateField.define<Map<number, DiagnosticInfo>>({
  create() {
    return new Map()
  },
  update(map, tr) {
    for (const effect of tr.effects) {
      if (effect.is(setDiagnosticsEffect)) {
        const next = new Map<number, DiagnosticInfo>()
        for (const diag of effect.value) {
          next.set(diag.start_line, diag)
        }
        return next
      }
    }
    return map
  },
})

const diagnosticGutter = gutter({
  class: 'lean-diag-gutter',
  lineMarker(view, line) {
    const lineNum = view.state.doc.lineAt(line.from).number // 1-indexed
    const diag = view.state.field(diagnosticsField).get(lineNum)
    if (!diag) return null

    const cssClass =
      diag.severity === 1 ? 'lean-diag-error'
      : diag.severity === 2 ? 'lean-diag-warning'
      : 'lean-diag-info'

    return new DiagnosticMarker(cssClass, diag.message)
  },
  initialSpacer: () => new DiagnosticMarker('lean-diag-info', ''),
})

// ---------------------------------------------------------------------------
// Public interface
// ---------------------------------------------------------------------------

export interface EditorHandle {
  applySemanticTokens(tokens: SemanticToken[]): void
  applyDiagnostics(diagnostics: DiagnosticInfo[]): void
  destroy(): void
}

export function mountEditor(
  container: HTMLElement,
  onChange: (content: string) => void,
  onCursorMove: (line: number, col: number) => void,
): EditorHandle {
  const updateListener = EditorView.updateListener.of((update: ViewUpdate) => {
    if (update.docChanged) {
      onChange(update.state.doc.toString())
    }
    if (update.selectionSet) {
      const pos = update.state.selection.main.head
      const line = update.state.doc.lineAt(pos)
      // get_goal_state expects 0-indexed line/col (LSP convention)
      onCursorMove(line.number - 1, pos - line.from)
    }
  })

  const view = new EditorView({
    state: EditorState.create({
      doc: '',
      extensions: [
        basicSetup,
        semanticTokensField,
        diagnosticsField,
        diagnosticGutter,
        updateListener,
        EditorView.theme({
          '&': { height: '100%', backgroundColor: '#1a1b2e', color: '#c0caf5' },
          '.cm-scroller': { overflow: 'auto', fontFamily: '"Courier New", Courier, monospace', fontSize: '14px', lineHeight: '1.5' },
          '.cm-content': { caretColor: '#c0caf5' },
          '.cm-cursor': { borderLeftColor: '#c0caf5' },
          '.cm-activeLine': { backgroundColor: '#24283b' },
          '.cm-gutters': { backgroundColor: '#16161e', color: '#565f89', border: 'none' },
          '.cm-activeLineGutter': { backgroundColor: '#24283b' },
          '.cm-selectionBackground, ::selection': { backgroundColor: '#2a2b3d' },
        }, { dark: true }),
      ],
    }),
    parent: container,
  })

  return {
    applySemanticTokens(tokens) {
      view.dispatch({ effects: setSemanticTokensEffect.of(tokens) })
    },
    applyDiagnostics(diagnostics) {
      view.dispatch({ effects: setDiagnosticsEffect.of(diagnostics) })
    },
    destroy() {
      view.destroy()
    },
  }
}
