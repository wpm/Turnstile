#!/usr/bin/env node
/**
 * CLI wrapper around the praxis scenario (see lean-lsp.mjs). The checks
 * themselves live in sqrt2.praxis.test.mjs and run under vitest via
 * `pnpm test:praxis`; this entry point exists for ad-hoc runs and for
 * recording notification fixtures:
 *
 *   node praxis/lean-lsp-harness.mjs [--record praxis/recordings/out.json]
 *
 * Env: TURNSTILE_LSP_CMD — lean binary (default: ~/.elan/bin/lean)
 */
import { writeFileSync } from "node:fs";
import { findLean, runSqrt2Scenario } from "./lean-lsp.mjs";

const leanBin = findLean();
if (!leanBin) {
  console.error("no lean binary found — set TURNSTILE_LSP_CMD or install elan");
  process.exit(1);
}

const recordPath = process.argv.includes("--record")
  ? process.argv[process.argv.indexOf("--record") + 1]
  : null;

const s = await runSqrt2Scenario(leanBin, { onLog: console.log });

console.log(`\nfileProgress events: ${s.progressEvents.length}`);
console.log(`diagnostics events:  ${s.diagnosticsEvents.length}`);
console.log(`goal at sorry (char ${s.goalChar}):`);
console.log(`  ${s.goalAtSorry?.goals?.[0]?.split("\n").pop() ?? "(none)"}`);
console.log(`final diagnostics:   ${s.finalDiags.diagnostics.length}`);
console.log(
  `goals at EOF:        ${JSON.stringify(s.goalAfter?.goals ?? null)}`,
);

if (recordPath) {
  writeFileSync(recordPath, JSON.stringify(s.record, null, 2));
  console.log(`\nrecorded ${s.record.length} notifications to ${recordPath}`);
}
