<script lang="ts">
  import { onMount } from 'svelte'
  import { invoke, listen } from './lib/tauri'
  import type {
    SetupProgressPayload,
    DiagnosticInfo,
    FileProgressRange,
    SemanticToken,
    SessionState,
  } from './lib/tauri'
  import Editor from './components/Editor.svelte'
  import SetupOverlay from './components/SetupOverlay.svelte'
  import ChatPanel from './components/ChatPanel.svelte'
  import SettingsModal from './components/SettingsModal.svelte'
  import ProofViewToggle from './components/ProofViewToggle.svelte'
  import ProsePanel from './components/ProsePanel.svelte'
  import { renderContent } from './lib/renderContent'
  import { theme, systemTheme, toggleTheme, resolveTheme } from './lib/theme'
  import type { ResolvedTheme } from './lib/theme'
  import {
    settings,
    parseSettings,
    applySettings,
    setAvailableModels,
    updateSetting,
  } from './lib/settings.svelte'
  import type { ModelInfo } from './lib/settings.svelte'
  import { handleMenuEvent } from './lib/menu'
  import { errorNotification, showError, dismissError } from './lib/errorNotification.svelte'

  let setupVisible = $state(true)
  let setupMessage = $state('Checking Lean installation...')
  let setupProgress = $state(0)
  let setupError = $state(false)
  let diagnostics = $state<DiagnosticInfo[] | null>(null)
  let semanticTokens = $state<SemanticToken[] | null>(null)
  let fileProgress = $state<FileProgressRange[] | null>(null)
  let showSettings = $state(false)

  // Derive the concrete dark/light theme from the preference + OS setting.
  let resolved: ResolvedTheme = $derived(resolveTheme($theme, $systemTheme))

  // .light on <html> so fixed-position elements (modals, overlays) inherit CSS variables.
  // data-theme-resolved disables the CSS prefers-color-scheme fallback once JS is in control.
  $effect(() => {
    document.documentElement.setAttribute('data-theme-resolved', '')
    document.documentElement.classList.toggle('light', resolved === 'light')
  })

  // Splitter state for resizable chat panel
  const CHAT_WIDTH_MIN = 10
  const CHAT_WIDTH_MAX = 60
  let chatWidthPct = $state(25)

  function onSplitterDown(e: MouseEvent): void {
    e.preventDefault()
    const onMove = (ev: MouseEvent): void => {
      const pct = ((window.innerWidth - ev.clientX) / window.innerWidth) * 100
      chatWidthPct = Math.min(CHAT_WIDTH_MAX, Math.max(CHAT_WIDTH_MIN, pct))
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
      chatWidthPct = Math.min(CHAT_WIDTH_MAX, chatWidthPct + step)
    } else if (e.key === 'ArrowRight') {
      e.preventDefault()
      chatWidthPct = Math.max(CHAT_WIDTH_MIN, chatWidthPct - step)
    } else if (e.key === 'Home') {
      e.preventDefault()
      chatWidthPct = CHAT_WIDTH_MIN
    } else if (e.key === 'End') {
      e.preventDefault()
      chatWidthPct = CHAT_WIDTH_MAX
    }
  }

  // Session state
  let editorRef = $state<Editor | null>(null)
  let editorContent = $state('')
  let proseText = $state('')
  let proseHash = $state<string | null>(null)
  let sessionDirty = $state(false)
  let proofView = $state<'formal' | 'prose'>('formal')
  let proseGenerating = $state(false)
  let renderedProseHtml = $derived(renderContent(proseText))
  let showRecoveryPrompt = $state(false)
  let autoSavePath = $state<string | null>(null)
  let recoveryPromptEl = $state<HTMLElement | null>(null)
  let recoveryTriggerEl: Element | null = null

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

  // Build the meta object to pass to save commands
  function buildMeta(): {
    format_version: number
    created_at: string
    saved_at: string
    cursor_line: number
    cursor_col: number
    editor_scroll_top: number
    chat_width_pct: number
    proof_view: string
  } {
    return {
      format_version: 1,
      created_at: '',
      saved_at: '',
      cursor_line: 0,
      cursor_col: 0,
      editor_scroll_top: 0,
      chat_width_pct: chatWidthPct,
      proof_view: proofView,
    }
  }

  function handleChange(content: string): void {
    editorContent = content
    sessionDirty = true
    invoke('update_document', { content }).catch(() => {
      /* LSP not yet connected */
    })
  }

  // Session command wrappers
  async function newSession(): Promise<void> {
    await invoke('new_session')
    sessionDirty = false
  }

  async function openSession(): Promise<void> {
    await invoke('open_session', { path: null })
    sessionDirty = false
  }

  async function saveSession(): Promise<void> {
    try {
      await invoke('save_session', {
        proofLean: editorContent,
        proseText: proseText,
        proseHash: proseHash,
        meta: buildMeta(),
      })
      sessionDirty = false
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err)
      showError(`Save failed: ${msg}`)
    }
  }

  async function saveSessionAs(): Promise<void> {
    try {
      await invoke('save_session_as', {
        proofLean: editorContent,
        proseText: proseText,
        proseHash: proseHash,
        meta: buildMeta(),
      })
      sessionDirty = false
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err)
      showError(`Save As failed: ${msg}`)
    }
  }

  async function autoSave(): Promise<void> {
    if (!sessionDirty) return
    try {
      await invoke('auto_save_session', {
        proofLean: editorContent,
        proseText: proseText,
        proseHash: proseHash,
        meta: buildMeta(),
      })
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err)
      showError(`Auto-save failed: ${msg}`)
    }
  }

  // Recovery flow helpers
  async function restoreAutoSave(): Promise<void> {
    showRecoveryPrompt = false
    if (autoSavePath) {
      await invoke('open_session', { path: autoSavePath }).catch(() => {
        /* ignore restore errors */
      })
    }
    await invoke('delete_auto_save').catch(() => {
      /* ignore delete errors */
    })
  }

  async function discardAutoSave(): Promise<void> {
    showRecoveryPrompt = false
    await invoke('delete_auto_save').catch(() => {
      /* ignore delete errors */
    })
  }

  // Keyboard shortcut handler — session (N/O/S/Shift+S) + settings (,)
  function handleKeydown(e: KeyboardEvent): void {
    const meta = e.metaKey || e.ctrlKey
    if (!meta) return

    if (e.key === ',') {
      e.preventDefault()
      showSettings = true
    } else if (e.key === 'n' || e.key === 'N') {
      e.preventDefault()
      void newSession()
    } else if (e.key === 'o' || e.key === 'O') {
      e.preventDefault()
      void openSession()
    } else if ((e.key === 's' || e.key === 'S') && e.shiftKey) {
      e.preventDefault()
      void saveSessionAs()
    } else if (e.key === 's' || e.key === 'S') {
      e.preventDefault()
      void saveSession()
    }
  }

  onMount(() => {
    window.addEventListener('keydown', handleKeydown)

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

    // Register listeners BEFORE calling start_lsp — same ordering constraint
    // as in the Rust/WASM version. Tauri events can arrive immediately after
    // start_lsp returns; any listener registered after would miss early events.
    const diagPromise = listen<DiagnosticInfo[]>('lsp-diagnostics', (diags) => {
      diagnostics = diags
    })
    const tokensPromise = listen<SemanticToken[]>('lsp-semantic-tokens', (tokens) => {
      semanticTokens = tokens
    })
    const progressPromise = listen<FileProgressRange[]>('lsp-file-progress', (ranges) => {
      fileProgress = ranges
    })

    // Listen for prose-updated events from other components
    const prosePromise = listen<{ text: string; hash: string | null }>('prose-updated', (data) => {
      proseText = data.text
      proseHash = data.hash
      sessionDirty = true
    })

    // Listen for session-loaded events (open/new session)
    const sessionPromise = listen<SessionState>('session-loaded', (session) => {
      // eslint-disable-next-line @typescript-eslint/no-unsafe-call -- Svelte 5 bind:this doesn't expose exported functions in the component type
      editorRef?.setContent(session.proof_lean)
      proseText = session.prose.text
      proseHash = session.prose.tactic_state_hash
      chatWidthPct = session.meta.chat_width_pct || 25
      proofView = session.meta.proof_view === 'prose' ? 'prose' : 'formal'
      sessionDirty = false
    })

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
      })
    })

    void Promise.all([
      diagPromise,
      tokensPromise,
      progressPromise,
      prosePromise,
      sessionPromise,
      menuPromise,
    ]).then(
      ([
        unlistenDiag,
        unlistenTokens,
        unlistenProgress,
        unlistenProse,
        unlistenSession,
        unlistenMenu,
      ]) => {
        void startLsp()
        return () => {
          unlistenDiag()
          unlistenTokens()
          unlistenProgress()
          unlistenProse()
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
      window.removeEventListener('keydown', handleKeydown)
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
      // Get the autosave path for restoration
      autoSavePath = null // The backend knows the path; open_session with null will use it
      showRecoveryPrompt = true
    } else {
      // No autosave — try reopening the last saved session.
      const lastPath = await invoke<string | null>('get_last_session').catch(() => null)
      if (lastPath) {
        await invoke('open_session', { path: lastPath }).catch(() => {
          // File may have been moved/deleted since last run — silently ignore.
        })
      }
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
            bg-red-900/90 text-red-100 text-[13px] border-b border-red-700"
        >
          <span>{message}</span>
          <button
            onclick={() => {
              dismissError(i)
            }}
            aria-label="Dismiss error"
            class="ml-4 shrink-0 px-2 py-0.5 rounded text-red-200 hover:text-white hover:bg-red-800
              transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-red-400"
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
      <div
        class="flex items-center justify-between px-4 py-2 border-b border-border bg-bg-secondary shrink-0"
      >
        <div class="flex items-center gap-2">
          <div class="w-2 h-2 rounded-full bg-accent opacity-80"></div>
          <span
            class="text-[13px] font-semibold text-text-primary tracking-wide uppercase opacity-70"
          >
            {proofView === 'formal' ? 'Formal Proof' : 'Prose Proof'}
          </span>
        </div>
        <ProofViewToggle
          view={proofView}
          onToggle={() => {
            proofView = proofView === 'formal' ? 'prose' : 'formal'
            if (proofView === 'prose' && !proseText && editorContent) {
              proseGenerating = true
              invoke('generate_prose')
                .catch((err: unknown) => {
                  const msg = err instanceof Error ? err.message : String(err)
                  showError(`Prose generation failed: ${msg}`)
                })
                .finally(() => {
                  proseGenerating = false
                })
            }
          }}
        />
      </div>
      <div class="flex-1 min-h-0">
        {#if proofView === 'formal'}
          <Editor
            bind:this={editorRef}
            initialTheme={resolved}
            theme={resolved}
            {diagnostics}
            {semanticTokens}
            {fileProgress}
            onchange={handleChange}
          />
        {:else}
          <ProsePanel
            proseHtml={renderedProseHtml}
            generating={proseGenerating}
            fontSize={settings.proseFontSize}
          />
        {/if}
      </div>
    </div>

    <!-- Draggable vertical splitter — interactive separator per APG window splitter pattern -->
    <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <div
      role="separator"
      aria-orientation="vertical"
      aria-label="Resize chat panel"
      aria-valuenow={chatWidthPct}
      aria-valuemin={CHAT_WIDTH_MIN}
      aria-valuemax={CHAT_WIDTH_MAX}
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

    <!-- Chat panel column (resizable width) -->
    <div
      class="flex flex-col flex-shrink-0 h-full border-l border-border"
      style="width: {chatWidthPct}%"
    >
      <ChatPanel
        theme={resolved}
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
</div>
