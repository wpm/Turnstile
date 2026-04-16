import * as path from 'path'
import * as fs from 'fs'
import { fileURLToPath } from 'url'
import { test, expect, injectTauriMock } from './fixtures'

const __filename = fileURLToPath(import.meta.url)
const __dirname = path.dirname(__filename)

const SCREENSHOT_DIR = path.resolve(__dirname, '../../test-results/audit-screenshots')

async function ensureScreenshotDir(): Promise<void> {
  await fs.promises.mkdir(SCREENSHOT_DIR, { recursive: true })
}

test.describe('Visual audit', () => {
  test.beforeAll(async () => {
    await ensureScreenshotDir()
  })

  test('01-initial: setup overlay visible on load with pulse indicator', async ({ page }) => {
    await injectTauriMock(page, { setupComplete: false })
    await page.goto('/')
    await page.waitForTimeout(500)
    await page.screenshot({
      path: path.join(SCREENSHOT_DIR, '01-initial.png'),
      fullPage: true,
    })

    await expect(page.locator('.animate-pulse')).toBeVisible()
  })

  test('02-main: assistant panel renders without theme toggle', async ({ page, mountApp }) => {
    await mountApp()
    await page.screenshot({ path: path.join(SCREENSHOT_DIR, '02-main.png'), fullPage: true })

    // Theme toggle was removed — theme now follows the OS automatically.
    await expect(page.getByLabel('Toggle theme')).toHaveCount(0)

    await expect(page.locator('.assistant-history')).toBeVisible()
  })

  test('04-assistant-messages: user and assistant bubbles are visually distinct', async ({
    page,
    mountApp,
  }) => {
    await mountApp()

    const textbox = page.locator('.assistant-input')
    await textbox.click()
    await page.keyboard.type('Hello, can you help me with my Lean proof?')
    await page.keyboard.press('Enter')

    await Promise.all([
      page.locator('.assistant-message-user').first().waitFor({ state: 'visible' }),
      page.locator('.assistant-message-assistant').first().waitFor({ state: 'visible' }),
    ])

    await page.screenshot({
      path: path.join(SCREENSHOT_DIR, '04-assistant-messages.png'),
      fullPage: true,
    })

    const [userBg, assistantBg] = await Promise.all([
      page
        .locator('.assistant-message-user')
        .first()
        .evaluate((el) => getComputedStyle(el).backgroundColor),
      page
        .locator('.assistant-message-assistant')
        .first()
        .evaluate((el) => getComputedStyle(el).backgroundColor),
    ])

    expect(userBg).not.toBe('rgba(0, 0, 0, 0)')
    expect(userBg).not.toBe('transparent')
    expect(assistantBg).not.toBe('rgba(0, 0, 0, 0)')
    expect(assistantBg).not.toBe('transparent')
    expect(userBg).not.toBe(assistantBg)
  })

  test('05-settings: Assistant tab headers are readable', async ({ page, mountApp }) => {
    await mountApp()
    await page.keyboard.press('Meta+,')
    await page.locator('[data-testid="settings-modal"]').waitFor({ state: 'visible' })
    await page.screenshot({
      path: path.join(SCREENSHOT_DIR, '05-settings.png'),
      fullPage: true,
    })

    const fontSizeLabel = page.locator('#assistant-font-size-label')
    await expect(fontSizeLabel).toBeVisible()
    const headerColor = await fontSizeLabel.evaluate((el) => getComputedStyle(el).color)
    expect(headerColor).not.toBe('rgba(0, 0, 0, 0)')
    expect(headerColor).not.toBe('transparent')
  })

  test('07-settings-proof-tab: Proof tab renders model and prompt', async ({
    page,
    mountApp,
  }) => {
    await mountApp()
    await page.keyboard.press('Meta+,')
    await page.locator('[data-testid="settings-modal"]').waitFor({ state: 'visible' })
    await page.locator('[data-testid="settings-tab-proof"]').click()
    await page.screenshot({
      path: path.join(SCREENSHOT_DIR, '07-settings-proof-tab.png'),
      fullPage: true,
    })

    await expect(page.locator('[data-testid="translation-model-select"]')).toBeVisible()
    await expect(page.locator('[data-testid="translation-prompt-textarea"]')).toBeVisible()
  })

  test('08-assistant-input-empty: send button is muted gray when input is empty', async ({
    page,
    mountApp,
  }) => {
    await mountApp()
    await page.locator('.assistant-input').click()
    await page.screenshot({
      path: path.join(SCREENSHOT_DIR, '08-assistant-input-empty.png'),
      fullPage: false,
      clip: {
        x: page.viewportSize()!.width * 0.6,
        y: page.viewportSize()!.height * 0.7,
        width: page.viewportSize()!.width * 0.4,
        height: page.viewportSize()!.height * 0.3,
      },
    })

    const sendBtn = page.locator('button[aria-label="Send message"]')
    const emptyBg = await sendBtn.evaluate((el) => getComputedStyle(el).backgroundColor)
    expect(emptyBg).not.toBe('rgb(0, 122, 204)')
    await expect(sendBtn).toBeDisabled()
  })

  test('09-assistant-input-filled: send button is accent blue when input has text', async ({
    page,
    mountApp,
  }) => {
    await mountApp()
    const textbox = page.locator('.assistant-input')
    await textbox.click()
    await page.keyboard.type('test message')
    await page.screenshot({
      path: path.join(SCREENSHOT_DIR, '09-assistant-input-filled.png'),
      fullPage: false,
      clip: {
        x: page.viewportSize()!.width * 0.6,
        y: page.viewportSize()!.height * 0.7,
        width: page.viewportSize()!.width * 0.4,
        height: page.viewportSize()!.height * 0.3,
      },
    })

    const sendBtn = page.locator('button[aria-label="Send message"]')
    await expect(sendBtn).toBeEnabled()
    // Button should have an accent-blue background, not the disabled gray.
    // Exact RGB varies by Tailwind version / color space, so assert blue channel dominates.
    const filledBg = await sendBtn.evaluate((el) => getComputedStyle(el).backgroundColor)
    const match = /rgb\((\d+), (\d+), (\d+)\)/.exec(filledBg)
    expect(match, 'expected rgb() background color').not.toBeNull()
    const [, r, , b] = match!
    expect(Number(b)).toBeGreaterThan(Number(r)) // blue > red → accent blue
  })

  test('10-gutter: CM gutter has visible right border', async ({ page, mountApp }) => {
    await mountApp()
    await page.screenshot({
      path: path.join(SCREENSHOT_DIR, '10-gutter.png'),
      fullPage: false,
      clip: { x: 0, y: 0, width: 120, height: 200 },
    })

    const border = await page.locator('.cm-gutters').evaluate((el) => {
      const s = getComputedStyle(el)
      return {
        width: s.borderRightWidth,
        style: s.borderRightStyle,
        color: s.borderRightColor,
      }
    })
    expect(border.width).not.toBe('0px')
    expect(border.style).toBe('solid')
    expect(border.color).not.toBe('rgba(0, 0, 0, 0)')
  })

  test('keyboard hint text is readable', async ({ page, mountApp }) => {
    await mountApp()
    const hint = page.locator('text=Enter to send · Shift+Enter for newline')
    await expect(hint).toBeVisible()
    const opacity = await hint.evaluate((el) => getComputedStyle(el).opacity)
    expect(parseFloat(opacity)).toBeGreaterThan(0.3)
  })

  test('assistant resize handle is 10px tall with three grip dots', async ({ page, mountApp }) => {
    await mountApp()
    const handle = page.locator('.assistant-resize-handle')
    await expect(handle).toBeVisible()

    const handleBox = await handle.boundingBox()
    expect(handleBox).not.toBeNull()
    expect(handleBox!.height).toBe(10)
    await expect(handle.locator('span.rounded-full')).toHaveCount(3)
  })

  test('setup overlay shows pulse shimmer at 30% width when progress is 0', async ({ page }) => {
    await injectTauriMock(page, { setupComplete: false })
    await page.goto('/')
    await page.waitForTimeout(400)

    const shimmerBar = page.locator('.animate-pulse')
    await expect(shimmerBar).toBeVisible()

    const barBox = await shimmerBar.boundingBox()
    const parentBox = await shimmerBar.locator('..').boundingBox()
    if (barBox && parentBox) {
      const widthPct = (barBox.width / parentBox.width) * 100
      expect(widthPct).toBeGreaterThan(20)
      expect(widthPct).toBeLessThan(40)
    }
  })
})
