<script lang="ts">
  import { onMount } from 'svelte'
  import { invoke, listen } from './lib/tauri'
  import type { SetupProgressPayload, DiagnosticInfo, SemanticToken } from './lib/tauri'
  import Editor from './components/Editor.svelte'
  import GoalPanel from './components/GoalPanel.svelte'
  import SetupOverlay from './components/SetupOverlay.svelte'

  let goalText = $state('')
  let goalVisible = $state(false)
  let setupVisible = $state(true)
  let setupMessage = $state('Checking Lean installation...')
  let setupProgress = $state(0)
  let setupError = $state(false)

  let editorRef: Editor

  function handleChange(content: string) {
    invoke('update_document', { content }).catch(() => {/* LSP not yet connected */})
  }

  async function handleCursorMove(line: number, col: number) {
    try {
      const rendered = await invoke<string>('get_goal_state', { line, col })
      goalText = rendered ?? ''
      goalVisible = !!goalText
    } catch {
      // LSP not yet connected; ignore
    }
  }

  onMount(async () => {
    // Register listeners BEFORE calling start_lsp — same ordering constraint
    // as in the Rust/WASM version. Tauri events can arrive immediately after
    // start_lsp returns; any listener registered after would miss early events.
    const unlistenDiag = await listen<DiagnosticInfo[]>('lsp-diagnostics', (diags) => {
      editorRef?.applyDiagnostics(diags)
    })

    const unlistenTokens = await listen<SemanticToken[]>('lsp-semantic-tokens', (tokens) => {
      editorRef?.applySemanticTokens(tokens)
    })

    await startLsp()

    return () => {
      unlistenDiag()
      unlistenTokens()
    }
  })

  async function startLsp() {
    const status = await invoke<{ complete: boolean; project_path: string }>('get_setup_status')

    if (!status.complete) {
      // Register the setup-progress listener BEFORE invoking start_setup to avoid
      // missing the "ready" event if setup completes before the listener is registered.
      await new Promise<void>(async (resolve) => {
        const unlisten = await listen<SetupProgressPayload>('setup-progress', (p) => {
          setupMessage = p.message
          setupProgress = p.progress_pct
          if (p.phase === 'error') {
            setupError = true
            unlisten()
            resolve()
          } else if (p.phase === 'ready') {
            unlisten()
            resolve()
          }
        })
        await invoke('start_setup')
      })
    }

    setupVisible = false
    await invoke('start_lsp')
  }
</script>

<SetupOverlay visible={setupVisible} message={setupMessage} progress={setupProgress} isError={setupError} />

<div class="fixed inset-0 bg-[#1a1b2e]">
  <Editor
    bind:this={editorRef}
    onchange={handleChange}
    oncursormove={handleCursorMove}
  />
</div>

<GoalPanel goal={goalText} visible={goalVisible} />
