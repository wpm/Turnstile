import {
  EditorState,
  StateField,
  StateEffect,
  RangeSet,
  type Text,
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
  hoverTooltip,
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
  constructor(private readonly cssClass: string) {
    super()
  }

  override toDOM(): HTMLElement {
    const el = document.createElement('div')
    el.className = this.cssClass
    el.textContent = '●'
    return el
  }
}

// ---------------------------------------------------------------------------
// Diagnostic underline decorations — StateField<DecorationSet>
// ---------------------------------------------------------------------------

const diagnosticUnderlineField = StateField.define<DecorationSet>({
  create() {
    return Decoration.none
  },
  update(decorations, tr) {
    decorations = decorations.map(tr.changes)
    for (const effect of tr.effects) {
      if (effect.is(setDiagnosticsEffect)) {
        const ranges: Range<Decoration>[] = []
        for (const diag of effect.value) {
          const cssClass =
            diag.severity === 1
              ? 'cm-diag-error'
              : diag.severity === 2
                ? 'cm-diag-warning'
                : 'cm-diag-info'

          // DiagnosticInfo uses 1-indexed lines; CM6 doc.line() is also 1-indexed.
          const startLineNum = diag.start_line
          const endLineNum = diag.end_line
          if (
            startLineNum < 1 ||
            startLineNum > tr.state.doc.lines ||
            endLineNum < 1 ||
            endLineNum > tr.state.doc.lines
          )
            continue

          const startLine = tr.state.doc.line(startLineNum)
          const endLine = tr.state.doc.line(endLineNum)
          const from = startLine.from + diag.start_col
          const to = endLine.from + diag.end_col

          if (from >= 0 && to <= tr.state.doc.length && from < to) {
            ranges.push(Decoration.mark({ class: cssClass }).range(from, to))
          }
        }
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
// Full diagnostic list — used by the hover tooltip to find any diag at a pos
// ---------------------------------------------------------------------------

const diagnosticListField = StateField.define<DiagnosticInfo[]>({
  create() {
    return []
  },
  update(list, tr) {
    for (const effect of tr.effects) {
      if (effect.is(setDiagnosticsEffect)) return effect.value
    }
    return list
  },
})

/** Returns the CM6 document offset range [from, to) for a DiagnosticInfo. */
function diagRange(diag: DiagnosticInfo, doc: Text): { from: number; to: number } | null {
  if (
    diag.start_line < 1 ||
    diag.start_line > doc.lines ||
    diag.end_line < 1 ||
    diag.end_line > doc.lines
  )
    return null
  const from = doc.line(diag.start_line).from + diag.start_col
  const to = doc.line(diag.end_line).from + diag.end_col
  if (from < 0 || to > doc.length || from >= to) return null
  return { from, to }
}

const diagnosticHoverTooltip = hoverTooltip((view, pos) => {
  const diags = view.state.field(diagnosticListField)
  const hit = diags.find((d) => {
    const r = diagRange(d, view.state.doc)
    return r !== null && pos >= r.from && pos <= r.to
  })
  if (!hit) return null

  const severityClass =
    hit.severity === 1
      ? 'lean-diag-popup-error'
      : hit.severity === 2
        ? 'lean-diag-popup-warning'
        : 'lean-diag-popup-info'

  return {
    pos,
    above: true,
    create() {
      const dom = document.createElement('div')
      dom.className = `lean-diag-popup ${severityClass}`
      dom.textContent = hit.message
      return { dom }
    },
  }
})

// ---------------------------------------------------------------------------
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

    return new DiagnosticMarker(cssClass)
  },
  initialSpacer: () => new DiagnosticMarker('lean-diag-info'),
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

  // Use the start of the current word as `from` so CM6 filters completions
  // against the typed prefix. Fall back to the cursor position for explicit
  // requests (Ctrl+Space) where there may be no word yet.
  const word = ctx.matchBefore(/\w+/)
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

// Shared color theme using CSS custom properties — the actual values are set
// by [data-theme="mocha"] / [data-theme="latte"] selectors in app.css.
// We still need the themeCompartment to toggle CM6's `dark` boolean for
// scrollbar appearance and highlight-style fallback.
const darkTheme = EditorView.theme(
  {
    '&': { height: '100%', backgroundColor: 'var(--bg-primary)', color: 'var(--text-primary)' },
    '.cm-content': { caretColor: 'var(--text-primary)' },
    '.cm-cursor': { borderLeftColor: 'var(--text-primary)' },
    '.cm-activeLine': { backgroundColor: 'var(--bg-tertiary)' },
    '.cm-gutters': {
      backgroundColor: 'var(--bg-secondary)',
      color: 'var(--text-secondary)',
      border: 'none',
      paddingLeft: '8px',
    },
    '.cm-activeLineGutter': { backgroundColor: 'var(--bg-tertiary)' },
  },
  { dark: true },
)

const lightTheme = EditorView.theme(
  {
    '&': { height: '100%', backgroundColor: 'var(--bg-primary)', color: 'var(--text-primary)' },
    '.cm-content': { caretColor: 'var(--text-primary)' },
    '.cm-cursor': { borderLeftColor: 'var(--text-primary)' },
    '.cm-activeLine': { backgroundColor: 'var(--bg-secondary)' },
    '.cm-gutters': {
      backgroundColor: 'var(--bg-secondary)',
      color: 'var(--text-secondary)',
      border: 'none',
      borderRight: '1px solid var(--border-primary)',
      paddingLeft: '8px',
    },
    '.cm-activeLineGutter': { backgroundColor: 'var(--bg-tertiary)' },
  },
  { dark: false },
)

function themeExtension(t: Theme): Extension {
  return t === 'mocha' ? darkTheme : lightTheme
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
): EditorHandle {
  const themeCompartment = new Compartment()
  const updateListener = EditorView.updateListener.of((update: ViewUpdate) => {
    if (update.docChanged) {
      onChange(update.state.doc.toString())
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
        diagnosticListField,
        diagnosticUnderlineField,
        diagnosticHoverTooltip,
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
