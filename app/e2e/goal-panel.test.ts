/**
 * E2E tests for the Goal State panel.
 *
 * Each fenced code block in the goal text is rendered through the
 * `CodeWindow` component — a read-only CodeMirror 6 instance — and hovers
 * on identifiers fan out through the `lsp_hover_goal_panel` Tauri command
 * to the real Formal Proof document.
 */

import { test, expect } from './fixtures'

/** A realistic two-case goal state mirroring the Rust fixture in goal_panel_map. */
const GOAL_TEXT = ['```lean', 'case left', 'hp : p', 'hq : q', '⊢ p', '```'].join('\n')

test.describe('GoalPanel', () => {
  test('renders each code block through a CodeMirror CodeWindow', async ({
    page,
    mountApp,
    emitEvent,
  }) => {
    await mountApp()
    await emitEvent('goal-state-updated', {
      full: GOAL_TEXT,
      panel_line_to_source_line: [1, 1, 1, 1],
    })

    // Two CodeMirror instances: the editable Editor and the read-only
    // CodeWindow inside the goal panel.
    await expect(page.locator('.cm-editor')).toHaveCount(2)

    // The goal-state text should be visible inside the CodeWindow.
    await expect(page.getByText('case left')).toBeVisible()
    await expect(page.getByText(/hp : p/)).toBeVisible()
  })

  test('clicking a goal-panel line highlights the mapped Formal Proof line', async ({
    page,
    mountApp,
    emitEvent,
  }) => {
    await mountApp()
    await emitEvent('goal-state-updated', {
      full: GOAL_TEXT,
      // Every panel line maps to Formal Proof source line 1.
      panel_line_to_source_line: [1, 1, 1, 1],
    })

    // Put some content in the editor so line 1 exists to be highlighted.
    const editor = page.locator('.cm-content').first()
    await editor.click()
    await page.keyboard.type('theorem t : True := by trivial')

    // Click the second line of the CodeWindow — "hp : p".
    const panelLine = page.locator('.code-window .cm-line').nth(1)
    await panelLine.click()

    // The Editor (first .cm-editor) should have exactly one cm-goal-line.
    const editorRoot = page.locator('.cm-editor').first()
    await expect(editorRoot.locator('.cm-goal-line')).toHaveCount(1)
  })
})
