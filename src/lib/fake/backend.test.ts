// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import {
  fakeInvoke,
  fakeListen,
  emit,
  ELABORATION_MS,
  PROSE_GEN_MS,
} from "./backend";

// Collect every payload emitted on `event` for the duration of a test.
function collect(event: string): { payloads: unknown[]; stop: () => void } {
  const payloads: unknown[] = [];
  const stop = fakeListen(event, (e) => payloads.push(e.payload));
  return { payloads, stop };
}

// The fake backend installs test hooks on `window`; throw rather than assert
// non-null so the strict lint rules stay satisfied.
function fake() {
  const f = window.__turnstileFake;
  if (!f) throw new Error("fake backend hooks not installed");
  return f;
}

// A whole-document replace expressed as the LSP content change the editor sends.
function replaceAll(text: string) {
  // Route (3): the editor sends the whole document; the incremental `changes`
  // are forwarded only to the LSP, so the fake (like the real backend) reads
  // `fullText` to update its source.
  return {
    fullText: text,
    changes: [
      {
        range: {
          start: { line: 0, character: 0 },
          end: { line: 1_000_000, character: 0 },
        },
        text,
      },
    ],
  };
}

beforeEach(() => {
  vi.useFakeTimers();
});

afterEach(() => {
  vi.runOnlyPendingTimers();
  vi.useRealTimers();
});

describe("fakeListen / emit", () => {
  it("delivers emitted payloads to subscribers and stops on unsubscribe", () => {
    const { payloads, stop } = collect("custom-event");
    emit("custom-event", { a: 1 });
    expect(payloads).toEqual([{ a: 1 }]);
    stop();
    emit("custom-event", { a: 2 });
    expect(payloads).toEqual([{ a: 1 }]);
  });

  it("emitting to an event with no listeners is a no-op", () => {
    expect(() => {
      emit("nobody-home", 1);
    }).not.toThrow();
  });

  it("announces lsp connected ~50ms after the first turnstile-message listener", async () => {
    const { payloads } = collect("turnstile-message");
    await vi.advanceTimersByTimeAsync(60);
    expect(payloads).toContainEqual({
      type: "lspStatus",
      state: "connected",
      message: "connected",
    });
    // get_lsp_status now reflects the announced status.
    expect(await fakeInvoke("get_lsp_status")).toEqual({
      state: "connected",
      message: "connected",
    });
  });
});

describe("update_document elaboration", () => {
  it("emits progress, annotations, done and a goal state for a clean proof", async () => {
    const { payloads } = collect("turnstile-message");
    const prose = collect("prose-updated");
    const proseGen = collect("prose-generating");

    await fakeInvoke(
      "update_document",
      replaceAll("theorem t : True := by\n  trivial"),
    );

    // Progress fires synchronously.
    expect(payloads).toContainEqual({
      type: "fileProgress",
      items: [{ start_line: 1, end_line: 2 }],
    });

    await vi.advanceTimersByTimeAsync(ELABORATION_MS + 5);
    const types = payloads.map((p) => (p as { type: string }).type);
    expect(types).toContain("annotationsUpdated");
    expect(types).toContain("elaborationDone");
    expect(types).toContain("goalStateUpdated");

    // Clean (non-empty) goal state ⇒ prose regeneration cycle.
    expect(proseGen.payloads).toContain(true);
    await vi.advanceTimersByTimeAsync(PROSE_GEN_MS + 5);
    expect(proseGen.payloads).toContain(false);
    expect(prose.payloads.length).toBe(1);
    expect((prose.payloads[0] as { hash: string }).hash).toBe(
      "fake-prose-hash",
    );
  });

  it("tokenizes keywords and flags sorry as a warning diagnostic", async () => {
    const { payloads } = collect("turnstile-message");
    await fakeInvoke(
      "update_document",
      replaceAll("theorem t : True := by\n  sorry"),
    );
    await vi.advanceTimersByTimeAsync(ELABORATION_MS + 5);

    const ann = payloads.find(
      (p) => (p as { type: string }).type === "annotationsUpdated",
    ) as {
      annotations: {
        syntaxColoring: unknown[];
        underlines: { severity: string }[];
      };
    };
    expect(ann.annotations.syntaxColoring.length).toBeGreaterThan(0);
    expect(
      ann.annotations.underlines.some((u) => u.severity === "warning"),
    ).toBe(true);
  });

  it("flags oops as an error diagnostic and yields no goal/prose", async () => {
    const { payloads } = collect("turnstile-message");
    const proseGen = collect("prose-generating");
    await fakeInvoke("update_document", replaceAll("theorem t := oops"));
    await vi.advanceTimersByTimeAsync(ELABORATION_MS + PROSE_GEN_MS + 10);

    const ann = payloads.find(
      (p) => (p as { type: string }).type === "annotationsUpdated",
    ) as { annotations: { underlines: { severity: string }[] } };
    expect(ann.annotations.underlines.some((u) => u.severity === "error")).toBe(
      true,
    );
    const goal = payloads.find(
      (p) => (p as { type: string }).type === "goalStateUpdated",
    ) as { full: string };
    expect(goal.full).toBe("");
    // Empty goal state ⇒ no prose cycle.
    expect(proseGen.payloads).not.toContain(true);
  });

  it("an empty document elaborates to an empty goal state", async () => {
    const { payloads } = collect("turnstile-message");
    await fakeInvoke("update_document", replaceAll(""));
    await vi.advanceTimersByTimeAsync(ELABORATION_MS + 5);
    const goal = payloads.find(
      (p) => (p as { type: string }).type === "goalStateUpdated",
    ) as { full: string };
    expect(goal.full).toBe("");
  });

  it("a superseded elaboration is cancelled by a newer edit", async () => {
    const prose = collect("prose-updated");
    // First edit would produce a sorry goal (prose-worthy); immediately
    // replaced before its timer fires.
    await fakeInvoke("update_document", replaceAll("theorem a := by sorry"));
    await fakeInvoke("update_document", replaceAll("theorem b := by sorry"));
    await vi.advanceTimersByTimeAsync(ELABORATION_MS + PROSE_GEN_MS + 20);
    // Only the surviving elaboration delivers prose.
    expect(prose.payloads.length).toBe(1);
  });

  it("defaults to no changes when none are supplied", async () => {
    await expect(fakeInvoke("update_document")).resolves.toBeNull();
    await vi.advanceTimersByTimeAsync(ELABORATION_MS + 5);
  });

  it("assigns the whole document and ignores incremental changes (route 3)", async () => {
    await fakeInvoke("update_document", replaceAll("abcd"));
    await vi.advanceTimersByTimeAsync(ELABORATION_MS + 5);
    // The backend is the source of truth: it takes fullText verbatim. Any
    // incremental `changes` are for the LSP only and must not be replayed onto
    // the stored source, so the diff below is ignored in favor of fullText.
    await fakeInvoke("update_document", {
      fullText: "XbcY",
      changes: [
        {
          range: {
            start: { line: 0, character: 0 },
            end: { line: 0, character: 1 },
          },
          text: "WRONG",
        },
      ],
    });
    await vi.advanceTimersByTimeAsync(ELABORATION_MS + 5);
    expect(fake().getSource()).toBe("XbcY");
  });

  it("cancels in-flight prose delivery when superseded mid-generation", async () => {
    const prose = collect("prose-updated");
    const proseGen = collect("prose-generating");
    // First clean proof reaches the prose-generating phase…
    await fakeInvoke(
      "update_document",
      replaceAll("theorem t : True := by\n  trivial"),
    );
    await vi.advanceTimersByTimeAsync(ELABORATION_MS + 5);
    expect(proseGen.payloads).toContain(true);
    // …then a new edit supersedes it before the prose timer fires.
    await fakeInvoke(
      "update_document",
      replaceAll("theorem u : True := by\n  trivial"),
    );
    await vi.advanceTimersByTimeAsync(PROSE_GEN_MS + 5);
    // The stale prose was dropped (busy indicator cleared, no prose delivered).
    expect(prose.payloads.length).toBe(0);
    expect(proseGen.payloads).toContain(false);
  });
});

describe("assistant send_message", () => {
  it("replies without echoing and records both turns", async () => {
    const deltas = collect("assistant-delta");
    const done = collect("assistant-stream-done");
    const p = fakeInvoke("send_message", { content: "hi there" });
    await vi.runAllTimersAsync();
    const reply = (await p) as string;
    // The fake must never echo the user's text back (#57/#62).
    expect(reply).not.toContain("[echo]");
    expect(reply).not.toContain("hi there");
    expect(reply).toContain("Proof Assistant");
    expect(deltas.payloads.join("")).toBe(reply);
    expect(done.payloads.length).toBe(1);

    const transcript = (await fakeInvoke("get_transcript")) as {
      turns: { role: string }[];
    };
    expect(transcript.turns.map((t) => t.role)).toEqual(
      expect.arrayContaining(["user", "assistant"]),
    );
  });

  it("answers a goal question with the current goal when one exists", async () => {
    await fakeInvoke("update_document", replaceAll("theorem t := by sorry"));
    await vi.advanceTimersByTimeAsync(ELABORATION_MS + 5);
    const p = fakeInvoke("send_message", { content: "what is the goal?" });
    await vi.runAllTimersAsync();
    const reply = (await p) as string;
    expect(reply).toContain("infinite descent");
  });

  it("reports the absence of a goal when the document is empty", async () => {
    await fakeInvoke("update_document", replaceAll(""));
    await vi.advanceTimersByTimeAsync(ELABORATION_MS + 5);
    const p = fakeInvoke("send_message", { content: "show me the goal" });
    await vi.runAllTimersAsync();
    expect((await p) as string).toContain("no goal state yet");
  });

  it("throws on the simulated failure trigger", async () => {
    const p = fakeInvoke("send_message", { content: "error !fail please" });
    await expect(p).rejects.toThrow("simulated LLM failure");
  });

  it("defaults to empty content when none is supplied", async () => {
    const p = fakeInvoke("send_message");
    await vi.runAllTimersAsync();
    const reply = (await p) as string;
    expect(reply).not.toContain("[echo]");
    expect(reply).toContain("Proof Assistant");
  });

  it("reports a key present and connected status by default", async () => {
    expect(await fakeInvoke("has_api_key")).toBe(true);
    expect(await fakeInvoke("get_assistant_status")).toEqual({
      state: "connected",
    });
  });
});

describe("lsp_hover", () => {
  it("returns markdown hover over a theorem line", async () => {
    await fakeInvoke(
      "update_document",
      replaceAll("theorem t : True := by\n  trivial"),
    );
    const hover = (await fakeInvoke("lsp_hover", { line: 0 })) as {
      kind: string;
    };
    expect(hover.kind).toBe("markdown");
  });

  it("returns null off a theorem line and for out-of-range lines", async () => {
    await fakeInvoke(
      "update_document",
      replaceAll("theorem t : True := by\n  trivial"),
    );
    expect(await fakeInvoke("lsp_hover", { line: 1 })).toBeNull();
    expect(await fakeInvoke("lsp_hover", { line: 99 })).toBeNull();
    // No line arg defaults to line 0 (the theorem line) ⇒ hover present.
    expect(await fakeInvoke("lsp_hover", {})).not.toBeNull();
  });
});

describe("settings, prompts and models", () => {
  it("round-trips settings through save/get", async () => {
    const next = {
      editor_font_size: 20,
      goal_state_font_size: 13,
      prose_proof_font_size: 13,
      assistant_font_size: 13,
      assistant_model: "claude-opus-4-6",
      translation_model: null,
      assistant_prompt: null,
      translation_prompt: null,
    };
    await fakeInvoke("save_settings", { settings: next });
    expect(await fakeInvoke("get_settings")).toEqual(next);
  });

  it("exposes default prompts and the model list", async () => {
    expect(await fakeInvoke("get_default_assistant_prompt")).toContain("fake");
    expect(await fakeInvoke("get_default_translation_prompt")).toContain(
      "fake",
    );
    const models = (await fakeInvoke("get_available_models")) as unknown[];
    expect(models.length).toBe(2);
  });

  it("load_transcript is accepted as a no-op", async () => {
    expect(await fakeInvoke("load_transcript")).toBeNull();
  });
});

describe("session lifecycle", () => {
  it("new_session clears state and emits an empty session-loaded", async () => {
    const loaded = collect("session-loaded");
    await fakeInvoke("new_session");
    expect((loaded.payloads[0] as { proof_lean: string }).proof_lean).toBe("");
  });

  it("open_session loads the sample session", async () => {
    const loaded = collect("session-loaded");
    await fakeInvoke("open_session");
    expect((loaded.payloads[0] as { proof_lean: string }).proof_lean).toContain(
      "sqrt2_irrational",
    );
  });

  it("save commands and misc setters are no-ops", async () => {
    for (const cmd of [
      "save_session",
      "save_session_as",
      "auto_save_session",
      "set_last_session",
      "set_window_title",
      "set_menu_item_enabled",
      "get_last_session",
    ]) {
      await expect(fakeInvoke(cmd)).resolves.toBeNull();
    }
  });

  it("autosave check / restore / delete flow", async () => {
    fake().setAutosave(true);
    expect(await fakeInvoke("check_auto_save")).toBe(true);

    const loaded = collect("session-loaded");
    await fakeInvoke("restore_auto_save");
    expect((loaded.payloads[0] as { proof_lean: string }).proof_lean).toContain(
      "sqrt2_irrational",
    );
    expect(await fakeInvoke("check_auto_save")).toBe(false);

    fake().setAutosave(true);
    await fakeInvoke("delete_auto_save");
    expect(await fakeInvoke("check_auto_save")).toBe(false);
  });

  it("throws on an unknown command", async () => {
    await expect(fakeInvoke("not_a_real_command")).rejects.toThrow(
      /unknown command/,
    );
  });
});

describe("window test hooks", () => {
  it("exposes emit, getSource and setAutosave", async () => {
    expect(typeof fake().emit).toBe("function");
    await fakeInvoke("update_document", replaceAll("hello world"));
    expect(fake().getSource()).toContain("hello world");
  });
});
