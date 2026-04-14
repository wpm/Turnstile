import {
  EditorState,
  StateField,
  StateEffect,
  RangeSet,
  Compartment,
  Transaction,
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
  tooltips,
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
import { history, defaultKeymap, historyKeymap, indentWithTab } from '@codemirror/commands'
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
import {
  invoke,
  type CompletionItem,
  type DiagnosticInfo,
  type FileProgressRange,
  type SemanticToken,
} from './tauri'
import { cmLineToLsp, cmPosToLsp } from './positionConvert'
import { fileProgressExtension, setFileProgressEffect } from './fileProgress'
import {
  diagRange,
  diagnosticGutterClass,
  diagnosticPopupClass,
  diagnosticSeverityClass,
  semanticTokenRange,
} from './editorHelpers'
import type { ResolvedTheme } from './theme'
import { findAbbrevReplacement } from './leanAbbrev'
import { indentationMarkers } from '@replit/codemirror-indentation-markers'
import { hoverTypeExtension } from './hoverTooltip'
import { gotoDefinitionExtension } from './gotoDefinition'
import { codeActionsExtension } from './codeActions'

// ---------------------------------------------------------------------------
// Lean abbreviation expansion — updateListener
// ---------------------------------------------------------------------------
//
// After each user input transaction, checks if the text just before the
// cursor completes a Lean abbreviation (e.g. \alpha, \to<space>). The
// replacement is dispatched asynchronously (after the current update cycle)
// to avoid the "dispatch during update" error.

const leanAbbrevExtension = EditorView.updateListener.of((update: ViewUpdate) => {
  if (!update.docChanged) return

  for (const tr of update.transactions) {
    if (!tr.isUserEvent('input.type') && !tr.isUserEvent('input.type.compose')) continue

    const pos = tr.state.selection.main.head
    // Slice only 100 chars back — no abbreviation is longer than ~20 chars.
    const windowStart = Math.max(0, pos - 100)
    const win = tr.state.doc.sliceString(windowStart, pos)

    const match = findAbbrevReplacement(win, win.length)
    if (!match) continue

    const docFrom = windowStart + match.from
    const docTo = windowStart + match.to
    const newCursorDocPos =
      match.cursorOffset !== null
        ? docFrom + match.cursorOffset
        : docFrom + match.replacement.length

    const view = update.view
    // Defer dispatch to avoid "update already in progress" error.
    void Promise.resolve().then(() => {
      view.dispatch({
        changes: { from: docFrom, to: docTo, insert: match.replacement },
        selection: { anchor: newCursorDocPos },
        annotations: Transaction.userEvent.of('input.abbrev'),
      })
    })
    break
  }
})

// ---------------------------------------------------------------------------
// Effects — used to dispatch state changes from outside the editor
// ---------------------------------------------------------------------------

const setSemanticTokensEffect = StateEffect.define<SemanticToken[]>()
const setDiagnosticsEffect = StateEffect.define<DiagnosticInfo[]>()
const setGoalLineEffect = StateEffect.define<number[]>()

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
          const r = semanticTokenRange(token, tr.state.doc)
          if (!r) continue
          ranges.push(Decoration.mark({ class: r.cssClass }).range(r.from, r.to))
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
          const r = diagRange(diag, tr.state.doc)
          if (!r) continue
          ranges.push(
            Decoration.mark({ class: diagnosticSeverityClass(diag.severity) }).range(r.from, r.to),
          )
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

const diagnosticHoverTooltip = hoverTooltip((view, pos) => {
  const diags = view.state.field(diagnosticListField)
  const hit = diags.find((d) => {
    const r = diagRange(d, view.state.doc)
    return r !== null && pos >= r.from && pos <= r.to
  })
  if (!hit) return null

  return {
    pos,
    above: true,
    create() {
      const dom = document.createElement('div')
      dom.className = `lean-diag-popup ${diagnosticPopupClass(hit.severity)}`
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

    return new DiagnosticMarker(diagnosticGutterClass(diag.severity))
  },
  initialSpacer: () => new DiagnosticMarker('lean-diag-info'),
})

// ---------------------------------------------------------------------------
// Goal line decoration — underline on editor lines linked from goal panel
// ---------------------------------------------------------------------------

const goalLineDeco = Decoration.line({ class: 'cm-goal-line' })

const goalLineField = StateField.define<DecorationSet>({
  create() {
    return Decoration.none
  },
  update(decorations, tr) {
    for (const effect of tr.effects) {
      if (effect.is(setGoalLineEffect)) {
        const lines = effect.value
        if (lines.length === 0) return Decoration.none

        const lineDecos: Range<Decoration>[] = []
        const doc = tr.state.doc

        for (const lineNum of lines) {
          if (lineNum >= 1 && lineNum <= doc.lines) {
            lineDecos.push(goalLineDeco.range(doc.line(lineNum).from))
          }
        }

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
    return decorations.map(tr.changes)
  },
  provide(field) {
    return EditorView.decorations.from(field)
  },
})

// ---------------------------------------------------------------------------
// LSP completion source
// ---------------------------------------------------------------------------

/** Invoke the Rust `get_completions` command and map results to CM6 completions. */
async function lspCompletionSource(ctx: CompletionContext): Promise<CompletionResult | null> {
  // Only trigger on an explicit request or when the user has typed a word character.
  if (!ctx.explicit && !ctx.matchBefore(/\w+/)) return null

  const pos = ctx.pos
  const { line: lspLine, character: lspCol } = cmPosToLsp(ctx.state.doc, pos)

  let items: CompletionItem[]
  try {
    items = await invoke<CompletionItem[]>('get_completions', {
      line: lspLine,
      col: lspCol,
    })
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
    fontFamily: '"JetBrains Mono", ui-monospace, "SF Mono", "Cascadia Mono", monospace',
    fontSize: '14px',
    lineHeight: '1.5',
  },
})

// Shared color theme using CSS custom properties — the actual values are set
// by :root (dark) / .light selectors in app.css.
// We still need the themeCompartment to toggle CM6's `dark` boolean for
// scrollbar appearance and highlight-style fallback.
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

function themeExtension(t: ResolvedTheme): Extension {
  return t === 'dark' ? darkTheme : lightTheme
}

interface EditorHandle {
  applySemanticTokens(tokens: SemanticToken[]): void
  applyDiagnostics(diagnostics: DiagnosticInfo[]): void
  applyFileProgress(ranges: FileProgressRange[]): void
  setGoalLines(lines: number[]): void
  setContent(text: string): void
  setTheme(theme: ResolvedTheme): void
  setWordWrap(enabled: boolean): void
  /**
   * Move the cursor to a 0-indexed LSP position and scroll into view.
   * Used by the symbol-outline palette.
   */
  jumpTo(line: number, character: number): void
  destroy(): void
}

interface MountEditorOptions {
  onChange: (content: string) => void
  onCursorChange?: ((line: number, col: number) => void) | undefined
  /** Called when the user presses Option/Alt+Z or clicks the footer wrap indicator. */
  onToggleWrap?: (() => void) | undefined
  /** Called when go-to-definition resolves a target in another file. */
  onExternalDef?: ((uri: string) => void) | undefined
  /** Returns the URI of the currently-open document. */
  currentUri?: (() => string) | undefined
}

export function mountEditor(
  container: HTMLElement,
  initialTheme: ResolvedTheme,
  optionsOrOnChange: MountEditorOptions | ((content: string) => void),
  onCursorChange?: (line: number, col: number) => void,
): EditorHandle {
  // Back-compat: also accept the old (onChange, onCursorChange?) signature
  // so existing tests and callers keep working while we migrate.
  const options: MountEditorOptions =
    typeof optionsOrOnChange === 'function'
      ? { onChange: optionsOrOnChange, onCursorChange }
      : optionsOrOnChange

  const onChange = options.onChange
  const onCursorChangeCb = options.onCursorChange
  const onToggleWrap = options.onToggleWrap
  const onExternalDef = options.onExternalDef ?? ((_uri: string) => undefined)
  const currentUri = options.currentUri ?? (() => 'file:///proof.lean')
  const themeCompartment = new Compartment()
  const wrapCompartment = new Compartment()
  const updateListener = EditorView.updateListener.of((update: ViewUpdate) => {
    if (update.docChanged) {
      onChange(update.state.doc.toString())
    }
  })

  const cursorListener = onCursorChangeCb
    ? EditorView.updateListener.of((update: ViewUpdate) => {
        if (update.selectionSet || update.docChanged) {
          const head = update.state.selection.main.head
          const line = update.state.doc.lineAt(head)
          onCursorChangeCb(cmLineToLsp(line.number), head - line.from)
        }
      })
    : []

  // Word-wrap keymap (Alt+Z / Option+Z). The handler calls back into the
  // host (App.svelte) which owns the authoritative state and persistence.
  const wrapKeymap = onToggleWrap
    ? keymap.of([
        {
          key: 'Alt-z',
          run() {
            onToggleWrap()
            return true
          },
        },
      ])
    : []

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
          indentWithTab,
          ...closeBracketsKeymap,
          ...defaultKeymap,
          ...searchKeymap,
          ...historyKeymap,
          ...foldKeymap,
          ...completionKeymap,
          ...lintKeymap,
        ]),
        leanAbbrevExtension,
        semanticTokensField,
        diagnosticsField,
        diagnosticListField,
        diagnosticUnderlineField,
        diagnosticHoverTooltip,
        hoverTypeExtension(),
        tooltips({ parent: document.body }),
        diagnosticGutter,
        fileProgressExtension(),
        goalLineField,
        indentationMarkers(),
        gotoDefinitionExtension({ onExternalDef, currentUri }),
        codeActionsExtension({ currentUri }),
        wrapKeymap,
        wrapCompartment.of([]),
        updateListener,
        cursorListener,
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
    applyFileProgress(ranges) {
      view.dispatch({ effects: setFileProgressEffect.of(ranges) })
    },
    setGoalLines(lines: number[]) {
      view.dispatch({ effects: setGoalLineEffect.of(lines) })
    },
    setContent(text: string) {
      view.dispatch({
        changes: { from: 0, to: view.state.doc.length, insert: text },
      })
    },
    setTheme(t: ResolvedTheme) {
      view.dispatch({ effects: themeCompartment.reconfigure(themeExtension(t)) })
    },
    setWordWrap(enabled: boolean) {
      view.dispatch({
        effects: wrapCompartment.reconfigure(enabled ? EditorView.lineWrapping : []),
      })
    },
    jumpTo(line: number, character: number) {
      const doc = view.state.doc
      const targetLineNum = line + 1
      if (targetLineNum < 1 || targetLineNum > doc.lines) return
      const targetLine = doc.line(targetLineNum)
      const anchor = Math.min(targetLine.from + character, targetLine.to)
      view.dispatch({
        selection: { anchor },
        effects: EditorView.scrollIntoView(anchor, { y: 'center' }),
      })
      view.focus()
    },
    destroy() {
      view.destroy()
    },
  }
}
