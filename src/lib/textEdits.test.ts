// @vitest-environment jsdom
import { describe, it, expect } from "vitest";
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import {
  buildTextEditChanges,
  applyTextEdits,
  type TextEdit,
} from "./textEdits";

function makeView(doc: string): EditorView {
  return new EditorView({
    state: EditorState.create({ doc }),
  });
}

describe("buildTextEditChanges", () => {
  it("maps a single-line replacement to correct CM positions", () => {
    const state = EditorState.create({ doc: "theorem foo : True" });
    const edits: TextEdit[] = [
      {
        start_line: 0,
        start_character: 8,
        end_line: 0,
        end_character: 11,
        new_text: "bar",
      },
    ];
    const [change] = buildTextEditChanges(state.doc, edits);
    // line(1).from == 0; col 8 → 8, col 11 → 11
    expect(change.from).toBe(8);
    expect(change.to).toBe(11);
    expect(change.insert).toBe("bar");
  });

  it("sorts edits in reverse order (bottom-up) to preserve positions", () => {
    const state = EditorState.create({ doc: "abc\ndef\n" });
    const edits: TextEdit[] = [
      {
        start_line: 0,
        start_character: 0,
        end_line: 0,
        end_character: 1,
        new_text: "A",
      },
      {
        start_line: 1,
        start_character: 0,
        end_line: 1,
        end_character: 1,
        new_text: "D",
      },
    ];
    const changes = buildTextEditChanges(state.doc, edits);
    // First change in output must be the later one (line 1)
    expect(changes[0].insert).toBe("D");
    expect(changes[1].insert).toBe("A");
  });

  it("handles insertion (from === to)", () => {
    const state = EditorState.create({ doc: "hello" });
    const edits: TextEdit[] = [
      {
        start_line: 0,
        start_character: 5,
        end_line: 0,
        end_character: 5,
        new_text: " world",
      },
    ];
    const [change] = buildTextEditChanges(state.doc, edits);
    expect(change.from).toBe(5);
    expect(change.to).toBe(5);
    expect(change.insert).toBe(" world");
  });

  it("handles deletion (new_text === '')", () => {
    const state = EditorState.create({ doc: "hello world" });
    const edits: TextEdit[] = [
      {
        start_line: 0,
        start_character: 5,
        end_line: 0,
        end_character: 11,
        new_text: "",
      },
    ];
    const [change] = buildTextEditChanges(state.doc, edits);
    expect(change.from).toBe(5);
    expect(change.to).toBe(11);
    expect(change.insert).toBe("");
  });

  it("handles multi-line edits", () => {
    const state = EditorState.create({ doc: "line1\nline2\nline3" });
    const edits: TextEdit[] = [
      {
        start_line: 0,
        start_character: 0,
        end_line: 1,
        end_character: 5,
        new_text: "replaced",
      },
    ];
    const [change] = buildTextEditChanges(state.doc, edits);
    expect(change.from).toBe(0);
    expect(change.to).toBe(11); // "line1\nline2" is 11 chars
    expect(change.insert).toBe("replaced");
  });
});

describe("applyTextEdits", () => {
  it("applies a single replacement to the editor document", () => {
    const view = makeView("theorem foo : True");
    const edits: TextEdit[] = [
      {
        start_line: 0,
        start_character: 8,
        end_line: 0,
        end_character: 11,
        new_text: "bar",
      },
    ];
    applyTextEdits(view, edits);
    expect(view.state.doc.toString()).toBe("theorem bar : True");
    view.destroy();
  });

  it("applies multiple edits atomically (bottom-up)", () => {
    const view = makeView("abc\ndef\n");
    const edits: TextEdit[] = [
      {
        start_line: 0,
        start_character: 0,
        end_line: 0,
        end_character: 1,
        new_text: "A",
      },
      {
        start_line: 1,
        start_character: 0,
        end_line: 1,
        end_character: 1,
        new_text: "D",
      },
    ];
    applyTextEdits(view, edits);
    expect(view.state.doc.toString()).toBe("Abc\nDef\n");
    view.destroy();
  });

  it("calls onBeforeDispatch before and onAfterDispatch after", () => {
    const view = makeView("hello");
    const log: string[] = [];
    const edits: TextEdit[] = [
      {
        start_line: 0,
        start_character: 5,
        end_line: 0,
        end_character: 5,
        new_text: "!",
      },
    ];
    applyTextEdits(
      view,
      edits,
      () => log.push("before"),
      () => log.push("after"),
    );
    expect(log).toEqual(["before", "after"]);
    expect(view.state.doc.toString()).toBe("hello!");
    view.destroy();
  });

  it("calls onAfterDispatch even if dispatch throws", () => {
    const view = makeView("x");
    const log: string[] = [];
    view.dispatch = () => {
      throw new Error("dispatch failed");
    };
    expect(() => {
      applyTextEdits(
        view,
        [
          {
            start_line: 0,
            start_character: 0,
            end_line: 0,
            end_character: 1,
            new_text: "y",
          },
        ],
        () => log.push("before"),
        () => log.push("after"),
      );
    }).toThrow("dispatch failed");
    expect(log).toEqual(["before", "after"]);
    view.destroy();
  });

  it("leaves doc unchanged when edits array is empty", () => {
    const view = makeView("unchanged");
    applyTextEdits(view, []);
    expect(view.state.doc.toString()).toBe("unchanged");
    view.destroy();
  });
});
