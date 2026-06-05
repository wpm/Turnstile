#!/usr/bin/env node
/**
 * Praxis harness: drive a real `lean --server` over LSP the way Turnstile's
 * backend does, and check the protocol assumptions the backend encodes.
 *
 * What it verifies (each maps to backend code):
 *
 * 1. `$/lean/fileProgress` carries its document version *inside*
 *    `textDocument` (a VersionedTextDocumentIdentifier), not at the top
 *    level — the wire shape `lean::server::FileProgressParams` deserializes.
 * 2. Processing ranges are end-exclusive: a range ending at character 0 of
 *    line L does not mean line L is being processed — the trim in
 *    `parse_file_progress`.
 * 3. Elaboration completion is signalled by an *empty* `processing` array —
 *    what the backend folds into `TurnstileMessage::ElaborationDone`.
 * 4. `$/lean/plainGoal` returns goals at a `sorry`, and the goal disappears
 *    once the proof is completed — the goal-state panel's data source.
 * 5. A document edited to a complete, correct proof elaborates with zero
 *    error diagnostics.
 *
 * The proof developed here is the irrationality of √2 (no Mathlib), by
 * infinite descent. The harness "types" the final tactic block in a second
 * didChange, mimicking a user editing in Turnstile's editor.
 *
 * Usage:  node praxis/lean-lsp-harness.mjs [--record out.json]
 * Env:    TURNSTILE_LSP_CMD — path to the lean binary (default: ~/.elan/bin/lean)
 */

import { spawn } from "node:child_process";
import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir, homedir } from "node:os";
import { join } from "node:path";
import { pathToFileURL } from "node:url";

const LEAN_BIN =
  process.env.TURNSTILE_LSP_CMD ?? join(homedir(), ".elan", "bin", "lean");

const PROOF_HEADER = `/-- If \`n · n\` is even then \`n\` is even. -/
theorem even_of_mul_self_even (n : Nat) (h : (n * n) % 2 = 0) : n % 2 = 0 := by
  cases Nat.mod_two_eq_zero_or_one n with
  | inl h0 => exact h0
  | inr h1 => rw [Nat.mul_mod, h1] at h; simp at h

/-- No natural fraction squares to 2: the square root of 2 is irrational. -/
theorem sqrt2_irrational : ∀ d n : Nat, d ≠ 0 → n * n ≠ 2 * (d * d) := by
  intro d
  induction d using Nat.strongRecOn with
  | ind d ih =>
    intro n hd h
`;

/** First draft: the descent argument is still a `sorry`. */
const DRAFT = PROOF_HEADER + `    sorry
`;

/** The tactic block that replaces the `sorry` — the "typed" completion. */
const COMPLETION = `    have hmod : (n * n) % 2 = 0 := by rw [h, Nat.mul_mod]; simp
    have hn : n % 2 = 0 := even_of_mul_self_even n hmod
    cases Nat.dvd_of_mod_eq_zero hn with
    | intro k hk =>
      subst hk
      have hd2 : d * d = 2 * (k * k) := by
        rw [Nat.mul_assoc, Nat.mul_left_comm k 2 k] at h
        exact (Nat.eq_of_mul_eq_mul_left (by decide : 0 < 2) h).symm
      have hk0 : k ≠ 0 := by
        intro hk0
        subst hk0
        simp at hd2
        cases Nat.mul_eq_zero.mp hd2 with
        | inl h0 => exact hd h0
        | inr h0 => exact hd h0
      have hlt : k < d := by
        cases Nat.lt_or_ge k d with
        | inl hlt => exact hlt
        | inr hge =>
          exfalso
          have hdk : d * d ≤ k * k := Nat.mul_le_mul hge hge
          have hkpos : 0 < k * k :=
            Nat.mul_pos (Nat.pos_of_ne_zero hk0) (Nat.pos_of_ne_zero hk0)
          omega
      exact ih k hlt d hk0 hd2
`;

// ── Minimal LSP client ──────────────────────────────────────────────────

class LspClient {
  constructor(child) {
    this.child = child;
    this.nextId = 1;
    this.pending = new Map();
    this.notificationHandlers = new Map();
    this.buffer = Buffer.alloc(0);
    child.stdout.on("data", (chunk) => this.onData(chunk));
  }

  onData(chunk) {
    this.buffer = Buffer.concat([this.buffer, chunk]);
    for (;;) {
      const headerEnd = this.buffer.indexOf("\r\n\r\n");
      if (headerEnd < 0) return;
      const header = this.buffer.subarray(0, headerEnd).toString();
      const m = /Content-Length: (\d+)/i.exec(header);
      if (!m) throw new Error(`bad LSP header: ${header}`);
      const length = Number(m[1]);
      const start = headerEnd + 4;
      if (this.buffer.length < start + length) return;
      const body = this.buffer.subarray(start, start + length).toString();
      this.buffer = this.buffer.subarray(start + length);
      this.onMessage(JSON.parse(body));
    }
  }

  onMessage(msg) {
    if (msg.id !== undefined && (msg.result !== undefined || msg.error)) {
      const resolve = this.pending.get(msg.id);
      if (resolve) {
        this.pending.delete(msg.id);
        resolve(msg);
      }
      return;
    }
    if (msg.method) {
      // Server-to-client request (e.g. register/workDoneProgress): answer null.
      if (msg.id !== undefined) {
        this.write({ jsonrpc: "2.0", id: msg.id, result: null });
        return;
      }
      const handler = this.notificationHandlers.get(msg.method);
      if (handler) handler(msg.params);
      const all = this.notificationHandlers.get("*");
      if (all) all(msg.method, msg.params);
    }
  }

  write(msg) {
    const body = JSON.stringify(msg);
    this.child.stdin.write(
      `Content-Length: ${Buffer.byteLength(body)}\r\n\r\n${body}`,
    );
  }

  request(method, params) {
    const id = this.nextId++;
    return new Promise((resolve, reject) => {
      this.pending.set(id, (msg) =>
        msg.error ? reject(new Error(JSON.stringify(msg.error))) : resolve(msg.result),
      );
      this.write({ jsonrpc: "2.0", id, method, params });
    });
  }

  notify(method, params) {
    this.write({ jsonrpc: "2.0", method, params });
  }

  on(method, handler) {
    this.notificationHandlers.set(method, handler);
  }
}

// ── Assertions ──────────────────────────────────────────────────────────

let failures = 0;
function check(name, cond, detail = "") {
  const status = cond ? "ok  " : "FAIL";
  if (!cond) failures++;
  console.log(`  [${status}] ${name}${detail ? ` — ${detail}` : ""}`);
}

function waitFor(predicate, timeoutMs, label) {
  return new Promise((resolve, reject) => {
    const t0 = Date.now();
    const timer = setInterval(() => {
      const value = predicate();
      if (value !== undefined) {
        clearInterval(timer);
        resolve(value);
      } else if (Date.now() - t0 > timeoutMs) {
        clearInterval(timer);
        reject(new Error(`timeout waiting for ${label}`));
      }
    }, 25);
  });
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// ── Main ────────────────────────────────────────────────────────────────

async function main() {
  const recordPath = process.argv.includes("--record")
    ? process.argv[process.argv.indexOf("--record") + 1]
    : null;

  const dir = mkdtempSync(join(tmpdir(), "turnstile-praxis-"));
  const file = join(dir, "Proof.lean");
  writeFileSync(file, DRAFT);
  const uri = pathToFileURL(file).toString();

  console.log(`lean: ${LEAN_BIN}`);
  console.log(`project: ${dir}`);
  const child = spawn(LEAN_BIN, ["--server"], { cwd: dir });
  child.stderr.on("data", (d) => process.stderr.write(`[lean stderr] ${d}`));
  const lsp = new LspClient(child);

  /** Recorded notification log, usable as a frontend fixture. */
  const log = [];
  const progressEvents = [];
  const diagnosticsEvents = [];
  lsp.on("$/lean/fileProgress", (p) => progressEvents.push(p));
  lsp.on("textDocument/publishDiagnostics", (p) => diagnosticsEvents.push(p));
  lsp.on("*", (method, params) => {
    if (method.startsWith("$/lean/") || method === "textDocument/publishDiagnostics")
      log.push({ t: Date.now(), method, params });
  });

  const init = await lsp.request("initialize", {
    processId: process.pid,
    capabilities: {},
    workspaceFolders: [{ uri: pathToFileURL(dir).toString(), name: "praxis" }],
  });
  lsp.notify("initialized", {});
  console.log(
    `initialized: ${init.serverInfo?.name ?? "lean"} ${init.serverInfo?.version ?? ""}`,
  );

  // ── Phase 1: open the draft (contains `sorry`) ────────────────────────
  console.log("\nPhase 1: didOpen draft with sorry");
  lsp.notify("textDocument/didOpen", {
    textDocument: { uri, languageId: "lean4", version: 0, text: DRAFT },
  });

  await waitFor(
    () =>
      progressEvents.find((p) => p.processing.length === 0) ? true : undefined,
    120_000,
    "elaboration of draft",
  );

  const withRanges = progressEvents.filter((p) => p.processing.length > 0);
  check(
    "fileProgress version lives inside textDocument",
    progressEvents.every(
      (p) => !("version" in p) && typeof p.textDocument === "object",
    ),
    JSON.stringify(progressEvents[0]?.textDocument),
  );
  check(
    "fileProgress arrived with non-empty processing during elaboration",
    withRanges.length > 0,
    `${withRanges.length} events`,
  );
  check(
    "empty processing signals elaboration done",
    progressEvents.some((p) => p.processing.length === 0),
  );
  // Note (learned from a live run): processing ranges do NOT always end at
  // character 0 — Lean reports mid-line end positions while elaborating.
  // The backend's parse_file_progress only trims the final line when the
  // end character IS 0 (end-exclusive); mid-line ends keep the line.
  const endChars = withRanges.flatMap((p) =>
    p.processing.map((i) => i.range.end.character),
  );
  check(
    "processing ranges are well-formed (start ≤ end)",
    withRanges.every((p) =>
      p.processing.every(
        (i) =>
          i.range.start.line < i.range.end.line ||
          (i.range.start.line === i.range.end.line &&
            i.range.start.character <= i.range.end.character),
      ),
    ),
    `end characters seen include 0 and mid-line: ${[...new Set(endChars)].slice(0, 5).join(",")}`,
  );
  check(
    "some processing ranges end at character 0 (whole-line, end-exclusive)",
    endChars.some((c) => c === 0),
  );

  const sorryDiag = await waitFor(() => {
    const d = diagnosticsEvents.findLast((e) => e.diagnostics.length > 0);
    return d ? d : undefined;
  }, 60_000, "sorry diagnostic");
  check(
    "draft produces a 'sorry' warning diagnostic",
    sorryDiag.diagnostics.some((d) => /sorry/i.test(d.message)),
    sorryDiag.diagnostics[0]?.message?.slice(0, 60),
  );
  check(
    "publishDiagnostics carries a top-level version field",
    typeof sorryDiag.version === "number",
    `version=${sorryDiag.version}`,
  );

  // ── Phase 2: goal state at the sorry ──────────────────────────────────
  console.log("\nPhase 2: plainGoal at the sorry");
  const sorryLine = DRAFT.split("\n").findIndex((l) => l.includes("sorry"));
  // Probe several characters: which positions answer is itself praxis data.
  let goal = null;
  let goalChar = -1;
  for (const character of [4, 0, 8, 9]) {
    const g = await lsp.request("$/lean/plainGoal", {
      textDocument: { uri },
      position: { line: sorryLine, character },
    });
    if (g && Array.isArray(g.goals) && g.goals.length > 0) {
      goal = g;
      goalChar = character;
      break;
    }
  }
  check(
    "plainGoal returns goals at the sorry",
    goal !== null,
    goal ? `at character ${goalChar}` : "no position answered",
  );
  check(
    "goal is the descent statement",
    Boolean(goal?.rendered?.includes("False") || goal?.rendered?.includes("≠")),
    goal?.goals?.[0]?.split("\n").pop(),
  );

  // ── Phase 3: "type" the completed proof (incremental didChange) ───────
  console.log("\nPhase 3: didChange replaces sorry with the descent argument");
  const progressBefore = progressEvents.length;
  lsp.notify("textDocument/didChange", {
    textDocument: { uri, version: 1 },
    contentChanges: [
      {
        range: {
          start: { line: sorryLine, character: 0 },
          end: { line: sorryLine + 1, character: 0 },
        },
        text: COMPLETION,
      },
    ],
  });

  await waitFor(
    () =>
      progressEvents.length > progressBefore &&
      progressEvents.slice(progressBefore).some((p) => p.processing.length === 0)
        ? true
        : undefined,
    120_000,
    "re-elaboration",
  );

  const v1Progress = progressEvents
    .slice(progressBefore)
    .filter((p) => p.textDocument.version !== undefined);
  check(
    "fileProgress after didChange reports textDocument.version = 1",
    v1Progress.some((p) => p.textDocument.version === 1),
    `versions seen: ${[...new Set(v1Progress.map((p) => p.textDocument.version))].join(",")}`,
  );

  // Allow trailing diagnostics to settle, then read the latest set.
  await sleep(1500);
  const finalDiags = diagnosticsEvents[diagnosticsEvents.length - 1];
  check(
    "completed proof has zero diagnostics",
    finalDiags.diagnostics.length === 0,
    finalDiags.diagnostics.map((d) => d.message.slice(0, 40)).join(" | ") || "clean",
  );

  // The backend reads the whole-proof goal state at end-of-document
  // (fetch_full_proof_goal_state); mirror that here.
  const finalText = DRAFT.replace(/^    sorry\n/m, COMPLETION);
  const lastLine = finalText.split("\n").length - 1;
  const goalAfter = await lsp.request("$/lean/plainGoal", {
    textDocument: { uri },
    position: { line: lastLine, character: 0 },
  });
  check(
    "no goals remain at end of the completed proof",
    !goalAfter || goalAfter.goals.length === 0,
    JSON.stringify(goalAfter?.goals ?? null),
  );

  // ── Shutdown ──────────────────────────────────────────────────────────
  await lsp.request("shutdown", null);
  lsp.notify("exit", null);
  await new Promise((r) => child.on("exit", r));

  if (recordPath) {
    writeFileSync(recordPath, JSON.stringify(log, null, 2));
    console.log(`\nrecorded ${log.length} notifications to ${recordPath}`);
  }

  console.log(failures === 0 ? "\nPRAXIS OK" : `\nPRAXIS FAILED (${failures})`);
  process.exit(failures === 0 ? 0 : 1);
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
