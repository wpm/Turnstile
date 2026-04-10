<script lang="ts">
  interface Props {
    message: string
    progress: number
    visible: boolean
    isError: boolean
  }

  let { message, progress, visible, isError }: Props = $props()
</script>

{#if visible}
  <div class="fixed inset-0 flex flex-col items-center justify-center bg-[#1a1b2e] z-50">
    <img src="/turnstile.svg" alt="Turnstile" class="w-16 h-16 mb-6" />
    <div class="text-[#c0caf5] text-sm mb-4">{message}</div>
    {#if !isError}
      <div class="w-64 h-1 bg-[#2a2b3d] rounded">
        <div
          class="h-1 bg-[#7aa2f7] rounded transition-all duration-300"
          style="width: {progress}%"
        ></div>
      </div>
    {:else}
      <div class="text-[#f7768e] text-xs mt-2">Setup failed. Check logs and restart.</div>
    {/if}
  </div>
{/if}
