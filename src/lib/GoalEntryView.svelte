<script lang="ts">
  import { hypothesisDelta, type GoalBlock } from "./goalState";

  // The single rendering path for one parsed goal (ADR-0004), reused by the
  // cursor-row card (compact) and the All-Goals panel (full). It operates on
  // the structured goal — case label, hypotheses, conclusion — so neither
  // caller re-splits goal text.
  //
  // - `full` (default): case label, the whole hypothesis stack, then the `⊢`
  //   conclusion. The shape the panel wants.
  // - `compact`: the conclusion with the hypothesis *delta* inline on one line
  //   where it fits — the shape the card wants, since a goal is typically
  //   horizontally short. Pass `previousHypotheses` (the before-state's
  //   hypotheses) to show only what this row added; omit it to show all.
  let {
    goal,
    compact = false,
    previousHypotheses,
  }: {
    goal: GoalBlock;
    compact?: boolean;
    previousHypotheses?: string[];
  } = $props();

  const shownHypotheses = $derived(
    compact
      ? hypothesisDelta(goal.hypotheses, previousHypotheses)
      : goal.hypotheses,
  );
</script>

{#if compact}
  <div class="goal-entry goal-entry--compact">
    {#if goal.caseLabel}
      <span class="goal-case goal-case--inline">{goal.caseLabel}</span>
    {/if}
    {#if shownHypotheses.length > 0}
      <span class="goal-hyp-inline">{shownHypotheses.join(", ")}</span>
    {/if}
    <span class="goal-turnstile" aria-label="proves">⊢</span>
    <span class="goal-conclusion goal-conclusion--inline"
      >{goal.conclusion}</span
    >
  </div>
{:else}
  <div class="goal-entry goal-block">
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
{/if}

<style>
  /* Full form — the stacked goal used by the All-Goals panel. */
  .goal-block {
    margin-bottom: 0.75rem;
    font-family: monospace;
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

  /* Compact form — one row where it fits, for the cursor card. */
  .goal-entry--compact {
    font-family: monospace;
    line-height: 1.5;
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: 0.4rem;
  }

  .goal-case--inline {
    margin-bottom: 0;
  }

  .goal-hyp-inline {
    color: var(--color-text-muted);
    white-space: pre-wrap;
  }

  .goal-entry--compact .goal-turnstile {
    margin-bottom: 0;
  }

  .goal-conclusion--inline {
    padding-left: 0;
    white-space: pre-wrap;
  }
</style>
