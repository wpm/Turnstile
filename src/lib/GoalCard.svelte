<script lang="ts">
  import GoalEntryView from "./GoalEntryView.svelte";
  import type { GoalEntry } from "./GoalEntry";

  // The cursor-row goal card (ADR-0004): the after-state of the cursor's row,
  // rendered compactly. Its right edge is pinned to the separator rail and it
  // grows leftward and downward, never crossing into the Prose pane. Purely
  // presentational — FormalProof owns the cache lookup and positioning.
  //
  // `goals` is the row's open goals; the first is shown on the card and the
  // rest become a `+N` notch on the rail (the brief unfocused multi-goal
  // moment). `top` is the card's vertical offset within the editor host.
  let {
    goals,
    top,
    dimmed = false,
  }: {
    goals: GoalEntry[];
    top: number;
    dimmed?: boolean;
  } = $props();

  const primary = $derived(goals[0]);
  const extra = $derived(Math.max(0, goals.length - 1));
</script>

{#if primary}
  <div
    class="goal-card"
    class:goal-card--dimmed={dimmed}
    style="top: {top}px"
    role="status"
    aria-label="goal state at cursor"
  >
    {#if extra > 0}
      <span
        class="goal-card-notch"
        title="{extra} more goal{extra > 1 ? 's' : ''}">+{extra}</span
      >
    {/if}
    <div class="goal-card-body">
      <GoalEntryView goal={primary} compact />
    </div>
  </div>
{/if}

<style>
  /* Right edge pinned to the rail; grows leftward (right-anchored) and
     downward. Never crosses the rail into the Prose pane. */
  .goal-card {
    position: absolute;
    right: 0;
    max-width: min(90%, 36rem);
    z-index: 5;
    background: var(--color-surface, var(--color-bg));
    border: 1px solid var(--color-border);
    border-right: none;
    border-radius: 0.375rem 0 0 0.375rem;
    box-shadow: -2px 2px 8px rgb(0 0 0 / 12%);
    padding: 0.375rem 0.625rem;
    font-size: 0.8125rem;
    pointer-events: none;
    transition: opacity 0.12s ease;
  }

  /* While the cursor row is being edited, fade the card rather than letting it
     fight the text being typed under it. */
  .goal-card--dimmed {
    opacity: 0.25;
  }

  .goal-card-body {
    overflow-x: auto;
    pointer-events: auto;
  }

  /* The `+N` notch sits on the rail at the card's pinned edge. */
  .goal-card-notch {
    position: absolute;
    top: -0.5rem;
    right: 0;
    font-size: 0.6875rem;
    font-weight: 600;
    line-height: 1;
    padding: 0.125rem 0.25rem;
    border-radius: 0.25rem;
    color: var(--color-accent-text, #fff);
    background: var(--color-accent);
  }
</style>
