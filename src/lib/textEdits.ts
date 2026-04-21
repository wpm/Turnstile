import type { Text } from "@codemirror/state";
import type { EditorView } from "@codemirror/view";

export interface TextEdit {
  start_line: number;
  start_character: number;
  end_line: number;
  end_character: number;
  new_text: string;
}

export function buildTextEditChanges(doc: Text, edits: TextEdit[]) {
  const sorted = [...edits].sort(
    (a, b) =>
      b.start_line - a.start_line || b.start_character - a.start_character,
  );
  return sorted.map((edit) => {
    const fromLine = doc.line(edit.start_line + 1);
    const toLine = doc.line(edit.end_line + 1);
    return {
      from: fromLine.from + edit.start_character,
      to: toLine.from + edit.end_character,
      insert: edit.new_text,
    };
  });
}

export function applyTextEdits(
  v: EditorView,
  edits: TextEdit[],
  onBeforeDispatch?: () => void,
  onAfterDispatch?: () => void,
) {
  const changes = buildTextEditChanges(v.state.doc, edits);
  onBeforeDispatch?.();
  try {
    v.dispatch({ changes });
  } finally {
    onAfterDispatch?.();
  }
}
