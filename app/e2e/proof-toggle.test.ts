import { test, expect } from './fixtures'
import { LEAN_SIMPLE_THEOREM } from './fixtures'

test.describe('Proof View Toggle', () => {
  test('toggle switches between editor and prose panel', async ({ page, mountApp }) => {
    await mountApp()

    // Initially the editor is visible and prose panel is not.
    await expect(page.locator('.cm-content')).toBeVisible()
    await expect(page.locator('[data-testid="prose-panel"]')).not.toBeVisible()

    // Click the toggle button.
    const toggle = page.locator('button[aria-label="Switch to Prose Proof"]')
    await expect(toggle).toBeVisible()
    await toggle.click()

    // Editor should be hidden, prose panel visible.
    await expect(page.locator('.cm-content')).not.toBeVisible()
    await expect(page.locator('[data-testid="prose-panel"]')).toBeVisible()
  })

  test('toggle back restores the editor', async ({ page, mountApp }) => {
    await mountApp()

    // Toggle to prose.
    await page.locator('button[aria-label="Switch to Prose Proof"]').click()
    await expect(page.locator('[data-testid="prose-panel"]')).toBeVisible()

    // Toggle back to formal.
    const toggleBack = page.locator('button[aria-label="Switch to Formal Proof"]')
    await expect(toggleBack).toBeVisible()
    await toggleBack.click()

    // Editor should be back.
    await expect(page.locator('.cm-content')).toBeVisible()
    await expect(page.locator('[data-testid="prose-panel"]')).not.toBeVisible()
  })

  test('header title updates with view', async ({ page, mountApp }) => {
    await mountApp()

    // Initial title is "Formal Proof". Use .first() since ChatPanel also has an uppercase span.
    const header = page.locator('.flex.flex-col.flex-1 span.uppercase').first()
    await expect(header).toHaveText('Formal Proof')

    // Toggle to prose.
    await page.locator('button[aria-label="Switch to Prose Proof"]').click()
    await expect(header).toHaveText('Prose Proof')

    // Toggle back.
    await page.locator('button[aria-label="Switch to Formal Proof"]').click()
    await expect(header).toHaveText('Formal Proof')
  })

  test('prose panel shows placeholder when no prose text', async ({ page, mountApp }) => {
    await mountApp()

    // Toggle to prose with empty content.
    await page.locator('button[aria-label="Switch to Prose Proof"]').click()

    const placeholder = page.locator('[data-testid="prose-panel"] p')
    await expect(placeholder).toContainText('Toggle to Formal Proof')
  })

  test('prose panel shows generated content after invoke', async ({
    page,
    mountApp,
    emitEvent,
  }) => {
    await mountApp()

    // Type some content so editorContent is non-empty.
    await page.locator('.cm-content').click()
    await page.keyboard.type(LEAN_SIMPLE_THEOREM)

    // Emit prose-updated to simulate background generation.
    await emitEvent('prose-updated', {
      text: '\\begin{theorem}\nA test theorem.\n\\end{theorem}\n\n\\begin{proof}\nBy ring.\n\\end{proof}',
      hash: 'test-hash',
    })

    // Toggle to prose.
    await page.locator('button[aria-label="Switch to Prose Proof"]').click()

    // Should show rendered content (not the placeholder).
    const panel = page.locator('[data-testid="prose-panel"]')
    await expect(panel).toBeVisible()
    // The theorem environment gets converted to bold markdown by renderContent.
    await expect(panel.locator('strong')).toBeVisible()
  })

  test('session-loaded with proof_view prose shows prose panel', async ({
    page,
    mountApp,
    emitEvent,
  }) => {
    await mountApp()

    // Simulate loading a session that was saved with prose view active.
    await emitEvent('session-loaded', {
      meta: {
        format_version: 1,
        created_at: '',
        saved_at: '',
        cursor_line: 0,
        cursor_col: 0,
        editor_scroll_top: 0,
        chat_width_pct: 25,
        proof_view: 'prose',
      },
      proof_lean: 'theorem foo : True := by trivial',
      prose: { text: 'A prose proof.', tactic_state_hash: null },
      transcript: [],
      summary: null,
    })

    // Prose panel should be visible, editor hidden.
    await expect(page.locator('[data-testid="prose-panel"]')).toBeVisible()
    await expect(page.locator('.cm-content')).not.toBeVisible()
  })

  test('toggle button is keyboard accessible', async ({ page, mountApp }) => {
    await mountApp()

    // Tab to the toggle button and activate with Enter.
    const toggle = page.locator('button[aria-label="Switch to Prose Proof"]')
    await toggle.focus()
    await page.keyboard.press('Enter')

    // Should switch to prose view.
    await expect(page.locator('[data-testid="prose-panel"]')).toBeVisible()
  })
})
