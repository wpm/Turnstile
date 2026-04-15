import { test, expect, type AppFixtures } from './fixtures'
import type { Page } from '@playwright/test'

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function chatInput(page: Page): ReturnType<Page['locator']> {
  return page.locator('.chat-input')
}

function chatHistory(page: Page): ReturnType<Page['locator']> {
  return page.locator('.chat-history')
}

// ---------------------------------------------------------------------------
// Layout
// ---------------------------------------------------------------------------

test.describe('ChatPanel layout', () => {
  test('chat panel is visible in the app layout', async ({ page, mountApp }) => {
    await mountApp()
    await expect(page.locator('.chat-panel')).toBeVisible()
  })

  test('chat history container is present', async ({ page, mountApp }) => {
    await mountApp()
    await expect(chatHistory(page)).toBeVisible()
  })

  test('chat input is present', async ({ page, mountApp }) => {
    await mountApp()
    await expect(chatInput(page)).toBeVisible()
  })

  test('resize handle is present', async ({ page, mountApp }) => {
    await mountApp()
    await expect(page.locator('.chat-resize-handle')).toBeVisible()
  })
})

// ---------------------------------------------------------------------------
// Input behaviour
// ---------------------------------------------------------------------------

test.describe('Chat input behaviour', () => {
  test('typing in the input changes its value', async ({ page, mountApp }) => {
    await mountApp()
    await chatInput(page).click()
    await page.keyboard.type('hello Lean')
    await expect(chatInput(page)).toHaveText('hello Lean')
  })

  test('Enter sends the message and clears the input', async ({ page, mountApp }) => {
    await mountApp()
    await chatInput(page).click()
    await page.keyboard.type('test message')
    await page.keyboard.press('Enter')
    // Input should be cleared after send
    await expect(chatInput(page)).toBeEmpty()
  })

  test('Shift+Enter inserts a newline instead of sending', async ({ page, mountApp }) => {
    await mountApp()
    await chatInput(page).click()
    await page.keyboard.type('line one')
    await page.keyboard.press('Shift+Enter')
    await page.keyboard.type('line two')
    const value = await chatInput(page).textContent()
    expect(value).toContain('line one')
    expect(value).toContain('line two')
  })

  test('Option+Enter (Alt+Enter) does not send the message', async ({ page, mountApp }) => {
    // Alt+Enter behavior is OS-dependent; the key constraint is that it must NOT
    // clear the input (i.e. must not trigger a send).
    await mountApp()
    await chatInput(page).click()
    await page.keyboard.type('line one')
    await page.keyboard.press('Alt+Enter')
    // Input should NOT be cleared — message was not sent
    await expect(chatInput(page)).toHaveText('line one')
  })
})

// ---------------------------------------------------------------------------
// Message rendering
// ---------------------------------------------------------------------------

test.describe('Message rendering', () => {
  /** Helper: send a message and wait for user bubble to appear. */
  async function sendAndWait(
    page: Page,
    { mountApp }: Pick<AppFixtures, 'mountApp'>,
    message: string,
  ): Promise<void> {
    await mountApp()
    await chatInput(page).click()
    await page.keyboard.type(message)
    await page.keyboard.press('Enter')
  }

  test('sent user message appears in message history', async ({ page, mountApp }) => {
    await sendAndWait(page, { mountApp }, 'hello there')
    await expect(chatHistory(page)).toContainText('hello there')
  })

  test('user message bubble has role indicator', async ({ page, mountApp }) => {
    await sendAndWait(page, { mountApp }, 'ping')
    await expect(page.locator('.chat-message-user')).toBeVisible()
  })

  test('assistant reply appears after proof-assistant-complete event', async ({
    page,
    mountApp,
  }) => {
    await mountApp()
    await chatInput(page).click()
    await page.keyboard.type('hello')
    await page.keyboard.press('Enter')

    // The fixture mock fires proof-assistant-complete automatically (see fixtures.ts)
    await expect(page.locator('.chat-message-assistant')).toBeVisible({ timeout: 3000 })
    await expect(chatHistory(page)).toContainText('[echo] hello')
  })

  test('lean code in backticks renders with syntax highlight class', async ({
    page,
    mountApp,
    emitEvent,
  }) => {
    await mountApp()
    // Simulate an assistant message containing Lean code in backticks
    await emitEvent('proof-assistant-complete', {
      role: 'assistant',
      content: 'Try `def foo := 42` in your file.',
      timestamp: Date.now(),
    })
    await expect(page.locator('.chat-message-assistant')).toBeVisible({ timeout: 3000 })
    // The backtick-wrapped code should have the lean code class
    await expect(page.locator('.chat-lean-code')).toBeVisible()
  })

  test('KaTeX math in assistant message renders a KaTeX container', async ({
    page,
    mountApp,
    emitEvent,
  }) => {
    await mountApp()
    await emitEvent('proof-assistant-complete', {
      role: 'assistant',
      content: 'The formula is $x^2 + y^2 = r^2$.',
      timestamp: Date.now(),
    })
    await expect(page.locator('.chat-message-assistant')).toBeVisible({ timeout: 3000 })
    // KaTeX renders a .katex element
    await expect(page.locator('.katex')).toBeVisible()
  })
})

// ---------------------------------------------------------------------------
// Auto-scroll
// ---------------------------------------------------------------------------

test.describe('Auto-scroll', () => {
  test('chat history scrolls to bottom when new message is added', async ({
    page,
    mountApp,
    emitEvent,
  }) => {
    await mountApp()
    // Add several messages to push content below the fold
    for (let i = 0; i < 5; i++) {
      await emitEvent('proof-assistant-complete', {
        role: 'assistant',
        content: `Message ${String(i)}: ${'A long message that takes up space. '.repeat(10)}`,
        timestamp: Date.now() + i,
      })
    }
    // Wait for messages to render
    await page.waitForTimeout(500)
    // The scroll anchor element must exist in the DOM (it's always there, h-0)
    await expect(page.locator('.chat-scroll-anchor')).toBeAttached({ timeout: 3000 })
    // The chat history should be scrolled to the bottom: scrollTop + clientHeight ≈ scrollHeight
    const isAtBottom = await page.locator('.chat-history').evaluate((el) => {
      const { scrollTop, scrollHeight, clientHeight } = el
      return scrollTop + clientHeight >= scrollHeight - 10
    })
    expect(isAtBottom).toBe(true)
  })
})

// ---------------------------------------------------------------------------
// Streaming & thinking indicator
// ---------------------------------------------------------------------------

test.describe('Streaming and thinking indicator', () => {
  test('thinking indicator shows while waiting for assistant reply', async ({ page, mountApp }) => {
    // Mount with a mock that does NOT auto-fire proof-assistant-complete
    await mountApp({ noAutoReply: true })
    await chatInput(page).click()
    await page.keyboard.type('hello')
    await page.keyboard.press('Enter')

    // The thinking indicator should appear
    await expect(page.locator('.chat-thinking-indicator')).toBeVisible({ timeout: 2000 })
  })

  test('thinking indicator disappears after proof-assistant-complete', async ({
    page,
    mountApp,
    emitEvent,
  }) => {
    await mountApp({ noAutoReply: true })
    await chatInput(page).click()
    await page.keyboard.type('hello')
    await page.keyboard.press('Enter')

    await expect(page.locator('.chat-thinking-indicator')).toBeVisible({ timeout: 2000 })

    // Simulate assistant reply completing
    await emitEvent('proof-assistant-stream-done', null)
    await emitEvent('proof-assistant-complete', {
      role: 'assistant',
      content: '[echo] hello',
      timestamp: Date.now(),
    })

    await expect(page.locator('.chat-thinking-indicator')).not.toBeVisible({ timeout: 2000 })
  })

  test('streaming text appears incrementally in a bubble', async ({
    page,
    mountApp,
    emitEvent,
  }) => {
    await mountApp({ noAutoReply: true })
    await chatInput(page).click()
    await page.keyboard.type('hello')
    await page.keyboard.press('Enter')

    // Send streaming deltas
    await emitEvent('proof-assistant-delta', 'Hello ')
    await emitEvent('proof-assistant-delta', 'world')

    // The streaming bubble should contain the accumulated text
    const streamingBubble = page.locator('.chat-message-streaming')
    await expect(streamingBubble).toBeVisible({ timeout: 2000 })
    await expect(streamingBubble).toContainText('Hello world')

    // Complete the message
    await emitEvent('proof-assistant-stream-done', null)
    await emitEvent('proof-assistant-complete', {
      role: 'assistant',
      content: 'Hello world',
      timestamp: Date.now(),
    })

    // Streaming bubble should be gone, replaced by normal assistant message
    await expect(streamingBubble).not.toBeVisible({ timeout: 2000 })
    await expect(page.locator('.chat-message-assistant')).toContainText('Hello world')
  })

  test('send button is disabled while streaming', async ({ page, mountApp, emitEvent }) => {
    await mountApp({ noAutoReply: true })
    await chatInput(page).click()
    await page.keyboard.type('hello')
    await page.keyboard.press('Enter')

    // Type something new — button should still be disabled during streaming
    await chatInput(page).click()
    await page.keyboard.type('another message')
    const sendBtn = page.locator('button[aria-label="Send message"]')
    await expect(sendBtn).toBeDisabled()

    // Complete the stream
    await emitEvent('proof-assistant-stream-done', null)
    await emitEvent('proof-assistant-complete', {
      role: 'assistant',
      content: '[echo] hello',
      timestamp: Date.now(),
    })

    // Now send button should be enabled again
    await expect(sendBtn).toBeEnabled({ timeout: 2000 })
  })
})

// ---------------------------------------------------------------------------
// Resize handle
// ---------------------------------------------------------------------------

test.describe('Resize handle', () => {
  test('drag handle changes input area height', async ({ page, mountApp }) => {
    await mountApp()
    const handle = page.locator('.chat-resize-handle')
    const inputArea = page.locator('.chat-input-area')

    const initialHeight = await inputArea.evaluate((el) => el.getBoundingClientRect().height)

    // Drag the handle upward to increase input area height
    const handleBox = await handle.boundingBox()
    if (handleBox) {
      await page.mouse.move(handleBox.x + handleBox.width / 2, handleBox.y + handleBox.height / 2)
      await page.mouse.down()
      await page.mouse.move(
        handleBox.x + handleBox.width / 2,
        handleBox.y + handleBox.height / 2 - 60,
      )
      await page.mouse.up()
    }

    const newHeight = await inputArea.evaluate((el) => el.getBoundingClientRect().height)
    // Height should have increased (drag up = bigger input)
    expect(newHeight).toBeGreaterThan(initialHeight)
  })
})
