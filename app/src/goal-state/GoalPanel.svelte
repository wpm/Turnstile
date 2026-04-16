<script lang="ts">
  import { SvelteSet } from 'svelte/reactivity'
  import { parseBlocks } from './markdown'
  import { computeHighlightedPanelIndices } from '../formal-proof/editorHelpers'
  import GoalBlock from './GoalBlock.svelte'

  interface Props {
    goalText: string
    /**
     * Parallel to the flattened code-block lines in `goalText`. Each entry is
     * the 1-indexed Formal Proof line that would have produced that panel
     * line, or `null` if no such line exists.
     */
    goalLineToProofLine: (number | null)[]
    /** Current cursor line in the Formal Proof editor (0-indexed LSP). */
    cursorLine: number | null
    /** True when the Formal Proof editor has focus. */
    editorFocused: boolean
  }

  let { goalText, goalLineToProofLine, cursorLine, editorFocused }: Props = $props()

  let blocks = $derived(parseBlocks(goalText))

  /**
   * For each block, precompute the starting flat index and line count so we
   * can map (blockIdx, localLine) ↔ flatIdx without re-splitting block
   * contents on every render. Text blocks get `{ start: -1, count: 0 }`.
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

  let highlightedFlatIndices = $derived(
    computeHighlightedPanelIndices(goalLineToProofLine, cursorLine, editorFocused),
  )

  /** Per-block sets of highlighted local line indices (0-indexed). */
  let highlightedLinesPerBlock = $derived.by(() => {
    const result: SvelteSet<number>[] = []
    for (let blockIdx = 0; blockIdx < blocks.length; blockIdx++) {
      const extent = blockExtents[blockIdx]
      const out = new SvelteSet<number>()
      if (extent && extent.start >= 0) {
        for (let local = 0; local < extent.count; local++) {
          if (highlightedFlatIndices.has(extent.start + local)) out.add(local)
        }
      }
      result.push(out)
    }
    return result
  })
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
            <GoalBlock
              content={block.content}
              highlightedLines={highlightedLinesPerBlock[blockIdx] ?? new SvelteSet()}
            />
          </div>
        {/if}
      {/each}
    {/if}
  </div>
</div>
