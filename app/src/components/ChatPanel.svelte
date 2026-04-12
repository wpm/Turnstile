<script lang="ts">
  import { onMount, onDestroy, tick } from 'svelte'
  import { invoke, listen } from '../lib/tauri'
  import type { ChatTurn } from '../lib/tauri'
  import { parseMathSegments, renderMath } from '../lib/math'
  import { highlightLean } from '../lib/leanHighlight'
  import { escapeHtml } from '../lib/chatUtils'

  // ---------------------------------------------------------------------------
  // State
  // ---------------------------------------------------------------------------

  type MessageItem = ChatTurn & { id: number }

  let messages = $state<MessageItem[]>([])
  let nextId = 0
  let inputText = $state('')
  let inputHeight = $state(80)
  let scrollAnchor: HTMLDivElement | null = $state(null)

  let unlisten: (() => void) | null = null

  // ---------------------------------------------------------------------------
  // Lifecycle
  // ---------------------------------------------------------------------------

  onMount(() => {
    void listen<ChatTurn>('chat-message-complete', (turn) => {
      messages = [...messages, { ...turn, id: nextId++ }]
    }).then((fn) => {
      unlisten = fn
    })

    // Scroll to bottom after mount
    tick()
      .then(scrollToBottom)
      .catch(() => undefined)

    // Re-scroll when window is resized
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
    unlisten?.()
  })

  // Auto-scroll when messages change or input height changes.
  // Accessing .length / the value establishes the reactive dependency in Svelte 5.
  $effect(() => {
    if (messages.length >= 0 && inputHeight >= 0) {
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
    const content = inputText.trim()
    if (!content) return

    messages = [...messages, { role: 'user', content, timestamp: Date.now(), id: nextId++ }]
    inputText = ''

    await invoke<null>('send_chat_message', { content }).catch(() => {
      /* backend not yet connected — the mock or real backend fires chat-message-complete */
    })
  }

  function onKeydown(e: KeyboardEvent): void {
    if (e.key === 'Enter' && !e.shiftKey && !e.altKey) {
      e.preventDefault()
      void sendMessage()
    }
    // Shift+Enter and Alt/Option+Enter fall through → default textarea newline
  }

  // ---------------------------------------------------------------------------
  // Resize handle drag
  // ---------------------------------------------------------------------------

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
    const delta = dragStartY - e.clientY // drag up = bigger input
    inputHeight = Math.min(400, Math.max(40, dragStartHeight + delta))
  }

  function onResizePointerUp(): void {
    dragging = false
  }

  // ---------------------------------------------------------------------------
  // Rich content rendering
  // ---------------------------------------------------------------------------

  /**
   * Render a message content string to HTML.
   *
   * Pipeline (applied left-to-right per line):
   * 1. Split on backtick spans → Lean highlighted code
   * 2. Split remaining text on $...$ / $$...$$ → KaTeX math
   * 3. Plain text → HTML-escaped
   */
  function renderContent(content: string): string {
    // Split on backtick spans first
    const parts = content.split(/(`[^`]+`)/)
    let html = ''

    for (const part of parts) {
      if (part.startsWith('`') && part.endsWith('`') && part.length > 2) {
        const code = part.slice(1, -1)
        html += `<code class="chat-lean-code">${highlightLean(code)}</code>`
      } else {
        // Render math segments in the remaining text
        const segments = parseMathSegments(part)
        for (const seg of segments) {
          if (seg.type === 'math') {
            html += renderMath(seg.content, seg.display)
          } else {
            html += escapeHtml(seg.content)
          }
        }
      }
    }

    return html
  }
</script>

<div
  class="chat-panel flex flex-col h-full border-l border-border-default bg-surface text-on-surface"
>
  <!-- Panel header -->
  <div
    class="flex items-center px-4 py-2.5 border-b border-border-default bg-surface-secondary shrink-0"
  >
    <span class="text-xs font-medium tracking-wide uppercase text-on-surface-secondary">Chat</span>
  </div>

  <!-- Message history -->
  <div class="chat-history flex-1 overflow-y-auto p-4 space-y-4">
    {#each messages as message (message.id)}
      {#if message.role === 'user'}
        <div
          class="chat-message chat-message-user rounded-lg rounded-tr-sm px-4 py-3 text-sm
            max-w-full break-words ml-8 bg-surface-tertiary"
        >
          <!-- eslint-disable-next-line svelte/no-at-html-tags -- content is sanitised via escapeHtml and rendered by KaTeX -->
          {@html renderContent(message.content)}
        </div>
      {:else}
        <div
          class="chat-message chat-message-assistant rounded-lg rounded-tl-sm px-4 py-3 text-sm
            max-w-full break-words mr-8 bg-surface-secondary"
        >
          <!-- eslint-disable-next-line svelte/no-at-html-tags -- content is sanitised via escapeHtml and rendered by KaTeX -->
          {@html renderContent(message.content)}
        </div>
      {/if}
    {/each}
    <!-- Scroll anchor — always keep at the bottom -->
    <div bind:this={scrollAnchor} class="chat-scroll-anchor h-0"></div>
  </div>

  <!-- Resize handle -->
  <div
    class="chat-resize-handle h-1.5 cursor-row-resize select-none bg-border-default
      hover:bg-border-active transition-colors"
    role="separator"
    aria-orientation="horizontal"
    onpointerdown={onResizePointerDown}
    onpointermove={onResizePointerMove}
    onpointerup={onResizePointerUp}
  ></div>

  <!-- Input area -->
  <div
    class="chat-input-area flex flex-col border-t border-border-default bg-surface-secondary shrink-0"
  >
    <!-- Resizable textarea region -->
    <div class="flex px-4 pt-4" style="height: {inputHeight}px;">
      <textarea
        class="chat-input flex-1 min-h-0 w-full resize-none rounded-lg border p-3 text-sm font-mono
          outline-none bg-surface-primary text-on-surface placeholder-placeholder border-border-default
          focus:border-border-active transition-colors"
        placeholder="Ask about your Lean proof…"
        bind:value={inputText}
        onkeydown={onKeydown}
      ></textarea>
    </div>
    <!-- Send bar — always visible below the textarea -->
    <div class="flex items-center justify-between px-4 pt-3 pb-5 shrink-0">
      <span class="text-xs text-on-surface-secondary">Enter to send · Shift+Enter for newline</span>
      <button
        onclick={() => void sendMessage()}
        aria-label="Send message"
        class="flex items-center gap-1.5 px-4 py-2 rounded-lg text-sm font-medium
          bg-accent text-on-accent hover:bg-accent-hover
          transition-all active:scale-95 disabled:opacity-40 disabled:cursor-not-allowed
          disabled:active:scale-100"
        disabled={!inputText.trim()}
      >
        <svg
          xmlns="http://www.w3.org/2000/svg"
          viewBox="0 0 24 24"
          fill="currentColor"
          class="w-4 h-4"
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
