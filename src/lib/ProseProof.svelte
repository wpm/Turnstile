<script lang="ts">
  let { content = "" }: { content?: string } = $props();
</script>

<!-- eslint-disable-next-line svelte/no-at-html-tags -- content is KaTeX-rendered HTML from our own backend -->
<div class="prose-proof">{@html content}</div>

<style>
  .prose-proof {
    height: 100%;
    overflow: auto;
    padding: 1rem;
    font-size: 0.875rem;
    color: var(--color-text);
  }
</style>
