import { describe, it, expect } from "vitest";
import {
  completeAbbreviation,
  unicodeCompletions,
} from "./unicodeAbbreviations";
import { EditorState } from "@codemirror/state";
import { CompletionContext } from "@codemirror/autocomplete";

describe("completeAbbreviation", () => {
  it("returns the glyph for a known escape, shown rendered with its name", () => {
    const opts = completeAbbreviation("alpha");
    const alpha = opts.find((o) => o.label === "\\alpha");
    expect(alpha).toBeDefined();
    // The list shows the rendered glyph; the escape rides alongside as detail.
    expect(alpha?.displayLabel).toBe("α");
    expect(alpha?.detail).toBe("\\alpha");
    // Accepting replaces the escape with the glyph.
    expect(alpha?.apply).toBe("α");
  });

  it("matches on a prefix so the menu narrows as you type", () => {
    const opts = completeAbbreviation("al");
    const labels = opts.map((o) => o.label);
    expect(labels).toContain("\\alpha");
    // Every option shares the typed prefix.
    expect(opts.every((o) => o.label.startsWith("\\al"))).toBe(true);
  });

  it("ranks a short exact match above a longer one sharing its prefix", () => {
    const opts = completeAbbreviation("to");
    const to = opts.find((o) => o.label === "\\to");
    const longer = opts.find((o) => o.label.length > "\\to".length);
    expect(to).toBeDefined();
    expect(longer).toBeDefined();
    // `\to` (exact) outranks any longer escape that merely starts with "to".
    expect(to?.boost ?? 0).toBeGreaterThan(longer?.boost ?? 0);
  });

  it("ranks shorter escapes above longer ones for the same prefix", () => {
    const opts = completeAbbreviation("a");
    const byLabel = new Map(opts.map((o) => [o.label, o.boost ?? 0]));
    const shortName = [...byLabel.keys()].reduce((a, b) =>
      a.length <= b.length ? a : b,
    );
    const longName = [...byLabel.keys()].reduce((a, b) =>
      a.length >= b.length ? a : b,
    );
    expect(byLabel.get(shortName)).toBeGreaterThanOrEqual(
      byLabel.get(longName) ?? 0,
    );
  });

  it("places the caret at the $CURSOR marker instead of inserting it", () => {
    // `\norm` → ‖$CURSOR‖: the marker is a caret position, not literal text.
    const norm = completeAbbreviation("norm").find((o) => o.label === "\\norm");
    expect(norm).toBeDefined();
    // A marker entry uses a functional apply (caret placement), not a string.
    expect(typeof norm?.apply).toBe("function");
    // The rendered preview drops the marker.
    expect(norm?.displayLabel).toBe("‖‖");
  });

  it("returns nothing for an unknown escape", () => {
    expect(completeAbbreviation("definitelynotanabbreviation")).toHaveLength(0);
  });
});

describe("unicodeCompletions source", () => {
  function resultAt(doc: string) {
    const state = EditorState.create({ doc });
    const context = new CompletionContext(state, doc.length, false);
    const result = unicodeCompletions(context);
    // The source is synchronous; narrow away the Promise arm of the union.
    if (result instanceof Promise) throw new Error("expected sync result");
    return result;
  }

  it("fires on a backslash escape and anchors at the backslash", () => {
    const result = resultAt("\\alph");
    expect(result).not.toBeNull();
    expect(result?.from).toBe(0); // replaces from the backslash
    expect(result?.options.some((o) => o.label === "\\alpha")).toBe(true);
  });

  it("does not fire without a leading backslash", () => {
    expect(resultAt("alpha")).toBeNull();
  });

  it("does not fire on a bare backslash with no letters", () => {
    expect(resultAt("\\")).toBeNull();
  });
});
