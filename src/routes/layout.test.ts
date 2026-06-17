import { describe, it, expect } from "vitest";
import { ssr } from "./+layout";

describe("+layout", () => {
  it("disables SSR (Tauri serves the SPA via adapter-static)", () => {
    expect(ssr).toBe(false);
  });
});
