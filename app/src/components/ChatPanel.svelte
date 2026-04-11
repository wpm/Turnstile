<script lang="ts">
  import { onMount, onDestroy, tick } from 'svelte'
  import { invoke, listen } from '../lib/tauri'
  import type { ChatTurn } from '../lib/tauri'
  import type { Theme } from '../lib/theme'
  import { parseMathSegments, renderMath } from '../lib/math'
  import { highlightLean } from '../lib/leanHighlight'
  import { escapeHtml } from '../lib/chatUtils'

  interface Props {
    theme: Theme
  }

  let { theme }: Props = $props()

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
    tick().then(scrollToBottom).catch(() => undefined)

    // Re-scroll when window is resized
    const onResize = (): void => {
      tick().then(scrollToBottom).catch(() => undefined)
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
      tick().then(scrollToBottom).catch(() => undefined)
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
  class="chat-panel flex flex-col h-full"
  class:bg-[#282a36]={theme === 'dracula'}
  class:text-[#f8f8f2]={theme === 'dracula'}
  class:bg-white={theme === 'light'}
  class:text-[#24292f]={theme === 'light'}
>
  <!-- Message history -->
  <div class="chat-history flex-1 overflow-y-auto p-3 space-y-2">
    {#each messages as message (message.id)}
      <div
        class="chat-message rounded-lg px-3 py-2 text-sm max-w-full break-words"
        class:chat-message-user={message.role === 'user'}
        class:chat-message-assistant={message.role === 'assistant'}
        class:bg-[#44475a]={message.role === 'user' && theme === 'dracula'}
        class:bg-[#eaeef2]={message.role === 'user' && theme === 'light'}
        class:text-[#f8f8f2]={message.role === 'user' && theme === 'dracula'}
        class:text-[#24292f]={message.role === 'user' && theme === 'light'}
        class:ml-4={message.role === 'user'}
        class:italic={message.role === 'assistant'}
      >
        <!-- eslint-disable-next-line svelte/no-at-html-tags -- content is sanitised via escapeHtml and rendered by KaTeX -->
        {@html renderContent(message.content)}
      </div>
    {/each}
    <!-- Scroll anchor — always keep at the bottom -->
    <div bind:this={scrollAnchor} class="chat-scroll-anchor h-0"></div>
  </div>

  <!-- Resize handle -->
  <div
    class="chat-resize-handle h-1.5 cursor-row-resize select-none"
    class:bg-[#44475a]={theme === 'dracula'}
    class:hover:bg-[#6272a4]={theme === 'dracula'}
    class:bg-[#d0d7de]={theme === 'light'}
    class:hover:bg-[#0550ae]={theme === 'light'}
    role="separator"
    aria-orientation="horizontal"
    onpointerdown={onResizePointerDown}
    onpointermove={onResizePointerMove}
    onpointerup={onResizePointerUp}
  ></div>

  <!-- Input area -->
  <div class="chat-input-area p-2" style="height: {inputHeight}px; flex-shrink: 0;">
    <textarea
      class="chat-input w-full h-full resize-none rounded border p-2 text-sm font-mono outline-none"
      class:bg-[#21222c]={theme === 'dracula'}
      class:text-[#f8f8f2]={theme === 'dracula'}
      class:placeholder-[#6272a4]={theme === 'dracula'}
      class:border-[#44475a]={theme === 'dracula'}
      class:bg-[#f6f8fa]={theme === 'light'}
      class:text-[#24292f]={theme === 'light'}
      class:placeholder-[#8c959f]={theme === 'light'}
      class:border-[#d0d7de]={theme === 'light'}
      placeholder="Ask about your Lean proof…"
      bind:value={inputText}
      onkeydown={onKeydown}
    ></textarea>
  </div>
</div>
