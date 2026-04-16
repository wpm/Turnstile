<script lang="ts">
  import { onMount, onDestroy } from 'svelte'
  import { mountEditor } from './editor'
  import type { DiagnosticInfo, FileProgressRange, SemanticToken } from '../session/tauri'
  import type { ResolvedTheme } from '../setup/theme'

  interface Props {
    initialTheme: ResolvedTheme
    theme: ResolvedTheme
    diagnostics?: DiagnosticInfo[] | null
    semanticTokens?: SemanticToken[] | null
    fileProgress?: FileProgressRange[] | null
    wordWrap?: boolean
    onchange: (content: string) => void
    oncursorchange?: (line: number, col: number) => void
    onfocuschange?: (focused: boolean) => void
    ontogglewrap?: () => void
    onexternaldef?: (uri: string) => void
    currentUri?: () => string
  }

  let {
    initialTheme,
    theme,
    diagnostics = null,
    semanticTokens = null,
    fileProgress = null,
    wordWrap = false,
    onchange,
    oncursorchange,
    onfocuschange,
    ontogglewrap,
    onexternaldef,
    currentUri,
  }: Props = $props()

  let container: HTMLDivElement
  let handle = $state<ReturnType<typeof mountEditor> | null>(null)

  export function setContent(text: string): void {
    handle?.setContent(text)
  }

  export function jumpTo(line: number, character: number): void {
    handle?.jumpTo(line, character)
  }

  $effect(() => {
    handle?.setTheme(theme)
  })

  $effect(() => {
    handle?.setWordWrap(wordWrap)
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
    handle = mountEditor(container, initialTheme, {
      onChange: onchange,
      onCursorChange: oncursorchange,
      onFocusChange: onfocuschange,
      onToggleWrap: ontogglewrap,
      onExternalDef: onexternaldef,
      currentUri,
    })
    // Initial wrap state, applied after mount.
    handle.setWordWrap(wordWrap)
  })

  onDestroy(() => {
    handle?.destroy()
  })
</script>

<div bind:this={container} class="w-full h-full"></div>
