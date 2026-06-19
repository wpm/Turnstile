import {
  EditorState,
  RangeSet,
  RangeSetBuilder,
  StateEffect,
  StateField,
  type Range,
} from "@codemirror/state";
import { severityIcon } from "./severity";
import {
  Decoration,
  EditorView,
  GutterMarker,
  gutter,
  hoverTooltip,
  type DecorationSet,
  type Tooltip,
} from "@codemirror/view";

import type { DiagnosticSeverity } from "./DiagnosticSeverity";
import type { DerivedAnnotations } from "./DerivedAnnotations";

/**
 * An empty annotation snapshot. Used as the field default and to clear
 * annotations on session load before the LSP repopulates them.
 *
 * Frozen because it is a shared singleton handed out as both the store default
 * and the StateField `create()` value: freezing turns any accidental in-place
 * mutation (e.g. `gutter.push(...)`) into an immediate error instead of silently
 * corrupting the shared default.
 */
// The inner arrays are frozen (the actual corruption vector — `.push(...)` on a
// shared array). The generated DerivedAnnotations type declares mutable arrays,
// so we cast through `unknown`; the frozen arrays still throw on mutation at
// runtime, which is the protection we want.
export const EMPTY_ANNOTATIONS = Object.freeze({
  syntaxColoring: Object.freeze([]),
  underlines: Object.freeze([]),
  gutter: Object.freeze([]),
  hover: Object.freeze([]),
}) as unknown as DerivedAnnotations;

/**
 * Clamp a Rust-resolved `[from, to)` UTF-16 offset range to the live document.
 * Offsets are resolved in the backend against its authoritative source; if the
 * user has typed since, they may point just past the current document, so we
 * guard rather than trust them blindly. Returns null if the range is unusable.
 */
function clampRange(
  state: EditorState,
  from: number,
  to: number,
): { from: number; to: number } | null {
  const len = state.doc.length;
  if (from < 0 || from >= to || to > len) return null;
  return { from, to };
}

export function buildAnnotationDecorations(
  state: EditorState,
  derived: DerivedAnnotations,
) {
  const marks: Range<Decoration>[] = [];
  // Offsets arrive pre-resolved from Rust; the renderer just maps them to marks.
  for (const tok of derived.syntaxColoring) {
    const r = clampRange(state, tok.from, tok.to);
    if (!r) continue;
    marks.push(
      Decoration.mark({ class: `cm-tok-${tok.tokenType}` }).range(r.from, r.to),
    );
  }
  for (const d of derived.underlines) {
    const r = clampRange(state, d.from, d.to);
    if (!r) continue;
    marks.push(
      Decoration.mark({ class: `cm-diag-${d.severity}` }).range(r.from, r.to),
    );
  }
  marks.sort((a, b) => a.from - b.from);
  return Decoration.set(marks, true);
}

export const setAnnotations = StateEffect.define<DerivedAnnotations>();

export const annotationField = StateField.define<DecorationSet>({
  create: () => Decoration.none,
  update(deco, tr) {
    try {
      for (const effect of tr.effects) {
        if (effect.is(setAnnotations)) {
          return buildAnnotationDecorations(tr.state, effect.value);
        }
      }
      return tr.docChanged ? deco.map(tr.changes) : deco;
    } catch {
      return Decoration.none;
    }
  },
  provide: (f) => EditorView.decorations.from(f),
});

export const annotationsDataField = StateField.define<DerivedAnnotations>({
  create: () => EMPTY_ANNOTATIONS,
  update(data, tr) {
    for (const effect of tr.effects) {
      if (effect.is(setAnnotations)) return effect.value;
    }
    return data;
  },
});

// ── Diagnostic hover tooltip ───────────────────────────────────────────

export function buildDiagnosticTooltip(
  state: EditorState,
  pos: number,
): Tooltip | null {
  // Hover spans arrive pre-resolved from Rust; collect every diagnostic whose
  // span contains the cursor (offset resolution is the one thing TS still does,
  // here only as a containment test against the clamped span).
  const hits = state.field(annotationsDataField).hover.filter((h) => {
    const r = clampRange(state, h.from, h.to);
    return r !== null && pos >= r.from && pos <= r.to;
  });

  if (hits.length === 0) return null;

  return {
    pos,
    above: true,
    strictSide: false,
    arrow: true,
    create() {
      const dom = document.createElement("div");
      dom.className = "cm-diagnostic-tooltip";
      hits.forEach((d, i) => {
        if (i > 0) {
          const sep = document.createElement("hr");
          sep.className = "cm-diagnostic-tooltip-sep";
          dom.appendChild(sep);
        }
        const row = document.createElement("div");
        row.className = `cm-diagnostic-tooltip-item cm-diagnostic-tooltip-item--${d.severity}`;
        const icon = document.createElement("span");
        icon.className = "cm-diagnostic-tooltip-icon";
        icon.textContent = severityIcon(d.severity);
        const msg = document.createElement("span");
        msg.className = "cm-diagnostic-tooltip-msg";
        msg.textContent = d.message;
        row.appendChild(icon);
        row.appendChild(msg);
        dom.appendChild(row);
      });
      return { dom };
    },
  };
}

export const diagnosticTooltip = hoverTooltip(
  (view, pos) => buildDiagnosticTooltip(view.state, pos),
  { hoverTime: 300 },
);

// ── Diagnostic gutter markers ──────────────────────────────────────────

class DiagnosticGutterMarker extends GutterMarker {
  constructor(readonly severity: DiagnosticSeverity) {
    super();
  }
  toDOM() {
    const span = document.createElement("span");
    span.className = `cm-diag-gutter cm-diag-gutter--${this.severity}`;
    span.textContent = this.severity === "error" ? "✕" : "⚠";
    span.setAttribute("aria-label", this.severity);
    return span;
  }
  eq(other: DiagnosticGutterMarker) {
    return this.severity === other.severity;
  }
}

export function buildGutterMarkers(state: EditorState): RangeSet<GutterMarker> {
  // The gutter list arrives pre-deduped from Rust (one mark per line, most
  // severe wins, info/hint excluded) and sorted by line, so we just place each.
  const builder = new RangeSetBuilder<GutterMarker>();
  for (const mark of state.field(annotationsDataField).gutter) {
    if (mark.line < 1 || mark.line > state.doc.lines) continue;
    const lineFrom = state.doc.line(mark.line).from;
    builder.add(lineFrom, lineFrom, new DiagnosticGutterMarker(mark.severity));
  }
  return builder.finish();
}

const diagnosticGutterMarkersField = StateField.define<RangeSet<GutterMarker>>({
  create: () => RangeSet.empty,
  update(markers, tr) {
    try {
      for (const effect of tr.effects) {
        if (effect.is(setAnnotations)) {
          return buildGutterMarkers(tr.state);
        }
      }
      return tr.docChanged ? markers.map(tr.changes) : markers;
    } catch {
      return RangeSet.empty;
    }
  },
});

export const diagnosticGutter = [
  annotationsDataField,
  diagnosticGutterMarkersField,
  gutter({
    class: "cm-diag-gutter-col",
    markers: (view) => view.state.field(diagnosticGutterMarkersField),
    initialSpacer: () => new DiagnosticGutterMarker("warning"),
  }),
  diagnosticTooltip,
];
