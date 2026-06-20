import { describe, it, expect, vi, beforeEach } from "vitest";
import { get } from "svelte/store";

// Capture the callback handed to `listen` so tests can push payloads through
// the real handler installed by startMessageListener.
type Listener = (event: { payload: unknown }) => void;
let captured: { event: string; cb: Listener } | null = null;
const unlisten = vi.fn();

vi.mock("@tauri-apps/api/event", () => ({
  listen: (event: string, cb: Listener) => {
    captured = { event, cb };
    return Promise.resolve(unlisten);
  },
}));

import {
  startMessageListener,
  fileProgress,
  annotations,
  lspStatus,
  goalState,
  goalCache,
  elaborationDone,
  showMessage,
} from "./turnstile_messages";
import { EMPTY_ANNOTATIONS } from "./annotations";

function emit(payload: unknown) {
  if (!captured) throw new Error("no turnstile-message listener registered");
  captured.cb({ payload });
}

beforeEach(() => {
  captured = null;
  fileProgress.set([]);
  annotations.set(EMPTY_ANNOTATIONS);
  lspStatus.set(null);
  goalState.set("");
  goalCache.set(new Map());
  elaborationDone.set(0);
  showMessage.set(null);
});

describe("startMessageListener", () => {
  it("subscribes to the turnstile-message event and returns the unlisten fn", async () => {
    const fn = await startMessageListener();
    if (!captured) throw new Error("no listener registered");
    expect(captured.event).toBe("turnstile-message");
    expect(fn).toBe(unlisten);
  });

  it("handles fileProgress", async () => {
    await startMessageListener();
    const items = [{ start_line: 1, end_line: 3 }];
    emit({ type: "fileProgress", items });
    expect(get(fileProgress)).toEqual(items);
  });

  it("handles annotationsUpdated", async () => {
    await startMessageListener();
    const payload = {
      syntaxColoring: [{ from: 0, to: 2, tokenType: "keyword" }],
      underlines: [],
      gutter: [],
      hover: [],
    };
    emit({ type: "annotationsUpdated", annotations: payload });
    expect(get(annotations)).toEqual(payload);
  });

  it("handles lspStatus", async () => {
    await startMessageListener();
    emit({ type: "lspStatus", state: "ready", message: "ok" });
    expect(get(lspStatus)).toEqual({ state: "ready", message: "ok" });
  });

  it("handles goalStateUpdated", async () => {
    await startMessageListener();
    emit({ type: "goalStateUpdated", full: "⊢ True" });
    expect(get(goalState)).toBe("⊢ True");
  });

  it("handles goalCacheUpdated by keying the snapshot rows by row number", async () => {
    await startMessageListener();
    emit({
      type: "goalCacheUpdated",
      rows: [
        {
          row: 1,
          status: "fresh",
          goals: [{ caseLabel: null, hypotheses: [], conclusion: "True" }],
        },
        { row: 2, status: "stale", goals: [] },
      ],
    });
    const cache = get(goalCache);
    expect(cache.size).toBe(2);
    expect(cache.get(1)?.status).toBe("fresh");
    expect(cache.get(1)?.goals[0].conclusion).toBe("True");
    expect(cache.get(2)?.status).toBe("stale");
  });

  it("replaces the whole cache on each goalCacheUpdated snapshot", async () => {
    await startMessageListener();
    emit({
      type: "goalCacheUpdated",
      rows: [{ row: 1, status: "fresh", goals: [] }],
    });
    emit({
      type: "goalCacheUpdated",
      rows: [{ row: 5, status: "empty", goals: [] }],
    });
    const cache = get(goalCache);
    expect(cache.has(1)).toBe(false);
    expect(cache.get(5)?.status).toBe("empty");
  });

  it("handles elaborationDone by clearing progress and bumping the counter", async () => {
    await startMessageListener();
    fileProgress.set([{ start_line: 1, end_line: 2 }]);
    emit({ type: "elaborationDone" });
    expect(get(fileProgress)).toEqual([]);
    expect(get(elaborationDone)).toBe(1);
    emit({ type: "elaborationDone" });
    expect(get(elaborationDone)).toBe(2);
  });

  it("handles showMessage", async () => {
    await startMessageListener();
    emit({ type: "showMessage", severity: "error", message: "boom" });
    expect(get(showMessage)).toEqual({ severity: "error", message: "boom" });
  });

  it("ignores backend-internal message types", async () => {
    await startMessageListener();
    emit({ type: "diagnostics", items: [] });
    emit({ type: "semanticTokenRefresh" });
    emit({ type: "semanticTokens", items: [] });
    // None of these mutate the public stores.
    expect(get(annotations)).toEqual(EMPTY_ANNOTATIONS);
    expect(get(goalState)).toBe("");
  });

  it("warns on an unknown message type", async () => {
    await startMessageListener();
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    emit({ type: "totallyUnknown" });
    expect(warn).toHaveBeenCalled();
    warn.mockRestore();
  });
});
