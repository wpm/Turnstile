import { afterEach, describe, expect, it, vi } from "vitest";
import { GOAL_STATE_ENABLED } from "./featureFlags";

describe("GOAL_STATE_ENABLED", () => {
  // The 1.0.0 contract: Goal State is off unless explicitly enabled. This is the
  // value the shipped build sees (no VITE_GOAL_STATE set).
  it("is off by default", () => {
    expect(GOAL_STATE_ENABLED).toBe(false);
  });

  // The flag is computed at module load from import.meta.env, so each case
  // stubs the env var and re-imports a fresh module to re-evaluate it.
  async function loadFlag(value: string): Promise<boolean> {
    vi.resetModules();
    vi.stubEnv("VITE_GOAL_STATE", value);
    const mod = await import("./featureFlags");
    return mod.GOAL_STATE_ENABLED;
  }

  afterEach(() => {
    vi.unstubAllEnvs();
    vi.resetModules();
  });

  it.each(["1", "true"])("is on when VITE_GOAL_STATE is %j", async (value) => {
    expect(await loadFlag(value)).toBe(true);
  });

  it.each(["0", "false", "", "yes"])(
    "stays off for the non-enabling value %j",
    async (value) => {
      expect(await loadFlag(value)).toBe(false);
    },
  );
});
