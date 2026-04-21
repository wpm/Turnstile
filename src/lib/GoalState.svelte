<script lang="ts">
  import { parseGoalText } from "./goalState";

  let { content = "" }: { content?: string } = $props();

  const goals = $derived(parseGoalText(content));
</script>

<div class="goal-state">
  {#if goals.length === 0}
    <div class="goal-empty">No goal state available.</div>
  {:else}
    {#each goals as goal, i (i)}
      {#if i > 0}
        <hr class="goal-separator" />
      {/if}
      <div class="goal-block">
        {#if goal.caseLabel}
          <div class="goal-case">{goal.caseLabel}</div>
        {/if}
        {#if goal.hypotheses.length > 0}
          <div class="goal-hypotheses">
            {#each goal.hypotheses as hyp, j (j)}
              <div class="goal-hyp">{hyp}</div>
            {/each}
          </div>
        {/if}
        <div class="goal-turnstile" aria-label="proves">⊢</div>
        <div class="goal-conclusion">{goal.conclusion}</div>
      </div>
    {/each}
  {/if}
</div>

<style>
  .goal-state {
    height: 100%;
    overflow: auto;
    padding: 1rem;
    font-family: monospace;
    font-size: 0.875rem;
    color: var(--color-text);
  }

  .goal-empty {
    color: var(--color-text-muted);
    font-style: italic;
  }

  .goal-block {
    margin-bottom: 0.75rem;
  }

  .goal-case {
    font-size: 0.75rem;
    font-weight: 600;
    letter-spacing: 0.03em;
    color: var(--color-text-muted);
    margin-bottom: 0.5rem;
  }

  .goal-hypotheses {
    padding-left: 0.5rem;
    border-left: 2px solid var(--color-border);
    margin-bottom: 0.5rem;
  }

  .goal-hyp {
    white-space: pre-wrap;
    line-height: 1.5;
  }

  .goal-turnstile {
    font-size: 1rem;
    color: var(--color-accent);
    margin-bottom: 0.25rem;
    user-select: none;
  }

  .goal-conclusion {
    padding-left: 1rem;
    white-space: pre-wrap;
    line-height: 1.5;
  }

  .goal-separator {
    border: none;
    border-top: 1px solid var(--color-border);
    margin: 0.75rem 0;
  }
</style>
