// @vitest-environment jsdom
import { describe, it, expect } from "vitest";
import { EditorState } from "@codemirror/state";
import { Decoration, EditorView } from "@codemirror/view";
import {
  annotationField,
  annotationsDataField,
  setAnnotations,
  buildAnnotationDecorations,
  buildDiagnosticTooltip,
  buildGutterMarkers,
  diagnosticGutter,
  EMPTY_ANNOTATIONS,
} from "./annotations";
import type { DerivedAnnotations } from "./DerivedAnnotations";
import type { SyntaxColor } from "./SyntaxColor";
import type { Underline } from "./Underline";
import type { GutterMark } from "./GutterMark";
import type { HoverHit } from "./HoverHit";

// Build a DerivedAnnotations snapshot from optional parts. Offsets are absolute
// UTF-16 code-unit positions, exactly as the Rust backend now resolves them.
function derived(parts: Partial<DerivedAnnotations>): DerivedAnnotations {
  return { ...EMPTY_ANNOTATIONS, ...parts };
}

function tok(from: number, to: number, tokenType: SyntaxColor["tokenType"]) {
  return { from, to, tokenType };
}
function diag(
  from: number,
  to: number,
  severity: Underline["severity"],
): Underline {
  return { from, to, severity };
}
function gutterMark(
  line: number,
  severity: GutterMark["severity"],
): GutterMark {
  return { line, severity };
}
function hoverHit(
  from: number,
  to: number,
  severity: HoverHit["severity"],
  message: string,
): HoverHit {
  return { from, to, severity, message };
}

function makeState(doc: string) {
  return EditorState.create({
    doc,
    extensions: [annotationField],
  });
}

// Simulate a typing insertion: insert `char` at position `pos`.
function typeChar(state: EditorState, char: string, pos: number): EditorState {
  return state.update({
    changes: { from: pos, insert: char },
    selection: { anchor: pos + char.length },
  }).state;
}

function applyAnnotations(
  state: EditorState,
  annotations: DerivedAnnotations,
): EditorState {
  return state.update({ effects: setAnnotations.of(annotations) }).state;
}

describe("annotationField", () => {
  it("starts with no decorations", () => {
    const state = makeState("theorem foo : True := by trivial");
    const deco = state.field(annotationField);
    expect(deco).toBe(Decoration.none);
  });

  it("applies token annotations without throwing", () => {
    const state = makeState("theorem foo : True := by trivial");
    const after = applyAnnotations(
      state,
      derived({ syntaxColoring: [tok(0, 7, "keyword")] }),
    );
    expect(after.field(annotationField)).not.toBe(Decoration.none);
  });

  it("allows typing after token annotations are applied", () => {
    const doc = "theorem foo : True := by trivial";
    let state = makeState(doc);
    state = applyAnnotations(
      state,
      derived({
        syntaxColoring: [tok(0, 7, "keyword"), tok(8, 11, "function")],
      }),
    );
    state = typeChar(state, "x", doc.length);
    expect(state.doc.toString()).toBe(doc + "x");
  });

  it("allows typing after diagnostic annotations are applied", () => {
    const doc = "theorem foo : True := by trivial";
    let state = makeState(doc);
    state = applyAnnotations(
      state,
      derived({ underlines: [diag(0, 7, "error")] }),
    );
    state = typeChar(state, "y", 0);
    expect(state.doc.toString()).toBe("y" + doc);
  });

  it("allows typing after annotations are cleared", () => {
    const doc = "theorem foo : True := by trivial";
    let state = makeState(doc);
    state = applyAnnotations(
      state,
      derived({ syntaxColoring: [tok(0, 7, "keyword")] }),
    );
    state = applyAnnotations(state, EMPTY_ANNOTATIONS);
    state = typeChar(state, "!", doc.length);
    expect(state.doc.toString()).toBe(doc + "!");
  });

  it("maps decorations across a doc change without throwing", () => {
    const doc = "theorem foo : True := by trivial";
    let state = makeState(doc);
    state = applyAnnotations(
      state,
      derived({ syntaxColoring: [tok(8, 11, "function")] }),
    );
    state = typeChar(state, "bar ", 0);
    expect(state.doc.toString()).toBe("bar " + doc);
    expect(() => state.field(annotationField)).not.toThrow();
  });

  it("skips out-of-bounds annotations (offsets past the live document)", () => {
    // A token resolved against a longer source than the editor currently holds
    // (the user deleted text since): the clamp must drop it, not throw.
    const state = makeState("hi");
    expect(() =>
      applyAnnotations(
        state,
        derived({ syntaxColoring: [tok(50, 52, "keyword")] }),
      ),
    ).not.toThrow();
    const after = applyAnnotations(
      state,
      derived({ syntaxColoring: [tok(50, 52, "keyword")] }),
    );
    // Nothing rendered for the out-of-bounds span.
    expect(after.field(annotationField)).toBe(Decoration.none);
  });

  it("skips zero-length and inverted spans", () => {
    const state = makeState("hello world");
    expect(() =>
      applyAnnotations(
        state,
        derived({ underlines: [diag(5, 5, "error"), diag(7, 6, "error")] }),
      ),
    ).not.toThrow();
  });

  it("buildAnnotationDecorations returns a sorted set from unsorted input", () => {
    const state = EditorState.create({ doc: "keyword function type\n" });
    expect(() =>
      buildAnnotationDecorations(
        state,
        derived({
          syntaxColoring: [tok(8, 16, "function"), tok(0, 7, "keyword")],
        }),
      ),
    ).not.toThrow();
  });
});

// ── annotationsDataField ───────────────────────────────────────────────

function makeStateWithData(doc: string, annotations?: DerivedAnnotations) {
  let state = EditorState.create({
    doc,
    extensions: [annotationField, annotationsDataField],
  });
  if (annotations) {
    state = state.update({ effects: setAnnotations.of(annotations) }).state;
  }
  return state;
}

describe("annotationsDataField", () => {
  it("starts empty", () => {
    const state = makeStateWithData("theorem foo : True");
    expect(state.field(annotationsDataField)).toEqual(EMPTY_ANNOTATIONS);
  });

  it("stores the derived snapshot after setAnnotations", () => {
    let state = makeStateWithData("theorem foo : True");
    const anns = derived({ syntaxColoring: [tok(0, 7, "keyword")] });
    state = state.update({ effects: setAnnotations.of(anns) }).state;
    expect(state.field(annotationsDataField)).toEqual(anns);
  });

  it("replaces the snapshot on each setAnnotations", () => {
    let state = makeStateWithData("theorem foo : True");
    const first = derived({ syntaxColoring: [tok(0, 7, "keyword")] });
    const second = derived({ underlines: [diag(0, 7, "error")] });
    state = state.update({ effects: setAnnotations.of(first) }).state;
    state = state.update({ effects: setAnnotations.of(second) }).state;
    expect(state.field(annotationsDataField)).toEqual(second);
  });

  it("preserves data across doc changes", () => {
    let state = makeStateWithData("hello");
    const anns = derived({ syntaxColoring: [tok(0, 5, "variable")] });
    state = state.update({ effects: setAnnotations.of(anns) }).state;
    state = state.update({ changes: { from: 5, insert: "!" } }).state;
    expect(state.field(annotationsDataField)).toEqual(anns);
  });
});

// ── buildDiagnosticTooltip ─────────────────────────────────────────────

describe("buildDiagnosticTooltip", () => {
  it("returns null when there are no hover hits", () => {
    const state = makeStateWithData("hello world", EMPTY_ANNOTATIONS);
    expect(buildDiagnosticTooltip(state, 3)).toBeNull();
  });

  it("returns null for a position outside all hover spans", () => {
    const state = makeStateWithData(
      "hello world",
      derived({ hover: [hoverHit(0, 5, "error", "err")] }),
    );
    expect(buildDiagnosticTooltip(state, 6)).toBeNull();
  });

  it("returns a tooltip for a position inside a hover span", () => {
    const state = makeStateWithData(
      "hello world",
      derived({ hover: [hoverHit(0, 5, "error", "bad token")] }),
    );
    const tooltip = buildDiagnosticTooltip(state, 2);
    if (!tooltip) throw new Error("expected a tooltip");
    expect(tooltip.pos).toBe(2);
    expect(tooltip.above).toBe(true);
    expect(tooltip.arrow).toBe(true);
  });

  it("includes all overlapping hover hits at the cursor", () => {
    const state = makeStateWithData(
      "hello",
      derived({
        hover: [
          hoverHit(0, 5, "error", "first"),
          hoverHit(2, 5, "warning", "second"),
        ],
      }),
    );
    const tooltip = buildDiagnosticTooltip(state, 3); // inside both
    if (!tooltip) throw new Error("expected a tooltip");
    const dom = tooltip.create(null as never).dom;
    expect(dom.textContent).toContain("first");
    expect(dom.textContent).toContain("second");
  });

  it("returns null for hover spans past the live document", () => {
    const state = makeStateWithData(
      "hi",
      derived({ hover: [hoverHit(50, 52, "error", "oof")] }),
    );
    expect(buildDiagnosticTooltip(state, 0)).toBeNull();
  });

  it("includes the diagnostic message in the tooltip DOM", () => {
    const state = makeStateWithData(
      "oops",
      derived({ hover: [hoverHit(0, 4, "error", "type mismatch")] }),
    );
    const tooltip = buildDiagnosticTooltip(state, 1);
    if (!tooltip) throw new Error("expected a tooltip");
    const dom = tooltip.create(null as never).dom;
    expect(dom.textContent).toContain("type mismatch");
  });
});

// ── Diagnostic gutter ──────────────────────────────────────────────────

describe("buildGutterMarkers", () => {
  // The 1-indexed document line of every gutter marker the builder produces.
  function markerLines(state: EditorState): number[] {
    const set = buildGutterMarkers(state);
    const lines: number[] = [];
    const cursor = set.iter();
    while (cursor.value) {
      lines.push(state.doc.lineAt(cursor.from).number);
      cursor.next();
    }
    return lines;
  }

  it("produces no markers when the gutter list is empty", () => {
    expect(
      markerLines(makeStateWithData("theorem foo", EMPTY_ANNOTATIONS)),
    ).toEqual([]);
  });

  it("places one marker per gutter entry (Rust has already deduped)", () => {
    const state = makeStateWithData(
      "aaaa\nbbbb",
      derived({ gutter: [gutterMark(1, "error"), gutterMark(2, "warning")] }),
    );
    expect(markerLines(state)).toEqual([1, 2]);
  });

  it("skips gutter marks on out-of-bounds lines", () => {
    const state = makeStateWithData(
      "only one line",
      derived({ gutter: [gutterMark(99, "error")] }),
    );
    expect(markerLines(state)).toEqual([]);
  });
});

describe("diagnosticGutter extension", () => {
  function mountedView(doc: string) {
    const parent = document.createElement("div");
    document.body.appendChild(parent);
    const view = new EditorView({
      state: EditorState.create({
        doc,
        extensions: [annotationsDataField, diagnosticGutter],
      }),
      parent,
    });
    return { view, parent };
  }

  it("renders error/warning gutter glyphs into the DOM", () => {
    const { view, parent } = mountedView("err line\nwarn line");
    try {
      view.dispatch({
        effects: setAnnotations.of(
          derived({
            gutter: [gutterMark(1, "error"), gutterMark(2, "warning")],
          }),
        ),
      });
      const glyphs = [...parent.querySelectorAll(".cm-diag-gutter")].map(
        (el) => el.textContent,
      );
      expect(glyphs).toContain("✕");
      expect(glyphs).toContain("⚠");
    } finally {
      view.destroy();
      parent.remove();
    }
  });

  it("mounts and survives annotation + plain-edit dispatches", () => {
    const { view, parent } = mountedView("theorem foo\nbar");
    try {
      view.dispatch({
        effects: setAnnotations.of(
          derived({ gutter: [gutterMark(1, "error")] }),
        ),
      });
      // A pure document edit exercises the markers-remap branch of update().
      view.dispatch({ changes: { from: 0, insert: "x" } });
      expect(view.state.field(annotationsDataField).gutter.length).toBe(1);
    } finally {
      view.destroy();
      parent.remove();
    }
  });
});
