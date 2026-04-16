<script lang="ts">
  import { onMount } from 'svelte'
  import type { DocumentSymbolInfo } from './lspRequests'
  import { flattenSymbols, filterSymbols, symbolKindTag } from './symbolOutline'

  interface Props {
    symbols: DocumentSymbolInfo[]
    onJump: (line: number, character: number) => void
    onClose: () => void
  }

  const { symbols, onJump, onClose }: Props = $props()

  let query = $state('')
  let selected = $state(0)
  let inputEl = $state<HTMLInputElement | null>(null)
  let triggerEl: Element | null = null

  const flat = $derived(flattenSymbols(symbols))
  const filtered = $derived(filterSymbols(flat, query))

  // Reset selection when the filter changes.
  $effect(() => {
    void query
    selected = 0
  })

  onMount(() => {
    triggerEl = document.activeElement
    inputEl?.focus()
    return () => {
      if (triggerEl instanceof HTMLElement) triggerEl.focus()
    }
  })

  function handleKeydown(e: KeyboardEvent): void {
    if (e.key === 'Escape') {
      e.preventDefault()
      onClose()
      return
    }
    if (e.key === 'ArrowDown') {
      e.preventDefault()
      if (filtered.length > 0) selected = (selected + 1) % filtered.length
    } else if (e.key === 'ArrowUp') {
      e.preventDefault()
      if (filtered.length > 0) selected = (selected - 1 + filtered.length) % filtered.length
    } else if (e.key === 'Enter') {
      e.preventDefault()
      const entry = filtered[selected]
      if (entry) {
        onJump(entry.symbol.start_line, entry.symbol.start_character)
        onClose()
      }
    }
  }

  function handleBackdropClick(e: MouseEvent): void {
    if (e.target === e.currentTarget) onClose()
  }
</script>

<div
  role="dialog"
  aria-modal="true"
  aria-label="Symbol outline"
  class="fixed inset-0 z-50 flex items-start justify-center bg-black/40 pt-24"
  onclick={handleBackdropClick}
  onkeydown={handleKeydown}
  tabindex="-1"
  data-testid="symbol-outline-overlay"
>
  <div
    class="w-[540px] max-w-[90vw] max-h-[60vh] flex flex-col bg-bg-secondary border border-border rounded-md shadow-lg"
    role="listbox"
    aria-label="Symbols"
  >
    <input
      bind:this={inputEl}
      type="text"
      class="px-3 py-2 bg-bg-secondary text-text-primary border-b border-border focus:outline-none"
      placeholder="Jump to symbol..."
      bind:value={query}
      data-testid="symbol-outline-input"
    />

    <div class="overflow-y-auto" data-testid="symbol-outline-list">
      {#if filtered.length === 0}
        <div class="px-3 py-2 text-text-secondary text-sm">No matching symbols</div>
      {:else}
        {#each filtered as entry, i (`${String(entry.symbol.start_line)}:${entry.symbol.name}:${String(i)}`)}
          <button
            type="button"
            class="w-full text-left px-3 py-1.5 flex items-center gap-2 text-text-primary hover:bg-bg-tertiary"
            class:bg-accent={selected === i}
            class:text-white={selected === i}
            style={`padding-left: ${String(12 + entry.depth * 16)}px`}
            onclick={() => {
              onJump(entry.symbol.start_line, entry.symbol.start_character)
              onClose()
            }}
            onmouseenter={() => {
              selected = i
            }}
            data-testid="symbol-outline-item"
            data-symbol-name={entry.symbol.name}
          >
            <span class="text-xs uppercase text-text-secondary shrink-0 w-16"
              >{symbolKindTag(entry.symbol.kind)}</span
            >
            <span class="truncate">{entry.symbol.name}</span>
          </button>
        {/each}
      {/if}
    </div>
  </div>
</div>
