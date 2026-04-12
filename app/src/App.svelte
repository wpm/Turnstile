<script lang="ts">
  import { onMount } from 'svelte'
  import { invoke, listen } from './lib/tauri'
  import type { SetupProgressPayload, DiagnosticInfo, SemanticToken } from './lib/tauri'
  import Editor from './components/Editor.svelte'
  import SetupOverlay from './components/SetupOverlay.svelte'
  import ChatPanel from './components/ChatPanel.svelte'
  import SettingsModal from './components/SettingsModal.svelte'
  import { theme, toggleTheme } from './lib/theme'
  import { parseSettings, applySettings, setAvailableModels } from './lib/settings.svelte'
  import type { ModelInfo } from './lib/settings.svelte'
  import { handleMenuEvent } from './lib/menu'

  let setupVisible = $state(true)
  let setupMessage = $state('Checking Lean installation...')
  let setupProgress = $state(0)
  let setupError = $state(false)
  let diagnostics = $state<DiagnosticInfo[] | null>(null)
  let semanticTokens = $state<SemanticToken[] | null>(null)
  let showSettings = $state(false)

  // Splitter state for resizable chat panel
  let chatWidthPct = $state(25)
  let splitterDragging = $state(false)

  function onSplitterDown(e: MouseEvent): void {
    e.preventDefault()
    splitterDragging = true
    const onMove = (ev: MouseEvent): void => {
      const pct = ((window.innerWidth - ev.clientX) / window.innerWidth) * 100
      chatWidthPct = Math.min(60, Math.max(10, pct))
    }
    const onUp = (): void => {
      splitterDragging = false
      window.removeEventListener('mousemove', onMove)
      window.removeEventListener('mouseup', onUp)
    }
    window.addEventListener('mousemove', onMove)
    window.addEventListener('mouseup', onUp)
  }

  // Session state
  let editorContent = $state('')
  let proseText = $state('')
  let proseHash = $state<string | null>(null)
  let sessionDirty = $state(false)
  let showRecoveryPrompt = $state(false)
  let autoSavePath = $state<string | null>(null)

  // Build the meta object to pass to save commands
  function buildMeta(): {
    format_version: number
    created_at: string
    saved_at: string
    cursor_line: number
    cursor_col: number
    editor_scroll_top: number
    chat_width_pct: number
  } {
    return {
      format_version: 1,
      created_at: '',
      saved_at: '',
      cursor_line: 0,
      cursor_col: 0,
      editor_scroll_top: 0,
      chat_width_pct: chatWidthPct,
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
    await invoke('save_session', {
      proofLean: editorContent,
      proseText: proseText,
      proseHash: proseHash,
      meta: buildMeta(),
    })
    sessionDirty = false
  }

  async function saveSessionAs(): Promise<void> {
    await invoke('save_session_as', {
      proofLean: editorContent,
      proseText: proseText,
      proseHash: proseHash,
      meta: buildMeta(),
    })
    sessionDirty = false
  }

  async function autoSave(): Promise<void> {
    if (!sessionDirty) return
    await invoke('auto_save_session', {
      proofLean: editorContent,
      proseText: proseText,
      proseHash: proseHash,
      meta: buildMeta(),
    }).catch(() => {
      /* ignore autosave errors */
    })
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

    // Load persisted settings and available models from Rust backend.
    invoke<Record<string, unknown>>('get_settings')
      .then((raw) => {
        applySettings(parseSettings(raw))
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

    // Listen for prose-updated events from other components
    const prosePromise = listen<{ text: string; hash: string | null }>('prose-updated', (data) => {
      proseText = data.text
      proseHash = data.hash
      sessionDirty = true
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

    void Promise.all([diagPromise, tokensPromise, prosePromise, menuPromise]).then(
      ([unlistenDiag, unlistenTokens, unlistenProse, unlistenMenu]) => {
        void startLsp()
        return () => {
          unlistenDiag()
          unlistenTokens()
          unlistenProse()
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
    }
  }
</script>

<!-- Root themed container — everything lives inside so CSS variables cascade -->
<div class="fixed inset-0" data-theme={$theme}>
  {#if showRecoveryPrompt}
    <!-- Recovery prompt overlay -->
    <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/50">
      <div class="rounded-lg p-6 shadow-xl max-w-sm w-full mx-4 bg-surface text-on-surface">
        <h2 class="text-lg font-semibold mb-3">Restore unsaved session?</h2>
        <p class="text-sm mb-5 text-on-surface-secondary">
          An unsaved session was found from your last Turnstile session.
        </p>
        <div class="flex gap-3 justify-end">
          <button
            onclick={() => void discardAutoSave()}
            class="px-4 py-2 rounded-md text-sm font-medium bg-surface-tertiary text-on-surface-secondary
              hover:text-on-surface transition-colors"
          >
            No, discard
          </button>
          <button
            onclick={() => void restoreAutoSave()}
            class="px-4 py-2 rounded-md text-sm font-semibold bg-accent text-on-accent
              hover:bg-accent-hover transition-colors"
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

  <div class="flex h-full bg-surface">
    <!-- Editor column (takes remaining space) -->
    <div class="flex-1 min-w-0">
      <Editor
        initialTheme={$theme}
        theme={$theme}
        {diagnostics}
        {semanticTokens}
        onchange={handleChange}
      />
    </div>

    <!-- Draggable vertical splitter -->
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="splitter-grip w-1.5 cursor-col-resize transition-colors flex-shrink-0"
      class:bg-border-default={!splitterDragging}
      class:bg-border-active={splitterDragging}
      class:hover:bg-border-active={!splitterDragging}
      onmousedown={onSplitterDown}
    ></div>

    <!-- Chat panel column (resizable width) -->
    <div class="flex flex-col flex-shrink-0 h-full" style="width: {chatWidthPct}%">
      <ChatPanel />
    </div>
  </div>

  <button
    onclick={() => {
      theme.update(toggleTheme)
    }}
    aria-label="Toggle theme"
    class="fixed top-3 right-3 z-20 w-8 h-8 flex items-center justify-center rounded-full
      bg-surface-secondary text-on-surface-secondary hover:text-on-surface hover:bg-surface-tertiary
      transition-colors"
  >
    {#if $theme === 'mocha'}
      <!-- Heroicons: sun -->
      <svg
        xmlns="http://www.w3.org/2000/svg"
        fill="none"
        viewBox="0 0 24 24"
        stroke-width="1.5"
        stroke="currentColor"
        class="w-4 h-4"
      >
        <path
          stroke-linecap="round"
          stroke-linejoin="round"
          d="M12 3v2.25m6.364.386-1.591 1.591M21 12h-2.25m-.386 6.364-1.591-1.591M12 18.75V21m-4.773-4.227-1.591 1.591M5.25 12H3m4.227-4.773L5.636 5.636M15.75 12a3.75 3.75 0 1 1-7.5 0 3.75 3.75 0 0 1 7.5 0Z"
        />
      </svg>
    {:else}
      <!-- Heroicons: moon -->
      <svg
        xmlns="http://www.w3.org/2000/svg"
        fill="none"
        viewBox="0 0 24 24"
        stroke-width="1.5"
        stroke="currentColor"
        class="w-4 h-4"
      >
        <path
          stroke-linecap="round"
          stroke-linejoin="round"
          d="M21.752 15.002A9.72 9.72 0 0 1 18 15.75c-5.385 0-9.75-4.365-9.75-9.75 0-1.33.266-2.597.748-3.752A9.753 9.753 0 0 0 3 11.25C3 16.635 7.365 21 12.75 21a9.753 9.753 0 0 0 9.002-5.998Z"
        />
      </svg>
    {/if}
  </button>

  {#if showSettings}
    <SettingsModal onClose={() => (showSettings = false)} />
  {/if}
</div>
