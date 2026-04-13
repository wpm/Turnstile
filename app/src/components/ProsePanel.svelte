<script lang="ts">
  let {
    proseHtml,
    generating,
    fontSize,
  }: {
    proseHtml: string
    generating: boolean
    fontSize: number
  } = $props()
</script>

<div class="relative h-full overflow-auto bg-bg-primary" data-testid="prose-panel">
  {#if !proseHtml}
    <div class="flex items-center justify-center h-full px-8">
      <p
        class="text-text-secondary text-sm text-center leading-relaxed max-w-md"
        style="font-size: {fontSize}px"
      >
        Toggle to Formal Proof, write your proof, then toggle back to see it in prose.
      </p>
    </div>
  {:else}
    <div class="prose-content p-6" style="font-size: {fontSize}px">
      <!-- eslint-disable-next-line svelte/no-at-html-tags -- proseHtml is generated internally by renderContent, not user-supplied -->
      {@html proseHtml}
    </div>
  {/if}

  {#if generating}
    <div
      class="absolute inset-0 bg-bg-primary/60 pointer-events-auto"
      data-testid="prose-generating-overlay"
    ></div>
  {/if}
</div>

<style>
  .prose-content :global(p) {
    margin-bottom: 0.75em;
    line-height: 1.7;
  }

  .prose-content :global(strong) {
    font-weight: 600;
    color: var(--text-primary);
  }

  .prose-content :global(em) {
    font-style: italic;
  }

  .prose-content :global(pre) {
    margin: 1em 0;
    padding: 0.75em 1em;
    border-radius: 6px;
    background: var(--bg-secondary);
    overflow-x: auto;
  }

  .prose-content :global(code) {
    font-family: var(--font-mono, monospace);
  }

  .prose-content :global(.katex-display) {
    margin: 1em 0;
    overflow-x: auto;
  }
</style>
