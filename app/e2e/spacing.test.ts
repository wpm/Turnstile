import { test, expect } from './fixtures'

test('send button has space below it inside the assistant panel', async ({ page, mountApp }) => {
  await page.setViewportSize({ width: 1280, height: 800 })
  await mountApp()

  const sendButton = page.locator('[aria-label="Send message"]')
  const assistantPanel = page.locator('.assistant-panel')

  const btnBox = await sendButton.boundingBox()
  const panelBox = await assistantPanel.boundingBox()

  const spaceBelow = panelBox!.y + panelBox!.height - (btnBox!.y + btnBox!.height)
  // Expect at least 12px of space below the Send button
  expect(spaceBelow).toBeGreaterThanOrEqual(12)
})
