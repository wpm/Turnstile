<script lang="ts">
  import { parseBlocks } from '../lib/markdown'
  import { lspHoverGoalPanel, type HoverInfo } from '../lib/lspRequests'
  import CodeWindow from './CodeWindow.svelte'
  import type { ResolvedTheme } from '../lib/theme'

  interface Props {
    goalText: string
    /**
     * Parallel to the flattened code-block lines in `goalText`. Each entry is
     * the 1-indexed Formal Proof line that would have produced that panel
     * line, or `null` if no such line exists.
     */
    goalLineToProofLine: (number | null)[]
    theme: ResolvedTheme
    onHighlightLine?: (line: number | null) => void
  }

  let { goalText, goalLineToProofLine, theme, onHighlightLine }: Props = $props()

  let highlightedFlatIdx = $state<number | null>(null)
  let blocks = $derived(parseBlocks(goalText))

  // Reset highlight whenever the goal text changes.
  let prevGoalText = ''
  $effect(() => {
    if (goalText !== prevGoalText) {
      prevGoalText = goalText
      highlightedFlatIdx = null
    }
  })

  /**
   * For each block, precompute the starting flat index and line count so
   * template callbacks can convert (blockIdx, localLine) ↔ flatIdx without
   * re-splitting block contents on every render. Text blocks get
   * `{ start: -1, count: 0 }` as a marker.
   */
  let blockExtents = $derived.by(() => {
    const extents: { start: number; count: number }[] = []
    let running = 0
    for (const block of blocks) {
      if (block.type === 'code') {
        const count = block.content.split('\n').length
        extents.push({ start: running, count })
        running += count
      } else {
        extents.push({ start: -1, count: 0 })
      }
    }
    return extents
  })

  function flatIdxFor(blockIdx: number, localLine: number): number {
    return (blockExtents[blockIdx]?.start ?? 0) + localLine
  }

  function handleLineClick(flatIdx: number): void {
    if (highlightedFlatIdx === flatIdx) {
      highlightedFlatIdx = null
      onHighlightLine?.(null)
    } else {
      highlightedFlatIdx = flatIdx
      onHighlightLine?.(goalLineToProofLine[flatIdx] ?? null)
    }
  }

  /**
   * Resolve the active line (1-indexed, within the given block) for
   * CodeWindow. Returns null when the current highlight is in a different
   * block or no highlight is active.
   */
  function activeLineForBlock(blockIdx: number): number | null {
    if (highlightedFlatIdx === null) return null
    const extent = blockExtents[blockIdx]
    if (!extent || extent.start < 0) return null
    const local = highlightedFlatIdx - extent.start
    if (local < 0 || local >= extent.count) return null
    return local + 1
  }

  function fetchHoverFor(
    blockIdx: number,
    lspLine: number,
    character: number,
  ): Promise<HoverInfo | null> {
    return lspHoverGoalPanel(flatIdxFor(blockIdx, lspLine), character)
  }
</script>

<div class="flex flex-col h-full">
  <div class="flex-1 min-h-0 overflow-y-auto p-3">
    {#if !goalText}
      <div class="flex items-center justify-center h-full">
        <span class="text-[13px] text-text-secondary opacity-60">No goal state yet</span>
      </div>
    {:else}
      {#each blocks as block, blockIdx (blockIdx)}
        {#if block.type === 'text'}
          <p class="text-text-secondary text-[13px] mb-2 font-mono whitespace-pre-wrap">
            {block.content}
          </p>
        {:else}
          <div class="mb-2">
            <CodeWindow
              content={block.content}
              {theme}
              activeLine={activeLineForBlock(blockIdx)}
              onLineClick={(line: number) => {
                handleLineClick(flatIdxFor(blockIdx, line - 1))
              }}
              fetchHover={(line: number, character: number) =>
                fetchHoverFor(blockIdx, line, character)}
            />
          </div>
        {/if}
      {/each}
    {/if}
  </div>
</div>
