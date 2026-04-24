import { hoverTooltip } from "@codemirror/view";
import type { EditorView, Tooltip } from "@codemirror/view";
import { invoke } from "@tauri-apps/api/core";
import { marked } from "marked";
import katex from "katex";

interface HoverInfo {
  contents: string;
  kind: "markdown" | "plaintext";
}

// ── Math rendering ─────────────────────────────────────────────────────

// Placeholder tokens injected before marked runs so math delimiters survive
// Markdown parsing. Unicode angle brackets never appear in Lean hover output.
const MATH_PLACEHOLDER_RE = /‹MATH:(\d+)›/g;

function renderMathInMarkdown(markdown: string): string {
  const slots: string[] = [];

  function slot(html: string): string {
    const id = slots.length;
    slots.push(html);
    return "‹MATH:" + String(id) + "›";
  }

  // Replace $$...$$ display math first (must precede inline $...$)
  let out = markdown.replace(/\$\$([\s\S]+?)\$\$/g, (_m, tex: string) => {
    try {
      return slot(
        katex.renderToString(tex.trim(), {
          displayMode: true,
          throwOnError: false,
        }),
      );
    } catch {
      return slot(`<span class="cm-hover-math-error">$$${tex}$$</span>`);
    }
  });

  // Replace $...$ inline math (not escaped, single-line)
  out = out.replace(/(?<!\\)\$([^$\n]+?)\$/g, (_m, tex: string) => {
    try {
      return slot(
        katex.renderToString(tex.trim(), {
          displayMode: false,
          throwOnError: false,
        }),
      );
    } catch {
      return slot(`<code>${tex}</code>`);
    }
  });

  const html = marked.parse(out, { async: false });
  return html.replace(
    MATH_PLACEHOLDER_RE,
    (_m, idx: string) => slots[Number(idx)] ?? "",
  );
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
