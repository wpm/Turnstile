<script lang="ts">
  import { onMount, onDestroy } from 'svelte'
  import { mountEditor } from '../lib/editor'
  import type { DiagnosticInfo, FileProgressRange, SemanticToken } from '../lib/tauri'
  import type { Theme } from '../lib/theme'

  interface Props {
    initialTheme: Theme
    theme: Theme
    diagnostics?: DiagnosticInfo[] | null
    semanticTokens?: SemanticToken[] | null
    fileProgress?: FileProgressRange[] | null
    onchange: (content: string) => void
  }

  let {
    initialTheme,
    theme,
    diagnostics = null,
    semanticTokens = null,
    fileProgress = null,
    onchange,
  }: Props = $props()

  let container: HTMLDivElement
  let handle = $state<ReturnType<typeof mountEditor> | null>(null)

  export function setContent(text: string): void {
    handle?.setContent(text)
  }

  $effect(() => {
    handle?.setTheme(theme)
  })

  $effect(() => {
    if (diagnostics) handle?.applyDiagnostics(diagnostics)
  })

  $effect(() => {
    if (semanticTokens) handle?.applySemanticTokens(semanticTokens)
  })

  $effect(() => {
    if (fileProgress !== null) handle?.applyFileProgress(fileProgress)
  })

  onMount(() => {
    handle = mountEditor(container, initialTheme, onchange)
  })

  onDestroy(() => {
    handle?.destroy()
  })
</script>

<div bind:this={container} class="w-full h-full"></div>
