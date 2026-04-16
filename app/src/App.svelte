<script lang="ts">
  import { onMount, untrack } from 'svelte'
  import { invoke, listen } from './session/tauri'
  import type { SetupProgressPayload } from './session/tauri'
  import Editor from './formal-proof/Editor.svelte'
  import SetupOverlay from './setup/SetupOverlay.svelte'
  import AssistantPanel from './assistant/AssistantPanel.svelte'
  import SettingsModal from './setup/SettingsModal.svelte'
  import ProofViewToggle from './session/ProofViewToggle.svelte'
  import ProsePanel from './prose-proof/ProsePanel.svelte'
  import GoalPanel from './goal-state/GoalPanel.svelte'
  import SymbolOutline from './formal-proof/SymbolOutline.svelte'
  import { lspDocumentSymbols } from './formal-proof/lspRequests'
  import { theme, systemTheme, toggleTheme, resolveTheme } from './setup/theme'
  import type { ResolvedTheme } from './setup/theme'
  import {
    settings,
    parseSettings,
    applySettings,
    setAvailableModels,
    updateSetting,
  } from './setup/settings.svelte'
  import type { ModelInfo } from './setup/settings.svelte'
  import { handleMenuEvent } from './session/menu'
  import { syncSaveMenuState } from './session/saveIndicator'
  import { errorNotification, showError, dismissError } from './session/errorNotification.svelte'
  import { lspState, setOutlineSymbols, setupLspListeners } from './formal-proof/lspState.svelte'
  import {
    layoutState,
    setAssistantWidthPct,
    setGoalPanelPct,
    toggleWordWrap,
    setOutlineOpen,
    ASSISTANT_WIDTH_MIN,
    ASSISTANT_WIDTH_MAX,
    GOAL_PANEL_MIN,
    GOAL_PANEL_MAX,
  } from './session/layoutState.svelte'
  import {
    sessionState,
    setEditorContent,
    setProofView,
    setProseGenerating,
    newSession,
    openSession,
    saveSession,
    saveSessionAs,
    autoSave,
    reopenLastSession,
    setupSessionListeners,
    type SessionSetupDeps,
  } from './session/sessionState.svelte'
  import { installKeyboardShortcuts } from './session/keyboard'

  let setupVisible = $state(true)
  let setupMessage = $state('Checking Lean installation...')
  let setupProgress = $state(0)
  let setupError = $state(false)
  let showSettings = $state(false)

  // Cursor position in 0-indexed LSP coordinates. Drives both the footer
  // display (1-indexed) and Goal-State-panel line highlighting.
  let cursorLine = $state<number | null>(null)
  let cursorCol = $state<number | null>(null)
  let editorFocused = $state(false)

  let cursorLineDisplay = $derived((cursorLine ?? 0) + 1)
  let cursorColDisplay = $derived((cursorCol ?? 0) + 1)

  const PROOF_URI = 'file:///proof.lean'

  function handleExternalDef(uri: string): void {
    showError(`Definition is in another file — out of scope for now (${uri})`)
  }

  async function openSymbolOutline(): Promise<void> {
    try {
      const symbols = await lspDocumentSymbols()
      setOutlineSymbols(symbols)
      setOutlineOpen(true)
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err)
      showError(`Could not load symbols: ${msg}`)
    }
  }

  function jumpToSymbol(line: number, character: number): void {
    // eslint-disable-next-line @typescript-eslint/no-unsafe-call -- Svelte 5 bind:this doesn't expose exported functions in the component type
    editorRef?.jumpTo(line, character)
  }

  // Derive the concrete dark/light theme from the preference + OS setting.
  let resolved: ResolvedTheme = $derived(resolveTheme($theme, $systemTheme))

  // .light on <html> so fixed-position elements (modals, overlays) inherit CSS variables.
  // data-theme-resolved disables the CSS prefers-color-scheme fallback once JS is in control.
  $effect(() => {
    document.documentElement.setAttribute('data-theme-resolved', '')
    document.documentElement.classList.toggle('light', resolved === 'light')
  })

  // Mirror the editor font size into a CSS custom property read by `.cm-editor`
  // in app.css. A `$effect` keeps it in sync whenever the setting changes,
  // so changes applied from the Settings dialog take effect immediately.
  $effect(() => {
    document.documentElement.style.setProperty(
      '--editor-font-size',
      `${String(settings.editorFontSize)}px`,
    )
  })

  // Keep the Save menu item enabled/disabled in sync with the dirty flag.
  $effect(() => {
    syncSaveMenuState(sessionState.sessionDirty).catch(() => {
      /* menu not yet available during setup */
    })
  })

  function onSplitterDown(e: MouseEvent): void {
    e.preventDefault()
    const onMove = (ev: MouseEvent): void => {
      const pct = ((window.innerWidth - ev.clientX) / window.innerWidth) * 100
      setAssistantWidthPct(pct)
    }
    const onUp = (): void => {
      window.removeEventListener('mousemove', onMove)
      window.removeEventListener('mouseup', onUp)
    }
    window.addEventListener('mousemove', onMove)
    window.addEventListener('mouseup', onUp)
  }

  function onSplitterKeydown(e: KeyboardEvent): void {
    const step = e.shiftKey ? 5 : 1
    if (e.key === 'ArrowLeft') {
      e.preventDefault()
      setAssistantWidthPct(layoutState.assistantWidthPct + step)
    } else if (e.key === 'ArrowRight') {
      e.preventDefault()
      setAssistantWidthPct(layoutState.assistantWidthPct - step)
    } else if (e.key === 'Home') {
      e.preventDefault()
      setAssistantWidthPct(ASSISTANT_WIDTH_MIN)
    } else if (e.key === 'End') {
      e.preventDefault()
      setAssistantWidthPct(ASSISTANT_WIDTH_MAX)
    }
  }

  // Editor component instance (bind:this target — cannot move to a store).
  let editorRef = $state<Editor | null>(null)

  // Goal panel drag state (local UI-only — not persisted).
  let goalDragging = false
  let goalDragStartY = 0
  let goalDragStartPct = 0

  let showRecoveryPrompt = $state(false)
  let recoveryPromptEl = $state<HTMLElement | null>(null)
  let recoveryTriggerEl: Element | null = null

  // Keep the native window title in sync with the theorem title.
  let lastSetTitle = ''
  $effect(() => {
    const title = sessionState.theoremTitle
    if (title !== lastSetTitle) {
      lastSetTitle = title
      invoke('set_window_title', { title }).catch(() => {
        /* window not yet available during setup */
      })
    }
  })

  // When the Editor remounts (e.g. after toggling prose → formal), restore the
  // current content into the fresh CodeMirror instance.  We read editorContent
  // inside untrack() so this effect only re-runs when editorRef changes — not
  // on every keystroke.
  $effect(() => {
    if (editorRef) {
      const content = untrack(() => sessionState.editorContent)
      if (content) {
        // eslint-disable-next-line @typescript-eslint/no-unsafe-call -- Svelte 5 bind:this doesn't expose exported functions in the component type
        editorRef.setContent(content)
      }
    }
  })

  // Focus management for the recovery prompt: move focus in on open, return on close.
  $effect(() => {
    if (showRecoveryPrompt) {
      recoveryTriggerEl = document.activeElement
      // Focus the first button in the prompt on the next tick.
      const el = recoveryPromptEl
      if (el) {
        const btn = el.querySelector<HTMLElement>('button')
        if (btn) btn.focus()
      }
    } else if (recoveryTriggerEl instanceof HTMLElement) {
      recoveryTriggerEl.focus()
      recoveryTriggerEl = null
    }
  })

  function handleChange(content: string): void {
    setEditorContent(content)
  }

  function handleCursorChange(line: number, col: number): void {
    cursorLine = line
    cursorCol = col
  }

  function handleFocusChange(focused: boolean): void {
    editorFocused = focused
  }

  function onGoalSplitterPointerDown(e: PointerEvent): void {
    goalDragging = true
    goalDragStartY = e.clientY
    goalDragStartPct = layoutState.goalPanelPct
    ;(e.currentTarget as HTMLElement).setPointerCapture(e.pointerId)
  }

  function onGoalSplitterPointerMove(e: PointerEvent): void {
    if (!goalDragging) return
    const container = (e.currentTarget as HTMLElement).parentElement
    if (!container) return
    const containerHeight = container.getBoundingClientRect().height
    const deltaPx = goalDragStartY - e.clientY
    const deltaPct = (deltaPx / containerHeight) * 100
    setGoalPanelPct(goalDragStartPct + deltaPct)
  }

  function onGoalSplitterPointerUp(e: PointerEvent): void {
    goalDragging = false
    ;(e.currentTarget as HTMLElement).releasePointerCapture(e.pointerId)
  }

  function onGoalSplitterKeydown(e: KeyboardEvent): void {
    const step = e.shiftKey ? 5 : 1
    if (e.key === 'ArrowUp') {
      e.preventDefault()
      setGoalPanelPct(layoutState.goalPanelPct + step)
    } else if (e.key === 'ArrowDown') {
      e.preventDefault()
      setGoalPanelPct(layoutState.goalPanelPct - step)
    } else if (e.key === 'Home') {
      e.preventDefault()
      setGoalPanelPct(GOAL_PANEL_MIN)
    } else if (e.key === 'End') {
      e.preventDefault()
      setGoalPanelPct(GOAL_PANEL_MAX)
    }
  }

  // Recovery flow helpers — thin wrappers that flip the overlay flag.
  async function restoreAutoSave(): Promise<void> {
    showRecoveryPrompt = false
    // restore_auto_save loads autosave.turn into session state, emits
    // session-loaded, and deletes the autosave file. We do NOT fall back
    // to get_last_session here — the restored draft is what the user asked
    // for, even if a saved session exists on disk.
    await invoke('restore_auto_save').catch((err: unknown) => {
      const msg = err instanceof Error ? err.message : String(err)
      showError(`Could not restore unsaved session: ${msg}`)
    })
  }

  async function discardAutoSave(): Promise<void> {
    showRecoveryPrompt = false
    await invoke('delete_auto_save').catch(() => {
      /* ignore delete errors */
    })
    // Discarding the draft means "start from my last saved state" — fall
    // back to the same last-session reopen the no-autosave branch uses.
    await reopenLastSession()
  }

  onMount(() => {
    const unlistenKeyboard = installKeyboardShortcuts({
      newSession: () => void newSession(),
      openSession: () => void openSession(),
      saveSession: () => void saveSession(),
      saveSessionAs: () => void saveSessionAs(),
      openSettings: () => {
        showSettings = true
      },
      openSymbolOutline: () => {
        void openSymbolOutline()
      },
    })

    // Track the OS color-scheme preference so "auto" mode can react in real time.
    const mql = window.matchMedia('(prefers-color-scheme: dark)')
    systemTheme.set(mql.matches ? 'dark' : 'light')
    const onSystemChange = (e: MediaQueryListEvent): void => {
      systemTheme.set(e.matches ? 'dark' : 'light')
    }
    mql.addEventListener('change', onSystemChange)

    // Load persisted settings and available models from Rust backend.
    invoke<Record<string, unknown>>('get_settings')
      .then((raw) => {
        const parsed = parseSettings(raw)
        applySettings(parsed)
        theme.set(parsed.theme)
      })
      .catch(() => {
        /* use defaults */
      })

    invoke<ModelInfo[]>('get_available_models')
      .then((models) => {
        setAvailableModels(models)
      })
      .catch(() => {
        /* no models available */
      })

    // Register ALL Tauri listeners BEFORE calling start_lsp — any listener
    // registered after start_lsp would miss events emitted immediately on
    // startup. Each setup function awaits its own Promise.all internally;
    // the outer Promise.all below then awaits all three before startLsp().
    const setupDeps: SessionSetupDeps = {
      setEditorText: (content) => {
        // eslint-disable-next-line @typescript-eslint/no-unsafe-call -- Svelte 5 bind:this doesn't expose exported functions in the component type
        editorRef?.setContent(content)
      },
    }
    const lspPromise = setupLspListeners()
    const sessionPromise = setupSessionListeners(setupDeps)

    // Listen for native menu events from the Rust backend
    const menuPromise = listen<string>('menu-event', (id) => {
      handleMenuEvent(id, {
        newSession: () => void newSession(),
        openSession: () => void openSession(),
        saveSession: () => void saveSession(),
        saveSessionAs: () => void saveSessionAs(),
        openSettings: () => {
          showSettings = true
        },
        toggleWordWrap,
      })
    })

    void Promise.all([lspPromise, sessionPromise, menuPromise]).then(
      ([unlistenLsp, unlistenSession, unlistenMenu]) => {
        void startLsp()
        return () => {
          unlistenLsp()
          unlistenSession()
          unlistenMenu()
        }
      },
    )

    // Start auto-save timer (every 60 seconds)
    const autoSaveTimer = setInterval(() => {
      void autoSave()
    }, 60_000)

    return () => {
      clearInterval(autoSaveTimer)
      unlistenKeyboard()
      mql.removeEventListener('change', onSystemChange)
    }
  })

  async function startLsp(): Promise<void> {
    const status = await invoke<{ complete: boolean; project_path: string }>('get_setup_status')

    if (!status.complete) {
      // Register the setup-progress listener BEFORE invoking start_setup to avoid
      // missing the "ready" event if setup completes before the listener is registered.
      await new Promise<void>((resolve) => {
        listen<SetupProgressPayload>('setup-progress', (p) => {
          setupMessage = p.message
          setupProgress = p.progress_pct
          if (p.phase === 'error') {
            setupError = true
            resolve()
          } else if (p.phase === 'ready') {
            resolve()
          }
        })
          .then((unlisten) => {
            void invoke('start_setup').catch(() => {
              resolve()
            })
            return unlisten
          })
          .catch(() => {
            resolve()
          })
      })
    }

    setupVisible = false
    await invoke('start_lsp')

    // Check for autosave recovery after setup is done
    const hasAutoSave = await invoke<boolean>('check_auto_save').catch(() => false)
    if (hasAutoSave) {
      showRecoveryPrompt = true
    } else {
      // No autosave — try reopening the last saved session.
      await reopenLastSession()
    }
  }
</script>

<!-- Root container: theme is applied to <html> via $effect above -->
<div class="fixed inset-0 bg-bg-primary text-text-primary">
  {#if errorNotification.messages.length > 0}
    <div class="fixed top-0 left-0 right-0 z-[60] flex flex-col" data-testid="error-banner">
      {#each errorNotification.messages as message, i (message)}
        <div
          role="alert"
          class="flex items-center justify-between px-4 py-2
            bg-error text-bg-primary text-[13px] border-b border-error/70"
        >
          <span>{message}</span>
          <button
            onclick={() => {
              dismissError(i)
            }}
            aria-label="Dismiss error"
            class="ml-4 shrink-0 px-2 py-0.5 rounded text-bg-primary/90 hover:text-bg-primary hover:bg-error/80
              transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-bg-primary/50"
          >
            Dismiss
          </button>
        </div>
      {/each}
    </div>
  {/if}

  {#if showRecoveryPrompt}
    <!-- Recovery prompt overlay -->
    <div
      bind:this={recoveryPromptEl}
      role="dialog"
      aria-modal="true"
      aria-labelledby="recovery-prompt-title"
      tabindex="-1"
      data-testid="recovery-prompt"
      class="fixed inset-0 z-50 flex items-center justify-center bg-black/50"
      onkeydown={(e) => {
        if (e.key === 'Escape') void discardAutoSave()
        // Focus trap: keep Tab/Shift+Tab within the two buttons.
        if (e.key === 'Tab') {
          const btns = recoveryPromptEl?.querySelectorAll<HTMLElement>('button') ?? []
          const first = btns[0]
          const last = btns[btns.length - 1]
          if (!first || !last) return
          if (e.shiftKey && document.activeElement === first) {
            e.preventDefault()
            last.focus()
          } else if (!e.shiftKey && document.activeElement === last) {
            e.preventDefault()
            first.focus()
          }
        }
      }}
    >
      <div class="rounded-lg p-6 shadow-xl max-w-sm w-full mx-4 bg-bg-primary border border-border">
        <h2 id="recovery-prompt-title" class="text-base font-semibold mb-3 text-text-primary">
          Restore unsaved session?
        </h2>
        <p class="text-sm mb-5 text-text-secondary">
          An unsaved session was found from your last Turnstile session.
        </p>
        <div class="flex gap-3 justify-end">
          <button
            onclick={() => void discardAutoSave()}
            class="px-3 py-1.5 rounded text-sm bg-bg-secondary text-text-secondary
              hover:bg-bg-tertiary hover:text-text-primary transition-colors
              focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent"
          >
            No, discard
          </button>
          <button
            onclick={() => void restoreAutoSave()}
            class="px-3 py-1.5 rounded text-sm font-medium bg-accent text-white
              hover:bg-accent-hover transition-colors
              focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent"
          >
            Yes, restore
          </button>
        </div>
      </div>
    </div>
  {/if}

  <SetupOverlay
    visible={setupVisible}
    message={setupMessage}
    progress={setupProgress}
    isError={setupError}
  />

  <div class="flex h-full bg-bg-primary">
    <!-- Editor column (takes remaining space) -->
    <div class="flex flex-col flex-1 min-w-0">
      <!-- Top header: vertically aligns with the Assistant header in AssistantPanel.
           min-h-[28px] on the inner row matches the w-7 h-7 toggle button that sets
           the height of the Assistant header, so both header bottoms align. -->
      <div class="flex items-center px-4 py-2 border-b border-border bg-bg-secondary shrink-0">
        <div class="flex items-center min-h-[28px]">
          <span
            class="text-[13px] font-semibold text-text-primary tracking-wide uppercase opacity-70"
          >
            Formal Proof
          </span>
        </div>
      </div>
      <div class="flex-1 min-h-0">
        <div class="flex flex-col h-full">
          <div class="min-h-0" style="flex: {100 - layoutState.goalPanelPct}">
            <Editor
              bind:this={editorRef}
              initialTheme={resolved}
              theme={resolved}
              diagnostics={lspState.diagnostics}
              semanticTokens={lspState.semanticTokens}
              fileProgress={lspState.fileProgress}
              wordWrap={layoutState.wordWrap}
              currentUri={() => PROOF_URI}
              onchange={handleChange}
              oncursorchange={handleCursorChange}
              onfocuschange={handleFocusChange}
              ontogglewrap={toggleWordWrap}
              onexternaldef={handleExternalDef}
            />
          </div>

          <!-- Horizontal splitter between editor and the lower panel -->
          <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
          <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
          <div
            role="separator"
            aria-orientation="horizontal"
            aria-label="Resize goal panel"
            aria-valuenow={layoutState.goalPanelPct}
            aria-valuemin={GOAL_PANEL_MIN}
            aria-valuemax={GOAL_PANEL_MAX}
            tabindex="0"
            class="splitter-grip cursor-row-resize flex-shrink-0 bg-bg-tertiary flex items-center justify-center
              border-t border-b border-border
              focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent"
            style="height: 10px"
            onpointerdown={onGoalSplitterPointerDown}
            onpointermove={onGoalSplitterPointerMove}
            onpointerup={onGoalSplitterPointerUp}
            onkeydown={onGoalSplitterKeydown}
          >
            <div class="flex gap-1">
              <span class="w-1 h-1 rounded-full bg-text-secondary opacity-40"></span>
              <span class="w-1 h-1 rounded-full bg-text-secondary opacity-40"></span>
              <span class="w-1 h-1 rounded-full bg-text-secondary opacity-40"></span>
            </div>
          </div>

          <!-- Lower panel: header (label + toggle) above either GoalPanel or ProsePanel -->
          <div class="min-h-0 flex flex-col" style="flex: {layoutState.goalPanelPct}">
            <div
              class="flex items-center justify-between px-4 py-2 border-b border-border bg-bg-secondary shrink-0"
            >
              <div class="flex items-center gap-2">
                <div
                  class="w-2 h-2 rounded-full bg-accent transition-opacity duration-200"
                  class:opacity-80={sessionState.sessionDirty}
                  class:opacity-0={!sessionState.sessionDirty}
                ></div>
                <span
                  class="text-[13px] font-semibold text-text-primary tracking-wide uppercase opacity-70"
                  data-testid="lower-panel-header"
                >
                  {sessionState.proofView === 'formal' ? 'Goal State' : 'Prose Proof'}
                </span>
              </div>
              <ProofViewToggle
                view={sessionState.proofView}
                onToggle={() => {
                  const nextView = sessionState.proofView === 'formal' ? 'prose' : 'formal'
                  setProofView(nextView)
                  if (
                    nextView === 'prose' &&
                    !sessionState.proseText &&
                    sessionState.editorContent
                  ) {
                    setProseGenerating(true)
                    invoke('generate_prose')
                      .catch((err: unknown) => {
                        const msg = err instanceof Error ? err.message : String(err)
                        showError(`Prose generation failed: ${msg}`)
                      })
                      .finally(() => {
                        setProseGenerating(false)
                      })
                  }
                }}
              />
            </div>
            <div class="flex-1 min-h-0">
              {#if sessionState.proofView === 'formal'}
                <GoalPanel
                  goalText={lspState.goalText}
                  goalLineToProofLine={lspState.goalLineToProofLine}
                  {cursorLine}
                  {editorFocused}
                />
              {:else}
                <ProsePanel
                  proseHtml={sessionState.renderedProseHtml}
                  generating={sessionState.proseGenerating}
                  fontSize={settings.proseFontSize}
                />
              {/if}
            </div>
          </div>
        </div>
      </div>
      <!-- Footer status strip: cursor position + word-wrap toggle. -->
      <div
        class="flex items-center justify-between px-3 py-1 border-t border-border bg-bg-secondary text-[11px] text-text-secondary shrink-0"
        data-testid="editor-footer"
      >
        <span data-testid="cursor-position">Ln {cursorLineDisplay}, Col {cursorColDisplay}</span>
        <button
          type="button"
          class="px-2 py-0.5 rounded hover:bg-bg-tertiary hover:text-text-primary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent"
          aria-pressed={layoutState.wordWrap}
          onclick={toggleWordWrap}
          data-testid="word-wrap-toggle"
        >
          Wrap: {layoutState.wordWrap ? 'On' : 'Off'}
        </button>
      </div>
    </div>

    <!-- Draggable vertical splitter — interactive separator per APG window splitter pattern -->
    <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <div
      role="separator"
      aria-orientation="vertical"
      aria-label="Resize assistant panel"
      aria-valuenow={layoutState.assistantWidthPct}
      aria-valuemin={ASSISTANT_WIDTH_MIN}
      aria-valuemax={ASSISTANT_WIDTH_MAX}
      tabindex="0"
      class="splitter-grip cursor-col-resize flex-shrink-0 bg-bg-tertiary flex flex-col items-center justify-center gap-1
        focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent"
      style="width: 10px"
      onmousedown={onSplitterDown}
      onkeydown={onSplitterKeydown}
    >
      <span class="w-1 h-1 rounded-full bg-text-secondary opacity-40"></span>
      <span class="w-1 h-1 rounded-full bg-text-secondary opacity-40"></span>
      <span class="w-1 h-1 rounded-full bg-text-secondary opacity-40"></span>
    </div>

    <!-- Assistant panel column (resizable width) -->
    <div
      class="flex flex-col flex-shrink-0 h-full border-l border-border"
      style="width: {layoutState.assistantWidthPct}%"
    >
      <AssistantPanel
        theme={resolved}
        sessionDirty={sessionState.sessionDirty}
        fontSize={settings.assistantFontSize}
        onToggleTheme={() => {
          const next = toggleTheme(resolved)
          theme.set(next)
          void updateSetting('theme', next).catch((err: unknown) => {
            const msg = err instanceof Error ? err.message : String(err)
            showError(`Failed to save theme: ${msg}`)
          })
        }}
      />
    </div>
  </div>

  {#if showSettings}
    <SettingsModal onClose={() => (showSettings = false)} />
  {/if}

  {#if layoutState.outlineOpen}
    <SymbolOutline
      symbols={lspState.outlineSymbols}
      onJump={jumpToSymbol}
      onClose={() => {
        setOutlineOpen(false)
      }}
    />
  {/if}
</div>
