import { type Locator, type Page } from '@playwright/test'
import { test, expect } from './fixtures'

/** Returns the computed box-shadow (Tailwind ring utilities render as box-shadow). */
async function focusRing(locator: Locator): Promise<string> {
  return locator.evaluate((el) => getComputedStyle(el).boxShadow)
}

/** Returns true if focus is currently inside the CodeMirror editor. */
async function editorHasFocus(page: Page): Promise<boolean> {
  return page.evaluate(() =>
    Boolean(document.querySelector('.cm-content')?.contains(document.activeElement)),
  )
}

// ---------------------------------------------------------------------------
// Button focus states
// ---------------------------------------------------------------------------

test.describe('Button focus states', () => {
  test('theme toggle shows focus ring when focused via keyboard', async ({ page, mountApp }) => {
    await mountApp()
    const btn = page.getByLabel('Toggle theme')
    await btn.focus()
    expect(await focusRing(btn)).not.toBe('none')
  })

  test('send button shows focus ring when focused via keyboard', async ({ page, mountApp }) => {
    await mountApp()
    const textbox = page.locator('.chat-input')
    await textbox.click()
    await page.keyboard.type('hello')
    const btn = page.locator('button[aria-label="Send message"]')
    await btn.focus()
    expect(await focusRing(btn)).not.toBe('none')
  })

  test('settings close button shows focus ring when focused via keyboard', async ({
    page,
    mountApp,
  }) => {
    await mountApp()
    await page.keyboard.press('Meta+,')
    await page.locator('[data-testid="settings-modal"]').waitFor({ state: 'visible' })
    const btn = page.getByLabel('Close settings')
    await btn.focus()
    expect(await focusRing(btn)).not.toBe('none')
  })

  test('settings tab buttons show focus ring when focused via keyboard', async ({
    page,
    mountApp,
  }) => {
    await mountApp()
    await page.keyboard.press('Meta+,')
    await page.locator('[data-testid="settings-modal"]').waitFor({ state: 'visible' })

    for (const tabId of ['appearance', 'model']) {
      const btn = page.locator(`[data-testid="settings-tab-${tabId}"]`)
      await btn.focus()
      expect(await focusRing(btn), `settings-tab-${tabId} should have a focus ring`).not.toBe(
        'none',
      )
    }
  })

  test('Restore Defaults button shows focus ring when focused via keyboard', async ({
    page,
    mountApp,
  }) => {
    await mountApp()
    await page.keyboard.press('Meta+,')
    await page.locator('[data-testid="settings-modal"]').waitFor({ state: 'visible' })
    const btn = page.locator('[data-testid="restore-defaults-button"]')
    await btn.focus()
    expect(await focusRing(btn)).not.toBe('none')
  })

  test('recovery prompt buttons show focus rings when focused via keyboard', async ({
    page,
    mountApp,
  }) => {
    await mountApp({ hasAutoSave: true })
    await page.locator('[data-testid="recovery-prompt"]').waitFor({ state: 'visible' })

    const discardBtn = page.getByRole('button', { name: 'No, discard' })
    await discardBtn.focus()
    expect(await focusRing(discardBtn)).not.toBe('none')

    const restoreBtn = page.getByRole('button', { name: 'Yes, restore' })
    await restoreBtn.focus()
    expect(await focusRing(restoreBtn)).not.toBe('none')
  })
})

// ---------------------------------------------------------------------------
// Settings modal keyboard navigation
// ---------------------------------------------------------------------------

test.describe('Settings modal keyboard navigation', () => {
  test('focus moves into modal when it opens', async ({ page, mountApp }) => {
    await mountApp()
    await page.keyboard.press('Meta+,')
    await page.locator('[data-testid="settings-modal"]').waitFor({ state: 'visible' })

    // The focused element should be inside the modal window, not in the background.
    const focusedTestId = await page.evaluate(() => {
      const el = document.activeElement
      return el?.getAttribute('data-testid') ?? el?.tagName ?? null
    })
    expect(focusedTestId).not.toBeNull()
    const isInsideModal = await page.evaluate(() => {
      const modal = document.querySelector('[data-testid="settings-modal"]')
      return modal?.contains(document.activeElement) ?? false
    })
    expect(isInsideModal).toBe(true)
  })

  test('Tab key does not move focus outside the modal', async ({ page, mountApp }) => {
    await mountApp()
    await page.keyboard.press('Meta+,')
    await page.locator('[data-testid="settings-modal"]').waitFor({ state: 'visible' })

    // Tab through all focusable elements more times than could possibly exist in the modal.
    for (let i = 0; i < 20; i++) {
      await page.keyboard.press('Tab')
      const isInsideModal = await page.evaluate(() => {
        const modal = document.querySelector('[data-testid="settings-modal"]')
        return modal?.contains(document.activeElement) ?? false
      })
      expect(isInsideModal, `focus escaped modal after ${String(i + 1)} Tab presses`).toBe(true)
    }
  })

  test('Shift+Tab cycles backwards and stays inside the modal', async ({ page, mountApp }) => {
    await mountApp()
    await page.keyboard.press('Meta+,')
    await page.locator('[data-testid="settings-modal"]').waitFor({ state: 'visible' })

    for (let i = 0; i < 20; i++) {
      await page.keyboard.press('Shift+Tab')
      const isInsideModal = await page.evaluate(() => {
        const modal = document.querySelector('[data-testid="settings-modal"]')
        return modal?.contains(document.activeElement) ?? false
      })
      expect(isInsideModal, `focus escaped modal after ${String(i + 1)} Shift+Tab presses`).toBe(
        true,
      )
    }
  })

  test('Tab from a tab button moves focus into the panel, not to the next tab', async ({
    page,
    mountApp,
  }) => {
    await mountApp()
    await page.keyboard.press('Meta+,')
    await page.locator('[data-testid="settings-modal"]').waitFor({ state: 'visible' })

    await page.locator('[data-testid="settings-tab-appearance"]').focus()
    await page.keyboard.press('Tab')

    // Focus should now be inside the panel, not on a tab button.
    const focused = await page.evaluate(() => document.activeElement?.getAttribute('data-testid'))
    expect(focused).not.toBe('settings-tab-appearance')
    expect(focused).not.toBe('settings-tab-model')
    // And still inside the modal.
    const isInsideModal = await page.evaluate(
      () =>
        document
          .querySelector('[data-testid="settings-modal"]')
          ?.contains(document.activeElement) ?? false,
    )
    expect(isInsideModal).toBe(true)
  })

  test('Tab moves between selects within the active tab panel', async ({ page, mountApp }) => {
    await mountApp()
    await page.keyboard.press('Meta+,')
    await page.locator('[data-testid="settings-modal"]').waitFor({ state: 'visible' })

    // Start from the Appearance tab button and Tab into the panel.
    await page.locator('[data-testid="settings-tab-appearance"]').focus()
    await page.keyboard.press('Tab')
    // First focusable in Appearance panel is the Theme select.
    const first = await page.evaluate(() => document.activeElement?.getAttribute('data-testid'))
    expect(first).toBe('theme-select')

    await page.keyboard.press('Tab')
    const second = await page.evaluate(() => document.activeElement?.getAttribute('data-testid'))
    expect(second).toBe('editor-font-size-select')

    await page.keyboard.press('Tab')
    const third = await page.evaluate(() => document.activeElement?.getAttribute('data-testid'))
    expect(third).toBe('prose-font-size-select')

    await page.keyboard.press('Tab')
    const fourth = await page.evaluate(() => document.activeElement?.getAttribute('data-testid'))
    expect(fourth).toBe('chat-font-size-select')
  })

  test('Tab from tab button enters active Model panel when Model tab is selected', async ({
    page,
    mountApp,
  }) => {
    await mountApp()
    await page.keyboard.press('Meta+,')
    await page.locator('[data-testid="settings-modal"]').waitFor({ state: 'visible' })

    // Switch to Model tab and Tab into the panel.
    await page.locator('[data-testid="settings-tab-model"]').click()
    await page.locator('[data-testid="settings-tab-model"]').focus()
    await page.keyboard.press('Tab')

    // First focusable in Model panel is the model select.
    const focused = await page.evaluate(() => document.activeElement?.getAttribute('data-testid'))
    expect(focused).toBe('model-select')
  })

  test('ArrowDown on a tab button switches to the next tab', async ({ page, mountApp }) => {
    await mountApp()
    await page.keyboard.press('Meta+,')
    await page.locator('[data-testid="settings-modal"]').waitFor({ state: 'visible' })

    await page.locator('[data-testid="settings-tab-appearance"]').focus()
    await page.keyboard.press('ArrowDown')

    await expect(page.locator('[data-testid="model-select"]')).toBeVisible()
    const focused = await page.evaluate(() => document.activeElement?.getAttribute('data-testid'))
    expect(focused).toBe('settings-tab-model')
  })

  test('ArrowUp on a tab button switches to the previous tab', async ({ page, mountApp }) => {
    await mountApp()
    await page.keyboard.press('Meta+,')
    await page.locator('[data-testid="settings-modal"]').waitFor({ state: 'visible' })

    await page.locator('[data-testid="settings-tab-model"]').click()
    await page.locator('[data-testid="settings-tab-model"]').focus()
    await page.keyboard.press('ArrowUp')

    await expect(page.locator('[data-testid="editor-font-size-select"]')).toBeVisible()
    const focused = await page.evaluate(() => document.activeElement?.getAttribute('data-testid'))
    expect(focused).toBe('settings-tab-appearance')
  })

  test('ArrowRight and ArrowLeft do not switch tabs (vertical tab list)', async ({
    page,
    mountApp,
  }) => {
    await mountApp()
    await page.keyboard.press('Meta+,')
    await page.locator('[data-testid="settings-modal"]').waitFor({ state: 'visible' })

    await page.locator('[data-testid="settings-tab-appearance"]').focus()
    await page.keyboard.press('ArrowRight')

    // Should still be on Appearance — ArrowRight has no effect in a vertical list.
    await expect(page.locator('[data-testid="editor-font-size-select"]')).toBeVisible()
    const focused = await page.evaluate(() => document.activeElement?.getAttribute('data-testid'))
    expect(focused).toBe('settings-tab-appearance')
  })

  test('focus returns to trigger element when modal closes via Escape', async ({
    page,
    mountApp,
  }) => {
    await mountApp()
    // Open modal from the theme toggle button (the natural trigger).
    const trigger = page.getByLabel('Toggle theme')
    await trigger.focus()
    await page.keyboard.press('Meta+,')
    await page.locator('[data-testid="settings-modal"]').waitFor({ state: 'visible' })

    await page.keyboard.press('Escape')
    await page.locator('[data-testid="settings-modal"]').waitFor({ state: 'hidden' })

    // Focus should be back on the element that had it before the modal opened.
    const focused = await page.evaluate(() => document.activeElement?.getAttribute('aria-label'))
    expect(focused).toBe('Toggle theme')
  })

  test('focus returns to trigger element when modal closes via close button', async ({
    page,
    mountApp,
  }) => {
    await mountApp()
    const trigger = page.getByLabel('Toggle theme')
    await trigger.focus()
    await page.keyboard.press('Meta+,')
    await page.locator('[data-testid="settings-modal"]').waitFor({ state: 'visible' })

    await page.getByLabel('Close settings').click()
    await page.locator('[data-testid="settings-modal"]').waitFor({ state: 'hidden' })

    const focused = await page.evaluate(() => document.activeElement?.getAttribute('aria-label'))
    expect(focused).toBe('Toggle theme')
  })
})

// ---------------------------------------------------------------------------
// Tablist ARIA roles and state
// ---------------------------------------------------------------------------

test.describe('Settings tablist ARIA', () => {
  test('tab container has role=tablist', async ({ page, mountApp }) => {
    await mountApp()
    await page.keyboard.press('Meta+,')
    await page.locator('[data-testid="settings-modal"]').waitFor({ state: 'visible' })
    await expect(page.locator('[role="tablist"]')).toBeVisible()
  })

  test('each tab button has role=tab', async ({ page, mountApp }) => {
    await mountApp()
    await page.keyboard.press('Meta+,')
    await page.locator('[data-testid="settings-modal"]').waitFor({ state: 'visible' })
    const tabs = page.locator('[role="tab"]')
    await expect(tabs).toHaveCount(2)
  })

  test('active tab has aria-selected=true, inactive has aria-selected=false', async ({
    page,
    mountApp,
  }) => {
    await mountApp()
    await page.keyboard.press('Meta+,')
    await page.locator('[data-testid="settings-modal"]').waitFor({ state: 'visible' })

    const appearanceTab = page.locator('[data-testid="settings-tab-appearance"]')
    const modelTab = page.locator('[data-testid="settings-tab-model"]')

    await expect(appearanceTab).toHaveAttribute('aria-selected', 'true')
    await expect(modelTab).toHaveAttribute('aria-selected', 'false')

    await modelTab.click()
    await expect(modelTab).toHaveAttribute('aria-selected', 'true')
    await expect(appearanceTab).toHaveAttribute('aria-selected', 'false')
  })

  test('tab panel has role=tabpanel and is labelled by its tab', async ({ page, mountApp }) => {
    await mountApp()
    await page.keyboard.press('Meta+,')
    await page.locator('[data-testid="settings-modal"]').waitFor({ state: 'visible' })

    // The visible panel should have role=tabpanel with aria-labelledby pointing to the active tab.
    const visiblePanel = page.locator('[role="tabpanel"]:not([hidden])')
    await expect(visiblePanel).toBeVisible()
    const labelledBy = await visiblePanel.getAttribute('aria-labelledby')
    expect(labelledBy).toBe('settings-tab-appearance')

    // The tab it points to must exist.
    await expect(page.locator(`#${String(labelledBy)}`)).toBeVisible()
  })

  test('inactive tab panel is hidden from the accessibility tree', async ({ page, mountApp }) => {
    await mountApp()
    await page.keyboard.press('Meta+,')
    await page.locator('[data-testid="settings-modal"]').waitFor({ state: 'visible' })

    const modelPanel = page.locator('#settings-panel-model')
    await expect(modelPanel).toHaveAttribute('hidden', '')
  })

  test('settings dialog uses aria-labelledby referencing visible title', async ({
    page,
    mountApp,
  }) => {
    await mountApp()
    await page.keyboard.press('Meta+,')
    const modal = page.locator('[data-testid="settings-modal"]')
    await modal.waitFor({ state: 'visible' })

    const labelledBy = await modal.getAttribute('aria-labelledby')
    expect(labelledBy).not.toBeNull()
    const titleEl = page.locator(`#${String(labelledBy)}`)
    await expect(titleEl).toHaveText('Settings')
  })
})

// ---------------------------------------------------------------------------
// Resize handle keyboard operation
// ---------------------------------------------------------------------------

test.describe('Resize handle keyboard operation', () => {
  test('chat input resize handle is focusable and adjusts height with arrow keys', async ({
    page,
    mountApp,
  }) => {
    await mountApp()
    const handle = page.locator('.chat-resize-handle')
    await handle.focus()

    const before = await page
      .locator('.chat-input-area')
      .evaluate((el) => (el as HTMLElement).offsetHeight)

    await page.keyboard.press('ArrowUp')
    await page.keyboard.press('ArrowUp')

    const after = await page
      .locator('.chat-input-area')
      .evaluate((el) => (el as HTMLElement).offsetHeight)
    expect(after).toBeGreaterThan(before)
  })

  test('chat input resize handle has correct ARIA attributes', async ({ page, mountApp }) => {
    await mountApp()
    const handle = page.locator('.chat-resize-handle')
    await expect(handle).toHaveAttribute('role', 'separator')
    await expect(handle).toHaveAttribute('aria-orientation', 'horizontal')
    await expect(handle).toHaveAttribute('aria-label', 'Resize input area')
    await expect(handle).toHaveAttribute('aria-valuenow')
    await expect(handle).toHaveAttribute('aria-valuemin')
    await expect(handle).toHaveAttribute('aria-valuemax')
  })

  test('vertical splitter is focusable and adjusts width with arrow keys', async ({
    page,
    mountApp,
  }) => {
    await mountApp()
    const splitter = page.locator('[aria-label="Resize chat panel"]')
    await splitter.focus()

    const before = await page.evaluate(() => {
      const panel = document.querySelector<HTMLElement>('[aria-label="Resize chat panel"]')
      return panel ? parseInt(panel.getAttribute('aria-valuenow') ?? '0', 10) : 0
    })

    await page.keyboard.press('ArrowLeft')
    await page.keyboard.press('ArrowLeft')

    const after = await page.evaluate(() => {
      const panel = document.querySelector<HTMLElement>('[aria-label="Resize chat panel"]')
      return panel ? parseInt(panel.getAttribute('aria-valuenow') ?? '0', 10) : 0
    })

    expect(after).toBeGreaterThan(before)
  })
})

// ---------------------------------------------------------------------------
// Textarea accessible label
// ---------------------------------------------------------------------------

test.describe('Chat input accessible label', () => {
  test('chat input has an accessible label', async ({ page, mountApp }) => {
    await mountApp()
    const textbox = page.locator('.chat-input')
    const ariaLabel = await textbox.getAttribute('aria-label')
    const ariaLabelledBy = await textbox.getAttribute('aria-labelledby')
    expect(
      ariaLabel ?? ariaLabelledBy,
      'chat input must have aria-label or aria-labelledby',
    ).not.toBeNull()
  })
})

// ---------------------------------------------------------------------------
// Recovery prompt — Escape and focus management
// ---------------------------------------------------------------------------

test.describe('Recovery prompt keyboard navigation', () => {
  test('Escape closes the recovery prompt', async ({ page, mountApp }) => {
    await mountApp({ hasAutoSave: true })
    const prompt = page.locator('[data-testid="recovery-prompt"]')
    await prompt.waitFor({ state: 'visible' })
    await page.keyboard.press('Escape')
    await expect(prompt).not.toBeVisible()
  })

  test('focus moves into recovery prompt on open', async ({ page, mountApp }) => {
    await mountApp({ hasAutoSave: true })
    await page.locator('[data-testid="recovery-prompt"]').waitFor({ state: 'visible' })
    const isInsidePrompt = await page.evaluate(() => {
      const prompt = document.querySelector('[data-testid="recovery-prompt"]')
      return prompt?.contains(document.activeElement) ?? false
    })
    expect(isInsidePrompt).toBe(true)
  })
})

// ---------------------------------------------------------------------------
// Dialog ARIA semantics
// ---------------------------------------------------------------------------

test.describe('Dialog ARIA semantics', () => {
  test('settings modal has correct ARIA dialog attributes', async ({ page, mountApp }) => {
    await mountApp()
    await page.keyboard.press('Meta+,')
    const modal = page.locator('[data-testid="settings-modal"]')
    await modal.waitFor({ state: 'visible' })

    await expect(modal).toHaveAttribute('role', 'dialog')
    await expect(modal).toHaveAttribute('aria-modal', 'true')
    const hasLabel =
      (await modal.getAttribute('aria-label')) !== null ||
      (await modal.getAttribute('aria-labelledby')) !== null
    expect(hasLabel, 'settings modal must have aria-label or aria-labelledby').toBe(true)
  })

  test('recovery prompt has correct ARIA dialog attributes', async ({ page, mountApp }) => {
    await mountApp({ hasAutoSave: true })
    const prompt = page.locator('[data-testid="recovery-prompt"]')
    await prompt.waitFor({ state: 'visible' })

    await expect(prompt).toHaveAttribute('role', 'dialog')
    await expect(prompt).toHaveAttribute('aria-modal', 'true')
    const hasLabel =
      (await prompt.getAttribute('aria-label')) !== null ||
      (await prompt.getAttribute('aria-labelledby')) !== null
    expect(hasLabel, 'recovery prompt must have aria-label or aria-labelledby').toBe(true)
  })

  test('recovery prompt buttons are operable via keyboard', async ({ page, mountApp }) => {
    await mountApp({ hasAutoSave: true })
    const prompt = page.locator('[data-testid="recovery-prompt"]')
    await prompt.waitFor({ state: 'visible' })

    // Focus discard and activate via Enter — prompt should close
    const discardBtn = page.getByRole('button', { name: 'No, discard' })
    await discardBtn.focus()
    await page.keyboard.press('Enter')
    await expect(prompt).not.toBeVisible()
  })
})

// ---------------------------------------------------------------------------
// Proof editor Tab key handling
// ---------------------------------------------------------------------------

test.describe('Proof editor Tab key handling', () => {
  test('Tab inserts indentation and does not move focus away from the editor', async ({
    page,
    mountApp,
  }) => {
    await mountApp()
    const editor = page.locator('.cm-content')
    await editor.click()

    await page.keyboard.press('Tab')

    expect(await editorHasFocus(page)).toBe(true)

    // The editor content must start with whitespace — indentWithTab inserts
    // the editor's indent unit (spaces or a tab depending on config).
    const text = (await editor.textContent()) ?? ''
    expect(text).toMatch(/^\s+/)
  })

  test('Shift+Tab dedents and does not move focus away from the editor', async ({
    page,
    mountApp,
  }) => {
    await mountApp()
    const editor = page.locator('.cm-content')
    await editor.click()

    await page.keyboard.press('Tab')
    await page.keyboard.press('Shift+Tab')

    expect(await editorHasFocus(page)).toBe(true)
  })
})

test.describe('Setup overlay ARIA', () => {
  test('progress bar has role=progressbar with correct attributes during setup', async ({
    page,
    mountApp,
    emitEvent,
  }) => {
    await mountApp({ setupComplete: false })

    // Emit a setup-progress event with a known progress value.
    await emitEvent('setup-progress', { message: 'Installing…', progress_pct: 42, phase: 'setup' })

    const bar = page.locator('[role="progressbar"]')
    await expect(bar).toBeVisible()
    await expect(bar).toHaveAttribute('aria-valuemin', '0')
    await expect(bar).toHaveAttribute('aria-valuemax', '100')
    await expect(bar).toHaveAttribute('aria-valuenow', '42')
  })

  test('progress bar is indeterminate (no aria-valuenow) when progress is 0', async ({
    page,
    mountApp,
    emitEvent,
  }) => {
    await mountApp({ setupComplete: false })

    // Emit a setup-progress event with progress 0 (indeterminate).
    await emitEvent('setup-progress', { message: 'Starting…', progress_pct: 0, phase: 'setup' })

    const bar = page.locator('[role="progressbar"]')
    await expect(bar).toBeVisible()
    // For indeterminate state, aria-valuenow must be absent.
    await expect(bar).not.toHaveAttribute('aria-valuenow')
  })
})
