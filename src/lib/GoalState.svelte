<script lang="ts">
  let { content = "" }: { content?: string } = $props();
</script>

<div class="goal-state">{content}</div>

<style>
  .goal-state {
    height: 100%;
    overflow: auto;
    padding: 1rem;
    font-family: monospace;
    font-size: 0.875rem;
    color: var(--color-text);
    white-space: pre-wrap;
  }
</style>
