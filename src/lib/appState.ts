import { writable } from "svelte/store";
import { listen } from "@tauri-apps/api/event";

/**
 * Frontend mirrors of editor/proof state needed to assemble session saves,
 * plus app-level UI state (word wrap, setup progress, settings).
 */

/** Current Lean source as held by the editor. */
export const proofSource = writable<string>("");

/** Current prose proof text (KaTeX-rendered HTML for display lives elsewhere). */
export const proseText = writable<string>("");

/** Goal-state hash the current prose was generated from, if any. */
export const proseHash = writable<string | null>(null);

/** Editor cursor, 0-indexed, for session metadata. */
export const cursorPosition = writable<{ line: number; col: number }>({
  line: 0,
  col: 0,
});

/** Word wrap state for the editor (View → Toggle Word Wrap). */
export const wordWrap = writable<boolean>(false);

/** True while a prose-translation LLM call is in flight (drives the Prose
 * Proof busy indicator). */
export const proseGenerating = writable<boolean>(false);

/** Latest setup progress message, or null once setup is done. */
export interface SetupProgress {
  phase: string;
  message: string;
  progress_pct: number;
}
export const setupProgress = writable<SetupProgress | null>(null);

/** Persisted user settings (mirrors the backend Settings struct). */
export interface Settings {
  editor_font_size: number;
  goal_state_font_size: number;
  prose_proof_font_size: number;
  assistant_font_size: number;
  assistant_model: string | null;
  translation_model: string | null;
  assistant_prompt: string | null;
  translation_prompt: string | null;
}
export const settings = writable<Settings | null>(null);

/** Register the setup-progress listener. Returns an unlisten function. */
export async function startSetupProgressListener(): Promise<() => void> {
  return listen<SetupProgress>("setup-progress", (e) => {
    setupProgress.set(e.payload.phase === "ready" ? null : e.payload);
  });
}

/** Register the prose-generating listener. Returns an unlisten function. */
export async function startProseGeneratingListener(): Promise<() => void> {
  return listen<boolean>("prose-generating", (e) => {
    proseGenerating.set(e.payload);
  });
}
