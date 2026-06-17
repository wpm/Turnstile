import { describe, it, expect, vi, beforeEach } from "vitest";
import { get } from "svelte/store";

// Each call to `listen` records the event name + callback. startSetupProgress /
// startProseGenerating each register one listener; we drive their callbacks
// directly to exercise the payload handling.
type Listener = (event: { payload: unknown }) => void;
const calls: { event: string; cb: Listener }[] = [];
const unlisten = vi.fn();

vi.mock("@tauri-apps/api/event", () => ({
  listen: (event: string, cb: Listener) => {
    calls.push({ event, cb });
    return Promise.resolve(unlisten);
  },
}));

import {
  startSetupProgressListener,
  startProseGeneratingListener,
  setupProgress,
  proseGenerating,
} from "./appState";

function lastCb() {
  return calls[calls.length - 1].cb;
}

beforeEach(() => {
  calls.length = 0;
  setupProgress.set(null);
  proseGenerating.set(false);
});

describe("startSetupProgressListener", () => {
  it("listens on setup-progress and returns the unlisten fn", async () => {
    const fn = await startSetupProgressListener();
    expect(calls[0].event).toBe("setup-progress");
    expect(fn).toBe(unlisten);
  });

  it("stores in-progress phases", async () => {
    await startSetupProgressListener();
    const payload = { phase: "downloading", message: "...", progress_pct: 42 };
    lastCb()({ payload });
    expect(get(setupProgress)).toEqual(payload);
  });

  it("clears progress to null once the ready phase arrives", async () => {
    await startSetupProgressListener();
    lastCb()({
      payload: { phase: "ready", message: "done", progress_pct: 100 },
    });
    expect(get(setupProgress)).toBeNull();
  });
});

describe("startProseGeneratingListener", () => {
  it("listens on prose-generating and mirrors the boolean payload", async () => {
    const fn = await startProseGeneratingListener();
    expect(calls[0].event).toBe("prose-generating");
    expect(fn).toBe(unlisten);

    lastCb()({ payload: true });
    expect(get(proseGenerating)).toBe(true);
    lastCb()({ payload: false });
    expect(get(proseGenerating)).toBe(false);
  });
});
