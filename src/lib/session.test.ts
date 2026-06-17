import { describe, it, expect, vi, beforeEach } from "vitest";

// Record every invoke(cmd, args) call; let individual tests control the
// resolved value where the code path reads it back (get_last_session).
const invoke =
  vi.fn<(cmd: string, args?: Record<string, unknown>) => Promise<unknown>>();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (cmd: string, args?: Record<string, unknown>) => invoke(cmd, args),
}));

import {
  saveSession,
  saveSessionAs,
  autoSaveSession,
  newSession,
  openSession,
  refreshWindowTitle,
  type UiLayout,
} from "./session";
import {
  cursorPosition,
  proofSource,
  proseHash,
  proseText,
  wordWrap,
} from "./appState";

const layout: UiLayout = {
  assistantWidthPct: 30,
  proofView: "prose",
  goalPanelPct: 40,
};

beforeEach(() => {
  invoke.mockReset();
  invoke.mockResolvedValue(undefined);
  proofSource.set("theorem t : True := trivial");
  proseText.set("It is true.");
  proseHash.set("hash123");
  cursorPosition.set({ line: 4, col: 2 });
  wordWrap.set(true);
});

describe("saveSession", () => {
  it("invokes save_session with assembled args and meta", async () => {
    await saveSession(layout);
    expect(invoke).toHaveBeenCalledTimes(1);
    const [cmd, args] = invoke.mock.calls[0];
    expect(cmd).toBe("save_session");
    if (!args) throw new Error("expected args");
    expect(args.proofLean).toBe("theorem t : True := trivial");
    expect(args.proseText).toBe("It is true.");
    expect(args.proseHash).toBe("hash123");
    expect(args.suggestedName).toBeNull();
    expect(args.meta).toMatchObject({
      cursor_line: 4,
      cursor_col: 2,
      assistant_width_pct: 30,
      proof_view: "prose",
      goal_panel_pct: 40,
      word_wrap: true,
    });
  });
});

describe("saveSessionAs", () => {
  it("invokes save_session_as with the suggestedName field present", async () => {
    await saveSessionAs(layout);
    const [cmd, args] = invoke.mock.calls[0];
    expect(cmd).toBe("save_session_as");
    expect(args).toHaveProperty("suggestedName", null);
  });
});

describe("autoSaveSession", () => {
  it("invokes auto_save_session and omits suggestedName", async () => {
    await autoSaveSession(layout);
    const [cmd, args] = invoke.mock.calls[0];
    expect(cmd).toBe("auto_save_session");
    expect(args).not.toHaveProperty("suggestedName");
    expect(args).toHaveProperty("proofLean");
  });
});

describe("newSession", () => {
  it("invokes new_session", async () => {
    await newSession();
    expect(invoke).toHaveBeenCalledWith("new_session", undefined);
  });
});

describe("openSession", () => {
  it("invokes open_session with a null path", async () => {
    await openSession();
    expect(invoke).toHaveBeenCalledWith("open_session", { path: null });
  });
});

describe("refreshWindowTitle", () => {
  it("derives the window title from the last session file name", async () => {
    invoke.mockImplementation((cmd: string) => {
      if (cmd === "get_last_session")
        return Promise.resolve("/home/u/proofs/MyProof.turn");
      return Promise.resolve(undefined);
    });
    await refreshWindowTitle();
    expect(invoke).toHaveBeenCalledWith("set_window_title", {
      title: "Turnstile — MyProof",
    });
  });

  it("falls back to the bare app name when there is no last session", async () => {
    invoke.mockImplementation((cmd: string) => {
      if (cmd === "get_last_session") return Promise.resolve(null);
      return Promise.resolve(undefined);
    });
    await refreshWindowTitle();
    expect(invoke).toHaveBeenCalledWith("set_window_title", {
      title: "Turnstile",
    });
  });

  it("swallows errors (cosmetic title only)", async () => {
    invoke.mockImplementation((cmd: string) => {
      if (cmd === "get_last_session")
        return Promise.reject(new Error("no ipc"));
      return Promise.resolve(undefined);
    });
    await expect(refreshWindowTitle()).resolves.toBeUndefined();
  });
});
