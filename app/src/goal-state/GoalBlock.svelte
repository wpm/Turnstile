<script lang="ts">
  import { parseGoalLine } from './goalLineParse'

  interface Props {
    content: string
    /** Set of 0-indexed line indices to highlight. */
    highlightedLines: Set<number>
  }

  let { content, highlightedLines }: Props = $props()

  let parsedLines = $derived(content.split('\n').map(parseGoalLine))
</script>

<div class="goal-block">
  {#each parsedLines as segments, idx (idx)}
    <div class="goal-line" class:goal-line-active={highlightedLines.has(idx)}>
      {#each segments as segment, segIdx (segIdx)}
        {#if segment.kind === 'name'}
          <span class="goal-hyp-name">{segment.text}</span>
        {:else if segment.kind === 'turnstile'}
          <span class="goal-turnstile">{segment.text}</span>
        {:else}
          {segment.text}
        {/if}
      {/each}
    </div>
  {/each}
</div>

<style>
  .goal-block {
    font-family: var(--font-mono);
    font-size: 13px;
    line-height: 1.5;
    color: var(--text-primary);
  }

  .goal-line {
    white-space: pre-wrap;
  }

  .goal-line-active {
    border-bottom: 2px solid var(--accent);
  }

  .goal-hyp-name {
    color: var(--accent);
  }

  .goal-turnstile {
    color: var(--accent);
  }
</style>
