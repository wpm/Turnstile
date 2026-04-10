<script lang="ts">
  interface Props {
    goal: string
    visible: boolean
  }

  let { goal, visible }: Props = $props()
</script>

{#if visible}
  <div class="fixed bottom-0 right-0 w-96 max-h-64 overflow-auto bg-[#16161e] border-t border-l border-[#2a2b3d] p-3 font-mono text-sm text-[#c0caf5] z-10">
    <div class="text-xs text-[#565f89] mb-1 uppercase tracking-wide">Goal State</div>
    <pre class="whitespace-pre-wrap">{goal}</pre>
  </div>
{/if}
