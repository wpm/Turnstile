/**
 * Praxis suite: protocol contracts checked against a real `lean --server`,
 * exercised on a real problem (the irrationality of √2, no Mathlib).
 *
 * Run with `pnpm test:praxis`. Skipped (loudly) when no lean binary is
 * available — set TURNSTILE_LSP_CMD or install via elan. Each assertion
 * names the backend code that depends on the contract.
 */
import { beforeAll, describe, expect, it } from "vitest";
import { findLean, runSqrt2Scenario } from "./lean-lsp.mjs";

const leanBin = findLean();

describe.skipIf(!leanBin)("lean --server protocol contracts (sqrt 2)", () => {
  /** @type {Awaited<ReturnType<typeof runSqrt2Scenario>>} */
  let s;

  beforeAll(async () => {
    s = await runSqrt2Scenario(leanBin);
  }, 300_000);

  // ── Wire shape: lean::server::FileProgressParams ──────────────────────

  it("fileProgress version lives inside textDocument, not at the top level", () => {
    expect(s.progressEvents.length).toBeGreaterThan(0);
    for (const p of s.progressEvents) {
      expect(p).not.toHaveProperty("version");
      expect(typeof p.textDocument).toBe("object");
    }
    expect(
      s.progressEvents.some((p) => typeof p.textDocument.version === "number"),
    ).toBe(true);
  });

  it("fileProgress arrives with non-empty processing during elaboration", () => {
    expect(
      s.progressEvents.filter((p) => p.processing.length > 0).length,
    ).toBeGreaterThan(0);
  });

  // ── ElaborationDone: turnstile::from_lean folds empty processing ──────

  it("an empty processing array signals elaboration done", () => {
    expect(s.progressEvents.some((p) => p.processing.length === 0)).toBe(true);
  });

  // ── parse_file_progress end-exclusive trim ────────────────────────────

  it("processing ranges are well-formed (start ≤ end)", () => {
    for (const p of s.progressEvents) {
      for (const i of p.processing) {
        const { start, end } = i.range;
        expect(
          start.line < end.line ||
            (start.line === end.line && start.character <= end.character),
        ).toBe(true);
      }
    }
  });

  it("records the end-character distribution (informational)", () => {
    // Praxis observation, not a contract: across runs Lean's progress
    // snapshots sometimes end ranges at character 0 (whole-line) and
    // sometimes mid-line — the mix is timing-dependent. This is why
    // parse_file_progress trims the final line only when the end character
    // is exactly 0, and why no test may assert a particular mix.
    const endChars = s.progressEvents.flatMap((p) =>
      p.processing.map((i) => i.range.end.character),
    );
    expect(endChars.length).toBeGreaterThan(0);
    console.info(
      `fileProgress end characters observed: ${[...new Set(endChars)].sort((a, b) => a - b).join(", ")}`,
    );
  });

  // ── Diagnostics: Protocol::keep_notification version field ────────────

  it("a draft with sorry produces a 'sorry' warning diagnostic", () => {
    expect(s.sorryDiag.diagnostics.some((d) => /sorry/i.test(d.message))).toBe(
      true,
    );
  });

  it("publishDiagnostics carries a top-level version field", () => {
    expect(typeof s.sorryDiag.version).toBe("number");
  });

  // ── Goal state: fetch_full_proof_goal_state's data source ─────────────

  it("plainGoal returns goals at the sorry (at the token start)", () => {
    expect(s.goalAtSorry).not.toBeNull();
    expect(s.goalAtSorry.goals.length).toBeGreaterThan(0);
  });

  it("the goal at the sorry is the descent statement", () => {
    const rendered = s.goalAtSorry?.rendered ?? "";
    expect(rendered.includes("False") || rendered.includes("≠")).toBe(true);
  });

  // ── Re-elaboration after an incremental edit ──────────────────────────

  it("fileProgress after didChange reports the new textDocument.version", () => {
    const after = s.progressEvents.slice(s.progressBefore);
    expect(after.some((p) => p.textDocument.version === 1)).toBe(true);
  });

  it("the completed proof elaborates with zero diagnostics", () => {
    expect(s.finalDiags.diagnostics).toEqual([]);
  });

  it("no goals remain at the end of the completed proof", () => {
    expect(
      s.goalAfter === null ||
        s.goalAfter === undefined ||
        s.goalAfter.goals.length === 0,
    ).toBe(true);
  });
});

if (!leanBin) {
  describe("lean --server protocol contracts (sqrt 2)", () => {
    it.skip("SKIPPED: no lean binary (set TURNSTILE_LSP_CMD or install elan)", () => {});
  });
}
