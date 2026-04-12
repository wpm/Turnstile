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

  test('01-dark-initial: setup overlay visible on load with pulse indicator', async ({ page }) => {
    await injectTauriMock(page, { setupComplete: false })
    await page.goto('/')
    await page.waitForTimeout(500)
    await page.screenshot({
      path: path.join(SCREENSHOT_DIR, '01-dark-initial.png'),
      fullPage: true,
    })

    await expect(page.locator('.animate-pulse')).toBeVisible()
  })

  test('02-dark-main: theme toggle is inside chat panel header', async ({ page, mountApp }) => {
    await mountApp()
    await page.screenshot({ path: path.join(SCREENSHOT_DIR, '02-dark-main.png'), fullPage: true })

    const toggleBtn = page.getByLabel('Toggle theme')
    await expect(toggleBtn).toBeVisible()

    const btnBox = await toggleBtn.boundingBox()
    expect(btnBox).not.toBeNull()
    expect(btnBox!.x).toBeGreaterThan(page.viewportSize()!.width * 0.4)

    await expect(page.locator('.chat-history')).toBeVisible()
  })

  test('03-light-main: light mode applies to entire app', async ({ page, mountApp }) => {
    await mountApp()
    await expect(page.locator('html')).not.toHaveClass(/light/)

    await page.getByLabel('Toggle theme').click()
    await expect(page.locator('html')).toHaveClass(/light/)

    // --bg-primary: #eff1f5 (Catppuccin Latte base) in light theme
    const editorBg = await page
      .locator('.cm-editor')
      .evaluate((el) => getComputedStyle(el).backgroundColor)
    expect(editorBg).toBe('rgb(239, 241, 245)')

    await page.screenshot({ path: path.join(SCREENSHOT_DIR, '03-light-main.png'), fullPage: true })
  })

  test('04-chat-messages: user and assistant bubbles are visually distinct', async ({
    page,
    mountApp,
  }) => {
    await mountApp()

    const textarea = page.locator('textarea')
    await textarea.click()
    await textarea.pressSequentially('Hello, can you help me with my Lean proof?', { delay: 20 })
    await page.keyboard.press('Enter')

    await Promise.all([
      page.locator('.chat-message-user').first().waitFor({ state: 'visible' }),
      page.locator('.chat-message-assistant').first().waitFor({ state: 'visible' }),
    ])

    await page.screenshot({
      path: path.join(SCREENSHOT_DIR, '04-chat-messages.png'),
      fullPage: true,
    })

    const [userBg, assistantBg] = await Promise.all([
      page
        .locator('.chat-message-user')
        .first()
        .evaluate((el) => getComputedStyle(el).backgroundColor),
      page
        .locator('.chat-message-assistant')
        .first()
        .evaluate((el) => getComputedStyle(el).backgroundColor),
    ])

    expect(userBg).not.toBe('rgba(0, 0, 0, 0)')
    expect(userBg).not.toBe('transparent')
    expect(assistantBg).not.toBe('rgba(0, 0, 0, 0)')
    expect(assistantBg).not.toBe('transparent')
    expect(userBg).not.toBe(assistantBg)
  })

  test('05-settings-dark: settings modal section headers are readable', async ({
    page,
    mountApp,
  }) => {
    await mountApp()
    await page.keyboard.press('Meta+,')
    await page.locator('[data-testid="settings-modal"]').waitFor({ state: 'visible' })
    await page.screenshot({
      path: path.join(SCREENSHOT_DIR, '05-settings-dark.png'),
      fullPage: true,
    })

    const fontSizesHeader = page.locator('text=Font Sizes')
    await expect(fontSizesHeader).toBeVisible()
    const headerColor = await fontSizesHeader.evaluate((el) => getComputedStyle(el).color)
    expect(headerColor).not.toBe('rgba(0, 0, 0, 0)')
    expect(headerColor).not.toBe('transparent')
  })

  test('06-settings-light: settings modal adapts to light mode', async ({ page, mountApp }) => {
    await mountApp()
    await page.getByLabel('Toggle theme').click()
    await expect(page.locator('html')).toHaveClass(/light/)

    await page.keyboard.press('Meta+,')
    await page.locator('[data-testid="settings-modal"]').waitFor({ state: 'visible' })
    await page.screenshot({
      path: path.join(SCREENSHOT_DIR, '06-settings-light.png'),
      fullPage: true,
    })

    const headerColor = await page
      .locator('text=Font Sizes')
      .evaluate((el) => getComputedStyle(el).color)
    // light text-secondary: rgb(85, 85, 85) — not white, not transparent
    expect(headerColor).not.toBe('rgb(255, 255, 255)')
    expect(headerColor).not.toBe('rgba(0, 0, 0, 0)')
  })

  test('07-settings-model-tab: model tab has description and readable header', async ({
    page,
    mountApp,
  }) => {
    await mountApp()
    await page.keyboard.press('Meta+,')
    await page.locator('[data-testid="settings-modal"]').waitFor({ state: 'visible' })
    await page.locator('[data-testid="settings-tab-model"]').click()
    await page.screenshot({
      path: path.join(SCREENSHOT_DIR, '07-settings-model-tab.png'),
      fullPage: true,
    })

    await expect(
      page.locator('text=The selected model is used for all proof assistant'),
    ).toBeVisible()
    await expect(page.locator('text=Language Model')).toBeVisible()
  })

  test('08-chat-input-empty: send button is muted gray when input is empty', async ({
    page,
    mountApp,
  }) => {
    await mountApp()
    await page.locator('textarea').click()
    await page.screenshot({
      path: path.join(SCREENSHOT_DIR, '08-chat-input-empty.png'),
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

  test('09-chat-input-filled: send button is accent blue when input has text', async ({
    page,
    mountApp,
  }) => {
    await mountApp()
    const textarea = page.locator('textarea')
    await textarea.click()
    await textarea.pressSequentially('test message', { delay: 30 })
    await page.screenshot({
      path: path.join(SCREENSHOT_DIR, '09-chat-input-filled.png'),
      fullPage: false,
      clip: {
        x: page.viewportSize()!.width * 0.6,
        y: page.viewportSize()!.height * 0.7,
        width: page.viewportSize()!.width * 0.4,
        height: page.viewportSize()!.height * 0.3,
      },
    })

    const sendBtn = page.locator('button[aria-label="Send message"]')
    const filledBg = await sendBtn.evaluate((el) => getComputedStyle(el).backgroundColor)
    expect(filledBg).toBe('rgb(137, 180, 250)') // Catppuccin Mocha blue: #89b4fa
    await expect(sendBtn).toBeEnabled()
  })

  test('10-gutter: CM gutter has visible right border in dark and light modes', async ({
    page,
    mountApp,
  }) => {
    await mountApp()
    await page.screenshot({
      path: path.join(SCREENSHOT_DIR, '10-gutter.png'),
      fullPage: false,
      clip: { x: 0, y: 0, width: 120, height: 200 },
    })

    async function getGutterBorder(): Promise<{ width: string; style: string; color: string }> {
      return page.locator('.cm-gutters').evaluate((el) => {
        const s = getComputedStyle(el)
        return {
          width: s.borderRightWidth,
          style: s.borderRightStyle,
          color: s.borderRightColor,
        }
      })
    }

    const darkBorder = await getGutterBorder()
    expect(darkBorder.width).not.toBe('0px')
    expect(darkBorder.style).toBe('solid')
    expect(darkBorder.color).not.toBe('rgba(0, 0, 0, 0)')

    await page.getByLabel('Toggle theme').click()
    await expect(page.locator('html')).toHaveClass(/light/)

    const lightBorder = await getGutterBorder()
    expect(lightBorder.width).not.toBe('0px')
    expect(lightBorder.style).toBe('solid')
    expect(lightBorder.color).not.toBe('rgba(0, 0, 0, 0)')
  })

  test('keyboard hint text is readable', async ({ page, mountApp }) => {
    await mountApp()
    const hint = page.locator('text=Enter to send · Shift+Enter for newline')
    await expect(hint).toBeVisible()
    const opacity = await hint.evaluate((el) => getComputedStyle(el).opacity)
    expect(parseFloat(opacity)).toBeGreaterThan(0.3)
  })

  test('chat resize handle is 10px tall with three grip dots', async ({ page, mountApp }) => {
    await mountApp()
    const handle = page.locator('.chat-resize-handle')
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

  test('theme toggle is inside chat panel bounds', async ({ page, mountApp }) => {
    await mountApp()

    const [toggleBox, chatPanelBox] = await Promise.all([
      page.getByLabel('Toggle theme').boundingBox(),
      page.locator('.chat-panel').boundingBox(),
    ])

    expect(toggleBox).not.toBeNull()
    expect(chatPanelBox).not.toBeNull()
    expect(toggleBox!.x).toBeGreaterThanOrEqual(chatPanelBox!.x)
    expect(toggleBox!.x + toggleBox!.width).toBeLessThanOrEqual(
      chatPanelBox!.x + chatPanelBox!.width + 5,
    )
  })
})
