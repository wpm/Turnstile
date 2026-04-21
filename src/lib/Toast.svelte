<script lang="ts">
  import { severityIcon } from "./severity";

  export interface ToastItem {
    id: number;
    severity: "error" | "warning" | "info" | "log";
    message: string;
  }

  let {
    toasts,
    onDismiss,
  }: {
    toasts: ToastItem[];
    onDismiss: (id: number) => void;
  } = $props();
</script>

{#if toasts.length > 0}
  <div class="toast-container" role="log" aria-live="polite">
    {#each toasts as toast (toast.id)}
      <div class="toast toast--{toast.severity}" role="alert">
        <span class="toast-icon" aria-hidden="true"
          >{severityIcon(toast.severity)}</span
        >
        <span class="toast-message">{toast.message}</span>
        <button
          class="toast-dismiss"
          aria-label="Dismiss"
          onclick={() => {
            onDismiss(toast.id);
          }}>×</button
        >
      </div>
    {/each}
  </div>
{/if}

<style>
  .toast-container {
    position: fixed;
    bottom: 1.25rem;
    right: 1.25rem;
    z-index: 9999;
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
    max-width: 420px;
    pointer-events: none;
  }

  .toast {
    display: flex;
    align-items: flex-start;
    gap: 0.5rem;
    padding: 0.625rem 0.75rem;
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-left-width: 3px;
    border-radius: 4px;
    box-shadow:
      0 2px 8px rgba(0, 0, 0, 0.12),
      0 1px 2px rgba(0, 0, 0, 0.06);
    font-size: 0.8125rem;
    font-family: system-ui, sans-serif;
    color: var(--color-text);
    pointer-events: all;
    animation: toast-in 120ms ease-out;
  }

  @keyframes toast-in {
    from {
      opacity: 0;
      transform: translateY(6px);
    }
    to {
      opacity: 1;
      transform: translateY(0);
    }
  }

  .toast--error {
    border-left-color: var(--cm-diag-error);
  }
  .toast--warning {
    border-left-color: var(--cm-diag-warning);
  }
  .toast--info {
    border-left-color: var(--cm-diag-info);
  }
  .toast--log {
    border-left-color: var(--color-border);
  }

  .toast-icon {
    font-size: 0.75rem;
    line-height: 1.5;
    flex-shrink: 0;
  }

  .toast--error .toast-icon {
    color: var(--cm-diag-error);
  }
  .toast--warning .toast-icon {
    color: var(--cm-diag-warning);
  }
  .toast--info .toast-icon {
    color: var(--cm-diag-info);
  }
  .toast--log .toast-icon {
    color: var(--color-text-muted);
  }

  .toast-message {
    flex: 1;
    line-height: 1.4;
    word-break: break-word;
    white-space: pre-wrap;
  }

  .toast-dismiss {
    flex-shrink: 0;
    background: none;
    border: none;
    cursor: pointer;
    color: var(--color-text-muted);
    font-size: 1rem;
    line-height: 1;
    padding: 0 0 0 0.25rem;
    margin-top: -0.0625rem;
  }

  .toast-dismiss:hover {
    color: var(--color-text);
  }
</style>
