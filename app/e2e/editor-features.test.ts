/**
 * E2E tests for the new editor features added in issue #75.
 *
 * Hover, go-to-definition, and code-actions require a live Lean LSP (covered
 * by Rust integration tests). These tests focus on features that can be
 * exercised with the Tauri mock: indent guides, word wrap, and the symbol
 * outline command palette.
 */

import { test, expect } from './fixtures'

test.describe('Indent guides', () => {
  test('renders indentation markers on nested Lean code', async ({ page, mountApp }) => {
    await mountApp()
    const editor = page.locator('.cm-content')
    await editor.click()

    // Type a small nested block so the indent-marker plugin has something to draw.
    await page.keyboard.type('def f (x : Nat) : Nat := by')
    await page.keyboard.press('Enter')
    await page.keyboard.type('    if x = 0 then')
    await page.keyboard.press('Enter')
    await page.keyboard.type('      x + 1')

    // @replit/codemirror-indentation-markers renders a .cm-indent-markers
    // container on the editor root when nested content is present.
    await expect(page.locator('.cm-indent-markers').first()).toBeAttached()
  })
})

test.describe('Word wrap', () => {
  test('footer toggle flips the editor to wrap mode', async ({ page, mountApp }) => {
    await mountApp()
    const toggle = page.getByTestId('word-wrap-toggle')
    // Wait for the footer to mount — it's below the editor column.
    await expect(toggle).toBeAttached()
    await expect(toggle).toHaveText(/Wrap: Off/)

    await toggle.click()
    await expect(toggle).toHaveText(/Wrap: On/)
    // EditorView.lineWrapping adds the cm-lineWrapping class to .cm-content
    // (via contentAttributes), not to the outer .cm-editor root.
    await expect(page.locator('.cm-content.cm-lineWrapping')).toBeVisible()

    await toggle.click()
    await expect(toggle).toHaveText(/Wrap: Off/)
    await expect(page.locator('.cm-content.cm-lineWrapping')).toHaveCount(0)
  })

  test('menu event toggles word wrap', async ({ page, mountApp, emitEvent }) => {
    await mountApp()
    const toggle = page.getByTestId('word-wrap-toggle')
    await expect(toggle).toHaveText(/Wrap: Off/)

    await emitEvent('menu-event', 'toggle_word_wrap')
    await expect(toggle).toHaveText(/Wrap: On/)
  })
})

test.describe('Symbol outline palette', () => {
  test('Cmd+Shift+O opens the outline and Enter jumps to a symbol', async ({ page, mountApp }) => {
    await mountApp()

    // Pre-populate lsp_document_symbols with a known list.
    await page.evaluate(() => {
      type TauriInvoke = (cmd: string, args?: unknown) => Promise<unknown>
      const tauri = window.__TAURI__ as { core: { invoke: TauriInvoke } }
      const originalInvoke: TauriInvoke = tauri.core.invoke.bind(tauri.core)
      tauri.core.invoke = (cmd: string, args?: unknown): Promise<unknown> => {
        if (cmd === 'lsp_document_symbols') {
          return Promise.resolve([
            {
              name: 'my_theorem',
              kind: 12,
              start_line: 0,
              start_character: 8,
              end_line: 0,
              end_character: 18,
              children: [],
            },
            {
              name: 'helper_lemma',
              kind: 12,
              start_line: 3,
              start_character: 8,
              end_line: 3,
              end_character: 20,
              children: [],
            },
          ])
        }
        return originalInvoke(cmd, args)
      }
    })

    // Put some content in the editor so "jump to line 3" is meaningful.
    const editor = page.locator('.cm-content')
    await editor.click()
    await page.keyboard.type('theorem my_theorem : True := by trivial')
    await page.keyboard.press('Enter')
    await page.keyboard.press('Enter')
    await page.keyboard.press('Enter')
    await page.keyboard.type('theorem helper_lemma : True := by trivial')

    // Cmd+Shift+O on macOS; Ctrl+Shift+O elsewhere. Playwright's Meta = Cmd on macOS.
    const isMac = process.platform === 'darwin'
    await page.keyboard.press(isMac ? 'Meta+Shift+O' : 'Control+Shift+O')

    await expect(page.getByTestId('symbol-outline-overlay')).toBeVisible()
    await expect(page.getByTestId('symbol-outline-input')).toBeFocused()

    // Fuzzy filter: type "help" to narrow to helper_lemma
    await page.keyboard.type('help')
    const items = page.getByTestId('symbol-outline-item')
    await expect(items.first()).toHaveAttribute('data-symbol-name', 'helper_lemma')

    // Enter jumps to the symbol
    await page.keyboard.press('Enter')
    await expect(page.getByTestId('symbol-outline-overlay')).toHaveCount(0)
  })

  test('Escape dismisses the outline without navigation', async ({ page, mountApp }) => {
    await mountApp()
    await page.evaluate(() => {
      type TauriInvoke = (cmd: string, args?: unknown) => Promise<unknown>
      const tauri = window.__TAURI__ as { core: { invoke: TauriInvoke } }
      const originalInvoke: TauriInvoke = tauri.core.invoke.bind(tauri.core)
      tauri.core.invoke = (cmd: string, args?: unknown): Promise<unknown> => {
        if (cmd === 'lsp_document_symbols') return Promise.resolve([])
        return originalInvoke(cmd, args)
      }
    })

    const isMac = process.platform === 'darwin'
    await page.keyboard.press(isMac ? 'Meta+Shift+O' : 'Control+Shift+O')
    await expect(page.getByTestId('symbol-outline-overlay')).toBeVisible()

    await page.keyboard.press('Escape')
    await expect(page.getByTestId('symbol-outline-overlay')).toHaveCount(0)
  })
})
