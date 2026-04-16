<script lang="ts">
  type ProofView = 'formal' | 'prose'
  let { view, onToggle }: { view: ProofView; onToggle: () => void } = $props()
</script>

<button
  onclick={onToggle}
  aria-label={view === 'formal' ? 'Switch to Prose Proof' : 'Switch to Formal Proof'}
  title={view === 'formal' ? 'Prose Proof' : 'Formal Proof'}
  class="w-7 h-7 flex items-center justify-center rounded text-text-secondary
    hover:text-text-primary hover:bg-bg-tertiary transition-all"
>
  {#if view === 'formal'}
    <!-- text lines icon — shows what you'll switch to (prose) -->
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
    <!-- proof tree icon — shows what you'll switch to (formal) -->
    <svg
      xmlns="http://www.w3.org/2000/svg"
      fill="none"
      viewBox="0 0 24 24"
      stroke-width="1.5"
      stroke="currentColor"
      class="w-4 h-4"
    >
      <circle cx="12" cy="4" r="1.5" fill="currentColor" stroke="none" />
      <path stroke-linecap="round" stroke-linejoin="round" d="M12 5.5v3.5M12 9l-5 5m5-5l5 5" />
      <circle cx="7" cy="14" r="1.5" fill="currentColor" stroke="none" />
      <path stroke-linecap="round" stroke-linejoin="round" d="M7 15.5v2.5" />
      <circle cx="7" cy="19.5" r="1.5" fill="currentColor" stroke="none" />
      <circle cx="17" cy="14" r="1.5" fill="currentColor" stroke="none" />
      <path stroke-linecap="round" stroke-linejoin="round" d="M17 15.5l-2 2.5m2-2.5l2 2.5" />
      <circle cx="15" cy="19.5" r="1" fill="currentColor" stroke="none" />
      <circle cx="19" cy="19.5" r="1" fill="currentColor" stroke="none" />
    </svg>
  {/if}
</button>
