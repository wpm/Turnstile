import { test, expect } from './fixtures'

test('header bottom edges are vertically aligned', async ({ page, mountApp }) => {
  await mountApp()

  const proofHeaderBottom = await page.evaluate(() => {
    // First child of the editor column — the FORMAL PROOF header div
    const editorCol = document.querySelector('.flex.flex-col.flex-1')
    const header = editorCol?.querySelector(':scope > div')
    return header?.getBoundingClientRect().bottom ?? null
  })

  const assistantHeaderBottom = await page.evaluate(() => {
    const chatHeader = document.querySelector('.chat-panel > div')
    return chatHeader?.getBoundingClientRect().bottom ?? null
  })

  expect(proofHeaderBottom).toBeCloseTo(assistantHeaderBottom ?? 0, 0)
})
