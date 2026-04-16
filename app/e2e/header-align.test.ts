import { test, expect } from './fixtures'

test('header bottom edges are vertically aligned', async ({ page, mountApp }) => {
  await mountApp()

  const proofHeaderBottom = await page.evaluate(() => {
    // First child of the editor column — the FORMAL PROOF header div
    const editorCol = document.querySelector('.flex.flex-col.flex-1')
    const header = editorCol?.querySelector(':scope > div')
    return header?.getBoundingClientRect().bottom ?? null
  })

  const assistantPanelHeaderBottom = await page.evaluate(() => {
    const assistantHeader = document.querySelector('.assistant-panel > div')
    return assistantHeader?.getBoundingClientRect().bottom ?? null
  })

  expect(proofHeaderBottom).toBeCloseTo(assistantPanelHeaderBottom ?? 0, 0)
})
