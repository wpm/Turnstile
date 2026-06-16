#!/usr/bin/env node
/**
 * CLI wrapper around the √2 scenario (see lean-lsp.mjs). The checks
 * themselves live in sqrt2.test.mjs and run under vitest via
 * `pnpm test:lean-server`; this entry point exists for ad-hoc runs and for
 * recording notification fixtures:
 *
 *   node e2e/lean-server/lean-lsp-harness.mjs \
 *     [--record e2e/lean-server/recordings/out.json]
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
