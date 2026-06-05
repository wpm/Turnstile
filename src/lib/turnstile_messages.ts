import { writable } from "svelte/store";
import { listen } from "@tauri-apps/api/event";
import type { TurnstileMessage } from "./TurnstileMessage";
import type { Annotation } from "./Annotation";
import type { FileProgressRange } from "./FileProgressRange";
import type { LspStatus } from "./LspStatus";

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

/** Register the single `"turnstile-message"` Tauri listener. Returns an unlisten function. */
export async function startMessageListener(): Promise<() => void> {
  return listen<TurnstileMessage>("turnstile-message", (e) => {
    const msg = e.payload;
    switch (msg.type) {
      case "fileProgress":
        fileProgress.set(msg.items);
        break;
      case "annotationsUpdated":
        annotations.set(msg.items);
        break;
      case "lspStatus":
        lspStatus.set({ state: msg.state, message: msg.message });
        break;
      case "goalStateUpdated":
        goalState.set(msg.full);
        break;
      case "elaborationDone":
        // Elaboration finished: no lines are being processed any more.
        // Lean signals this with an empty fileProgress, which the backend
        // folds into elaborationDone — so clear the highlight ranges here.
        fileProgress.set([]);
        elaborationDone.update((n) => n + 1);
        break;
      case "showMessage":
        showMessage.set({ severity: msg.severity, message: msg.message });
        break;
      case "diagnostics":
      case "semanticTokenRefresh":
      case "semanticTokens":
        // Backend-internal messages; the backend folds these into
        // annotationsUpdated before they reach the frontend.
        break;
      default: {
        const _exhaustive: never = msg;
        console.warn("unknown TurnstileMessage", _exhaustive);
      }
    }
  });
}
