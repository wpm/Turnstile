<script lang="ts">
  import ProofViewToggle from "$lib/ProofViewToggle.svelte";
  import ThemeToggle from "$lib/ThemeToggle.svelte";
  import FormalProof from "$lib/FormalProof.svelte";
  import GoalState from "$lib/GoalState.svelte";
  import ProseProof from "$lib/ProseProof.svelte";
  import Assistant from "$lib/Assistant.svelte";
  import Divider from "$lib/Divider.svelte";
  import Toast, { type ToastItem } from "$lib/Toast.svelte";
  import type { ProofView } from "$lib/types";
  import { onMount } from "svelte";
  import { SvelteMap } from "svelte/reactivity";
  import { listen } from "@tauri-apps/api/event";
  import { invoke } from "@tauri-apps/api/core";

  let proofView = $state<ProofView>("formal");
  let dark = $state(false);
  let goalText = $state("");
  let proseText = $state("");
  let lspReady = $state(false);

  let toasts = $state<ToastItem[]>([]);
  let toastSeq = 0;
  const AUTO_DISMISS_MS = 5000;
  const MAX_ERROR_TOASTS = 5;
  const toastTimers = new SvelteMap<number, ReturnType<typeof setTimeout>>();

  /* eslint-disable @typescript-eslint/no-unsafe-member-access, @typescript-eslint/no-unsafe-assignment -- $state<T[]> proxy loses generic in ESLint Svelte plugin */
  function addToast(severity: ToastItem["severity"], message: string) {
    const id = ++toastSeq;
    if (severity === "error") {
      const errorCount = toasts.filter((t) => t.severity === "error").length;
      if (errorCount >= MAX_ERROR_TOASTS) {
        const oldest = toasts.find((t) => t.severity === "error");
        if (oldest) toasts = toasts.filter((t) => t.id !== oldest.id);
      }
    }
    toasts = [...toasts, { id, severity, message }];
    if (severity !== "error") {
      toastTimers.set(
        id,
        setTimeout(() => {
          dismissToast(id);
        }, AUTO_DISMISS_MS),
      );
    }
  }

  function dismissToast(id: number) {
    const timer = toastTimers.get(id);
    if (timer !== undefined) {
      clearTimeout(timer);
      toastTimers.delete(id);
    }
    toasts = toasts.filter((t) => t.id !== id);
  }
  /* eslint-enable @typescript-eslint/no-unsafe-member-access, @typescript-eslint/no-unsafe-assignment */

  onMount(() => {
    let unlistenGoal: (() => void) | undefined;
    let unlistenProse: (() => void) | undefined;
    let unlistenLsp: (() => void) | undefined;
    let unlistenShowMessage: (() => void) | undefined;

    const setup = Promise.all([
      listen<string>("goal-state-updated", (e) => {
        goalText = e.payload;
      }),
      listen<{ text: string; hash: string | null }>("prose-updated", (e) => {
        proseText = e.payload.text;
      }),
      listen<{ state: string; message: string }>("lsp-status", (e) => {
        lspReady = e.payload.state === "connected";
      }),
      listen<{ severity: string; message: string }>("lsp-show-message", (e) => {
        addToast(
          e.payload.severity as ToastItem["severity"],
          e.payload.message,
        );
      }),
    ]).then(([g, p, l, s]) => {
      unlistenGoal = g;
      unlistenProse = p;
      unlistenLsp = l;
      unlistenShowMessage = s;
      // Sync initial state: the lsp-status event may have fired before our
      // listener was registered, so explicitly poll the current state.
      void invoke<boolean>("get_lsp_ready").then((ready) => {
        lspReady = ready;
      });
    });
    void setup;

    return () => {
      unlistenGoal?.();
      unlistenProse?.();
      unlistenLsp?.();
      unlistenShowMessage?.();
      for (const timer of toastTimers.values()) clearTimeout(timer);
      toastTimers.clear();
    };
  });

  function toggleTheme() {
    dark = !dark;
    document.documentElement.classList.toggle("dark", dark);
  }

  let leftWidth = $state(50); // percent
  let topHeight = $state(50); // percentage of left column

  let draggingVertical = $state(false);
  let draggingHorizontal = $state(false);
  let dragRect: DOMRect | null = null;
  let containerEl: HTMLDivElement;

  function onVerticalDragStart(e: MouseEvent) {
    draggingVertical = true;
    dragRect = containerEl.getBoundingClientRect();
    e.preventDefault();
  }

  function onHorizontalDragStart(e: MouseEvent) {
    draggingHorizontal = true;
    dragRect = containerEl.getBoundingClientRect();
    e.preventDefault();
  }

  function onMouseMove(e: MouseEvent) {
    if (!dragRect) return;
    if (draggingVertical) {
      leftWidth = Math.min(
        80,
        Math.max(20, ((e.clientX - dragRect.left) / dragRect.width) * 100),
      );
    }
    if (draggingHorizontal) {
      topHeight = Math.min(
        85,
        Math.max(15, ((e.clientY - dragRect.top) / dragRect.height) * 100),
      );
    }
  }

  function onMouseUp() {
    draggingVertical = false;
    draggingHorizontal = false;
    dragRect = null;
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="app-container"
  bind:this={containerEl}
  onmousemove={onMouseMove}
  onmouseup={onMouseUp}
  onmouseleave={onMouseUp}
>
  <div class="column" style="width: {leftWidth}%">
    <div class="panel" style="height: {topHeight}%">
      <div class="panel-header">Formal Proof</div>
      <div class="panel-content">
        <FormalProof {dark} {lspReady} />
      </div>
    </div>

    <Divider orientation="horizontal" onDragStart={onHorizontalDragStart} />

    <div class="panel" style="height: {100 - topHeight}%">
      <div class="panel-header">
        {proofView === "formal" ? "Goal State" : "Prose Proof"}
        <ProofViewToggle
          view={proofView}
          onToggle={() =>
            (proofView = proofView === "formal" ? "prose" : "formal")}
        />
      </div>
      <div class="panel-content">
        {#if proofView === "formal"}
          <GoalState content={goalText} />
        {:else}
          <ProseProof content={proseText} />
        {/if}
      </div>
    </div>
  </div>

  <Divider orientation="vertical" onDragStart={onVerticalDragStart} />

  <div class="column" style="width: {100 - leftWidth}%">
    <div class="panel-header">
      Proof Assistant
      <ThemeToggle {dark} onToggle={toggleTheme} />
    </div>
    <div class="panel-content">
      <Assistant />
    </div>
  </div>

  <Toast {toasts} onDismiss={dismissToast} />
</div>

<style>
  .app-container {
    display: flex;
    flex-direction: row;
    width: 100vw;
    height: 100vh;
    overflow: hidden;
    background: var(--color-bg);
    color: var(--color-text);
    font-family: system-ui, sans-serif;
    user-select: none;
  }

  .column {
    display: flex;
    flex-direction: column;
    min-width: 0;
    overflow: hidden;
  }

  .panel {
    display: flex;
    flex-direction: column;
    min-height: 0;
    overflow: hidden;
  }

  .panel-header {
    height: 2.75rem;
    padding: 0 1rem;
    font-size: 0.75rem;
    font-weight: 600;
    letter-spacing: 0.05em;
    text-transform: uppercase;
    color: var(--color-text-muted);
    background: var(--color-header-bg);
    border-bottom: 1px solid var(--color-border);
    flex-shrink: 0;
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  .panel-content {
    flex: 1;
    overflow: hidden;
    display: flex;
    flex-direction: column;
  }
</style>
