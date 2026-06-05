import { marked } from "marked";
import katex from "katex";

// ── Math-aware Markdown rendering ──────────────────────────────────────
//
// Shared by the LSP hover tooltip and the assistant chat: both receive
// Markdown that may contain $...$ / $$...$$ TeX. Math is replaced by
// placeholder tokens before marked runs so the delimiters survive Markdown
// parsing, then re-substituted with KaTeX HTML.

// Unicode angle brackets never appear in Lean hover output or LLM prose.
const MATH_PLACEHOLDER_RE = /‹MATH:(\d+)›/g;

export function renderMathInMarkdown(markdown: string): string {
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
