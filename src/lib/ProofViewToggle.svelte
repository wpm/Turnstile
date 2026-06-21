<script lang="ts">
  import type { ProofView } from "./types";
  let { view, onToggle }: { view: ProofView; onToggle: () => void } = $props();
</script>

<button
  onclick={onToggle}
  aria-label={view === "assistant"
    ? "Switch to Prose Proof"
    : "Switch to Proof Assistant"}
  title={view === "assistant" ? "Prose Proof" : "Proof Assistant"}
  class="w-7 h-7 flex items-center justify-center rounded text-text-secondary
    hover:text-text-primary hover:bg-bg-tertiary transition-all"
>
  {#if view === "assistant"}
    <!-- text lines icon — shows what you'll switch to (Prose Proof) -->
    <svg
      xmlns="http://www.w3.org/2000/svg"
      fill="none"
      viewBox="0 0 24 24"
      stroke-width="1.5"
      stroke="currentColor"
      class="w-4 h-4"
    >
      <path
        stroke-linecap="round"
        stroke-linejoin="round"
        d="M3.75 6.75h16.5M3.75 12h16.5m-16.5 5.25h10.5"
      />
    </svg>
  {:else}
    <!-- speech-balloon icon — shows what you'll switch to (Proof Assistant) -->
    <svg
      xmlns="http://www.w3.org/2000/svg"
      fill="none"
      viewBox="0 0 24 24"
      stroke-width="1.5"
      stroke="currentColor"
      class="w-4 h-4"
    >
      <path
        stroke-linecap="round"
        stroke-linejoin="round"
        d="M2.25 12c0 3.728 3.694 6.75 8.25 6.75.74 0 1.457-.08 2.143-.23l4.107 1.18-1.05-3.36C18.32 15.99 18.75 14.55 18.75 12c0-3.728-3.694-6.75-8.25-6.75S2.25 8.272 2.25 12Z"
      />
    </svg>
  {/if}
</button>
