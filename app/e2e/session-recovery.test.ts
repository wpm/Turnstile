// Behavioural coverage for the "Restore unsaved session?" recovery dialog.
//
// The dialog appears on launch when `check_auto_save` reports that
// `autosave.turn` exists on disk. It offers two actions — "Yes, restore"
// and "No, discard" — and regressing the wiring on either one causes the
// symptoms described in issue #90: the app ends up blank because neither
// button actually loads anything into the editor.
//
// These tests spy on the invoke calls each button produces so a future
// refactor that re-routes the buttons through a different backend command
// fails here before it ships.

import type { Page } from '@playwright/test'

import { test, expect } from './fixtures'

// ---------------------------------------------------------------------------
// In-page invoke spy — records the command name of every Tauri invoke call
// without disturbing the mock's return values.
// ---------------------------------------------------------------------------

async function installInvokeSpy(page: Page): Promise<void> {
  await page.evaluate(() => {
    const log: string[] = []
    ;(window as unknown as { __invokeLog: string[] }).__invokeLog = log

    type TauriInvoke = (cmd: string, args?: unknown) => Promise<unknown>
    const tauri = window.__TAURI__ as { core: { invoke: TauriInvoke } }
    const original: TauriInvoke = tauri.core.invoke.bind(tauri.core)
    tauri.core.invoke = (cmd: string, args?: unknown): Promise<unknown> => {
      log.push(cmd)
      return original(cmd, args)
    }
  })
}

async function invokeLog(page: Page): Promise<string[]> {
  return page.evaluate(() => (window as unknown as { __invokeLog: string[] }).__invokeLog.slice())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

test.describe('Recovery prompt: "Yes, restore"', () => {
  test('calls restore_auto_save (not open_session with a null path)', async ({
    page,
    mountApp,
  }) => {
    await mountApp({ hasAutoSave: true })
    await installInvokeSpy(page)
    await page.getByTestId('recovery-prompt').waitFor({ state: 'visible' })

    await page.getByRole('button', { name: 'Yes, restore' }).click()
    await expect(page.getByTestId('recovery-prompt')).toHaveCount(0)

    const log = await invokeLog(page)
    expect(log).toContain('restore_auto_save')
    // Regression guard: the buggy implementation called open_session with
    // path:null, which in the backend triggers a file picker rather than
    // loading the autosave. The new restore_auto_save command replaces it.
    expect(log).not.toContain('open_session')
  })

  test('restores the autosaved proof into the editor', async ({ page, mountApp }) => {
    // The fixture's restore_auto_save mock emits a session-loaded event
    // whose proof_lean payload is "restored autosave content". If the
    // restore wiring is broken, the editor stays empty.
    await mountApp({ hasAutoSave: true })
    await page.getByTestId('recovery-prompt').waitFor({ state: 'visible' })

    await page.getByRole('button', { name: 'Yes, restore' }).click()

    const editor = page.locator('.cm-content')
    await expect(editor).toContainText('restored autosave content')
  })
})

test.describe('Recovery prompt: "No, discard"', () => {
  test('deletes the autosave and falls back to reopening the last saved session', async ({
    page,
    mountApp,
  }) => {
    // Make get_last_session return a path so we can observe the fallback
    // reopen fire. Without the fallback the app would sit blank — the
    // exact symptom reported in issue #90 after pressing "No, discard".
    await mountApp({ hasAutoSave: true })
    await page.evaluate(() => {
      type TauriInvoke = (cmd: string, args?: unknown) => Promise<unknown>
      const tauri = window.__TAURI__ as { core: { invoke: TauriInvoke } }
      const original: TauriInvoke = tauri.core.invoke.bind(tauri.core)
      tauri.core.invoke = (cmd: string, args?: unknown): Promise<unknown> => {
        if (cmd === 'get_last_session') {
          return Promise.resolve('/mock/last/session.turn')
        }
        return original(cmd, args)
      }
    })
    await installInvokeSpy(page)
    await page.getByTestId('recovery-prompt').waitFor({ state: 'visible' })

    await page.getByRole('button', { name: 'No, discard' }).click()
    await expect(page.getByTestId('recovery-prompt')).toHaveCount(0)

    // Give the async fallback chain a moment to run.
    await expect
      .poll(async () => {
        const log = await invokeLog(page)
        return log.includes('delete_auto_save') && log.includes('get_last_session')
      })
      .toBe(true)

    const log = await invokeLog(page)
    // delete_auto_save must fire before get_last_session — the autosave is
    // committed to removal before we fall back to last-saved.
    const deleteIdx = log.indexOf('delete_auto_save')
    const lastIdx = log.indexOf('get_last_session')
    expect(deleteIdx).toBeGreaterThan(-1)
    expect(lastIdx).toBeGreaterThan(deleteIdx)
    // And since get_last_session returned a path, open_session must have
    // been invoked to reopen that file.
    expect(log).toContain('open_session')
  })

  test('does not try to reopen anything when there is no last saved session', async ({
    page,
    mountApp,
  }) => {
    // Default fixture: get_last_session returns null.
    await mountApp({ hasAutoSave: true })
    await installInvokeSpy(page)
    await page.getByTestId('recovery-prompt').waitFor({ state: 'visible' })

    await page.getByRole('button', { name: 'No, discard' }).click()
    await expect(page.getByTestId('recovery-prompt')).toHaveCount(0)

    await expect.poll(async () => (await invokeLog(page)).includes('get_last_session')).toBe(true)
    const log = await invokeLog(page)
    // get_last_session was consulted, but since it returned null there is
    // nothing to open — open_session must not be invoked.
    expect(log).not.toContain('open_session')
  })
})
