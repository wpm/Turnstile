// @vitest-environment jsdom
import { describe, it, expect } from "vitest";
import { EditorState } from "@codemirror/state";
import { Decoration } from "@codemirror/view";
import {
  annotationField,
  annotationsDataField,
  setAnnotations,
  buildAnnotationDecorations,
  buildDiagnosticTooltip,
  type Annotation,
} from "./annotations";

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

// Apply annotations via the StateEffect.
function applyAnnotations(
  state: EditorState,
  annotations: Annotation[],
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
    const doc = "theorem foo : True := by trivial";
    const state = makeState(doc);
    const annotations: Annotation[] = [
      {
        kind: "token",
        line: 1,
        col: 0,
        length: 7,
        tokenType: "keyword",
        modifiers: [],
      },
    ];
    const after = applyAnnotations(state, annotations);
    const deco = after.field(annotationField);
    expect(deco).not.toBe(Decoration.none);
  });

  it("allows typing after token annotations are applied", () => {
    const doc = "theorem foo : True := by trivial";
    let state = makeState(doc);

    const annotations: Annotation[] = [
      {
        kind: "token",
        line: 1,
        col: 0,
        length: 7,
        tokenType: "keyword",
        modifiers: [],
      },
      {
        kind: "token",
        line: 1,
        col: 8,
        length: 3,
        tokenType: "function",
        modifiers: [],
      },
    ];
    state = applyAnnotations(state, annotations);

    // Type a character at the end of the document.
    state = typeChar(state, "x", doc.length);
    expect(state.doc.toString()).toBe(doc + "x");
  });

  it("allows typing after diagnostic annotations are applied", () => {
    const doc = "theorem foo : True := by trivial";
    let state = makeState(doc);

    const annotations: Annotation[] = [
      {
        kind: "diagnostic",
        startLine: 1,
        startCol: 0,
        endLine: 1,
        endCol: 7,
        severity: "error",
        message: "fake error",
      },
    ];
    state = applyAnnotations(state, annotations);
    state = typeChar(state, "y", 0);
    expect(state.doc.toString()).toBe("y" + doc);
  });

  it("allows typing after mixed annotations are applied", () => {
    const doc = "theorem foo : True := by trivial\n-- comment\n";
    let state = makeState(doc);

    const annotations: Annotation[] = [
      {
        kind: "token",
        line: 1,
        col: 0,
        length: 7,
        tokenType: "keyword",
        modifiers: [],
      },
      {
        kind: "token",
        line: 2,
        col: 3,
        length: 7,
        tokenType: "comment",
        modifiers: [],
      },
      {
        kind: "diagnostic",
        startLine: 1,
        startCol: 8,
        endLine: 1,
        endCol: 11,
        severity: "warning",
        message: "unused",
      },
    ];
    state = applyAnnotations(state, annotations);

    // Type in the middle of the document
    const insertAt = doc.indexOf("\n") + 1;
    state = typeChar(state, "z", insertAt);
    expect(state.doc.toString()).toBe(
      "theorem foo : True := by trivial\nz-- comment\n",
    );
  });

  it("allows typing after annotations are cleared", () => {
    const doc = "theorem foo : True := by trivial";
    let state = makeState(doc);
    state = applyAnnotations(state, [
      {
        kind: "token",
        line: 1,
        col: 0,
        length: 7,
        tokenType: "keyword",
        modifiers: [],
      },
    ]);
    state = applyAnnotations(state, []);
    state = typeChar(state, "!", doc.length);
    expect(state.doc.toString()).toBe(doc + "!");
  });

  it("maps decorations across a doc change without throwing", () => {
    const doc = "theorem foo : True := by trivial";
    let state = makeState(doc);
    state = applyAnnotations(state, [
      {
        kind: "token",
        line: 1,
        col: 8,
        length: 3,
        tokenType: "function",
        modifiers: [],
      },
    ]);
    // Insert text before the decoration.
    state = typeChar(state, "bar ", 0);
    expect(state.doc.toString()).toBe("bar " + doc);
    // The field should still be readable (no throw on deco.map).
    expect(() => state.field(annotationField)).not.toThrow();
  });

  it("skips out-of-bounds token annotations", () => {
    const doc = "hi";
    const state = makeState(doc);
    // line 99 doesn't exist
    const annotations: Annotation[] = [
      {
        kind: "token",
        line: 99,
        col: 0,
        length: 2,
        tokenType: "keyword",
        modifiers: [],
      },
    ];
    expect(() => applyAnnotations(state, annotations)).not.toThrow();
  });

  it("skips zero-length token annotations", () => {
    const doc = "hello";
    const state = makeState(doc);
    const annotations: Annotation[] = [
      {
        kind: "token",
        line: 1,
        col: 2,
        length: 0,
        tokenType: "variable",
        modifiers: [],
      },
    ];
    expect(() => applyAnnotations(state, annotations)).not.toThrow();
  });

  it("skips diagnostic annotations where from >= to", () => {
    const doc = "hello world";
    const state = makeState(doc);
    const annotations: Annotation[] = [
      {
        kind: "diagnostic",
        startLine: 1,
        startCol: 5,
        endLine: 1,
        endCol: 5,
        severity: "error",
        message: "zero-length",
      },
    ];
    expect(() => applyAnnotations(state, annotations)).not.toThrow();
  });

  it("buildAnnotationDecorations returns sorted, non-overlapping-safe set", () => {
    const doc = "keyword function type\n";
    const state = EditorState.create({ doc });
    const annotations: Annotation[] = [
      {
        kind: "token",
        line: 1,
        col: 8,
        length: 8,
        tokenType: "function",
        modifiers: [],
      },
      {
        kind: "token",
        line: 1,
        col: 0,
        length: 7,
        tokenType: "keyword",
        modifiers: [],
      },
    ];
    // Should not throw even though input is unsorted
    expect(() => buildAnnotationDecorations(state, annotations)).not.toThrow();
  });
});

// ── annotationsDataField ───────────────────────────────────────────────

function makeStateWithData(doc: string, annotations?: Annotation[]) {
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
    expect(state.field(annotationsDataField)).toEqual([]);
  });

  it("stores the raw annotation array after setAnnotations", () => {
    let state = makeStateWithData("theorem foo : True");
    const annotations: Annotation[] = [
      {
        kind: "token",
        line: 1,
        col: 0,
        length: 7,
        tokenType: "keyword",
        modifiers: [],
      },
    ];
    state = state.update({ effects: setAnnotations.of(annotations) }).state;
    expect(state.field(annotationsDataField)).toEqual(annotations);
  });

  it("replaces the array on each setAnnotations", () => {
    let state = makeStateWithData("theorem foo : True");
    const first: Annotation[] = [
      {
        kind: "token",
        line: 1,
        col: 0,
        length: 7,
        tokenType: "keyword",
        modifiers: [],
      },
    ];
    const second: Annotation[] = [
      {
        kind: "diagnostic",
        startLine: 1,
        startCol: 0,
        endLine: 1,
        endCol: 7,
        severity: "error",
        message: "oops",
      },
    ];
    state = state.update({ effects: setAnnotations.of(first) }).state;
    state = state.update({ effects: setAnnotations.of(second) }).state;
    expect(state.field(annotationsDataField)).toEqual(second);
  });

  it("preserves data across doc changes", () => {
    let state = makeStateWithData("hello");
    const annotations: Annotation[] = [
      {
        kind: "token",
        line: 1,
        col: 0,
        length: 5,
        tokenType: "variable",
        modifiers: [],
      },
    ];
    state = state.update({ effects: setAnnotations.of(annotations) }).state;
    state = state.update({ changes: { from: 5, insert: "!" } }).state;
    expect(state.field(annotationsDataField)).toEqual(annotations);
  });
});

// ── buildDiagnosticTooltip ─────────────────────────────────────────────

describe("buildDiagnosticTooltip", () => {
  it("returns null when no annotations are present", () => {
    const state = makeStateWithData("hello world", []);
    expect(buildDiagnosticTooltip(state, 3)).toBeNull();
  });

  it("returns null for a position outside all diagnostic ranges", () => {
    const state = makeStateWithData("hello world", [
      {
        kind: "diagnostic",
        startLine: 1,
        startCol: 0,
        endLine: 1,
        endCol: 5,
        severity: "error",
        message: "err",
      },
    ]);
    expect(buildDiagnosticTooltip(state, 6)).toBeNull();
  });

  it("returns a tooltip for a position inside a diagnostic range", () => {
    const state = makeStateWithData("hello world", [
      {
        kind: "diagnostic",
        startLine: 1,
        startCol: 0,
        endLine: 1,
        endCol: 5,
        severity: "error",
        message: "bad token",
      },
    ]);
    const tooltip = buildDiagnosticTooltip(state, 2);
    if (!tooltip) throw new Error("expected a tooltip");
    expect(tooltip.pos).toBe(2);
    expect(tooltip.above).toBe(true);
    expect(tooltip.arrow).toBe(true);
  });

  it("returns null for token annotations (not diagnostics)", () => {
    const state = makeStateWithData("hello world", [
      {
        kind: "token",
        line: 1,
        col: 0,
        length: 5,
        tokenType: "keyword",
        modifiers: [],
      },
    ]);
    expect(buildDiagnosticTooltip(state, 2)).toBeNull();
  });

  it("includes all overlapping diagnostics at the hovered position", () => {
    const state = makeStateWithData("hello", [
      {
        kind: "diagnostic",
        startLine: 1,
        startCol: 0,
        endLine: 1,
        endCol: 5,
        severity: "error",
        message: "first",
      },
      {
        kind: "diagnostic",
        startLine: 1,
        startCol: 2,
        endLine: 1,
        endCol: 5,
        severity: "warning",
        message: "second",
      },
    ]);
    // pos=3 is inside both
    const tooltip = buildDiagnosticTooltip(state, 3);
    if (!tooltip) throw new Error("expected a tooltip");
    const dom = tooltip.create(null as never).dom;
    expect(dom.textContent).toContain("first");
    expect(dom.textContent).toContain("second");
  });

  it("returns null for diagnostics on out-of-bounds lines", () => {
    const state = makeStateWithData("hi", [
      {
        kind: "diagnostic",
        startLine: 99,
        startCol: 0,
        endLine: 99,
        endCol: 2,
        severity: "error",
        message: "oof",
      },
    ]);
    expect(buildDiagnosticTooltip(state, 0)).toBeNull();
  });

  it("includes the diagnostic message in the tooltip DOM", () => {
    const state = makeStateWithData("oops", [
      {
        kind: "diagnostic",
        startLine: 1,
        startCol: 0,
        endLine: 1,
        endCol: 4,
        severity: "error",
        message: "type mismatch",
      },
    ]);
    const tooltip = buildDiagnosticTooltip(state, 1);
    if (!tooltip) throw new Error("expected a tooltip");
    const dom = tooltip.create(null as never).dom;
    expect(dom.textContent).toContain("type mismatch");
  });
});
