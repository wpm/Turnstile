<script lang="ts">
  import { onDestroy, onMount, tick } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { EditorState, Compartment } from "@codemirror/state";
  import {
    EditorView,
    keymap,
    placeholder as placeholderExt,
  } from "@codemirror/view";
  import {
    defaultKeymap,
    history,
    historyKeymap,
    insertNewline,
  } from "@codemirror/commands";
  import Divider from "./Divider.svelte";
  import { renderMathInMarkdown } from "./render";
  import { assistantStatus } from "./turnstile_messages";
  import { unicodeAbbreviationsOnly } from "./unicodeAbbreviations";

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

  // Disable input while the assistant is disconnected (no usable backend), so
  // the user can't send a message that will only fail. The status arrives via
  // the `assistantStatus` store (#58); before the first status we treat it as
  // usable so a slow status emit doesn't lock the input on a healthy app.
  let disconnected = $state(false);
  const placeholder = $derived(
    disconnected
      ? "Proof Assistant unavailable — set your API key in Settings"
      : "Message Proof Assistant…",
  );
  /** Streaming text of the in-flight assistant turn ("" = thinking). */
  let streamingText = $state("");
  let transcriptEl: HTMLDivElement | undefined;
  let inputPanelHeight = $state(80); // px
  let dragging = $state(false);
  let dragRect: DOMRect | null = null;
  let containerEl: HTMLDivElement;

  // The message entry is a CodeMirror instance (not a plain <textarea>) so it
  // shares the `\`-escape unicode glyph dropdown with the Formal Proof editor
  // (#98). `input` mirrors the editor's document for the send guards below.
  let inputEl: HTMLDivElement;
  let inputView: EditorView | undefined;
  const editableCompartment = new Compartment();
  const placeholderCompartment = new Compartment();

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

  /** Replace the editor's document, keeping the `input` mirror in sync. */
  function setInput(value: string) {
    input = value;
    if (inputView && inputView.state.doc.toString() !== value) {
      inputView.dispatch({
        changes: { from: 0, to: inputView.state.doc.length, insert: value },
      });
    }
  }

  async function send() {
    const text = input.trim();
    if (!text || busy || disconnected) return;
    messages.push({ id: nextId++, role: "user", text });
    setInput("");
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

  const unsubscribeStatus = assistantStatus.subscribe((status) => {
    // Null (no status yet) is treated as usable; only an explicit
    // "disconnected" disables the input.
    disconnected = status?.state === "disconnected";
  });

  /**
   * Build the message-entry editor. Prose surface (#98): the unicode glyph
   * dropdown is the *only* completion source, and native browser spell-check is
   * enabled via content attributes — the `\`-dropdown and the spelling squiggle
   * are separate layers and coexist. Enter sends; Shift-Enter inserts a newline.
   * `autocompletion` (in `unicodeAbbreviationsOnly`) registers its accept-on-
   * Enter binding at Prec.highest, so while the glyph dropdown is open Enter
   * accepts the glyph; it only falls through to send when the dropdown is shut.
   */
  function createInputEditor() {
    inputView = new EditorView({
      state: EditorState.create({
        extensions: [
          history(),
          keymap.of([
            {
              key: "Enter",
              run: () => {
                void send();
                return true;
              },
            },
            { key: "Shift-Enter", run: insertNewline },
            ...defaultKeymap,
            ...historyKeymap,
          ]),
          unicodeAbbreviationsOnly,
          EditorView.lineWrapping,
          editableCompartment.of(EditorView.editable.of(!disconnected)),
          placeholderCompartment.of(placeholderExt(placeholder)),
          // Prose entry: opt into the native browser spell-check layer that
          // CodeMirror ships with off.
          EditorView.contentAttributes.of({
            spellcheck: "true",
            autocorrect: "on",
            autocapitalize: "on",
            "aria-label": "Message Proof Assistant",
          }),
          EditorView.theme({
            "&": { height: "100%", fontSize: "0.875rem" },
            ".cm-scroller": {
              overflow: "auto",
              fontFamily: "inherit",
              lineHeight: "1.5",
            },
            ".cm-content": { padding: "0.5rem 0.75rem" },
            "&.cm-focused": { outline: "none" },
          }),
          EditorView.updateListener.of((update) => {
            if (update.docChanged) input = update.state.doc.toString();
          }),
        ],
      }),
      parent: inputEl,
    });
  }

  onMount(async () => {
    createInputEditor();
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
    unsubscribeStatus();
    inputView?.destroy();
  });

  // Disconnecting locks the entry and swaps in the explanatory placeholder;
  // reconnecting restores both. Reconfigured through compartments so the
  // editor's document and history survive the toggle.
  $effect(() => {
    inputView?.dispatch({
      effects: [
        editableCompartment.reconfigure(EditorView.editable.of(!disconnected)),
        placeholderCompartment.reconfigure(placeholderExt(placeholder)),
      ],
    });
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
  <!-- data-select-scope: Command-A selects the transcript only (#113); the
       entry below is a CodeMirror editor with its own in-editor Select All. -->
  <div class="transcript" bind:this={transcriptEl} data-select-scope>
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
    <div class="input-editor" class:disconnected bind:this={inputEl}></div>
    <button
      onclick={() => void send()}
      disabled={!input.trim() || busy || disconnected}
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
    /* The transcript is a scrollable column flex container. In WebKit (the
       macOS Tauri webview) its rows otherwise get a stretched used height —
       too short (clipping the bubble's text) when the turns overflow, too tall
       (a bubble with empty space below the text) when they don't. Pinning each
       row to its content height sizes the bubble to exactly its text and lets
       the transcript scroll. */
    flex: 0 0 auto;
    height: fit-content;
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

  /* CodeMirror message entry. Mirrors the old <textarea> chrome; the editor's
     own theme (set in createInputEditor) handles padding/scroll/wrap. */
  .input-editor {
    flex: 1;
    height: 100%;
    min-width: 0;
    border: 1px solid var(--color-border);
    border-radius: 0.5rem;
    background: var(--color-bg);
    color: var(--color-text);
    overflow: hidden;
    user-select: text;
  }

  .input-editor :global(.cm-editor) {
    height: 100%;
    background: transparent;
  }

  .input-editor :global(.cm-content) {
    caret-color: var(--color-text);
  }

  .input-editor:focus-within {
    border-color: var(--color-accent);
  }

  /* Disconnected: the entry is read-only and reads as disabled. */
  .input-editor.disconnected {
    background: var(--color-header-bg);
    cursor: not-allowed;
  }

  .input-editor.disconnected :global(.cm-content) {
    color: var(--color-text-muted);
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
