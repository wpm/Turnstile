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

export type TokenType =
  | "namespace"
  | "type"
  | "class"
  | "enum"
  | "interface"
  | "struct"
  | "typeParameter"
  | "parameter"
  | "variable"
  | "property"
  | "enumMember"
  | "event"
  | "function"
  | "method"
  | "macro"
  | "keyword"
  | "modifier"
  | "comment"
  | "string"
  | "number"
  | "regexp"
  | "operator"
  | "decorator"
  | "unknown";

export type DiagnosticSeverity = "error" | "warning" | "info" | "hint";

export type Annotation =
  | {
      kind: "token";
      line: number;
      col: number;
      length: number;
      tokenType: TokenType;
      modifiers: string[];
    }
  | {
      kind: "diagnostic";
      startLine: number;
      startCol: number;
      endLine: number;
      endCol: number;
      severity: DiagnosticSeverity;
      message: string;
    };

export function buildAnnotationDecorations(
  state: EditorState,
  annotations: Annotation[],
) {
  const marks: Range<Decoration>[] = [];
  for (const ann of annotations) {
    if (ann.kind === "token") {
      if (ann.line < 1 || ann.line > state.doc.lines) continue;
      const lineObj = state.doc.line(ann.line);
      const from = lineObj.from + ann.col;
      const to = from + ann.length;
      if (from >= to || to > lineObj.to) continue;
      marks.push(
        Decoration.mark({ class: `cm-tok-${ann.tokenType}` }).range(from, to),
      );
    } else {
      if (ann.startLine < 1 || ann.startLine > state.doc.lines) continue;
      if (ann.endLine < 1 || ann.endLine > state.doc.lines) continue;
      const from = state.doc.line(ann.startLine).from + ann.startCol;
      const to = state.doc.line(ann.endLine).from + ann.endCol;
      if (from >= to) continue;
      marks.push(
        Decoration.mark({ class: `cm-diag-${ann.severity}` }).range(from, to),
      );
    }
  }
  marks.sort((a, b) => a.from - b.from);
  return Decoration.set(marks, true);
}

export const setAnnotations = StateEffect.define<Annotation[]>();

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

export const annotationsDataField = StateField.define<Annotation[]>({
  create: () => [],
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
  const annotations = state.field(annotationsDataField);
  const hits = annotations.filter(
    (a): a is Extract<Annotation, { kind: "diagnostic" }> => {
      if (a.kind !== "diagnostic") return false;
      if (a.startLine < 1 || a.startLine > state.doc.lines) return false;
      if (a.endLine < 1 || a.endLine > state.doc.lines) return false;
      const from = state.doc.line(a.startLine).from + a.startCol;
      const to = state.doc.line(a.endLine).from + a.endCol;
      return pos >= from && pos <= to;
    },
  );

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

const GUTTER_SEVERITIES = new Set<DiagnosticSeverity>(["error", "warning"]);

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
  const annotations = state.field(annotationsDataField);
  const lineMap = new Map<number, DiagnosticSeverity>();
  for (const a of annotations) {
    if (a.kind !== "diagnostic") continue;
    if (!GUTTER_SEVERITIES.has(a.severity)) continue;
    const existing = lineMap.get(a.startLine);
    if (!existing || (existing !== "error" && a.severity === "error")) {
      lineMap.set(a.startLine, a.severity);
    }
  }

  const sorted = [...lineMap.entries()].sort((a, b) => a[0] - b[0]);
  const builder = new RangeSetBuilder<GutterMarker>();
  for (const [lineNum, severity] of sorted) {
    if (lineNum < 1 || lineNum > state.doc.lines) continue;
    const lineFrom = state.doc.line(lineNum).from;
    builder.add(lineFrom, lineFrom, new DiagnosticGutterMarker(severity));
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
