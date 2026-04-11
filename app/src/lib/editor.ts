import {
  EditorState,
  StateField,
  StateEffect,
  RangeSet,
  Compartment,
  type Extension,
  type Range,
} from '@codemirror/state'
import {
  EditorView,
  Decoration,
  type DecorationSet,
  gutter,
  GutterMarker,
  type ViewUpdate,
  lineNumbers,
  highlightActiveLineGutter,
  highlightSpecialChars,
  dropCursor,
  rectangularSelection,
  crosshairCursor,
  highlightActiveLine,
  keymap,
} from '@codemirror/view'
import { history, defaultKeymap, historyKeymap } from '@codemirror/commands'
import {
  foldGutter,
  foldKeymap,
  indentOnInput,
  syntaxHighlighting,
  defaultHighlightStyle,
  bracketMatching,
} from '@codemirror/language'
import {
  closeBrackets,
  closeBracketsKeymap,
  autocompletion,
  completionKeymap,
  type CompletionContext,
  type CompletionResult,
} from '@codemirror/autocomplete'
import { searchKeymap, highlightSelectionMatches } from '@codemirror/search'
import { lintKeymap } from '@codemirror/lint'
import type { CompletionItem, DiagnosticInfo, SemanticToken } from './tauri'
import { tokenTypeToCssClass } from './tokenTypes'
import type { Theme } from './theme'

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
        const ranges: Range<Decoration>[] = []
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

  override toDOM(): HTMLElement {
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
      diag.severity === 1
        ? 'lean-diag-error'
        : diag.severity === 2
          ? 'lean-diag-warning'
          : 'lean-diag-info'

    return new DiagnosticMarker(cssClass, diag.message)
  },
  initialSpacer: () => new DiagnosticMarker('lean-diag-info', ''),
})

// ---------------------------------------------------------------------------
// LSP completion source
// ---------------------------------------------------------------------------

/** Invoke the Rust `get_completions` command and map results to CM6 completions. */
async function lspCompletionSource(ctx: CompletionContext): Promise<CompletionResult | null> {
  // Only trigger on an explicit request or when the user has typed a word character.
  if (!ctx.explicit && !ctx.matchBefore(/\w+/)) return null

  const pos = ctx.pos
  const line = ctx.state.doc.lineAt(pos)
  // LSP expects 0-indexed line and character
  const lspLine = line.number - 1
  const lspCol = pos - line.from

  let items: CompletionItem[]
  try {
    items = (await window.__TAURI__.core.invoke('get_completions', {
      line: lspLine,
      col: lspCol,
    })) as CompletionItem[]
  } catch {
    return null
  }

  if (items.length === 0) return null

  const word = ctx.matchBefore(/\w*/)
  return {
    from: word ? word.from : pos,
    options: items.map((item) => ({
      label: item.label,
      ...(item.detail != null && { detail: item.detail }),
      apply: item.insert_text ?? item.label,
    })),
  }
}

// ---------------------------------------------------------------------------
// Public interface
// ---------------------------------------------------------------------------

// Typography shared by both themes — not color-dependent.
const baseTheme = EditorView.baseTheme({
  '.cm-scroller': {
    overflow: 'auto',
    fontFamily: '"Courier New", Courier, monospace',
    fontSize: '14px',
    lineHeight: '1.5',
  },
})

const draculaTheme = EditorView.theme(
  {
    '&': { height: '100%', backgroundColor: '#282a36', color: '#f8f8f2' },
    '.cm-content': { caretColor: '#f8f8f2' },
    '.cm-cursor': { borderLeftColor: '#f8f8f2' },
    '.cm-activeLine': { backgroundColor: '#44475a' },
    '.cm-gutters': { backgroundColor: '#21222c', color: '#6272a4', border: 'none' },
    '.cm-activeLineGutter': { backgroundColor: '#44475a' },
  },
  { dark: true },
)

const lightTheme = EditorView.theme(
  {
    '&': { height: '100%', backgroundColor: '#ffffff', color: '#24292f' },
    '.cm-content': { caretColor: '#24292f' },
    '.cm-cursor': { borderLeftColor: '#24292f' },
    '.cm-activeLine': { backgroundColor: '#f6f8fa' },
    '.cm-gutters': {
      backgroundColor: '#f6f8fa',
      color: '#6e7781',
      border: 'none',
      borderRight: '1px solid #d0d7de',
    },
    '.cm-activeLineGutter': { backgroundColor: '#eaeef2' },
  },
  { dark: false },
)

function themeExtension(t: Theme): Extension {
  return t === 'dracula' ? draculaTheme : lightTheme
}

interface EditorHandle {
  applySemanticTokens(tokens: SemanticToken[]): void
  applyDiagnostics(diagnostics: DiagnosticInfo[]): void
  setTheme(theme: Theme): void
  destroy(): void
}

export function mountEditor(
  container: HTMLElement,
  initialTheme: Theme,
  onChange: (content: string) => void,
  onCursorMove: (line: number, col: number) => void,
): EditorHandle {
  const themeCompartment = new Compartment()
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
        // basicSetup minus drawSelection — WKWebView doesn't render CM6's
        // canvas-based selection layer, so we use native ::selection instead.
        lineNumbers(),
        highlightActiveLineGutter(),
        highlightSpecialChars(),
        history(),
        foldGutter(),
        dropCursor(),
        EditorState.allowMultipleSelections.of(true),
        indentOnInput(),
        syntaxHighlighting(defaultHighlightStyle, { fallback: true }),
        bracketMatching(),
        closeBrackets(),
        autocompletion({ override: [lspCompletionSource] }),
        rectangularSelection(),
        crosshairCursor(),
        highlightActiveLine(),
        highlightSelectionMatches(),
        keymap.of([
          ...closeBracketsKeymap,
          ...defaultKeymap,
          ...searchKeymap,
          ...historyKeymap,
          ...foldKeymap,
          ...completionKeymap,
          ...lintKeymap,
        ]),
        semanticTokensField,
        diagnosticsField,
        diagnosticGutter,
        updateListener,
        baseTheme,
        themeCompartment.of(themeExtension(initialTheme)),
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
    setTheme(t: Theme) {
      view.dispatch({ effects: themeCompartment.reconfigure(themeExtension(t)) })
    },
    destroy() {
      view.destroy()
    },
  }
}
