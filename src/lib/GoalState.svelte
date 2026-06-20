<script lang="ts">
  import { goalCache } from "./turnstile_messages";
  import GoalEntryView from "./GoalEntryView.svelte";

  // The All-Goals panel (ADR-0004): the entire per-row goal cache rendered at
  // once, for the reader who wants the whole stack. Served from the same cache
  // as the cursor-row card (#91) — no extra Lean queries, instant when toggled.
  //
  // Only rows with a fresh goal contribute; stale rows are mid-elaboration and
  // empty rows carry no goal. A closed proof simply runs out of goals (the
  // green "proof complete" banner is gone) — shown as a quiet empty-state.
  const freshRows = $derived(
    [...$goalCache.values()]
      .filter((r) => r.status === "fresh" && r.goals.length > 0)
      .sort((a, b) => a.row - b.row),
  );
</script>

<!-- data-select-scope: Command-A selects this panel's goals only (#113). -->
<div class="goal-state" data-select-scope>
  {#if freshRows.length === 0}
    <div class="goal-empty">
      {$goalCache.size > 0 ? "No goals." : "No goal state available."}
    </div>
  {:else}
    {#each freshRows as row (row.row)}
      <div class="goal-row">
        <div class="goal-row-label">line {row.row}</div>
        {#each row.goals as goal, i (i)}
          {#if i > 0}
            <hr class="goal-separator" />
          {/if}
          <GoalEntryView {goal} />
        {/each}
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
    /* Override the app shell's user-select: none so the goals can be selected
       and copied (and scoped Select All can highlight them). */
    user-select: text;
  }

  .goal-empty {
    color: var(--color-text-muted);
    font-style: italic;
  }

  .goal-row {
    margin-bottom: 1rem;
  }

  .goal-row-label {
    font-size: 0.6875rem;
    font-weight: 600;
    letter-spacing: 0.03em;
    text-transform: uppercase;
    color: var(--color-text-muted);
    margin-bottom: 0.25rem;
  }

  .goal-separator {
    border: none;
    border-top: 1px solid var(--color-border);
    margin: 0.75rem 0;
  }
</style>
