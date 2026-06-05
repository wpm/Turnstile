/**
 * A faithful-enough in-browser simulation of the Turnstile Rust backend.
 *
 * Activated by running Vite in the `e2e` mode (see vite.config.js), which
 * aliases `@tauri-apps/api/core` and `@tauri-apps/api/event` to the thin
 * shims in this directory. The rest of the frontend runs unmodified, which
 * is the point: Playwright drives the real UI against this backend.
 *
 * The simulation mirrors the contracts documented in src-tauri:
 * - `update_document` applies LSP content changes to a document store,
 *   then "elaborates": fileProgress → annotations → elaborationDone →
 *   goalStateUpdated, on the turnstile-message event, exactly as
 *   dispatch_turnstile_message does.
 * - `send_message` streams assistant-delta events and returns the reply.
 * - Settings, transcript, and session commands keep in-memory state.
 *
 * Tests can reach the simulation through `window.__turnstileFake`.
 */

type Listener = (event: { payload: unknown }) => void;

const listeners = new Map<string, Set<Listener>>();

export function fakeListen(event: string, cb: Listener): () => void {
  let set = listeners.get(event);
  if (!set) {
    set = new Set();
    listeners.set(event, set);
  }
  set.add(cb);
  if (event === "turnstile-message" && !lspAnnounced) {
    lspAnnounced = true;
    setTimeout(() => {
      emit("turnstile-message", {
        type: "lspStatus",
        state: "connected",
        message: "connected",
      });
    }, 50);
  }
  return () => set.delete(cb);
}

export function emit(event: string, payload: unknown): void {
  for (const cb of listeners.get(event) ?? []) cb({ payload });
}

// ── Simulated state ─────────────────────────────────────────────────────

let lspAnnounced = false;
let source = "";
let goalState = "";
const transcript: {
  summary: string | null;
  turns: { role: string; content: string; spans: never[]; timestamp: number }[];
  max_tokens: number;
} = { summary: null, turns: [], max_tokens: 200_000 };

let settings = {
  editor_font_size: 13,
  goal_state_font_size: 13,
  prose_proof_font_size: 13,
  assistant_font_size: 13,
  assistant_model: null as string | null,
  translation_model: null as string | null,
  assistant_prompt: null as string | null,
  translation_prompt: null as string | null,
};

let elaborationTimer: ReturnType<typeof setTimeout> | null = null;
let elaborationSeq = 0;

/** How long simulated elaboration holds the progress highlight (ms). */
export const ELABORATION_MS = 300;

// ── Lean-ish elaboration ────────────────────────────────────────────────

interface LspPosition {
  line: number;
  character: number;
}
interface ContentChange {
  range: { start: LspPosition; end: LspPosition };
  text: string;
}

function posToOffset(doc: string, pos: LspPosition): number {
  const lines = doc.split("\n");
  let offset = 0;
  for (let i = 0; i < pos.line && i < lines.length; i++) {
    offset += lines[i].length + 1;
  }
  return Math.min(offset + pos.character, doc.length);
}

function applyChanges(doc: string, changes: ContentChange[]): string {
  const edits = changes
    .map((c) => ({
      start: posToOffset(doc, c.range.start),
      end: posToOffset(doc, c.range.end),
      text: c.text,
    }))
    .sort((a, b) => b.start - a.start);
  let out = doc;
  for (const e of edits) {
    out = out.slice(0, e.start) + e.text + out.slice(e.end);
  }
  return out;
}

const LEAN_KEYWORDS = new Set([
  "theorem",
  "lemma",
  "example",
  "by",
  "intro",
  "exact",
  "apply",
  "have",
  "show",
  "calc",
  "match",
  "with",
  "fun",
  "def",
  "import",
  "omega",
  "simp",
  "rcases",
  "obtain",
  "rfl",
  "sorry",
  "induction",
  "cases",
  "constructor",
]);

/** Token + diagnostic annotations a real Lean session would produce. */
function computeAnnotations(doc: string) {
  const annotations: object[] = [];
  doc.split("\n").forEach((lineText, i) => {
    const re = /[A-Za-z_][A-Za-z0-9_.']*/g;
    let m;
    while ((m = re.exec(lineText)) !== null) {
      if (LEAN_KEYWORDS.has(m[0])) {
        annotations.push({
          kind: "token",
          line: i + 1,
          col: m.index,
          length: m[0].length,
          tokenType: "keyword",
          modifiers: [],
        });
      }
    }
    const sorryIdx = lineText.indexOf("sorry");
    if (sorryIdx >= 0) {
      annotations.push({
        kind: "diagnostic",
        startLine: i + 1,
        startCol: sorryIdx,
        endLine: i + 1,
        endCol: sorryIdx + 5,
        severity: "warning",
        message: "declaration uses 'sorry'",
      });
    }
    const oopsIdx = lineText.indexOf("oops");
    if (oopsIdx >= 0) {
      annotations.push({
        kind: "diagnostic",
        startLine: i + 1,
        startCol: oopsIdx,
        endLine: i + 1,
        endCol: oopsIdx + 4,
        severity: "error",
        message: `unknown identifier 'oops'`,
      });
    }
  });
  return annotations;
}

function computeGoalState(doc: string): string {
  if (doc.trim() === "") return "";
  if (doc.includes("sorry")) {
    return "case h\nn d : ℕ\nhd : d ≠ 0\n⊢ n * n ≠ 2 * (d * d)";
  }
  if (doc.includes("oops")) return "";
  return "No goals";
}

/** Simulate Lean elaborating the current document. */
function elaborate(): void {
  const seq = ++elaborationSeq;
  if (elaborationTimer) clearTimeout(elaborationTimer);
  const lineCount = source.split("\n").length;

  // Progress covers the whole file while "elaborating".
  emit("turnstile-message", {
    type: "fileProgress",
    items: [{ start_line: 1, end_line: lineCount }],
  });

  elaborationTimer = setTimeout(() => {
    if (seq !== elaborationSeq) return;
    emit("turnstile-message", {
      type: "annotationsUpdated",
      items: computeAnnotations(source),
    });
    emit("turnstile-message", { type: "elaborationDone" });
    goalState = computeGoalState(source);
    emit("turnstile-message", {
      type: "goalStateUpdated",
      full: goalState,
    });
  }, ELABORATION_MS);
}

// ── Assistant ───────────────────────────────────────────────────────────

async function streamReply(reply: string): Promise<void> {
  for (const word of reply.split(/(?<=\s)/)) {
    emit("assistant-delta", word);
    await new Promise((r) => setTimeout(r, 5));
  }
}

async function sendMessage(content: string): Promise<string> {
  transcript.turns.push({
    role: "user",
    content,
    spans: [],
    timestamp: Date.now(),
  });
  let reply: string;
  if (/error/i.test(content) && content.includes("!fail")) {
    throw new Error("simulated LLM failure");
  }
  if (/goal/i.test(content)) {
    reply =
      goalState === ""
        ? "There is no goal state yet — the editor is empty."
        : `The current goal is:\n\n$$${goalState.split("⊢").pop()?.trim() ?? ""}$$\n\nWe can finish by infinite descent.`;
  } else {
    reply = `[echo] ${content}`;
  }
  await streamReply(reply);
  transcript.turns.push({
    role: "assistant",
    content: reply,
    spans: [],
    timestamp: Date.now(),
  });
  emit("assistant-stream-done", null);
  return reply;
}

// ── Sessions ────────────────────────────────────────────────────────────

const SAMPLE_SESSION = {
  proof_lean: [
    "theorem sqrt2_irrational (n d : Nat) (hd : d ≠ 0) :",
    "    n * n ≠ 2 * (d * d) := by",
    "  sorry",
  ].join("\n"),
  prose: {
    text: "<p><strong>Theorem.</strong> The square root of 2 is irrational.</p>",
    tactic_state_hash: null as string | null,
  },
  turns: [],
  summary: null as string | null,
};

// Survives page reloads so tests can exercise the recovery prompt.
let hasAutosave =
  typeof sessionStorage !== "undefined" &&
  sessionStorage.getItem("fake-autosave") === "1";

function setAutosave(v: boolean) {
  hasAutosave = v;
  if (typeof sessionStorage !== "undefined") {
    sessionStorage.setItem("fake-autosave", v ? "1" : "0");
  }
}

// ── Command dispatch ────────────────────────────────────────────────────

export async function fakeInvoke(
  cmd: string,
  args?: Record<string, unknown>,
): Promise<unknown> {
  switch (cmd) {
    case "update_document":
      source = applyChanges(
        source,
        (args?.changes as ContentChange[] | undefined) ?? [],
      );
      elaborate();
      return null;
    case "lsp_hover": {
      const doc = source.split("\n");
      const line = doc[(args?.line as number | undefined) ?? 0] ?? "";
      if (line.includes("theorem")) {
        return {
          contents:
            "```lean\ntheorem sqrt2_irrational (n d : Nat) (hd : d ≠ 0)\n```\n\nNo rational $p/q$ squares to $2$.",
          kind: "markdown",
        };
      }
      return null;
    }
    case "send_message":
      return sendMessage((args?.content as string | undefined) ?? "");
    case "get_transcript":
      return structuredClone(transcript);
    case "load_transcript":
      return null;
    case "get_settings":
      return { ...settings };
    case "save_settings":
      settings = { ...(args?.settings as typeof settings) };
      return null;
    case "get_default_assistant_prompt":
      return "You are a proof collaborator. (fake default)";
    case "get_default_translation_prompt":
      return "You are a mathematical writing assistant. (fake default)";
    case "get_available_models":
      return [
        { id: "claude-opus-4-6", display_name: "Claude Opus 4.6" },
        { id: "claude-sonnet-4-6", display_name: "Claude Sonnet 4.6" },
      ];
    case "new_session":
      source = "";
      transcript.turns = [];
      transcript.summary = null;
      emit("session-loaded", {
        proof_lean: "",
        prose: { text: "", tactic_state_hash: null },
        turns: [],
        summary: null,
      });
      return null;
    case "open_session":
      source = SAMPLE_SESSION.proof_lean;
      emit("session-loaded", SAMPLE_SESSION);
      return null;
    case "save_session":
    case "save_session_as":
    case "auto_save_session":
      return null;
    case "check_auto_save":
      return hasAutosave;
    case "restore_auto_save":
      setAutosave(false);
      source = SAMPLE_SESSION.proof_lean;
      emit("session-loaded", SAMPLE_SESSION);
      return null;
    case "delete_auto_save":
      setAutosave(false);
      return null;
    case "get_last_session":
      return null;
    case "set_last_session":
    case "set_window_title":
    case "set_menu_item_enabled":
      return null;
    default:
      throw new Error(`fake backend: unknown command ${cmd}`);
  }
}

// ── Test hooks ──────────────────────────────────────────────────────────

declare global {
  interface Window {
    __turnstileFake?: {
      emit: typeof emit;
      getSource: () => string;
      setAutosave: (v: boolean) => void;
    };
  }
}

if (typeof window !== "undefined") {
  window.__turnstileFake = {
    emit,
    getSource: () => source,
    setAutosave,
  };
}
