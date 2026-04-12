import { test, expect } from './fixtures'

test('send button has space below it inside the chat panel', async ({ page, mountApp }) => {
  await page.setViewportSize({ width: 1280, height: 800 })
  await mountApp()

  const sendButton = page.locator('[aria-label="Send message"]')
  const chatPanel = page.locator('.chat-panel')

  const btnBox = await sendButton.boundingBox()
  const panelBox = await chatPanel.boundingBox()

  const spaceBelow = panelBox!.y + panelBox!.height - (btnBox!.y + btnBox!.height)
  // Expect at least 12px of space below the Send button
  expect(spaceBelow).toBeGreaterThanOrEqual(12)
})
