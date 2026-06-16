/**
 * A minimal LSP client and the √2 scenario it drives against a real
 * `lean --server`. Checks the protocol contracts the Rust backend encodes
 * against the live server — theory says what the code should do; this checks
 * it against reality.
 *
 * Consumed two ways:
 * - `e2e/lean-server/sqrt2.test.mjs` — vitest suite (`pnpm test:lean-server`)
 * - `e2e/lean-server/lean-lsp-harness.mjs` — CLI with `--record` for
 *   capturing notification fixtures
 *
 * The scenario mirrors how the Rust backend drives Lean: didOpen a draft
 * proof of the irrationality of √2 (no Mathlib) ending in `sorry`, read
 * the goal at the sorry via `$/lean/plainGoal`, replace the sorry with the
 * completed descent argument via incremental didChange, re-elaborate.
 */

import { spawn, spawnSync } from "node:child_process";
import { existsSync, mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir, homedir } from "node:os";
import { join } from "node:path";
import { pathToFileURL } from "node:url";

export const PROOF_HEADER = `/-- If \`n · n\` is even then \`n\` is even. -/
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
export const DRAFT =
  PROOF_HEADER +
  `    sorry
`;

/** The tactic block that replaces the `sorry` — the "typed" completion. */
export const COMPLETION = `    have hmod : (n * n) % 2 = 0 := by rw [h, Nat.mul_mod]; simp
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

/**
 * Locate a working lean binary: $TURNSTILE_LSP_CMD, then ~/.elan/bin/lean.
 *
 * Candidates are probed with `lean --version` (10 s timeout) because
 * existence is not enough: the elan shim can be present with no toolchain
 * installed, in which case spawning it would hang the suite rather than
 * skip it.
 */
export function findLean() {
  const candidates = [
    process.env.TURNSTILE_LSP_CMD,
    join(homedir(), ".elan", "bin", "lean"),
  ].filter(Boolean);
  for (const c of candidates) {
    if (!existsSync(c)) continue;
    const probe = spawnSync(c, ["--version"], { timeout: 10_000 });
    if (probe.status === 0) return c;
  }
  return null;
}

// ── Minimal LSP client ──────────────────────────────────────────────────

export class LspClient {
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
      // Server-to-client request (e.g. workDoneProgress): answer null.
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
        msg.error
          ? reject(new Error(JSON.stringify(msg.error)))
          : resolve(msg.result),
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

// ── The scenario ────────────────────────────────────────────────────────

/**
 * Run the full √2 session against `leanBin` and return everything observed.
 * Pure data out; assertions live in the callers (vitest spec or CLI).
 */
export async function runSqrt2Scenario(leanBin, { onLog } = {}) {
  const dir = mkdtempSync(join(tmpdir(), "turnstile-lean-server-"));
  const file = join(dir, "Proof.lean");
  writeFileSync(file, DRAFT);
  const uri = pathToFileURL(file).toString();
  const log = (...a) => onLog?.(...a);

  log(`lean: ${leanBin}`);
  log(`project: ${dir}`);
  const child = spawn(leanBin, ["--server"], { cwd: dir });
  const lsp = new LspClient(child);

  const record = [];
  const progressEvents = [];
  const diagnosticsEvents = [];
  lsp.on("$/lean/fileProgress", (p) => progressEvents.push(p));
  lsp.on("textDocument/publishDiagnostics", (p) => diagnosticsEvents.push(p));
  lsp.on("*", (method, params) => {
    if (
      method.startsWith("$/lean/") ||
      method === "textDocument/publishDiagnostics"
    )
      record.push({ t: Date.now(), method, params });
  });

  const initResult = await lsp.request("initialize", {
    processId: process.pid,
    capabilities: {},
    workspaceFolders: [
      { uri: pathToFileURL(dir).toString(), name: "turnstile-lean-server" },
    ],
  });
  lsp.notify("initialized", {});
  log(`initialized: ${initResult.serverInfo?.name ?? "lean"}`);

  // Phase 1: open the draft (contains `sorry`).
  log("phase 1: didOpen draft with sorry");
  lsp.notify("textDocument/didOpen", {
    textDocument: { uri, languageId: "lean4", version: 0, text: DRAFT },
  });
  await waitFor(
    () =>
      progressEvents.find((p) => p.processing.length === 0) ? true : undefined,
    120_000,
    "elaboration of draft",
  );
  const sorryDiag = await waitFor(
    () => {
      const d = diagnosticsEvents.findLast((e) => e.diagnostics.length > 0);
      return d ?? undefined;
    },
    60_000,
    "sorry diagnostic",
  );

  // Phase 2: goal at the sorry. Probe several characters — which positions
  // answer is itself an observation about the server's behavior.
  log("phase 2: plainGoal at the sorry");
  const sorryLine = DRAFT.split("\n").findIndex((l) => l.includes("sorry"));
  let goalAtSorry = null;
  let goalChar = -1;
  for (const character of [4, 0, 8, 9]) {
    const g = await lsp.request("$/lean/plainGoal", {
      textDocument: { uri },
      position: { line: sorryLine, character },
    });
    if (g && Array.isArray(g.goals) && g.goals.length > 0) {
      goalAtSorry = g;
      goalChar = character;
      break;
    }
  }

  // Phase 3: "type" the completed proof (incremental didChange).
  log("phase 3: didChange replaces sorry with the descent argument");
  const progressBefore = progressEvents.length;
  const diagsBefore = diagnosticsEvents.length;
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
      progressEvents
        .slice(progressBefore)
        .some((p) => p.processing.length === 0)
        ? true
        : undefined,
    120_000,
    "re-elaboration",
  );
  // Observed against the live server: final diagnostics may arrive AFTER the
  // empty fileProgress (~1.5 s late) — wait for them, don't assume.
  await waitFor(
    () =>
      diagnosticsEvents.length > diagsBefore &&
      diagnosticsEvents[diagnosticsEvents.length - 1].version === 1
        ? true
        : undefined,
    60_000,
    "post-edit diagnostics",
  );
  await sleep(1_500);

  const finalText = DRAFT.replace(/^ {4}sorry\n/m, COMPLETION);
  const lastLine = finalText.split("\n").length - 1;
  const goalAfter = await lsp.request("$/lean/plainGoal", {
    textDocument: { uri },
    position: { line: lastLine, character: 0 },
  });

  await lsp.request("shutdown", null);
  lsp.notify("exit", null);
  await new Promise((r) => child.on("exit", r));

  return {
    initResult,
    progressEvents,
    progressBefore,
    diagnosticsEvents,
    sorryDiag,
    goalAtSorry,
    goalChar,
    goalAfter,
    finalDiags: diagnosticsEvents[diagnosticsEvents.length - 1],
    sorryLine,
    record,
  };
}
