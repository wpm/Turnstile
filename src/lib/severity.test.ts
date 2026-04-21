import { describe, it, expect } from "vitest";
import { severityIcon } from "./severity";

describe("severityIcon", () => {
  it("returns ✕ for error", () => {
    expect(severityIcon("error")).toBe("✕");
  });

  it("returns ⚠ for warning", () => {
    expect(severityIcon("warning")).toBe("⚠");
  });

  it("returns ℹ for info", () => {
    expect(severityIcon("info")).toBe("ℹ");
  });

  it("returns ◦ for log", () => {
    expect(severityIcon("log")).toBe("◦");
  });

  it("returns ◦ for any unrecognized severity", () => {
    expect(severityIcon("hint")).toBe("◦");
    expect(severityIcon("")).toBe("◦");
    expect(severityIcon("unknown")).toBe("◦");
  });
});
