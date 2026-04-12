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

    void Promise.all([diagPromise, tokensPromise, prosePromise]).then(
      ([unlistenDiag, unlistenTokens, unlistenProse]) => {
        void startLsp()
        return () => {
          unlistenDiag()
          unlistenTokens()
          unlistenProse()
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

{#if showRecoveryPrompt}
  <!-- Recovery prompt overlay -->
  <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/50">
    <div
      class="rounded-lg p-6 shadow-xl max-w-sm w-full mx-4"
      class:bg-[#282a36]={$theme === 'dracula'}
      class:text-[#f8f8f2]={$theme === 'dracula'}
      class:bg-white={$theme === 'light'}
      class:text-[#24292f]={$theme === 'light'}
    >
      <h2 class="text-lg font-semibold mb-3">Restore unsaved session?</h2>
      <p class="text-sm mb-5 opacity-75">
        An unsaved session was found from your last Turnstile session.
      </p>
      <div class="flex gap-3 justify-end">
        <button
          onclick={() => void discardAutoSave()}
          class="px-4 py-2 rounded text-sm font-mono opacity-75 hover:opacity-100 transition-opacity"
          class:bg-[#44475a]={$theme === 'dracula'}
          class:bg-[#eaeef2]={$theme === 'light'}
        >
          No, discard
        </button>
        <button
          onclick={() => void restoreAutoSave()}
          class="px-4 py-2 rounded text-sm font-mono font-semibold hover:opacity-90 transition-opacity"
          class:bg-[#50fa7b]={$theme === 'dracula'}
          class:text-[#282a36]={$theme === 'dracula'}
          class:bg-[#0969da]={$theme === 'light'}
          class:text-white={$theme === 'light'}
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

<div
  class="fixed inset-0 flex"
  class:bg-[#282a36]={$theme === 'dracula'}
  class:bg-white={$theme === 'light'}
  data-theme={$theme}
>
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
    class="w-1 cursor-col-resize transition-colors flex-shrink-0"
    class:bg-[#44475a]={$theme === 'dracula' && !splitterDragging}
    class:bg-[#6272a4]={$theme === 'dracula' && splitterDragging}
    class:bg-[#d0d7de]={$theme === 'light' && !splitterDragging}
    class:bg-[#0969da]={$theme === 'light' && splitterDragging}
    class:hover:bg-[#6272a4]={$theme === 'dracula'}
    class:hover:bg-[#0969da]={$theme === 'light'}
    onmousedown={onSplitterDown}
  ></div>

  <!-- Chat panel column (resizable width) -->
  <div class="flex flex-col flex-shrink-0" style="width: {chatWidthPct}%">
    <ChatPanel theme={$theme} />
  </div>
</div>

<button
  onclick={() => {
    theme.update(toggleTheme)
  }}
  class="fixed top-2 right-2 z-20 px-2 py-1 rounded text-xs font-mono opacity-60 hover:opacity-100 transition-opacity"
  class:bg-[#44475a]={$theme === 'dracula'}
  class:text-[#f8f8f2]={$theme === 'dracula'}
  class:bg-[#eaeef2]={$theme === 'light'}
  class:text-[#24292f]={$theme === 'light'}
>
  {$theme === 'dracula' ? '☀' : '☾'}
</button>

{#if showSettings}
  <SettingsModal theme={$theme} onClose={() => (showSettings = false)} />
{/if}
