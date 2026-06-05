import type { FileProgressRange } from "./FileProgressRange";

/**
 * Map fileProgress ranges (1-indexed, inclusive) to the list of document
 * lines that should carry the "elaborating" highlight.
 *
 * Lines outside the document are dropped — a progress notification can
 * arrive for a version of the document that is longer than the current one,
 * and decorating a non-existent line throws inside CodeMirror.
 */
export function progressHighlightLines(
  ranges: FileProgressRange[],
  docLines: number,
): number[] {
  const lines = new Set<number>();
  for (const r of ranges) {
    const from = Math.max(1, r.start_line);
    const to = Math.min(docLines, r.end_line);
    for (let line = from; line <= to; line++) {
      lines.add(line);
    }
  }
  return [...lines].sort((a, b) => a - b);
}
