<script lang="ts">
  import { onMount } from 'svelte'
  import { invoke, listen } from './lib/tauri'
  import type { SetupProgressPayload, DiagnosticInfo, SemanticToken } from './lib/tauri'
  import Editor from './components/Editor.svelte'
  import SetupOverlay from './components/SetupOverlay.svelte'
  import ChatPanel from './components/ChatPanel.svelte'
  import { theme, toggleTheme } from './lib/theme'

  let setupVisible = $state(true)
  let setupMessage = $state('Checking Lean installation...')
  let setupProgress = $state(0)
  let setupError = $state(false)
  let diagnostics = $state<DiagnosticInfo[] | null>(null)
  let semanticTokens = $state<SemanticToken[] | null>(null)

  function handleChange(content: string): void {
    invoke('update_document', { content }).catch(() => {
      /* LSP not yet connected */
    })
  }

  onMount(() => {
    // Register listeners BEFORE calling start_lsp — same ordering constraint
    // as in the Rust/WASM version. Tauri events can arrive immediately after
    // start_lsp returns; any listener registered after would miss early events.
    const diagPromise = listen<DiagnosticInfo[]>('lsp-diagnostics', (diags) => {
      diagnostics = diags
    })
    const tokensPromise = listen<SemanticToken[]>('lsp-semantic-tokens', (tokens) => {
      semanticTokens = tokens
    })

    void Promise.all([diagPromise, tokensPromise]).then(([unlistenDiag, unlistenTokens]) => {
      void startLsp()
      return () => {
        unlistenDiag()
        unlistenTokens()
      }
    })
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
  }
</script>

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

  <!-- Chat panel column (fixed width, themed border) -->
  <div
    class="w-80 flex flex-col border-l"
    class:border-[#44475a]={$theme === 'dracula'}
    class:border-[#d0d7de]={$theme === 'light'}
  >
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
