import { describe, it, expect } from "vitest";
import { renderMathInMarkdown } from "./render";

describe("renderMathInMarkdown", () => {
  it("renders plain markdown to HTML", () => {
    const html = renderMathInMarkdown("# Title\n\nsome **bold** text");
    expect(html).toContain("<h1");
    expect(html).toContain("Title");
    expect(html).toContain("<strong>bold</strong>");
  });

  it("renders inline $...$ math via KaTeX", () => {
    const html = renderMathInMarkdown("the value $x + 1$ here");
    // KaTeX wraps output in a span with the `katex` class.
    expect(html).toContain("katex");
    // The raw delimiters should not survive.
    expect(html).not.toContain("$x + 1$");
  });

  it("renders display $$...$$ math via KaTeX before inline", () => {
    const html = renderMathInMarkdown("$$a^2 + b^2 = c^2$$");
    expect(html).toContain("katex");
    expect(html).toContain("katex-display");
    expect(html).not.toContain("$$");
  });

  it("handles a mix of display and inline math", () => {
    const html = renderMathInMarkdown(
      "intro $$E = mc^2$$ then $p \\land q$ end",
    );
    expect(html).toContain("katex-display");
    expect(html).toContain("intro");
    expect(html).toContain("end");
  });

  it("does not treat escaped \\$ as inline math", () => {
    const html = renderMathInMarkdown("costs \\$5 and \\$6");
    expect(html).not.toContain("katex");
  });

  it("leaves text without math untouched aside from markdown", () => {
    const html = renderMathInMarkdown("just a sentence.");
    expect(html).toContain("just a sentence.");
  });

  it("recovers from invalid TeX without throwing", () => {
    // throwOnError:false means KaTeX renders an error node rather than
    // throwing; either way the call must return a string.
    const html = renderMathInMarkdown("broken $\\frac{1}{$ inline");
    expect(typeof html).toBe("string");
  });
});
