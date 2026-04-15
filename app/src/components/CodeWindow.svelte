<script lang="ts">
  import { onMount, onDestroy } from 'svelte'
  import {
    mountCodeWindow,
    type CodeWindowHandle,
    type CodeWindowHoverFn,
  } from '../lib/mountCodeWindow'
  import type { ResolvedTheme } from '../lib/theme'

  interface Props {
    content: string
    theme: ResolvedTheme
    /** 1-indexed within this CodeWindow's own document, or null. */
    activeLine?: number | null
    onLineClick?: (line: number) => void
    fetchHover?: CodeWindowHoverFn
  }

  let { content, theme, activeLine = null, onLineClick, fetchHover }: Props = $props()

  let container: HTMLDivElement
  let handle: CodeWindowHandle | null = null

  onMount(() => {
    handle = mountCodeWindow(container, {
      initialTheme: theme,
      initialContent: content,
      onLineClick,
      fetchHover,
    })
  })

  onDestroy(() => {
    handle?.destroy()
  })

  // `setContent`, `setTheme`, and `setActiveLine` are idempotent (they
  // short-circuit when the value matches the current state), so these
  // effects can fire unguarded on every render.
  $effect(() => {
    handle?.setTheme(theme)
  })
  $effect(() => {
    handle?.setContent(content)
  })
  $effect(() => {
    handle?.setActiveLine(activeLine)
  })

  // Callbacks identity changes every render (Svelte re-creates closures);
  // forward the latest pair into the long-lived CM6 extensions.
  $effect(() => {
    handle?.setCallbacks({ onLineClick, fetchHover })
  })
</script>

<div bind:this={container} class="code-window"></div>

<style>
  .code-window :global(.cm-editor) {
    background: transparent;
  }
  .code-window :global(.cm-scroller) {
    font-size: 13px;
  }
</style>
