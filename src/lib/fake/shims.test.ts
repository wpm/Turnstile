// @vitest-environment jsdom
import { describe, it, expect } from "vitest";
import { invoke } from "./core";
import { listen, type Event } from "./event";

// The e2e shims simply forward to the in-browser fake backend. These tests
// confirm the wiring (the alias targets in vite.config.js) stays intact.

describe("fake core shim", () => {
  it("forwards invoke() to the fake backend", async () => {
    const status = await invoke("get_lsp_status");
    // Either null (before announce) or the connected status object — both are
    // valid; what matters is the call resolves through the fake dispatcher.
    expect(status === null || typeof status === "object").toBe(true);
  });

  it("rejects unknown commands like the fake backend does", async () => {
    await expect(invoke("definitely_not_a_command")).rejects.toThrow();
  });
});

describe("fake event shim", () => {
  it("subscribes through the fake backend and receives emitted payloads", async () => {
    const received: number[] = [];
    const unlisten = await listen<number>(
      "shim-test-event",
      (e: Event<number>) => {
        received.push(e.payload);
      },
    );
    // Reach the fake's emit via the global test hook installed at module load.
    const fake = window.__turnstileFake;
    if (!fake) throw new Error("fake backend hooks not installed");
    fake.emit("shim-test-event", 7);
    expect(received).toEqual([7]);
    unlisten();
    fake.emit("shim-test-event", 8);
    expect(received).toEqual([7]);
  });
});
