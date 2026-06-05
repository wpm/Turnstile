<script lang="ts">
  import { onDestroy, onMount, tick } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import Divider from "./Divider.svelte";
  import { renderMathInMarkdown } from "./render";

  type Role = "user" | "assistant" | "error";
  interface Message {
    id: number;
    role: Role;
    text: string;
  }

  /** A turn as returned by the backend `get_transcript` command. */
  interface TranscriptTurn {
    role: "user" | "assistant" | "system";
    content: string;
    timestamp: number;
  }
  interface Transcript {
    summary: string | null;
    turns: TranscriptTurn[];
  }

  let nextId = 0;

  let messages = $state<Message[]>([]);
  let input = $state("");
  let busy = $state(false);
  /** Streaming text of the in-flight assistant turn ("" = thinking). */
  let streamingText = $state("");
  let transcriptEl: HTMLDivElement | undefined;
  let inputPanelHeight = $state(80); // px
  let dragging = $state(false);
  let dragRect: DOMRect | null = null;
  let containerEl: HTMLDivElement;

  const MIN_INPUT_PANEL_HEIGHT = 48; // single line
  const MAX_INPUT_PANEL_RATIO = 0.5;

  let unlistenDelta: (() => void) | undefined;
  let unlistenSessionLoaded: (() => void) | undefined;

  function onDragStart(e: MouseEvent) {
    dragging = true;
    dragRect = containerEl.getBoundingClientRect();
    e.preventDefault();
  }

  function onMouseMove(e: MouseEvent) {
    if (!dragging || !dragRect) return;
    const fromBottom = dragRect.bottom - e.clientY;
    const maxHeight = dragRect.height * MAX_INPUT_PANEL_RATIO;
    inputPanelHeight = Math.max(
      MIN_INPUT_PANEL_HEIGHT,
      Math.min(maxHeight, fromBottom),
    );
  }

  function onMouseUp() {
    dragging = false;
    dragRect = null;
  }

  async function scrollToBottom() {
    await tick();
    if (transcriptEl) transcriptEl.scrollTop = transcriptEl.scrollHeight;
  }

  /** Replace the local message list with the backend transcript. */
  async function loadTranscript() {
    try {
      const transcript = await invoke<Transcript>("get_transcript");
      messages = transcript.turns
        .filter((t) => t.role === "user" || t.role === "assistant")
        .map((t) => ({ id: nextId++, role: t.role as Role, text: t.content }));
      void scrollToBottom();
    } catch (e) {
      console.error("get_transcript failed", e);
    }
  }

  async function send() {
    const text = input.trim();
    if (!text || busy) return;
    messages.push({ id: nextId++, role: "user", text });
    input = "";
    busy = true;
    streamingText = "";
    void scrollToBottom();

    try {
      const response = await invoke<string>("send_message", {
        content: text,
      });
      messages.push({ id: nextId++, role: "assistant", text: response });
    } catch (e) {
      messages.push({ id: nextId++, role: "error", text: String(e) });
    } finally {
      busy = false;
      streamingText = "";
      void scrollToBottom();
    }
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      void send();
    }
  }

  onMount(async () => {
    await loadTranscript();
    unlistenDelta = await listen<string>("assistant-delta", (e) => {
      if (!busy) return;
      streamingText += e.payload;
      void scrollToBottom();
    });
    unlistenSessionLoaded = await listen("session-loaded", () => {
      void loadTranscript();
    });
  });

  onDestroy(() => {
    unlistenDelta?.();
    unlistenSessionLoaded?.();
  });
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="assistant"
  bind:this={containerEl}
  onmousemove={onMouseMove}
  onmouseup={onMouseUp}
  onmouseleave={onMouseUp}
>
  <div class="transcript" bind:this={transcriptEl}>
    {#each messages as message (message.id)}
      <div class="bubble-row {message.role}">
        <div
          class="bubble {message.role}"
          role="article"
          aria-label={message.role === "user" ? "You" : "Proof Assistant"}
        >
          {#if message.role === "assistant"}
            <!-- eslint-disable-next-line svelte/no-at-html-tags -- markdown+KaTeX rendered from our own backend -->
            {@html renderMathInMarkdown(message.text)}
          {:else}
            {message.text}
          {/if}
        </div>
      </div>
    {/each}
    {#if busy}
      <div class="bubble-row assistant">
        <div
          class="bubble assistant"
          role="article"
          aria-label="Proof Assistant"
        >
          {#if streamingText}
            <!-- eslint-disable-next-line svelte/no-at-html-tags -- markdown+KaTeX rendered from our own backend -->
            {@html renderMathInMarkdown(streamingText)}
          {:else}
            <span class="thinking" aria-label="Thinking"
              ><span></span><span></span><span></span></span
            >
          {/if}
        </div>
      </div>
    {/if}
    <div class="transcript-end"></div>
  </div>

  <Divider orientation="horizontal" {onDragStart} />

  <div class="input-panel" style="height: {inputPanelHeight}px">
    <textarea
      bind:value={input}
      onkeydown={onKeydown}
      placeholder="Message Proof Assistant…"
      rows="1"
    ></textarea>
    <button
      onclick={() => void send()}
      disabled={!input.trim() || busy}
      aria-label="Send message"
      title="Send"
    >
      <svg
        xmlns="http://www.w3.org/2000/svg"
        fill="none"
        viewBox="0 0 24 24"
        stroke-width="1.5"
        stroke="currentColor"
      >
        <path
          stroke-linecap="round"
          stroke-linejoin="round"
          d="M6 12 3.269 3.125A59.769 59.769 0 0 1 21.485 12 59.768 59.768 0 0 1 3.27 20.875L5.999 12Zm0 0h7.5"
        />
      </svg>
    </button>
  </div>
</div>

<style>
  .assistant {
    display: flex;
    flex-direction: column;
    height: 100%;
    overflow: hidden;
    user-select: none;
  }

  .transcript {
    flex: 1;
    overflow-y: auto;
    padding: 1rem;
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
  }

  .transcript-end {
    height: 0.5rem;
    flex-shrink: 0;
  }

  .bubble-row {
    display: flex;
  }

  .bubble-row.user {
    justify-content: flex-end;
  }

  .bubble-row.assistant,
  .bubble-row.error {
    justify-content: flex-start;
  }

  .bubble {
    max-width: 75%;
    padding: 0.5rem 0.75rem;
    border-radius: 1rem;
    font-size: 0.875rem;
    line-height: 1.5;
    white-space: pre-wrap;
    word-break: break-word;
    user-select: text;
  }

  /* outgoing message convention */
  .bubble.user {
    background: var(--color-accent);
    color: var(--color-accent-text);
    border-bottom-right-radius: 0.25rem;
  }

  /* incoming message convention */
  .bubble.assistant {
    background: var(--color-surface);
    color: var(--color-text);
    border: 1px solid var(--color-border);
    border-bottom-left-radius: 0.25rem;
    white-space: normal;
  }

  .bubble.error {
    background: color-mix(in srgb, #dc2626 12%, var(--color-surface));
    color: var(--color-text);
    border: 1px solid #dc2626;
    border-bottom-left-radius: 0.25rem;
  }

  /* Rendered markdown inside assistant bubbles */
  .bubble.assistant :global(p) {
    margin: 0 0 0.5rem;
  }
  .bubble.assistant :global(p:last-child) {
    margin-bottom: 0;
  }
  .bubble.assistant :global(pre) {
    background: var(--color-bg);
    border: 1px solid var(--color-border);
    border-radius: 0.375rem;
    padding: 0.5rem;
    overflow-x: auto;
    font-size: 0.8125rem;
  }
  .bubble.assistant :global(code) {
    font-family: ui-monospace, monospace;
    font-size: 0.8125rem;
  }
  .bubble.assistant :global(.katex-display) {
    margin: 0.5rem 0;
    overflow-x: auto;
  }

  /* "Thinking" indicator */
  .thinking {
    display: inline-flex;
    gap: 0.25rem;
    align-items: center;
    height: 1rem;
  }
  .thinking span {
    width: 0.375rem;
    height: 0.375rem;
    border-radius: 50%;
    background: var(--color-text-muted);
    animation: thinking-pulse 1.2s ease-in-out infinite;
  }
  .thinking span:nth-child(2) {
    animation-delay: 0.2s;
  }
  .thinking span:nth-child(3) {
    animation-delay: 0.4s;
  }
  @keyframes thinking-pulse {
    0%,
    80%,
    100% {
      opacity: 0.25;
    }
    40% {
      opacity: 1;
    }
  }

  .input-panel {
    flex-shrink: 0;
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.5rem;
    background: var(--color-header-bg);
  }

  textarea {
    flex: 1;
    height: 100%;
    resize: none;
    border: 1px solid var(--color-border);
    border-radius: 0.5rem;
    padding: 0.5rem 0.75rem;
    font-size: 0.875rem;
    font-family: inherit;
    background: var(--color-bg);
    color: var(--color-text);
    outline: none;
    overflow-y: auto;
    user-select: text;
  }

  textarea:focus {
    border-color: var(--color-accent);
  }

  button {
    flex-shrink: 0;
    width: 2rem;
    height: 2rem;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: 0.5rem;
    background: var(--color-accent);
    color: var(--color-accent-text);
    transition: background 0.15s;
  }

  button:hover:not(:disabled) {
    background: var(--color-accent-hover);
  }

  button:disabled {
    background: var(--color-border);
    cursor: default;
  }

  button svg {
    width: 1rem;
    height: 1rem;
  }
</style>
