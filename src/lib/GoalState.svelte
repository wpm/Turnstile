<script lang="ts">
  import { isProofComplete, parseGoalText } from "./goalState";
  import GoalEntryView from "./GoalEntryView.svelte";

  let { content = "" }: { content?: string } = $props();

  const complete = $derived(isProofComplete(content));
  const goals = $derived(complete ? [] : parseGoalText(content));
</script>

<div class="goal-state">
  {#if goals.length === 0}
    <!-- A closed proof simply runs out of goals (ADR-0004); no banner, just a
         quiet empty-state. `complete` still distinguishes "no goals" (proof
         closed) from "nothing elaborated yet" so we never render `⊢ no goals`. -->
    <div class="goal-empty">
      {complete ? "No goals." : "No goal state available."}
    </div>
  {:else}
    {#each goals as goal, i (i)}
      {#if i > 0}
        <hr class="goal-separator" />
      {/if}
      <!-- The shared renderer in full form (ADR-0004): one rendering path for
           the goal, also used by the cursor card in compact form (#91). -->
      <GoalEntryView {goal} />
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

  .goal-separator {
    border: none;
    border-top: 1px solid var(--color-border);
    margin: 0.75rem 0;
  }
</style>
