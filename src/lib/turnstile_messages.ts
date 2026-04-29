import { writable } from "svelte/store";
import { listen } from "@tauri-apps/api/event";
import type { TurnstileMessage } from "./TurnstileMessage";
import type { Annotation } from "./Annotation";
import type { FileProgressRange } from "./FileProgressRange";
import type { LspStatus } from "./LspStatus";

// Stores for each stream a component cares about.

/** Current file progress ranges (lines being elaborated). */
export const fileProgress = writable<FileProgressRange[]>([]);

/** Current annotations (semantic tokens + diagnostics). */
export const annotations = writable<Annotation[]>([]);

/** Current LSP server status, or null before first status message. */
export const lspStatus = writable<LspStatus | null>(null);

/** Current goal state text (the `.full` string from GoalStateInfo). */
export const goalState = writable<string>("");

/** Counter incremented on each ElaborationDone message so components can react. */
export const elaborationDone = writable<number>(0);

/** Latest show-message notification, or null when none pending. */
export const showMessage = writable<{
  severity: string;
  message: string;
} | null>(null);

/**
 * Start listening for `"turnstile-message"` Tauri events and dispatch
 * each message to the appropriate store.
 *
 * Call this once from the app root (e.g. `+layout.svelte` onMount).
 */
export async function startMessageListener(): Promise<() => void> {
  return listen<TurnstileMessage>("turnstile-message", (e) => {
    const msg = e.payload;
    switch (msg.type) {
      case "fileProgress":
        fileProgress.set(msg as unknown as FileProgressRange[]);
        break;
      case "annotationsUpdated":
        annotations.set(msg as unknown as Annotation[]);
        break;
      case "lspStatus":
        lspStatus.set({ state: msg.state, message: msg.message });
        break;
      case "goalStateUpdated":
        goalState.set(msg.full);
        break;
      case "elaborationDone":
        elaborationDone.update((n) => n + 1);
        break;
      case "showMessage":
        showMessage.set({ severity: msg.severity, message: msg.message });
        break;
      case "diagnostics":
        console.warn("unhandled TurnstileMessage", msg);
        break;
      case "semanticTokenRefresh":
        console.warn("unhandled TurnstileMessage", msg);
        break;
      case "semanticTokens":
        console.warn(
          "unhandled TurnstileMessage (no frontend consumer yet)",
          msg,
        );
        break;
      default: {
        // Exhaustiveness check
        const _exhaustive: never = msg;
        console.warn("unknown TurnstileMessage", _exhaustive);
      }
    }
  });
}
