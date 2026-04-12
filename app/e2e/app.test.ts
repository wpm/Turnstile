import { type Page, type Locator } from '@playwright/test'
import {
  test,
  expect,
  LEAN_SIMPLE_THEOREM,
  LEAN_SIMPLE_THEOREM_LINE1,
  LEAN_DEFINITION,
  LEAN_DEFINITION_LINE2,
  LEAN_WITH_ERROR,
  LEAN_MULTI_STEP_PROOF,
  type AppFixtures,
  type CompletionItemFixture,
  type DiagnosticInfoFixture,
  type SemanticTokenFixture,
} from './fixtures'

// ---------------------------------------------------------------------------
// Page-level helpers
// ---------------------------------------------------------------------------

/** Type a multi-line string into the editor, one line at a time. */
async function typeMultiLine(page: Page, text: string): Promise<void> {
  for (const line of text.split('\n')) {
    await page.keyboard.type(line)
    await page.keyboard.press('Enter')
  }
}

/** Locator for the theme toggle button (☀ / ☾). */
function themeToggleBtn(page: Page): Locator {
  return page.locator('button').filter({ hasText: /[☀☾]/ })
}

// ---------------------------------------------------------------------------
// Setup overlay
// ---------------------------------------------------------------------------

test.describe('SetupOverlay', () => {
  test('shows the overlay while setup is in progress', async ({ page, mountApp: _ }) => {
    // Mount WITHOUT calling mountApp — we want to catch the overlay before it hides.
    await page.addInitScript(() => {
      let resolveLsp!: () => void
      const lspReady = new Promise<void>((r) => (resolveLsp = r))
      window.__resolveLsp = resolveLsp
      window.__TAURI__ = {
        core: {
          invoke(cmd: string) {
            if (cmd === 'get_setup_status') {
              return Promise.resolve({ complete: false, project_path: '/mock/project' })
            }
            if (cmd === 'start_setup') return lspReady
            if (cmd === 'start_lsp') return Promise.resolve(null)
            if (cmd === 'update_document') return Promise.resolve(null)
            if (cmd === 'get_goal_state') return Promise.resolve(null)
            if (cmd === 'get_settings')
              return Promise.resolve({
                editor_font_size: 13,
                prose_font_size: 13,
                chat_font_size: 13,
                model: null,
              })
            if (cmd === 'get_available_models') return Promise.resolve([])
            if (cmd === 'save_settings') return Promise.resolve(null)
            if (cmd === 'set_model') return Promise.resolve(null)
            return Promise.resolve(null)
          },
        },
        event: {
          listen(_event: string) {
            return Promise.resolve(() => {})
          },
        },
      }
    })

    await page.goto('/')

    // The overlay should be visible before setup resolves.
    const overlay = page.locator('text=Checking Lean installation...')
    await overlay.waitFor({ state: 'visible' })
    await expect(overlay).toBeVisible()
  })

  test('hides the overlay once setup is complete', async ({ page, mountApp }) => {
    await mountApp({ setupComplete: true })
    // The overlay is removed from the DOM when setup is done.
    await expect(page.locator('text=Checking Lean installation...')).not.toBeVisible()
    await expect(page.locator('.cm-editor')).toBeVisible()
  })

  test('hides overlay and shows editor when setup is already complete', async ({
    page,
    mountApp,
  }) => {
    await mountApp({ setupComplete: true })
    await expect(page.locator('.cm-editor')).toBeVisible()
    // Overlay text must not be present.
    await expect(page.locator('text=Checking Lean installation...')).not.toBeVisible()
  })
})

// ---------------------------------------------------------------------------
// Editor layout
// ---------------------------------------------------------------------------

test.describe('Editor', () => {
  test('renders the CodeMirror editor', async ({ page, mountApp }) => {
    await mountApp()
    await expect(page.locator('.cm-editor')).toBeVisible()
  })

  test('renders line numbers gutter', async ({ page, mountApp }) => {
    await mountApp()
    await expect(page.locator('.cm-lineNumbers')).toBeVisible()
  })

  test('editor starts with an empty document', async ({ page, mountApp }) => {
    await mountApp()
    const content = await page.locator('.cm-content').textContent()
    expect(content?.trim()).toBe('')
  })

  test('typing Lean code appears in the editor', async ({ page, mountApp }) => {
    await mountApp()
    await page.locator('.cm-content').click()
    await page.keyboard.type('def foo := 42')
    await expect(page.locator('.cm-content')).toContainText('def foo := 42')
  })

  test('renders multi-line Lean theorem', async ({ page, mountApp }) => {
    await mountApp()
    const editor = page.locator('.cm-content')
    await editor.click()
    // Type line by line to preserve line structure
    await typeMultiLine(page, LEAN_SIMPLE_THEOREM)
    await expect(editor).toContainText('theorem add_comm_simple')
    await expect(editor).toContainText('ring')
  })

  test('renders multi-step proof', async ({ page, mountApp }) => {
    await mountApp()
    const editor = page.locator('.cm-content')
    await editor.click()
    await typeMultiLine(page, LEAN_MULTI_STEP_PROOF)
    await expect(editor).toContainText('constructor')
    await expect(editor).toContainText('exact hp')
  })
})

// ---------------------------------------------------------------------------
// Theme toggle
// ---------------------------------------------------------------------------

test.describe('Theme toggle', () => {
  test('toggle button is visible', async ({ page, mountApp }) => {
    await mountApp()
    await expect(themeToggleBtn(page)).toBeVisible()
  })

  test('starts in Dracula (dark) theme', async ({ page, mountApp }) => {
    await mountApp()
    const root = page.locator('[data-theme]')
    await expect(root).toHaveAttribute('data-theme', 'dracula')
  })

  test('toggle switches to light theme', async ({ page, mountApp }) => {
    await mountApp()
    await themeToggleBtn(page).click()
    const root = page.locator('[data-theme]')
    await expect(root).toHaveAttribute('data-theme', 'light')
  })

  test('toggle switches back to dark theme', async ({ page, mountApp }) => {
    await mountApp()
    const btn = themeToggleBtn(page)
    await btn.click() // → light
    await btn.click() // → dracula
    const root = page.locator('[data-theme]')
    await expect(root).toHaveAttribute('data-theme', 'dracula')
  })

  test('editor background changes with theme', async ({ page, mountApp }) => {
    await mountApp()
    const editor = page.locator('.cm-editor')

    // Dark theme background
    await expect(editor).toHaveCSS('background-color', 'rgb(40, 42, 54)') // #282a36

    // Switch to light
    await themeToggleBtn(page).click()
    await expect(editor).toHaveCSS('background-color', 'rgb(255, 255, 255)')
  })
})

// ---------------------------------------------------------------------------
// Semantic token highlighting
// ---------------------------------------------------------------------------

test.describe('Semantic token highlighting', () => {
  /**
   * Build a token fixture for the first word on the first line of a snippet.
   * Tokens are 1-indexed to match the backend convention.
   */
  function keywordToken(col: number, length: number): SemanticTokenFixture {
    return { line: 1, col, length, token_type: 'keyword' }
  }

  test('applies keyword class to "def" in a definition', async ({ page, mountApp, emitEvent }) => {
    // "def identity (x : α) : α := x"
    //  col 0, length 3 → "def"
    const tokens: SemanticTokenFixture[] = [keywordToken(0, 3)]
    await mountApp({ semanticTokens: tokens })

    // Type the fixture into the editor so the token positions are valid.
    await page.locator('.cm-content').click()
    await page.keyboard.type(LEAN_DEFINITION_LINE2) // skip comment line

    // Emit updated tokens after typing
    await emitEvent('lsp-semantic-tokens', tokens)

    await expect(page.locator('.cm-lean-keyword').first()).toBeVisible()
  })

  test('applies type class to Nat type annotation', async ({ page, mountApp, emitEvent }) => {
    // "def badType : Nat := ..."  → "Nat" at col 14, length 3
    const tokens: SemanticTokenFixture[] = [{ line: 1, col: 14, length: 3, token_type: 'type' }]
    await mountApp({ semanticTokens: tokens })

    await page.locator('.cm-content').click()
    await page.keyboard.type(LEAN_WITH_ERROR)
    await emitEvent('lsp-semantic-tokens', tokens)

    await expect(page.locator('.cm-lean-type').first()).toBeVisible()
  })

  test('multiple token types rendered simultaneously', async ({ page, mountApp, emitEvent }) => {
    // "theorem add_comm_simple (a b : Nat) : a + b = b + a := by"
    // "theorem" → keyword at col 0 len 7
    // "Nat"     → type at col 32 len 3
    const tokens: SemanticTokenFixture[] = [
      { line: 1, col: 0, length: 7, token_type: 'keyword' },
      { line: 1, col: 32, length: 3, token_type: 'type' },
    ]
    await mountApp({ semanticTokens: tokens })

    await page.locator('.cm-content').click()
    await page.keyboard.type(LEAN_SIMPLE_THEOREM_LINE1)
    await emitEvent('lsp-semantic-tokens', tokens)

    await expect(page.locator('.cm-lean-keyword').first()).toBeVisible()
    await expect(page.locator('.cm-lean-type').first()).toBeVisible()
  })

  test('semantic tokens cleared when new empty token list is emitted', async ({
    page,
    mountApp,
    emitEvent,
  }) => {
    const tokens: SemanticTokenFixture[] = [{ line: 1, col: 0, length: 3, token_type: 'keyword' }]
    await mountApp({ semanticTokens: tokens })

    await page.locator('.cm-content').click()
    await page.keyboard.type('def foo := 1')
    await emitEvent('lsp-semantic-tokens', tokens)
    await expect(page.locator('.cm-lean-keyword').first()).toBeVisible()

    // Clear tokens
    await emitEvent('lsp-semantic-tokens', [])
    await expect(page.locator('.cm-lean-keyword')).toHaveCount(0)
  })
})

// ---------------------------------------------------------------------------
// Diagnostic gutter markers
// ---------------------------------------------------------------------------

test.describe('Diagnostic gutter markers', () => {
  test('error marker shown on line with type error', async ({ page, mountApp, emitEvent }) => {
    const diag: DiagnosticInfoFixture = {
      start_line: 1,
      start_col: 18,
      end_line: 1,
      end_col: 38,
      severity: 1,
      message: 'type mismatch',
    }
    await mountApp({ diagnostics: [diag] })

    await page.locator('.cm-content').click()
    await page.keyboard.type(LEAN_WITH_ERROR)
    await emitEvent('lsp-diagnostics', [diag])

    await expect(page.locator('.lean-diag-error').first()).toBeVisible()
  })

  test('warning marker shown for warning severity', async ({ page, mountApp, emitEvent }) => {
    const diag: DiagnosticInfoFixture = {
      start_line: 1,
      start_col: 0,
      end_line: 1,
      end_col: 10,
      severity: 2,
      message: 'unused variable',
    }
    await mountApp({ diagnostics: [diag] })

    await page.locator('.cm-content').click()
    await page.keyboard.type(LEAN_DEFINITION)
    await emitEvent('lsp-diagnostics', [diag])

    await expect(page.locator('.lean-diag-warning').first()).toBeVisible()
  })

  test('info marker shown for info severity', async ({ page, mountApp, emitEvent }) => {
    const diag: DiagnosticInfoFixture = {
      start_line: 2,
      start_col: 0,
      end_line: 2,
      end_col: 5,
      severity: 3,
      message: 'try this: ring',
    }
    await mountApp({ diagnostics: [diag] })

    await page.locator('.cm-content').click()
    await typeMultiLine(page, LEAN_SIMPLE_THEOREM)
    await emitEvent('lsp-diagnostics', [diag])

    // .nth(0) is the hidden initialSpacer; .nth(1) is the real gutter marker.
    await expect(page.locator('.lean-diag-info').nth(1)).toBeVisible()
  })

  test('diagnostic gutter marker shows tooltip message on hover', async ({
    page,
    mountApp,
    emitEvent,
  }) => {
    const diag: DiagnosticInfoFixture = {
      start_line: 1,
      start_col: 0,
      end_line: 1,
      end_col: 20,
      severity: 1,
      message: 'type mismatch: expected Nat, got String',
    }
    await mountApp({ diagnostics: [diag] })

    await page.locator('.cm-content').click()
    await page.keyboard.type(LEAN_WITH_ERROR)
    await emitEvent('lsp-diagnostics', [diag])

    // Message is now shown via hover popup, not a native title tooltip.
    // Verify the dot has no title attribute (popup test covers the message content).
    const marker = page.locator('.lean-diag-error').first()
    await expect(marker).not.toHaveAttribute('title')
  })

  test('multiple diagnostics on different lines', async ({ page, mountApp, emitEvent }) => {
    const diags: DiagnosticInfoFixture[] = [
      {
        start_line: 1,
        start_col: 0,
        end_line: 1,
        end_col: 3,
        severity: 1,
        message: 'error on line 1',
      },
      {
        start_line: 2,
        start_col: 0,
        end_line: 2,
        end_col: 3,
        severity: 2,
        message: 'warning on line 2',
      },
    ]
    await mountApp({ diagnostics: diags })

    await page.locator('.cm-content').click()
    await typeMultiLine(page, LEAN_MULTI_STEP_PROOF)
    await emitEvent('lsp-diagnostics', diags)

    await expect(page.locator('.lean-diag-error')).toHaveCount(1)
    await expect(page.locator('.lean-diag-warning')).toHaveCount(1)
  })
})

// ---------------------------------------------------------------------------
// Tab completion
// ---------------------------------------------------------------------------

test.describe('Tab completion', () => {
  /** Shared completion items used across multiple tests. */
  const COMPLETIONS: CompletionItemFixture[] = [
    { label: 'theorem', detail: 'keyword', insert_text: 'theorem' },
    { label: 'Nat.succ', detail: 'Nat → Nat', insert_text: null },
    { label: 'List.map', detail: 'List α → (α → β) → List β', insert_text: null },
  ]

  test('completion menu appears after typing a word character', async ({ page, mountApp }) => {
    await mountApp({ completionItems: COMPLETIONS })
    await page.locator('.cm-content').click()
    await page.keyboard.type('th')
    // The CM6 autocompletion tooltip
    await expect(page.locator('.cm-tooltip-autocomplete')).toBeVisible()
  })

  test('completion menu lists items returned by get_completions', async ({ page, mountApp }) => {
    await mountApp({ completionItems: COMPLETIONS })
    const editor = page.locator('.cm-content')
    await editor.click()
    // Ctrl+Space triggers an explicit completion request with no prefix filter,
    // so all three items from the mock should appear.
    await page.keyboard.press('Control+Space')
    const menu = page.locator('.cm-tooltip-autocomplete')
    await expect(menu).toBeVisible()
    await expect(menu).toContainText('theorem')
    await expect(menu).toContainText('Nat.succ')
    await expect(menu).toContainText('List.map')
  })

  test('completion detail is shown alongside the label', async ({ page, mountApp }) => {
    await mountApp({ completionItems: COMPLETIONS })
    await page.locator('.cm-content').click()
    await page.keyboard.type('Na')
    await expect(page.locator('.cm-tooltip-autocomplete')).toBeVisible()
    // CM6 renders the detail in a .cm-completionDetail span next to the label
    await expect(page.locator('.cm-completionDetail').first()).toBeVisible()
  })

  test('clicking a completion item inserts its label', async ({ page, mountApp }) => {
    const items: CompletionItemFixture[] = [
      { label: 'theorem', detail: 'keyword', insert_text: null },
    ]
    await mountApp({ completionItems: items })
    const editor = page.locator('.cm-content')
    await editor.click()
    await page.keyboard.type('th')
    // Wait for the item to appear in the menu
    const option = page
      .locator('.cm-tooltip-autocomplete [role=option]')
      .filter({ hasText: 'theorem' })
    await expect(option).toBeVisible()
    await option.click()
    await expect(editor).toContainText('theorem')
    await expect(page.locator('.cm-tooltip-autocomplete')).not.toBeVisible()
  })

  test('insert_text is used instead of label when present', async ({ page, mountApp }) => {
    const items: CompletionItemFixture[] = [
      { label: 'mkApp', detail: 'Expr → Expr → Expr', insert_text: 'mkApp f a' },
    ]
    await mountApp({ completionItems: items })
    const editor = page.locator('.cm-content')
    await editor.click()
    await page.keyboard.type('mk')
    // Click the option to accept it
    const option = page
      .locator('.cm-tooltip-autocomplete [role=option]')
      .filter({ hasText: 'mkApp' })
    await expect(option).toBeVisible()
    await option.click()
    await expect(editor).toContainText('mkApp f a')
  })

  test('Escape closes the completion menu without inserting', async ({ page, mountApp }) => {
    await mountApp({ completionItems: COMPLETIONS })
    const editor = page.locator('.cm-content')
    await editor.click()
    await page.keyboard.type('Na')
    await expect(page.locator('.cm-tooltip-autocomplete')).toBeVisible()
    await page.keyboard.press('Escape')
    await expect(page.locator('.cm-tooltip-autocomplete')).not.toBeVisible()
    // Editor still shows what was typed — nothing was inserted
    await expect(editor).toContainText('Na')
  })

  test('completion menu does not appear when get_completions returns empty', async ({
    page,
    mountApp,
  }) => {
    await mountApp({ completionItems: [] })
    const editor = page.locator('.cm-content')
    await editor.click()
    await page.keyboard.type('th')
    // Give CM6 time to call the source and render (or not)
    await page.waitForTimeout(300)
    await expect(page.locator('.cm-tooltip-autocomplete')).not.toBeVisible()
  })
})

// ---------------------------------------------------------------------------
// Diagnostic underlines + hover popup — shared fixtures
// ---------------------------------------------------------------------------

// LEAN_WITH_ERROR = 'def badType : Nat := "this is not a nat"'  (40 chars)
// The string literal "this is not a nat" spans cols 21-40 (exclusive end).
const ERROR_DIAG: DiagnosticInfoFixture = {
  start_line: 1,
  start_col: 21,
  end_line: 1,
  end_col: 40,
  severity: 1,
  message: 'type mismatch',
}

// LEAN_DEFINITION line 2 = 'def identity (x : α) : α := x'
// 'identity' spans cols 4-12 (exclusive end).
const WARNING_DIAG: DiagnosticInfoFixture = {
  start_line: 1,
  start_col: 4,
  end_line: 1,
  end_col: 12,
  severity: 2,
  message: 'unused variable',
}

interface DiagFixtures {
  mountApp: AppFixtures['mountApp']
  emitEvent: AppFixtures['emitEvent']
}

/** Type LEAN_WITH_ERROR into the editor then emit a single error diagnostic. */
async function setupErrorDiag(
  page: Page,
  { mountApp, emitEvent }: DiagFixtures,
  diag: DiagnosticInfoFixture = ERROR_DIAG,
): Promise<void> {
  await mountApp({ diagnostics: [diag] })
  await page.locator('.cm-content').click()
  await page.keyboard.type(LEAN_WITH_ERROR)
  await emitEvent('lsp-diagnostics', [diag])
}

/** Type LEAN_DEFINITION (second line) into the editor then emit a single warning diagnostic. */
async function setupWarningDiag(page: Page, { mountApp, emitEvent }: DiagFixtures): Promise<void> {
  await mountApp({ diagnostics: [WARNING_DIAG] })
  await page.locator('.cm-content').click()
  await page.keyboard.type(LEAN_DEFINITION_LINE2)
  await emitEvent('lsp-diagnostics', [WARNING_DIAG])
}

// ---------------------------------------------------------------------------
// Diagnostic underlines
// ---------------------------------------------------------------------------

test.describe('Diagnostic underlines', () => {
  test('error underline applied to text span covered by diagnostic range', async ({
    page,
    mountApp,
    emitEvent,
  }) => {
    await setupErrorDiag(page, { mountApp, emitEvent })
    await expect(page.locator('.cm-diag-error').first()).toBeVisible()
  })

  test('warning underline applied for severity 2', async ({ page, mountApp, emitEvent }) => {
    await setupWarningDiag(page, { mountApp, emitEvent })
    await expect(page.locator('.cm-diag-warning').first()).toBeVisible()
  })

  test('info underline applied for severity 3', async ({ page, mountApp, emitEvent }) => {
    // LEAN_SIMPLE_THEOREM line 2: '  ring' — 'ring' at cols 2-6
    const diag: DiagnosticInfoFixture = {
      start_line: 2,
      start_col: 2,
      end_line: 2,
      end_col: 6,
      severity: 3,
      message: 'try this: ring',
    }
    await mountApp({ diagnostics: [diag] })
    await page.locator('.cm-content').click()
    await typeMultiLine(page, LEAN_SIMPLE_THEOREM)
    await emitEvent('lsp-diagnostics', [diag])

    await expect(page.locator('.cm-diag-info').first()).toBeVisible()
  })

  test('underlines cleared when diagnostics updated to empty list', async ({
    page,
    mountApp,
    emitEvent,
  }) => {
    await setupErrorDiag(page, { mountApp, emitEvent })
    await expect(page.locator('.cm-diag-error').first()).toBeVisible()

    await emitEvent('lsp-diagnostics', [])
    await expect(page.locator('.cm-diag-error')).toHaveCount(0)
  })

  test('error underline has wavy red text-decoration in Dracula theme', async ({
    page,
    mountApp,
    emitEvent,
  }) => {
    await setupErrorDiag(page, { mountApp, emitEvent })

    const underlined = page.locator('.cm-diag-error').first()
    await expect(underlined).toBeVisible()
    const decoration = await underlined.evaluate((el) => getComputedStyle(el).textDecoration)
    expect(decoration).toContain('wavy')
    // #ff5555 → rgb(255, 85, 85)
    expect(decoration).toContain('rgb(255, 85, 85)')
  })

  test('multiple diagnostics produce multiple underlined spans', async ({
    page,
    mountApp,
    emitEvent,
  }) => {
    // LEAN_SIMPLE_THEOREM line 1: 'theorem ...' — 'theorem' at cols 0-7
    // LEAN_SIMPLE_THEOREM line 2: '  ring' — 'ring' at cols 2-6
    const diags: DiagnosticInfoFixture[] = [
      { start_line: 1, start_col: 0, end_line: 1, end_col: 7, severity: 1, message: 'error' },
      { start_line: 2, start_col: 2, end_line: 2, end_col: 6, severity: 2, message: 'warning' },
    ]
    await mountApp({ diagnostics: diags })
    await page.locator('.cm-content').click()
    await typeMultiLine(page, LEAN_SIMPLE_THEOREM)
    await emitEvent('lsp-diagnostics', diags)

    await expect(page.locator('.cm-diag-error')).toHaveCount(1)
    await expect(page.locator('.cm-diag-warning')).toHaveCount(1)
  })
})

// ---------------------------------------------------------------------------
// Diagnostic hover popup
// ---------------------------------------------------------------------------

test.describe('Diagnostic hover popup', () => {
  test('hovering an error underline shows the diagnostic message', async ({
    page,
    mountApp,
    emitEvent,
  }) => {
    const diag = { ...ERROR_DIAG, message: 'type mismatch: expected Nat, got String' }
    await setupErrorDiag(page, { mountApp, emitEvent }, diag)

    const underline = page.locator('.cm-diag-error').first()
    await expect(underline).toBeVisible()
    await underline.hover()

    await expect(page.locator('.lean-diag-popup')).toBeVisible()
    await expect(page.locator('.lean-diag-popup')).toContainText(
      'type mismatch: expected Nat, got String',
    )
  })

  test('error popup has the error severity style', async ({ page, mountApp, emitEvent }) => {
    await setupErrorDiag(page, { mountApp, emitEvent })
    await page.locator('.cm-diag-error').first().hover()
    await expect(page.locator('.lean-diag-popup-error')).toBeVisible()
  })

  test('warning popup has the warning severity style', async ({ page, mountApp, emitEvent }) => {
    await setupWarningDiag(page, { mountApp, emitEvent })
    await page.locator('.cm-diag-warning').first().hover()
    await expect(page.locator('.lean-diag-popup-warning')).toBeVisible()
  })

  test('popup disappears when mouse leaves the underline', async ({
    page,
    mountApp,
    emitEvent,
  }) => {
    await setupErrorDiag(page, { mountApp, emitEvent })

    await page.locator('.cm-diag-error').first().hover()
    await expect(page.locator('.lean-diag-popup')).toBeVisible()

    await page.mouse.move(0, 0)
    await expect(page.locator('.lean-diag-popup')).not.toBeVisible()
  })

  test('gutter dot no longer shows a native tooltip', async ({ page, mountApp, emitEvent }) => {
    await setupErrorDiag(page, { mountApp, emitEvent })

    const dot = page.locator('.lean-diag-error').first()
    await expect(dot).toBeVisible()
    await expect(dot).not.toHaveAttribute('title')
  })
})
