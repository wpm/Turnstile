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
  <div class="fixed inset-0 flex flex-col items-center justify-center bg-surface z-50">
    <img src="/turnstile.svg" alt="Turnstile" class="w-16 h-16 mb-6" />
    <div class="text-on-surface text-sm mb-4">{message}</div>
    {#if !isError}
      <div class="w-64 h-1 bg-surface-tertiary rounded">
        <div
          class="h-1 bg-progress rounded transition-all duration-300"
          style="width: {progress}%"
        ></div>
      </div>
    {:else}
      <div class="text-error text-xs mt-2">Setup failed. Check logs and restart.</div>
    {/if}
  </div>
{/if}
