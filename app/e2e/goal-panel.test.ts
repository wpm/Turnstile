/**
 * E2E tests for the Goal State panel.
 *
 * Each fenced code block in the goal text is rendered through the
 * `GoalBlock` component — a plain HTML renderer with hypothesis-name
 * coloring and active-line highlighting.
 */

import { test, expect } from './fixtures'

/** A realistic two-case goal state mirroring the Rust fixture in goal_panel_map. */
const GOAL_TEXT = ['```lean', 'case left', 'hp : p', 'hq : q', '⊢ p', '```'].join('\n')

test.describe('GoalPanel', () => {
  test('renders each code block as plain HTML with hypothesis styling', async ({
    page,
    mountApp,
    emitEvent,
  }) => {
    await mountApp()
    await emitEvent('goal-state-updated', {
      full: GOAL_TEXT,
      panel_line_to_source_line: [1, 1, 1, 1],
    })

    // Only one CodeMirror instance remains: the editable Editor.
    await expect(page.locator('.cm-editor')).toHaveCount(1)

    // The goal-state text should be visible inside the GoalBlock.
    await expect(page.locator('.goal-block')).toHaveCount(1)
    await expect(page.getByText('case left')).toBeVisible()
    await expect(page.getByText(/hp : p/)).toBeVisible()

    // Hypothesis names are wrapped in styled spans.
    await expect(page.locator('.goal-hyp-name')).toHaveCount(2)
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

    const goalBlock = page.locator('.goal-block')

    // Cursor on source line 1 → both panel lines mapping to source 1 highlight
    // ("case left" and "hp : p").
    await page.keyboard.press('Home')
    await page.keyboard.press('ArrowUp')
    await expect(goalBlock.locator('.goal-line-active')).toHaveCount(2)
    await expect(goalBlock.locator('.goal-line-active').first()).toHaveText(/case left/)

    // Move the cursor down to source line 2 → both panel lines mapping to
    // source 2 highlight ("hq : q" and "⊢ p").
    await page.keyboard.press('ArrowDown')
    await expect(goalBlock.locator('.goal-line-active')).toHaveCount(2)
    await expect(goalBlock.locator('.goal-line-active').first()).toHaveText(/hq : q/)

    // Click into the goal panel — editor loses focus, highlight clears.
    await goalBlock.click()
    await expect(goalBlock.locator('.goal-line-active')).toHaveCount(0)
  })
})
