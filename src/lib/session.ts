import { get } from "svelte/store";
import { invoke } from "@tauri-apps/api/core";
import {
  cursorPosition,
  proofSource,
  proseHash,
  proseText,
  wordWrap,
} from "./appState";

/** Session metadata sent to the backend on save (mirrors session::Meta). */
export interface Meta {
  format_version: number;
  created_at: string;
  saved_at: string;
  cursor_line: number;
  cursor_col: number;
  editor_scroll_top: number;
  assistant_width_pct: number;
  proof_view: string | null;
  goal_panel_pct: number | null;
  word_wrap?: boolean;
}

export interface UiLayout {
  assistantWidthPct: number;
  proofView: string;
  goalPanelPct: number;
}

function buildMeta(layout: UiLayout): Meta {
  const cursor = get(cursorPosition);
  return {
    // created_at/saved_at/format_version are filled in by the backend.
    format_version: 0,
    created_at: "",
    saved_at: "",
    cursor_line: cursor.line,
    cursor_col: cursor.col,
    editor_scroll_top: 0,
    assistant_width_pct: layout.assistantWidthPct,
    proof_view: layout.proofView,
    goal_panel_pct: layout.goalPanelPct,
    word_wrap: get(wordWrap),
  };
}

function saveArgs(layout: UiLayout) {
  return {
    proofLean: get(proofSource),
    proseText: get(proseText),
    proseHash: get(proseHash),
    meta: buildMeta(layout),
    suggestedName: null as string | null,
  };
}

export async function saveSession(layout: UiLayout): Promise<void> {
  await invoke("save_session", saveArgs(layout));
}

export async function saveSessionAs(layout: UiLayout): Promise<void> {
  await invoke("save_session_as", saveArgs(layout));
}

export async function autoSaveSession(layout: UiLayout): Promise<void> {
  const args: Record<string, unknown> = { ...saveArgs(layout) };
  delete args.suggestedName;
  await invoke("auto_save_session", args);
}

export async function newSession(): Promise<void> {
  await invoke("new_session");
}

export async function openSession(): Promise<void> {
  await invoke("open_session", { path: null });
}

/** Title-bar helper: reflect the most recently saved/opened session file. */
export async function refreshWindowTitle(): Promise<void> {
  try {
    const last = await invoke<string | null>("get_last_session");
    const name = last
      ?.split("/")
      .pop()
      ?.replace(/\.turn$/, "");
    await invoke("set_window_title", {
      title: name ? `Turnstile — ${name}` : "Turnstile",
    });
  } catch {
    // Window title is cosmetic; ignore failures.
  }
}
