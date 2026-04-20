import {
  EditorState,
  StateEffect,
  StateField,
  type Range,
} from "@codemirror/state";
import { Decoration, EditorView, type DecorationSet } from "@codemirror/view";

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
