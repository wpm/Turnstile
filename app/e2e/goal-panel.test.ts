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

  test('editor cursor highlights the matching goal-panel line when focused', async ({
    page,
    mountApp,
    emitEvent,
  }) => {
    await mountApp()
    // Two-line proof so we can move the cursor between source lines and see
    // the panel highlight follow.
    const proofText = 'theorem t : True := by\n  trivial'
    const editor = page.locator('.cm-content').first()
    await editor.click()
    await page.keyboard.type(proofText)

    // Each panel row maps to a distinct Formal Proof line:
    //   case left → 1, hp : p → 1, hq : q → 2, ⊢ p → 2
    await emitEvent('goal-state-updated', {
      full: GOAL_TEXT,
      panel_line_to_source_line: [1, 1, 2, 2],
    })

    const codeWindow = page.locator('.code-window .cm-editor')

    // Cursor on source line 1 (after typing the first character) → first panel
    // line that maps to source 1 is "case left" (CodeWindow line 1).
    await page.keyboard.press('Home')
    await page.keyboard.press('ArrowUp')
    await expect(codeWindow.locator('.cm-line.cm-goal-line')).toHaveCount(1)
    await expect(codeWindow.locator('.cm-line.cm-goal-line')).toHaveText(/case left/)

    // Move the cursor down to source line 2 → first panel line that maps to
    // source 2 is "hq : q" (CodeWindow line 3).
    await page.keyboard.press('ArrowDown')
    await expect(codeWindow.locator('.cm-line.cm-goal-line')).toHaveText(/hq : q/)

    // Click into the goal panel — editor loses focus, highlight clears.
    await codeWindow.click()
    await expect(codeWindow.locator('.cm-line.cm-goal-line')).toHaveCount(0)
  })
})
