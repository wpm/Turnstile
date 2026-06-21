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
  import { GOAL_STATE_ENABLED } from "$lib/featureFlags";
  import { onMount } from "svelte";
  import { SvelteMap } from "svelte/reactivity";
  import { invoke, isTauri } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { runUpdateFlow } from "$lib/updater";
  import {
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
  import { handleSelectAll } from "$lib/selectAll";

  // The right column shows the Proof Assistant by default; the header toggle
  // flips it to the Prose Proof. "goal" is a dev-only third view that rejoins
  // the cycle when GOAL_STATE_ENABLED is on (the mothballed All-Goals view,
  // #119) — it is never produced when the flag is off, so it stays out of the
  // shipped `ProofView` type.
  type RightView = ProofView | "goal";
  let proofView = $state<RightView>("assistant");
  let dark = $state(false);
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
    // A single vertical split now: Formal Proof (left) vs. the Proof Assistant /
    // Prose Proof column (right). Persist the right column's width fraction in
    // both legacy Meta fields so older `.turn` readers keep working.
    const rightWidthPct = 100 - formalWidth;
    return {
      assistantWidthPct: rightWidthPct,
      proofView,
      goalPanelPct: rightWidthPct,
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

  /**
   * Launch-time auto-update check (#53). Non-blocking and Tauri-only: in the
   * browser-driven e2e harness `isTauri()` is false, so the native updater /
   * process / dialog plugins are never loaded. The flow itself lives in
   * `$lib/updater` with its plugin bindings injected here, so it stays
   * unit-testable without a Tauri runtime.
   */
  async function runUpdateCheck() {
    if (!isTauri()) return;
    try {
      const [{ check }, { relaunch }, { ask }] = await Promise.all([
        import("@tauri-apps/plugin-updater"),
        import("@tauri-apps/plugin-process"),
        import("@tauri-apps/plugin-dialog"),
      ]);
      await runUpdateFlow({
        isSupported: () => invoke<boolean>("updater_target_supported"),
        check: () => check(),
        confirm: (message, title) =>
          ask(message, {
            title,
            kind: "info",
            okLabel: "Update",
            cancelLabel: "Later",
          }),
        relaunch: () => relaunch(),
        notify: addToast,
        log: (message) => {
          console.warn(message);
        },
      });
    } catch (e) {
      // Loading or wiring the updater must never break startup.
      console.warn("updater unavailable", e);
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

    // Check for an app update once on launch (non-blocking, Tauri-only).
    void runUpdateCheck();

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

    // Scope Command-A / Ctrl-A to the focused panel instead of the whole window
    // (#113). Bubble phase (not capture) so a focused CodeMirror editor's own
    // Mod-a runs first and marks the event handled; this handler then leaves it
    // alone.
    const onKeydown = (e: KeyboardEvent) => handleSelectAll(e, document);
    window.addEventListener("keydown", onKeydown);

    return () => {
      window.removeEventListener("keydown", onKeydown);
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

  let formalWidth = $state(55); // width % of the Formal Proof (left) column

  let draggingVertical = $state(false);
  let dragRect: DOMRect | null = null;
  let containerEl: HTMLDivElement;

  function onVerticalDragStart(e: MouseEvent) {
    draggingVertical = true;
    dragRect = containerEl.getBoundingClientRect();
    e.preventDefault();
  }

  function onMouseMove(e: MouseEvent) {
    if (!dragRect || !draggingVertical) return;
    // The vertical divider splits the Formal Proof (left) column from the
    // Proof Assistant / Prose Proof (right) column.
    formalWidth = Math.min(
      80,
      Math.max(20, ((e.clientX - dragRect.left) / dragRect.width) * 100),
    );
  }

  function onMouseUp() {
    draggingVertical = false;
    dragRect = null;
  }

  // Cycle the right column's view. The shipped path is binary (Proof Assistant
  // ⇄ Prose Proof). When the Goal State flag is on (dev only), the mothballed
  // All-Goals view rejoins the cycle as a third stop (#119).
  function cycleView() {
    if (proofView === "assistant") {
      proofView = "prose";
    } else if (proofView === "prose") {
      proofView = GOAL_STATE_ENABLED ? "goal" : "assistant";
    } else {
      proofView = "assistant";
    }
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
    <!-- Left column, full height: the Formal Proof. -->
    <div class="panel formal-panel" style="width: {formalWidth}%">
      <div class="panel-header">
        Formal Proof
        <ThemeToggle {dark} onToggle={toggleTheme} />
      </div>
      <div class="panel-content formal-content">
        <FormalProof {dark} {lspReady} />
        {#if GOAL_STATE_ENABLED}
          <!-- The separator rail: a thin spine between code and prose that the
               cursor-row goal card (#91) pins its right edge to. Behind the
               Goal State flag (#119) — it is only a pin target for those
               cards, so it stays hidden when they are. -->
          <div class="rail" aria-hidden="true"></div>
        {/if}
      </div>
    </div>

    <Divider orientation="vertical" onDragStart={onVerticalDragStart} />

    <!-- Right column, full height: Proof Assistant (default) ⇄ Prose Proof. -->
    <div
      class="panel right-panel"
      class:assistant-panel={proofView === "assistant"}
      style="width: {100 - formalWidth}%"
    >
      <div class="panel-header">
        {proofView === "prose"
          ? "Prose Proof"
          : proofView === "goal"
            ? "Goal State"
            : "Proof Assistant"}
        <ProofViewToggle
          view={proofView === "prose" ? "prose" : "assistant"}
          onToggle={cycleView}
        />
      </div>
      <div
        class="panel-content"
        style={proofView === "prose"
          ? proseFontSize
            ? `font-size: ${String(proseFontSize)}pt`
            : ""
          : proofView === "goal"
            ? goalFontSize
              ? `font-size: ${String(goalFontSize)}pt`
              : ""
            : assistantFontSize
              ? `font-size: ${String(assistantFontSize)}pt`
              : ""}
      >
        {#if proofView === "prose"}
          <ProseProof content={prose} generating={$proseGenerating} />
        {:else if GOAL_STATE_ENABLED && proofView === "goal"}
          <!-- All-Goals view: reads the per-row cache directly (ADR-0004).
               Reachable only when the Goal State flag is on (#119). -->
          <GoalState />
        {:else}
          <Assistant />
        {/if}
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

  /* Two full-height columns: Formal Proof (left) and the Proof Assistant /
     Prose Proof column (right), split by a draggable vertical divider. */
  .app-container {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: row;
    overflow: hidden;
    user-select: none;
  }

  .panel {
    display: flex;
    flex-direction: column;
    min-height: 0;
    min-width: 0;
    overflow: hidden;
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
