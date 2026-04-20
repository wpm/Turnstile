<script lang="ts">
  import { tick } from "svelte";
  import Divider from "./Divider.svelte";

  type Role = "user" | "assistant";
  interface Message {
    id: number;
    role: Role;
    text: string;
  }

  let nextId = 0;

  let messages = $state<Message[]>([]);
  let input = $state("");
  let transcriptEl: HTMLDivElement | undefined;
  let inputPanelHeight = $state(80); // px
  let dragging = $state(false);
  let dragRect: DOMRect | null = null;
  let containerEl: HTMLDivElement;

  const MIN_INPUT_PANEL_HEIGHT = 48; // single line
  const MAX_INPUT_PANEL_RATIO = 0.5;

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

  function send() {
    const text = input.trim();
    if (!text) return;
    messages.push({ id: nextId++, role: "user", text });
    input = "";
    void scrollToBottom();
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      send();
    }
  }
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
          {message.text}
        </div>
      </div>
    {/each}
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
      onclick={send}
      disabled={!input.trim()}
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

  .bubble-row.assistant {
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
