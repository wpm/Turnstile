// @vitest-environment jsdom
import { describe, it, expect, beforeEach } from "vitest";
import {
  isSelectAllEvent,
  activeSelectScope,
  handleSelectAll,
  SELECT_SCOPE_ATTR,
} from "./selectAll";

function key(init: KeyboardEventInit): KeyboardEvent {
  return new KeyboardEvent("keydown", { ...init, cancelable: true });
}

/** A `data-select-scope` panel containing a paragraph of `text`, appended. */
function scopePanel(text: string): { panel: HTMLElement; para: HTMLElement } {
  const panel = document.createElement("div");
  panel.setAttribute(SELECT_SCOPE_ATTR, "");
  const para = document.createElement("p");
  para.textContent = text;
  panel.appendChild(para);
  document.body.appendChild(panel);
  return { panel, para };
}

/** Place a (collapsed when `collapse`) selection over a node's contents. */
function select(node: Node, collapse = false): Selection {
  const range = document.createRange();
  range.selectNodeContents(node);
  if (collapse) range.collapse(true);
  const sel = document.getSelection();
  if (!sel) throw new Error("no selection");
  sel.removeAllRanges();
  sel.addRange(range);
  return sel;
}

describe("isSelectAllEvent", () => {
  it("matches plain Command-A and Ctrl-A", () => {
    expect(isSelectAllEvent(key({ key: "a", metaKey: true }))).toBe(true);
    expect(isSelectAllEvent(key({ key: "a", ctrlKey: true }))).toBe(true);
    // Capitalised key (some layouts / caps lock) still counts.
    expect(isSelectAllEvent(key({ key: "A", metaKey: true }))).toBe(true);
  });

  it("ignores A without a modifier and other modified A's", () => {
    expect(isSelectAllEvent(key({ key: "a" }))).toBe(false);
    expect(
      isSelectAllEvent(key({ key: "a", metaKey: true, shiftKey: true })),
    ).toBe(false);
    expect(
      isSelectAllEvent(key({ key: "a", metaKey: true, altKey: true })),
    ).toBe(false);
    expect(isSelectAllEvent(key({ key: "b", metaKey: true }))).toBe(false);
  });
});

describe("activeSelectScope", () => {
  beforeEach(() => {
    document.body.innerHTML = "";
    document.getSelection()?.removeAllRanges();
  });

  it("finds the scope owning the selection anchor", () => {
    scopePanel("alpha");
    const b = scopePanel("beta");
    select(b.para);
    expect(activeSelectScope(document)).toBe(b.panel);
  });

  it("falls back to activeElement when there is no selection", () => {
    const { panel } = scopePanel("");
    const input = document.createElement("input");
    panel.appendChild(input);
    document.getSelection()?.removeAllRanges();
    input.focus();
    expect(activeSelectScope(document)).toBe(panel);
  });

  it("returns null when nothing relevant is focused", () => {
    const plain = document.createElement("div");
    plain.textContent = "no scope here";
    document.body.appendChild(plain);
    document.getSelection()?.removeAllRanges();
    expect(activeSelectScope(document)).toBeNull();
  });
});

describe("handleSelectAll", () => {
  beforeEach(() => {
    document.body.innerHTML = "";
    document.getSelection()?.removeAllRanges();
  });

  it("scopes the selection to the focused panel", () => {
    const goal = scopePanel("the goal");
    scopePanel("the prose");
    // A click leaves a collapsed caret inside the goal panel.
    const sel = select(goal.para, true);

    const e = key({ key: "a", metaKey: true });
    expect(handleSelectAll(e, document)).toBe(true);
    expect(e.defaultPrevented).toBe(true);
    // The whole goal panel is selected; the prose panel is untouched.
    expect(sel.toString()).toBe("the goal");
    expect(goal.panel.contains(sel.getRangeAt(0).commonAncestorContainer)).toBe(
      true,
    );
  });

  it("suppresses Select All but selects nothing when no panel is focused", () => {
    const plain = document.createElement("div");
    plain.textContent = "outside any scope";
    document.body.appendChild(plain);
    scopePanel("the goal");
    document.getSelection()?.removeAllRanges();

    const e = key({ key: "a", metaKey: true });
    expect(handleSelectAll(e, document)).toBe(true);
    // Default is suppressed (no whole-window select), and nothing got selected.
    expect(e.defaultPrevented).toBe(true);
    expect(document.getSelection()?.toString()).toBe("");
  });

  it("leaves an already-handled event (a focused editor's Mod-a) alone", () => {
    scopePanel("the goal");
    const e = key({ key: "a", metaKey: true });
    e.preventDefault(); // simulate CodeMirror having handled it
    expect(handleSelectAll(e, document)).toBe(false);
  });

  it("ignores keys that are not Select All", () => {
    const e = key({ key: "c", metaKey: true });
    expect(handleSelectAll(e, document)).toBe(false);
    expect(e.defaultPrevented).toBe(false);
  });
});
