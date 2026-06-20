<script lang="ts">
  import ProofViewToggle from "$lib/ProofViewToggle.svelte";
  import ThemeToggle from "$lib/ThemeToggle.svelte";
  import FormalProof from "$lib/FormalProof.svelte";
  import GoalState from "$lib/GoalState.svelte";
  import ProseProof from "$lib/ProseProof.svelte";
  import Assistant from "$lib/Assistant.svelte";
  import Divider from "$lib/Divider.svelte";
  import StatusBar from "$lib/StatusBar.svelte";
  import SettingsDialog from "$lib/SettingsDialog.svelte";
  import Toast, { type ToastItem } from "$lib/Toast.svelte";
  import type { ProofView } from "$lib/types";
  import { onMount } from "svelte";
  import { SvelteMap } from "svelte/reactivity";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import {
    goalState,
    lspStatus as lspStatusStore,
    showMessage,
    assistantStatus,
  } from "$lib/turnstile_messages";
  import type { DisconnectReason } from "$lib/DisconnectReason";
  import {
    proseGenerating,
    proseHash,
    proseText,
    settings as settingsStore,
    setupProgress,
    startProseGeneratingListener,
    startSetupProgressListener,
    wordWrap,
    type Settings,
  } from "$lib/appState";
  import {
    autoSaveSession,
    newSession,
    openSession,
    refreshWindowTitle,
    saveSession,
    saveSessionAs,
    type UiLayout,
  } from "$lib/session";

  let proofView = $state<ProofView>("prose");
  let dark = $state(false);
  let goalText = $state("");
  let prose = $state("");
  let lspReady = $state(false);
  let settingsOpen = $state(false);
  let firstRunOpen = $state(false);
  let restorePromptOpen = $state(false);

  let toasts = $state<ToastItem[]>([]);
  let toastSeq = 0;
  const AUTO_DISMISS_MS = 5000;
  const MAX_ERROR_TOASTS = 5;
  const AUTOSAVE_INTERVAL_MS = 30_000;
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

  /**
   * Toast copy for each disconnect reason — names the cause AND the fix, so the
   * failure is never vague. Driven by the backend `DisconnectReason` (#58).
   */
  function disconnectToastMessage(reason: DisconnectReason): string {
    switch (reason) {
      case "noKey":
        return "Proof Assistant unavailable — no API key set. Add one in Settings.";
      case "keyRejected":
        return "Proof Assistant unavailable — API key was rejected. Update it in Settings.";
      case "storeUnavailable":
        return "Proof Assistant unavailable — the OS secret store could not be reached. Set the ANTHROPIC_API_KEY environment variable.";
    }
  }

  function currentLayout(): UiLayout {
    return {
      // The Proof Assistant now occupies the bottom band at full width; persist
      // its height fraction. The Prose pane width fraction rides in goalPanelPct.
      assistantWidthPct: 100 - topHeight,
      proofView,
      goalPanelPct: 100 - formalWidth,
    };
  }

  async function handleMenuEvent(id: string) {
    try {
      switch (id) {
        case "new_session":
          await newSession();
          await refreshWindowTitle();
          break;
        case "open_session":
          await openSession();
          await refreshWindowTitle();
          break;
        case "save_session":
          await saveSession(currentLayout());
          await refreshWindowTitle();
          break;
        case "save_session_as":
          await saveSessionAs(currentLayout());
          await refreshWindowTitle();
          break;
        case "settings":
          settingsOpen = true;
          break;
        case "toggle_word_wrap":
          wordWrap.update((w) => !w);
          break;
        default:
          console.warn("unhandled menu event", id);
      }
    } catch (e) {
      addToast("error", String(e));
    }
  }

  async function restoreAutosave(restore: boolean) {
    restorePromptOpen = false;
    try {
      if (restore) {
        await invoke("restore_auto_save");
      } else {
        await invoke("delete_auto_save");
      }
    } catch (e) {
      addToast("error", String(e));
    }
  }

  onMount(() => {
    let unlistenProse: (() => void) | undefined;
    let unlistenSessionLoaded: (() => void) | undefined;
    let unlistenMenu: (() => void) | undefined;
    let unlistenSetup: (() => void) | undefined;
    let unlistenProseGenerating: (() => void) | undefined;

    const unsubscribeGoal = goalState.subscribe((text) => {
      goalText = text;
    });

    const unsubscribeLsp = lspStatusStore.subscribe((status) => {
      lspReady = status?.state === "connected";
    });

    const unsubscribeShowMessage = showMessage.subscribe((msg) => {
      if (msg === null) return;
      addToast(msg.severity as ToastItem["severity"], msg.message);
    });

    // Surface assistant disconnection as an error toast (which requires manual
    // dismissal). Only toast on a *transition* into a disconnected reason — not
    // on every store emit — so reconnecting and re-failing doesn't spam, but a
    // changed reason (e.g. noKey → keyRejected) does re-notify.
    let lastDisconnectReason: DisconnectReason | null = null;
    const unsubscribeAssistant = assistantStatus.subscribe((status) => {
      if (status === null) return;
      if (status.state === "disconnected") {
        if (status.reason !== lastDisconnectReason) {
          lastDisconnectReason = status.reason;
          addToast("error", disconnectToastMessage(status.reason));
        }
      } else {
        lastDisconnectReason = null;
      }
    });

    const unsubscribeSetup = setupProgress.subscribe((progress) => {
      if (progress?.phase === "error") {
        addToast("error", progress.message);
      }
    });

    const unsubscribeProse = proseText.subscribe((text) => {
      prose = text;
    });

    const setup = Promise.all([
      listen<{ text: string; hash: string | null }>("prose-updated", (e) => {
        proseText.set(e.payload.text);
        proseHash.set(e.payload.hash);
      }),
      listen<{ prose: { text: string; tactic_state_hash: string | null } }>(
        "session-loaded",
        (e) => {
          proseText.set(e.payload.prose.text);
          proseHash.set(e.payload.prose.tactic_state_hash);
        },
      ),
      listen<string>("menu-event", (e) => {
        void handleMenuEvent(e.payload);
      }),
      startSetupProgressListener(),
      startProseGeneratingListener(),
    ]).then(([p, sl, m, sp, pg]) => {
      unlistenProse = p;
      unlistenSessionLoaded = sl;
      unlistenMenu = m;
      unlistenSetup = sp;
      unlistenProseGenerating = pg;
    });
    void setup;

    // Load persisted settings; enable Save; offer autosave recovery.
    void (async () => {
      try {
        settingsStore.set(await invoke<Settings>("get_settings"));
      } catch {
        // Defaults apply when settings can't be loaded.
      }
      // Warn (once) if settings.json was present but unparseable at startup —
      // the backend reset it to defaults and backed up the bad file. Drained
      // by command rather than a startup emit so it can't fire before this
      // listener exists.
      try {
        const warning = await invoke<string | null>(
          "take_settings_load_warning",
        );
        if (warning) addToast("warning", warning);
      } catch {
        // No backend (browser/dev): nothing to drain.
      }
      void invoke("set_menu_item_enabled", {
        id: "save_session",
        enabled: true,
      }).catch(() => undefined);
      // First-run pre-check: only prompt for a key when none is already
      // available (keychain entry or ANTHROPIC_API_KEY env var — resolved by
      // has_api_key on the backend). If a key exists, skip onboarding entirely.
      try {
        if (!(await invoke<boolean>("has_api_key"))) {
          firstRunOpen = true;
        }
      } catch {
        // No backend (browser/dev): skip onboarding.
      }
      try {
        if (await invoke<boolean>("check_auto_save")) {
          restorePromptOpen = true;
        }
      } catch {
        // No autosave to offer.
      }
    })();

    // The 30s autosave is the crash-recovery safety net. Surface failures
    // (disk full, permissions, missing app-data dir) as an error toast, but
    // dedupe: toast only on the transition working→failing so a persistent
    // problem doesn't fire every interval. A later success silently re-arms.
    let autosaveFailing = false;
    const autosaveTimer = setInterval(() => {
      autoSaveSession(currentLayout()).then(
        () => {
          autosaveFailing = false;
        },
        (e: unknown) => {
          if (!autosaveFailing) {
            autosaveFailing = true;
            addToast(
              "error",
              `Autosave failed — your recovery snapshot is not being updated. ${String(e)}`,
            );
          }
        },
      );
    }, AUTOSAVE_INTERVAL_MS);

    return () => {
      unsubscribeGoal();
      unsubscribeLsp();
      unsubscribeShowMessage();
      unsubscribeAssistant();
      unsubscribeSetup();
      unsubscribeProse();
      unlistenProse?.();
      unlistenSessionLoaded?.();
      unlistenMenu?.();
      unlistenSetup?.();
      unlistenProseGenerating?.();
      clearInterval(autosaveTimer);
      for (const timer of toastTimers.values()) clearTimeout(timer);
      toastTimers.clear();
    };
  });

  function toggleTheme() {
    dark = !dark;
    document.documentElement.classList.toggle("dark", dark);
  }

  let formalWidth = $state(55); // width % of the Formal pane within the upper row
  let topHeight = $state(60); // height % of the upper row (Formal + Prose)

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
      // Vertical divider splits Formal (left) from Prose (right) in the upper row.
      formalWidth = Math.min(
        80,
        Math.max(20, ((e.clientX - dragRect.left) / dragRect.width) * 100),
      );
    }
    if (draggingHorizontal) {
      // Horizontal divider splits the upper row from the Proof Assistant band.
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

  const goalFontSize = $derived($settingsStore?.goal_state_font_size);
  const proseFontSize = $derived($settingsStore?.prose_proof_font_size);
  const assistantFontSize = $derived($settingsStore?.assistant_font_size);
</script>

<div class="app-shell">
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="app-container"
    bind:this={containerEl}
    onmousemove={onMouseMove}
    onmouseup={onMouseUp}
    onmouseleave={onMouseUp}
  >
    <div class="upper-row" style="height: {topHeight}%">
      <div class="panel formal-panel" style="width: {formalWidth}%">
        <div class="panel-header">Formal Proof</div>
        <div class="panel-content formal-content">
          <FormalProof {dark} {lspReady} />
          <!-- The separator rail: a thin spine between code and prose that the
               cursor-row goal card (#91) pins its right edge to. -->
          <div class="rail" aria-hidden="true"></div>
        </div>
      </div>

      <Divider orientation="vertical" onDragStart={onVerticalDragStart} />

      <div class="panel prose-panel" style="width: {100 - formalWidth}%">
        <div class="panel-header">
          {proofView === "prose" ? "Prose Proof" : "Goal State"}
          <ProofViewToggle
            view={proofView}
            onToggle={() =>
              (proofView = proofView === "prose" ? "formal" : "prose")}
          />
        </div>
        <div
          class="panel-content"
          style={proofView === "prose"
            ? proseFontSize
              ? `font-size: ${String(proseFontSize)}pt`
              : ""
            : goalFontSize
              ? `font-size: ${String(goalFontSize)}pt`
              : ""}
        >
          {#if proofView === "prose"}
            <ProseProof content={prose} generating={$proseGenerating} />
          {:else}
            <GoalState content={goalText} />
          {/if}
        </div>
      </div>
    </div>

    <Divider orientation="horizontal" onDragStart={onHorizontalDragStart} />

    <div class="panel assistant-panel" style="height: {100 - topHeight}%">
      <div class="panel-header">
        Proof Assistant
        <ThemeToggle {dark} onToggle={toggleTheme} />
      </div>
      <div
        class="panel-content"
        style={assistantFontSize
          ? `font-size: ${String(assistantFontSize)}pt`
          : ""}
      >
        <Assistant />
      </div>
    </div>

    <Toast {toasts} onDismiss={dismissToast} />
  </div>

  <StatusBar />

  {#if settingsOpen}
    <SettingsDialog onClose={() => (settingsOpen = false)} />
  {/if}

  {#if firstRunOpen}
    <SettingsDialog firstRun onClose={() => (firstRunOpen = false)} />
  {/if}

  {#if restorePromptOpen}
    <div class="overlay" role="presentation">
      <div
        class="confirm"
        role="dialog"
        aria-modal="true"
        aria-label="Restore session"
      >
        <p>
          Turnstile closed with unsaved work. Restore the autosaved session?
        </p>
        <div class="confirm-buttons">
          <button class="secondary" onclick={() => void restoreAutosave(false)}>
            Discard
          </button>
          <button class="primary" onclick={() => void restoreAutosave(true)}>
            Restore
          </button>
        </div>
      </div>
    </div>
  {/if}
</div>

<style>
  .app-shell {
    display: flex;
    flex-direction: column;
    width: 100vw;
    height: 100vh;
    overflow: hidden;
    background: var(--color-bg);
    color: var(--color-text);
    font-family: system-ui, sans-serif;
  }

  .app-container {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    user-select: none;
  }

  /* Upper band: Formal Proof (left) and Prose Proof (right), side by side. */
  .upper-row {
    display: flex;
    flex-direction: row;
    min-height: 0;
    overflow: hidden;
  }

  .panel {
    display: flex;
    flex-direction: column;
    min-height: 0;
    min-width: 0;
    overflow: hidden;
  }

  /* The Proof Assistant spans the full width of the lower band. */
  .assistant-panel {
    width: 100%;
  }

  /* Anchor context for the rail and the cursor-row goal card (#91). */
  .formal-content {
    position: relative;
  }

  /* Thin vertical spine on the inner edge of the Formal pane. A small cosmetic
     gutter (the divider) separates it from the Prose pane. The cursor-row goal
     card will pin its right edge here and grow leftward, never crossing it. */
  .rail {
    position: absolute;
    top: 0;
    right: 0;
    bottom: 0;
    width: 2px;
    background: var(--color-border);
    pointer-events: none;
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

  .overlay {
    position: fixed;
    inset: 0;
    background: rgb(0 0 0 / 40%);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 100;
  }

  .confirm {
    width: min(26rem, calc(100vw - 4rem));
    background: var(--color-bg);
    color: var(--color-text);
    border: 1px solid var(--color-border);
    border-radius: 0.75rem;
    box-shadow: 0 20px 50px rgb(0 0 0 / 30%);
    padding: 1.25rem;
    font-size: 0.875rem;
  }

  .confirm p {
    margin: 0 0 1rem;
  }

  .confirm-buttons {
    display: flex;
    justify-content: flex-end;
    gap: 0.5rem;
  }

  .confirm button {
    padding: 0.375rem 1rem;
    border-radius: 0.5rem;
    font-size: 0.8125rem;
  }

  .confirm button.primary {
    background: var(--color-accent);
    color: var(--color-accent-text);
  }

  .confirm button.primary:hover {
    background: var(--color-accent-hover);
  }

  .confirm button.secondary {
    background: var(--color-surface);
    color: var(--color-text);
    border: 1px solid var(--color-border);
  }
</style>
