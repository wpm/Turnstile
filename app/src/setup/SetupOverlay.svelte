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
  <div
    role="status"
    aria-live="polite"
    aria-label="Application setup"
    class="fixed inset-0 flex flex-col items-center justify-center bg-bg-primary z-50"
  >
    <img src="/turnstile.svg" alt="Turnstile" class="w-16 h-16 mb-6" />
    <div class="text-text-primary text-sm mb-4">{message}</div>
    {#if !isError}
      <div
        role="progressbar"
        aria-label="Setup progress"
        aria-valuemin="0"
        aria-valuemax="100"
        aria-valuenow={progress > 0 ? progress : undefined}
        aria-valuetext={progress === 0 ? 'Starting…' : `${String(progress)}%`}
        class="w-64 h-1 bg-bg-tertiary rounded overflow-hidden"
      >
        {#if progress > 0}
          <div
            class="h-1 bg-progress rounded transition-all duration-300"
            style="width: {progress}%"
          ></div>
        {:else}
          <!-- Indeterminate shimmer while waiting to start -->
          <div class="h-1 bg-progress rounded animate-pulse" style="width: 30%"></div>
        {/if}
      </div>
    {:else}
      <div class="text-error text-xs mt-2">Setup failed. Check logs and restart.</div>
    {/if}
  </div>
{/if}
