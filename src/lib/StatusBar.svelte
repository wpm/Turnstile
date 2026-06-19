<script lang="ts">
  import { lspStatus, assistantStatus } from "./turnstile_messages";
  import { setupProgress } from "./appState";

  const status = $derived($setupProgress);
  const lsp = $derived($lspStatus);
  const assistant = $derived($assistantStatus);

  const text = $derived.by(() => {
    if (status) {
      return status.phase === "error"
        ? `Setup failed: ${status.message}`
        : `${status.message} (${String(status.progress_pct)}%)`;
    }
    if (!lsp) return "Starting…";
    return lsp.state === "connected" ? "Lean: connected" : lsp.message;
  });

  const kind = $derived.by(() => {
    if (status?.phase === "error") return "error";
    if (status) return "busy";
    return lsp?.state === "connected" ? "ok" : "busy";
  });

  // The Proof Assistant indicator reflects the live connection status. Before
  // the first status arrives it stays neutral ("…") rather than asserting a
  // connection state we don't yet know.
  const assistantConnected = $derived(assistant?.state === "connected");
  const assistantText = $derived.by(() => {
    if (!assistant) return "Proof Assistant: …";
    return assistantConnected
      ? "Proof Assistant: connected"
      : "Proof Assistant: disconnected";
  });
  const assistantKind = $derived.by(() => {
    if (!assistant) return "busy";
    return assistantConnected ? "ok" : "error";
  });
</script>

<div class="status-bar" role="status">
  <span class="status-group">
    <span class="dot dot--{kind}" aria-hidden="true"></span>
    <span class="status-text">{text}</span>
  </span>
  <span class="status-group status-group--right">
    <span class="dot dot--{assistantKind}" aria-hidden="true"></span>
    <span class="status-text">{assistantText}</span>
  </span>
</div>

<style>
  .status-bar {
    height: 1.5rem;
    flex-shrink: 0;
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0 0.75rem;
    font-size: 0.6875rem;
    color: var(--color-text-muted);
    background: var(--color-header-bg);
    border-top: 1px solid var(--color-border);
    user-select: none;
  }

  .status-group {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    min-width: 0;
  }

  /* Right-justify the Proof Assistant group; the left group keeps its place. */
  .status-group--right {
    margin-left: auto;
  }

  .status-text {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .dot {
    width: 0.5rem;
    height: 0.5rem;
    border-radius: 50%;
    flex-shrink: 0;
  }
  .dot--ok {
    background: #16a34a;
  }
  .dot--busy {
    background: #d97706;
    animation: status-pulse 1.2s ease-in-out infinite;
  }
  .dot--error {
    background: #dc2626;
  }

  @keyframes status-pulse {
    0%,
    100% {
      opacity: 0.4;
    }
    50% {
      opacity: 1;
    }
  }
</style>
