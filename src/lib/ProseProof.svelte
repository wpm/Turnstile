<script lang="ts">
  let {
    content = "",
    generating = false,
  }: { content?: string; generating?: boolean } = $props();
</script>

<div class="prose-proof" class:prose-generating={generating}>
  <!-- eslint-disable-next-line svelte/no-at-html-tags -- content is KaTeX-rendered HTML from our own backend -->
  {@html content}
</div>

<style>
  .prose-proof {
    height: 100%;
    overflow: auto;
    padding: 1rem;
    font-size: 0.875rem;
    color: var(--color-text);
  }

  /* Whole-window blink while prose is being generated — mirrors the
     per-line .cm-elaborating pulse in the formal proof editor. */
  .prose-proof.prose-generating {
    animation: prose-generating-pulse 1.2s ease-in-out infinite;
  }

  @keyframes prose-generating-pulse {
    0%,
    100% {
      background-color: transparent;
    }
    50% {
      background-color: color-mix(
        in srgb,
        var(--color-accent) 12%,
        transparent
      );
    }
  }
</style>
