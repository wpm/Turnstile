import { test as base, type Page } from '@playwright/test'

// ---------------------------------------------------------------------------
// Lean code snippet fixtures — reused across tests
// ---------------------------------------------------------------------------

/** A simple theorem with one tactic proof step. */
export const LEAN_SIMPLE_THEOREM = `theorem add_comm_simple (a b : Nat) : a + b = b + a := by
  ring`

/** A definition with a doc comment. */
export const LEAN_DEFINITION = `-- A simple identity function
def identity (x : α) : α := x`

/** Code with a deliberate type error to trigger a diagnostic. */
export const LEAN_WITH_ERROR = `def badType : Nat := "this is not a nat"`

/** A multi-line proof with visible goal state at each step. */
export const LEAN_MULTI_STEP_PROOF = `theorem and_comm (p q : Prop) (hp : p) (hq : q) : p ∧ q := by
  constructor
  · exact hp
  · exact hq`

// ---------------------------------------------------------------------------
// Tauri mock helpers
// ---------------------------------------------------------------------------

/**
 * Options for the Tauri mock injected into the page.
 * All fields are optional; defaults produce a "setup complete / LSP silent" state.
 */
export interface TauriMockOptions {
  /** Whether get_setup_status returns complete:true (default true) */
  setupComplete?: boolean
  /** Diagnostics to emit via lsp-diagnostics event (default []) */
  diagnostics?: DiagnosticInfoFixture[]
  /** Semantic tokens to emit via lsp-semantic-tokens event (default []) */
  semanticTokens?: SemanticTokenFixture[]
  /** Goal text returned by get_goal_state (default '') */
  goalText?: string
  /** Items returned by get_completions (default []) */
  completionItems?: CompletionItemFixture[]
}

export interface DiagnosticInfoFixture {
  start_line: number
  start_col: number
  end_line: number
  end_col: number
  severity: number // 1=error, 2=warning, 3=info
  message: string
}

export interface SemanticTokenFixture {
  line: number
  col: number
  length: number
  token_type: string
}

export interface CompletionItemFixture {
  label: string
  detail?: string | null
  insert_text?: string | null
}

/**
 * Injects a window.__TAURI__ mock before the app script runs.
 * Must be called in a `page.addInitScript` before navigation.
 */
export async function injectTauriMock(page: Page, opts: TauriMockOptions = {}): Promise<void> {
  const {
    setupComplete = true,
    diagnostics = [],
    semanticTokens = [],
    goalText = '',
    completionItems = [],
  } = opts

  await page.addInitScript(
    ({ setupComplete, diagnostics, semanticTokens, goalText, completionItems }) => {
      type Listener = (e: { payload: unknown }) => void
      const listeners = new Map<string, Listener[]>()

      // Expose a helper so tests can fire LSP events from outside.
      ;(window as unknown as Record<string, unknown>).__tauriEmit = (
        event: string,
        payload: unknown,
      ) => {
        for (const cb of listeners.get(event) ?? []) {
          cb({ payload })
        }
      }

      window.__TAURI__ = {
        core: {
          invoke(cmd: string) {
            if (cmd === 'get_setup_status') {
              return Promise.resolve({ complete: setupComplete, project_path: '/mock/project' })
            }
            if (cmd === 'start_setup') return Promise.resolve(null)
            if (cmd === 'start_lsp') return Promise.resolve(null)
            if (cmd === 'update_document') return Promise.resolve(null)
            if (cmd === 'get_goal_state') return Promise.resolve(goalText || null)
            if (cmd === 'get_completions') return Promise.resolve(completionItems)
            return Promise.resolve(null)
          },
        },
        event: {
          listen(event: string, cb: Listener) {
            if (!listeners.has(event)) listeners.set(event, [])
            listeners.get(event)!.push(cb)

            // After all listeners are registered the app calls start_lsp, which
            // calls get_setup_status. We fire synthetic events a tick later to
            // simulate the LSP server sending its first batch of data.
            if (event === 'lsp-semantic-tokens') {
              Promise.resolve().then(() => {
                for (const cb2 of listeners.get('lsp-diagnostics') ?? []) {
                  cb2({ payload: diagnostics })
                }
                for (const cb2 of listeners.get('lsp-semantic-tokens') ?? []) {
                  cb2({ payload: semanticTokens })
                }
              })
            }
            return Promise.resolve(() => {
              const list = listeners.get(event)
              if (list) {
                const idx = list.indexOf(cb)
                if (idx !== -1) list.splice(idx, 1)
              }
            })
          },
        },
      }
    },
    { setupComplete, diagnostics, semanticTokens, goalText, completionItems },
  )
}

// ---------------------------------------------------------------------------
// Custom test fixture — wraps base test with ready-app helpers
// ---------------------------------------------------------------------------

interface AppFixtures {
  /** Navigate to the app with Tauri mocked; resolves once the editor is visible. */
  mountApp(opts?: TauriMockOptions): Promise<void>
  /** Emit a Tauri LSP event from inside the page. */
  emitEvent(event: string, payload: unknown): Promise<void>
}

export const test = base.extend<AppFixtures>({
  mountApp: async ({ page }, use) => {
    await use(async (opts: TauriMockOptions = {}) => {
      await injectTauriMock(page, opts)
      await page.goto('/')
      // Wait until the CodeMirror editor content area is present.
      await page.locator('.cm-content').waitFor({ state: 'visible' })
    })
  },

  emitEvent: async ({ page }, use) => {
    await use(async (event: string, payload: unknown) => {
      await page.evaluate(
        ({ event, payload }) => {
          ;(window as unknown as Record<string, unknown>).__tauriEmit(event, payload)
        },
        { event, payload },
      )
    })
  },
})

export { expect } from '@playwright/test'
