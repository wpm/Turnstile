// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach } from "vitest";
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";

const invoke =
  vi.fn<(cmd: string, args?: Record<string, unknown>) => Promise<unknown>>();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (cmd: string, args?: Record<string, unknown>) => invoke(cmd, args),
}));

import {
  buildTooltipDom,
  buildHoverTooltip,
  lspHoverTooltip,
  type HoverInfo,
} from "./hover";

function makeView(doc: string): EditorView {
  const state = EditorState.create({ doc });
  return new EditorView({ state });
}

beforeEach(() => {
  invoke.mockReset();
});

describe("buildTooltipDom", () => {
  it("renders plaintext into a <pre> verbatim", () => {
    const dom = buildTooltipDom({ kind: "plaintext", contents: "x : Nat" });
    const pre = dom.querySelector("pre.cm-hover-plain");
    if (!pre) throw new Error("expected a <pre> element");
    expect(pre.textContent).toBe("x : Nat");
  });

  it("renders markdown (with math) into innerHTML", () => {
    const info: HoverInfo = { kind: "markdown", contents: "**bold** $x$" };
    const dom = buildTooltipDom(info);
    expect(dom.innerHTML).toContain("<strong>bold</strong>");
    expect(dom.innerHTML).toContain("katex");
  });
});

describe("buildHoverTooltip", () => {
  it("returns null when the backend has no hover info", async () => {
    invoke.mockResolvedValue(null);
    const view = makeView("theorem t : True");
    expect(await buildHoverTooltip(view, 3)).toBeNull();
  });

  it("returns null when the invoke rejects", async () => {
    invoke.mockRejectedValue(new Error("lsp down"));
    const view = makeView("theorem t : True");
    expect(await buildHoverTooltip(view, 3)).toBeNull();
  });

  it("builds a tooltip and maps the offset to an LSP position", async () => {
    invoke.mockResolvedValue({ kind: "markdown", contents: "doc" });
    const view = makeView("line0\nline1\nline2");
    // Offset 8 => line index 1 (second line), character 2.
    const tip = await buildHoverTooltip(view, 8);
    if (!tip) throw new Error("expected a tooltip");
    expect(tip.pos).toBe(8);
    expect(tip.above).toBe(true);
    expect(invoke).toHaveBeenCalledWith("lsp_hover", { line: 1, character: 2 });
    // The create() factory yields the rendered DOM.
    const { dom } = tip.create(view);
    expect(dom.textContent).toContain("doc");
  });
});

describe("lspHoverTooltip", () => {
  it("is a defined CodeMirror extension", () => {
    expect(lspHoverTooltip).toBeDefined();
  });
});
