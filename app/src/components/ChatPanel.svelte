<script lang="ts">
  import { onMount, onDestroy, tick } from 'svelte'
  import { invoke, listen } from '../lib/tauri'
  import type { ChatTurn, SessionState } from '../lib/tauri'
  import type { Theme } from '../lib/theme'
  import { renderContent } from '../lib/renderContent'
  import { showError } from '../lib/errorNotification.svelte'

  // ---------------------------------------------------------------------------
  // Props
  // ---------------------------------------------------------------------------

  interface Props {
    theme: Theme
    onToggleTheme: () => void
  }

  let { theme, onToggleTheme }: Props = $props()

  // ---------------------------------------------------------------------------
  // State
  // ---------------------------------------------------------------------------

  type MessageItem = ChatTurn & { id: number }

  let messages = $state<MessageItem[]>([])
  let nextId = 0
  let inputText = $state('')
  let inputHeight = $state(120)
  let scrollAnchor: HTMLDivElement | null = $state(null)
  let streaming = $state(false)
  let streamingContent = $state('')

  const canSend = $derived(inputText.trim().length > 0 && !streaming)
  const sendBtnClass = $derived(
    canSend
      ? 'bg-accent text-white hover:bg-accent-hover'
      : 'bg-bg-tertiary text-text-secondary cursor-default',
  )

  let unlistenComplete: (() => void) | null = null
  let unlistenDelta: (() => void) | null = null
  let unlistenDone: (() => void) | null = null
  let unlistenSession: (() => void) | null = null

  // ---------------------------------------------------------------------------
  // Lifecycle
  // ---------------------------------------------------------------------------

  onMount(() => {
    void listen<ChatTurn>('chat-message-complete', (turn) => {
      streaming = false
      streamingContent = ''
      messages = [...messages, { ...turn, id: nextId++ }]
    }).then((fn) => {
      unlistenComplete = fn
    })

    void listen<string>('chat-stream-delta', (text) => {
      streamingContent += text
    }).then((fn) => {
      unlistenDelta = fn
    })

    void listen<unknown>('chat-stream-done', () => {
      streaming = false
      streamingContent = ''
    }).then((fn) => {
      unlistenDone = fn
    })

    void listen<SessionState>('session-loaded', (session) => {
      messages = session.transcript.map((t) => ({
        role: t.role,
        content: t.content,
        timestamp: t.timestamp,
        id: nextId++,
      }))
      streaming = false
      streamingContent = ''
    }).then((fn) => {
      unlistenSession = fn
    })

    tick()
      .then(scrollToBottom)
      .catch(() => undefined)

    const onResize = (): void => {
      tick()
        .then(scrollToBottom)
        .catch(() => undefined)
    }
    window.addEventListener('resize', onResize)

    return () => {
      window.removeEventListener('resize', onResize)
    }
  })

  onDestroy(() => {
    unlistenComplete?.()
    unlistenDelta?.()
    unlistenDone?.()
    unlistenSession?.()
  })

  // Scroll to bottom when a new message is added, streaming content changes,
  // or the input is resized. Track values explicitly so the effect only runs
  // when they change.
  let lastMessageCount = 0
  let lastInputHeight = 120 // matches inputHeight initial value
  let lastStreamingLen = 0
  $effect(() => {
    const countChanged = messages.length !== lastMessageCount
    const heightChanged = inputHeight !== lastInputHeight
    const streamChanged = streamingContent.length !== lastStreamingLen
    if (countChanged || heightChanged || streamChanged) {
      lastMessageCount = messages.length
      lastInputHeight = inputHeight
      lastStreamingLen = streamingContent.length
      tick()
        .then(scrollToBottom)
        .catch(() => undefined)
    }
  })

  function scrollToBottom(): void {
    if (scrollAnchor) scrollAnchor.scrollIntoView({ behavior: 'smooth' })
  }

  // ---------------------------------------------------------------------------
  // Send message
  // ---------------------------------------------------------------------------

  async function sendMessage(): Promise<void> {
    if (!canSend) return
    const content = inputText.trim()

    messages = [...messages, { role: 'user', content, timestamp: Date.now(), id: nextId++ }]
    inputText = ''
    streaming = true
    streamingContent = ''

    try {
      await invoke<null>('send_chat_message', { content })
    } catch (err: unknown) {
      const msg = err instanceof Error ? err.message : String(err)
      showError(`Failed to send message: ${msg}`)
      streaming = false
    }
  }

  function onKeydown(e: KeyboardEvent): void {
    if (e.key === 'Enter' && !e.shiftKey && !e.altKey) {
      e.preventDefault()
      void sendMessage()
    }
  }

  // ---------------------------------------------------------------------------
  // Resize handle drag
  // ---------------------------------------------------------------------------

  const INPUT_HEIGHT_MIN = 40
  const INPUT_HEIGHT_MAX = 400

  let dragging = false
  let dragStartY = 0
  let dragStartHeight = 0

  function onResizePointerDown(e: PointerEvent): void {
    dragging = true
    dragStartY = e.clientY
    dragStartHeight = inputHeight
    ;(e.currentTarget as HTMLElement).setPointerCapture(e.pointerId)
  }

  function onResizePointerMove(e: PointerEvent): void {
    if (!dragging) return
    const delta = dragStartY - e.clientY
    inputHeight = Math.min(INPUT_HEIGHT_MAX, Math.max(INPUT_HEIGHT_MIN, dragStartHeight + delta))
  }

  function onResizePointerUp(): void {
    dragging = false
  }

  const INPUT_HEIGHT_STEP = 10
  const INPUT_HEIGHT_STEP_LARGE = 40

  function onResizeKeydown(e: KeyboardEvent): void {
    const step = e.shiftKey ? INPUT_HEIGHT_STEP_LARGE : INPUT_HEIGHT_STEP
    if (e.key === 'ArrowUp') {
      e.preventDefault()
      inputHeight = Math.min(INPUT_HEIGHT_MAX, inputHeight + step)
    } else if (e.key === 'ArrowDown') {
      e.preventDefault()
      inputHeight = Math.max(INPUT_HEIGHT_MIN, inputHeight - step)
    } else if (e.key === 'Home') {
      e.preventDefault()
      inputHeight = INPUT_HEIGHT_MIN
    } else if (e.key === 'End') {
      e.preventDefault()
      inputHeight = INPUT_HEIGHT_MAX
    }
  }
</script>

<div class="chat-panel flex flex-col h-full bg-bg-primary text-text-primary">
  <!-- Panel header -->
  <div
    class="flex items-center justify-between px-4 py-2 border-b border-border bg-bg-secondary shrink-0"
  >
    <div class="flex items-center gap-2">
      <div class="w-2 h-2 rounded-full bg-accent opacity-80"></div>
      <span class="text-[13px] font-semibold text-text-primary tracking-wide uppercase opacity-70">
        Proof Assistant
      </span>
    </div>
    <!-- Theme toggle lives here so it's discoverable and doesn't float over content -->
    <button
      onclick={onToggleTheme}
      aria-label="Toggle theme"
      class="w-7 h-7 flex items-center justify-center rounded text-text-secondary
        hover:text-text-primary hover:bg-bg-tertiary transition-colors
        focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent"
    >
      {#if theme === 'dark'}
        <!-- Sun: switch to light -->
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
            d="M12 3v2.25m6.364.386-1.591 1.591M21 12h-2.25m-.386 6.364-1.591-1.591M12 18.75V21m-4.773-4.227-1.591 1.591M5.25 12H3m4.227-4.773L5.636 5.636M15.75 12a3.75 3.75 0 1 1-7.5 0 3.75 3.75 0 0 1 7.5 0Z"
          />
        </svg>
      {:else}
        <!-- Moon: switch to dark -->
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
            d="M21.752 15.002A9.72 9.72 0 0 1 18 15.75c-5.385 0-9.75-4.365-9.75-9.75 0-1.33.266-2.597.748-3.752A9.753 9.753 0 0 0 3 11.25C3 16.635 7.365 21 12.75 21a9.753 9.753 0 0 0 9.002-5.998Z"
          />
        </svg>
      {/if}
    </button>
  </div>

  <!-- Message history — aria-live so screen readers announce incoming messages -->
  <div
    class="chat-history flex-1 overflow-y-auto flex flex-col gap-3 pt-4 px-4 min-h-0"
    aria-live="polite"
    aria-relevant="additions"
    aria-label="Conversation history"
  >
    {#each messages as message (message.id)}
      {#if message.role === 'user'}
        <!-- User: right-aligned, accent-tinted background -->
        <div class="flex justify-end">
          <div
            class="chat-message-user rounded-xl rounded-tr-sm px-3.5 py-2.5 text-[13px] leading-relaxed
              max-w-[85%] break-words bg-accent/15 border border-accent/25 text-text-primary"
          >
            <!-- eslint-disable-next-line svelte/no-at-html-tags -->
            {@html renderContent(message.content)}
          </div>
        </div>
      {:else}
        <!-- Assistant: left-aligned, secondary background -->
        <div class="flex justify-start">
          <div
            class="chat-message-assistant rounded-xl rounded-tl-sm px-3.5 py-2.5 text-[13px] leading-relaxed
              max-w-[85%] break-words bg-bg-secondary border border-border text-text-primary"
          >
            <!-- eslint-disable-next-line svelte/no-at-html-tags -->
            {@html renderContent(message.content)}
          </div>
        </div>
      {/if}
    {/each}
    {#if streaming}
      <div class="flex justify-start">
        {#if streamingContent}
          <div
            class="chat-message-streaming rounded-xl rounded-tl-sm px-3.5 py-2.5 text-[13px] leading-relaxed
              max-w-[85%] break-words bg-bg-secondary border border-border text-text-primary"
          >
            <!-- eslint-disable-next-line svelte/no-at-html-tags -->
            {@html renderContent(streamingContent)}
          </div>
        {:else}
          <div
            class="chat-thinking-indicator rounded-xl rounded-tl-sm px-4 py-3
              bg-bg-secondary border border-border"
            aria-label="Assistant is thinking"
          >
            <div class="flex gap-1.5 items-center">
              <span class="thinking-dot w-1.5 h-1.5 rounded-full bg-text-secondary"></span>
              <span class="thinking-dot w-1.5 h-1.5 rounded-full bg-text-secondary"></span>
              <span class="thinking-dot w-1.5 h-1.5 rounded-full bg-text-secondary"></span>
            </div>
          </div>
        {/if}
      </div>
    {/if}
    <div class="shrink-0 h-4"></div>
    <div bind:this={scrollAnchor} class="chat-scroll-anchor h-0"></div>
  </div>

  <!-- Resize handle — visible grip with dots.
       role=separator with aria-value* and tabindex=0 is an interactive splitter per APG. -->
  <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div
    class="chat-resize-handle cursor-row-resize select-none shrink-0 flex items-center justify-center
      bg-bg-secondary border-t border-border hover:bg-bg-tertiary transition-colors
      focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent focus-visible:ring-inset"
    style="height: 10px"
    role="separator"
    aria-orientation="horizontal"
    aria-label="Resize input area"
    aria-valuenow={inputHeight}
    aria-valuemin={INPUT_HEIGHT_MIN}
    aria-valuemax={INPUT_HEIGHT_MAX}
    tabindex="0"
    onpointerdown={onResizePointerDown}
    onpointermove={onResizePointerMove}
    onpointerup={onResizePointerUp}
    onkeydown={onResizeKeydown}
  >
    <!-- Three horizontal dots as grip indicator -->
    <div class="flex gap-1">
      <span class="w-1 h-1 rounded-full bg-text-secondary opacity-40"></span>
      <span class="w-1 h-1 rounded-full bg-text-secondary opacity-40"></span>
      <span class="w-1 h-1 rounded-full bg-text-secondary opacity-40"></span>
    </div>
  </div>

  <!-- Input area -->
  <div
    class="chat-input-area flex flex-col gap-2 p-3 bg-bg-secondary shrink-0"
    style="height: {inputHeight}px; min-height: 60px"
  >
    <textarea
      class="chat-input flex-1 min-h-0 w-full resize-none rounded border border-border p-2 text-[13px]
        font-mono outline-none bg-bg-primary text-text-primary placeholder:text-text-secondary
        focus:border-accent transition-colors"
      aria-label="Message to proof assistant"
      placeholder="Ask about your Lean proof…"
      bind:value={inputText}
      onkeydown={onKeydown}
    ></textarea>
    <div class="flex items-center justify-between shrink-0">
      <span class="text-[11px] text-text-secondary"> Enter to send · Shift+Enter for newline </span>
      <button
        onclick={() => void sendMessage()}
        aria-label="Send message"
        class="flex items-center gap-1.5 px-3 py-1.5 rounded text-[12px] font-medium
          transition-all active:scale-95
          disabled:cursor-not-allowed disabled:active:scale-100
          focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent
          {sendBtnClass}"
        disabled={!canSend}
      >
        <svg
          xmlns="http://www.w3.org/2000/svg"
          viewBox="0 0 24 24"
          fill="currentColor"
          class="w-3.5 h-3.5"
        >
          <path
            d="M3.478 2.405a.75.75 0 00-.926.94l2.432 7.905H13.5a.75.75 0 010 1.5H4.984l-2.432 7.905a.75.75 0 00.926.94 60.519 60.519 0 0018.445-8.986.75.75 0 000-1.218A60.517 60.517 0 003.478 2.405z"
          />
        </svg>
        Send
      </button>
    </div>
  </div>
</div>

<style>
  .thinking-dot {
    animation: thinking-pulse 1.4s ease-in-out infinite;
  }
  .thinking-dot:nth-child(2) {
    animation-delay: 0.2s;
  }
  .thinking-dot:nth-child(3) {
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
</style>
