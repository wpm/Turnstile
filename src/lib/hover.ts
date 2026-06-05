import { hoverTooltip } from "@codemirror/view";
import type { EditorView, Tooltip } from "@codemirror/view";
import { invoke } from "@tauri-apps/api/core";
import { renderMathInMarkdown } from "./render";

interface HoverInfo {
  contents: string;
  kind: "markdown" | "plaintext";
}

// ── Tooltip DOM construction ───────────────────────────────────────────

function buildTooltipDom(info: HoverInfo): HTMLElement {
  const dom = document.createElement("div");
  dom.className = "cm-hover-tooltip-content";

  if (info.kind === "plaintext") {
    const pre = document.createElement("pre");
    pre.className = "cm-hover-plain";
    pre.textContent = info.contents;
    dom.appendChild(pre);
  } else {
    dom.innerHTML = renderMathInMarkdown(info.contents);
  }

  return dom;
}

// ── Tooltip builder ────────────────────────────────────────────────────

function offsetToLspPosition(
  doc: EditorView["state"]["doc"],
  offset: number,
): { line: number; character: number } {
  const lineObj = doc.lineAt(offset);
  return { line: lineObj.number - 1, character: offset - lineObj.from };
}

async function buildHoverTooltip(
  view: EditorView,
  pos: number,
): Promise<Tooltip | null> {
  const { line, character } = offsetToLspPosition(view.state.doc, pos);
  let info: HoverInfo | null;
  try {
    info = await invoke<HoverInfo | null>("lsp_hover", { line, character });
  } catch {
    return null;
  }
  if (!info) return null;

  return {
    pos,
    above: true,
    strictSide: false,
    arrow: true,
    create: () => ({ dom: buildTooltipDom(info) }),
  };
}

export const lspHoverTooltip = hoverTooltip(
  (view, pos) => buildHoverTooltip(view, pos),
  { hoverTime: 300 },
);
