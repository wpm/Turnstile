/**
 * Unicode abbreviation input — type a `\`-escape, get the rendered glyph.
 *
 * A CodeMirror 6 completion source backed by Lean's abbreviation table (vendored
 * `abbreviations.json` from vscode-lean4). When the user types `\` followed by
 * one or more letters, a dropdown opens beneath the cursor showing the *rendered
 * glyph* for each matching escape — accepted on Enter/Tab, dismissed on Esc,
 * exactly like a spelling correction. This is the discovery mechanism: the menu
 * is already open and narrowing at `\al`, so escapes don't have to be memorised.
 *
 * Two surface configurations are exported (see issue #98):
 *   - `unicodeAbbreviationsOnly` — `override`s all completion sources with just
 *     this one. For the Proof Assistant prose entry, which has no LSP smarts.
 *   - `unicodeAbbreviations` — registers the source via language data so it
 *     *composes* with other sources (the future Lean LSP completions through
 *     the Rust bridge). For the Formal Proof editor.
 */
import {
  autocompletion,
  type Completion,
  type CompletionContext,
  type CompletionResult,
  type CompletionSource,
} from "@codemirror/autocomplete";
import { EditorState, type Extension } from "@codemirror/state";
import type { EditorView } from "@codemirror/view";
import abbreviations from "./abbreviations.json";

// Some replacements embed a `$CURSOR` marker telling us where the caret should
// land after insertion — e.g. `\norm` → `‖$CURSOR‖`, so typing keeps going
// *between* the bars. We strip the marker for display and honour it on apply.
const CURSOR = "$CURSOR";

// escape name → replacement glyph (1000+ static entries). Computed once.
const entries: [string, string][] = Object.entries(
  abbreviations as Record<string, string>,
);

/**
 * Build the `apply` for a glyph carrying a `$CURSOR` marker: insert the glyph
 * with the marker removed, then place the caret where the marker was. Plain
 * glyphs use a string `apply` and never reach this path.
 */
function applyWithCursor(
  glyph: string,
): (
  view: EditorView,
  completion: Completion,
  from: number,
  to: number,
) => void {
  const at = glyph.indexOf(CURSOR);
  const text = glyph.slice(0, at) + glyph.slice(at + CURSOR.length);
  return (view, _completion, from, to) => {
    view.dispatch({
      changes: { from, to, insert: text },
      selection: { anchor: from + at },
      // Mark as a user input so the change participates in history normally.
      userEvent: "input.complete",
    });
  };
}

/**
 * Rank: shorter names score higher so common short symbols surface above longer
 * ones that share their prefix (`\to` above `\topological`), and an exact name
 * match is boosted above every prefix match (`\to` → → even as `\topo…` exists).
 * CodeMirror folds this `boost` into its own match score; the values stay inside
 * the documented −99…99 range (names are at most ~20 chars).
 */
function boostFor(name: string, query: string): number {
  if (name === query) return 99;
  return Math.max(-99, Math.min(98, 50 - name.length));
}

function toCompletion(name: string, glyph: string, query: string): Completion {
  const hasCursor = glyph.includes(CURSOR);
  return {
    // Matched against the typed text (which includes the leading backslash), so
    // the label must carry it too. The glyph is shown via `displayLabel`.
    label: "\\" + name,
    displayLabel: hasCursor ? glyph.replace(CURSOR, "") : glyph,
    detail: "\\" + name,
    type: "constant",
    boost: boostFor(name, query),
    apply: hasCursor ? applyWithCursor(glyph) : glyph,
  };
}

/**
 * Pure core of the completion source: every escape whose name is prefixed by
 * `query`, as ranked CodeMirror completions. Exposed for unit testing without a
 * DOM. `query` is the text after the backslash (letters only, per the trigger).
 */
export function completeAbbreviation(query: string): Completion[] {
  const options: Completion[] = [];
  for (const [name, glyph] of entries) {
    if (name.startsWith(query)) options.push(toCompletion(name, glyph, query));
  }
  return options;
}

/**
 * CodeMirror completion source. Fires on `\` + ≥1 letter; the dropdown lists the
 * rendered glyphs and accepting one replaces the `\escape` with the character.
 */
export const unicodeCompletions: CompletionSource = (
  context: CompletionContext,
): CompletionResult | null => {
  const token = context.matchBefore(/\\[a-zA-Z]+/);
  if (!token) return null;
  const query = token.text.slice(1); // drop the backslash
  const options = completeAbbreviation(query);
  if (options.length === 0) return null;
  // No `validFor`: re-running the source per keystroke keeps the exact-match
  // boost current as the query grows. The table is small, so this is cheap.
  return { from: token.from, options };
};

const baseConfig = {
  activateOnTyping: true, // eager: the menu is open and filtering at `\al`
  icons: false,
} as const;

/**
 * Formal Proof editor: register the glyph source *without* `override` so it
 * composes with other completion sources — notably the Lean LSP completions
 * that will arrive through the Rust bridge. Language data is CodeMirror's
 * mechanism for stacking sources; `override` would lock the LSP out.
 */
export const unicodeAbbreviations: Extension = [
  autocompletion(baseConfig),
  EditorState.languageData.of(() => [{ autocomplete: unicodeCompletions }]),
];

/**
 * Proof Assistant prose entry: the glyph source is the *only* completion source
 * (`override`), since there are no LSP smarts here. Native browser spell-check
 * is a separate layer enabled via content attributes at the call site.
 */
export const unicodeAbbreviationsOnly: Extension = autocompletion({
  ...baseConfig,
  override: [unicodeCompletions],
});
