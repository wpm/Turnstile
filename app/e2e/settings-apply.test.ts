import { type Locator, type Page } from '@playwright/test'
import { test, expect } from './fixtures'

// ---------------------------------------------------------------------------
// These tests codify the user-visible contract for the Settings "Apply" button:
//
//   1. On Apply, the setting is immediately applied to the UI in the background.
//   2. On Apply, the button disables itself (the tab is no longer "dirty").
//   3. Changing a field after Apply re-enables the button.
//
// They are end-to-end because the bug being guarded against is specifically
// "a setting exists in the store but no UI component reads it." A unit test
// against the store can't distinguish that from a fully working app.
// ---------------------------------------------------------------------------

/** Open the Settings modal via the Cmd/Ctrl-, keyboard shortcut. */
async function openSettings(page: Page): Promise<void> {
  // The app listens for Meta+, on both macOS and Linux/Windows (meta || ctrl).
  await page.keyboard.press('ControlOrMeta+Comma')
  await page.getByTestId('settings-modal').waitFor({ state: 'visible' })
  // Appearance is the default active tab; make sure it's visible.
  await expect(page.getByTestId('apply-appearance-button')).toBeVisible()
}

/**
 * Pick a value in a SelectField (a custom combobox, not a native <select>).
 * Opens the listbox by clicking the trigger, then clicks the option whose
 * label matches e.g. "20px".
 */
async function pickSelectFieldOption(page: Page, testId: string, labelText: string): Promise<void> {
  await page.getByTestId(testId).click()
  // The listbox is a sibling <ul> with role=listbox; its <li> options show labels.
  await page.getByRole('option', { name: labelText }).click()
}

/** Assert every locator has the given computed `font-size`. */
async function expectFontSize(locators: Locator[], size: string): Promise<void> {
  for (const l of locators) await expect(l).toHaveCSS('font-size', size)
}

test.describe('Settings Apply — Appearance', () => {
  test('Editor font size applies to the editor DOM immediately on Apply', async ({
    page,
    mountApp,
  }) => {
    await mountApp()
    await openSettings(page)

    // Baseline: editor renders at the default 13px (via CSS fallback).
    const editor = page.locator('.cm-editor').first()
    await expect(editor).toHaveCSS('font-size', '13px')

    // The `.cm-scroller` inside is what actually renders the visible text.
    // Asserting here guards against CodeMirror theme extensions pinning the
    // size at a higher specificity than the inherited `.cm-editor` rule.
    const scroller = page.locator('.cm-scroller').first()
    await expect(scroller).toHaveCSS('font-size', '13px')

    // Change Editor font size to 20 and Apply.
    await pickSelectFieldOption(page, 'editor-font-size-select', '20px')
    await page.getByTestId('apply-appearance-button').click()

    // The editor font size visibly updates in the background while the
    // Settings modal is still open — both on the `.cm-editor` wrapper and
    // on the `.cm-scroller` that renders text.
    await expect(editor).toHaveCSS('font-size', '20px')
    await expect(scroller).toHaveCSS('font-size', '20px')
    await expect(page.getByTestId('settings-modal')).toBeVisible()
  })

  test('Chat font size applies to the chat panel immediately on Apply', async ({
    page,
    mountApp,
  }) => {
    await mountApp()

    // Pre-Apply: post a message so a user bubble exists in the DOM. The
    // fixture's Tauri echo mock also produces an assistant bubble.
    const chatInput = page.locator('.chat-input')
    await chatInput.click()
    await page.keyboard.type('scale me')
    await page.keyboard.press('Enter')
    const userBubble = page.locator('.chat-message-user').first()
    const assistantBubble = page.locator('.chat-message-assistant').first()
    await expect(userBubble).toBeVisible()
    await expect(assistantBubble).toBeVisible()

    await openSettings(page)

    // Regression guard for #127: asserting the `.chat-panel` wrapper alone
    // is not enough. If a child pins its own `text-[Npx]`, the wrapper's
    // inline `font-size` updates but the user sees no change — so assert
    // on the elements that actually render conversational text too.
    const chatPanel = page.locator('.chat-panel')
    const scalable = [chatInput, userBubble, assistantBubble]
    await expect(chatPanel).toHaveCSS('font-size', '13px')
    await expectFontSize(scalable, '13px')

    await pickSelectFieldOption(page, 'chat-font-size-select', '18px')
    await page.getByTestId('apply-appearance-button').click()

    await expect(chatPanel).toHaveCSS('font-size', '18px')
    await expectFontSize(scalable, '18px')
  })

  test('Apply button disables after Apply and re-enables on next change', async ({
    page,
    mountApp,
  }) => {
    await mountApp()
    await openSettings(page)

    const applyBtn = page.getByTestId('apply-appearance-button')

    // Opens in a non-dirty state.
    await expect(applyBtn).toBeDisabled()

    // Dirtying a field enables the button.
    await pickSelectFieldOption(page, 'editor-font-size-select', '16px')
    await expect(applyBtn).toBeEnabled()

    // Clicking Apply returns the button to a disabled state — the tab is
    // no longer dirty because the committed snapshot now matches the draft.
    await applyBtn.click()
    await expect(applyBtn).toBeDisabled()

    // Changing any field again re-enables the button.
    await pickSelectFieldOption(page, 'editor-font-size-select', '18px')
    await expect(applyBtn).toBeEnabled()
  })

  test('Multiple font changes in one Apply all land in the background', async ({
    page,
    mountApp,
  }) => {
    await mountApp()
    await openSettings(page)

    await pickSelectFieldOption(page, 'editor-font-size-select', '16px')
    await pickSelectFieldOption(page, 'chat-font-size-select', '20px')
    await page.getByTestId('apply-appearance-button').click()

    await expect(page.locator('.cm-editor').first()).toHaveCSS('font-size', '16px')
    await expect(page.locator('.chat-panel')).toHaveCSS('font-size', '20px')
    await expect(page.getByTestId('apply-appearance-button')).toBeDisabled()
  })
})
