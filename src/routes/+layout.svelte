<script lang="ts">
  import "../app.css";
  import { onMount, onDestroy } from "svelte";
  import {
    startMessageListener,
    lspStatus,
    assistantStatus,
  } from "$lib/turnstile_messages";
  import { invoke } from "@tauri-apps/api/core";
  import type { LspStatus } from "$lib/LspStatus";
  import type { AssistantStatus } from "$lib/AssistantStatus";
  import type { Snippet } from "svelte";
  const { children }: { children: Snippet } = $props();

  let unlistenMessages: (() => void) | undefined;

  onMount(async () => {
    // Register the listener first, then recover any status the backend may
    // have emitted before we subscribed. `start_lsp` can reach "connected"
    // before this listener exists (e.g. when Mathlib is already built), and
    // Tauri drops events that have no registered listener — so without this
    // seed the UI stays stuck on "Starting…" and the editor stays read-only.
    unlistenMessages = await startMessageListener();
    try {
      const status = await invoke<LspStatus | null>("get_lsp_status");
      // Don't clobber a status that arrived via the live listener in the gap.
      if (status) lspStatus.update((current) => current ?? status);
    } catch {
      // No backend (e.g. browser/dev without Tauri): listener-driven only.
    }
    try {
      // Recover the assistant status the backend emits on startup, which is
      // dropped if it fires before this listener exists — same race as the LSP
      // status above. Without this seed a fresh launch with a missing/bad key
      // would show no disconnected indicator or toast.
      const status = await invoke<AssistantStatus | null>(
        "get_assistant_status",
      );
      if (status) assistantStatus.update((current) => current ?? status);
    } catch {
      // No backend (browser/dev without Tauri): listener-driven only.
    }
  });

  onDestroy(() => {
    unlistenMessages?.();
  });
</script>

{@render children()}
