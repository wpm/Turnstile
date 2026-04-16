import { test as base, type Page } from '@playwright/test'

// ---------------------------------------------------------------------------
// Lean code snippet fixtures — reused across tests
// ---------------------------------------------------------------------------

/** First line of LEAN_SIMPLE_THEOREM (the theorem declaration). */
export const LEAN_SIMPLE_THEOREM_LINE1 = 'theorem add_comm_simple (a b : Nat) : a + b = b + a := by'

/** A simple theorem with one tactic proof step. */
export const LEAN_SIMPLE_THEOREM = `${LEAN_SIMPLE_THEOREM_LINE1}
  ring`

/** Second line of LEAN_DEFINITION (the actual definition, skipping the comment). */
export const LEAN_DEFINITION_LINE2 = 'def identity (x : α) : α := x'

/** A definition with a doc comment. */
export const LEAN_DEFINITION = `-- A simple identity function
${LEAN_DEFINITION_LINE2}`

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
  /** Whether check_auto_save returns true, showing the recovery prompt (default false) */
  hasAutoSave?: boolean
  /** Diagnostics to emit via lsp-diagnostics event (default []) */
  diagnostics?: DiagnosticInfoFixture[]
  /** Semantic tokens to emit via lsp-semantic-tokens event (default []) */
  semanticTokens?: SemanticTokenFixture[]
  /** Items returned by get_completions (default []) */
  completionItems?: CompletionItemFixture[]
  /** When true, send_message does NOT auto-fire assistant-complete (default false) */
  noAutoReply?: boolean
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
  token_modifiers?: string[]
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
    hasAutoSave = false,
    diagnostics = [],
    semanticTokens = [],
    completionItems = [],
    noAutoReply = false,
  } = opts

  await page.addInitScript(
    ({ setupComplete, hasAutoSave, diagnostics, semanticTokens, completionItems, noAutoReply }) => {
      type Listener = (e: { payload: unknown }) => void
      const listeners = new Map<string, Listener[]>()

      // Expose a helper so tests can fire LSP events from outside.
      window.__tauriEmit = (event: string, payload: unknown) => {
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
            if (cmd === 'get_completions') return Promise.resolve(completionItems)
            if (cmd === 'get_settings')
              return Promise.resolve({
                editor_font_size: 13,
                prose_font_size: 13,
                assistant_font_size: 13,
                model: null,
                theme: 'dark',
              })
            if (cmd === 'get_available_models')
              return Promise.resolve([
                { id: 'claude-opus-4-6', display_name: 'Claude Opus 4.6' },
                { id: 'claude-sonnet-4-6', display_name: 'Claude Sonnet 4.6' },
                { id: 'claude-haiku-4-5-20251001', display_name: 'Claude Haiku 4.5' },
              ])
            if (cmd === 'save_settings') return Promise.resolve(null)
            if (cmd === 'set_model') return Promise.resolve(null)
            if (cmd === 'check_auto_save') return Promise.resolve(hasAutoSave)
            if (cmd === 'delete_auto_save') return Promise.resolve(null)
            if (cmd === 'restore_auto_save') {
              // Simulate the backend emitting session-loaded with the
              // autosave's content so the frontend can restore the editor.
              void Promise.resolve().then(() => {
                for (const cb2 of listeners.get('session-loaded') ?? []) {
                  cb2({
                    payload: {
                      meta: {
                        format_version: 1,
                        created_at: '',
                        saved_at: '',
                        cursor_line: 0,
                        cursor_col: 0,
                        editor_scroll_top: 0,
                        assistant_width_pct: 25,
                        proof_view: 'formal',
                        goal_panel_pct: 30,
                        word_wrap: false,
                      },
                      proof_lean: 'restored autosave content',
                      prose: { text: '', tactic_state_hash: null },
                      turns: [],
                      summary: null,
                    },
                  })
                }
              })
              return Promise.resolve(null)
            }
            if (cmd === 'get_last_session') return Promise.resolve(null)
            if (cmd === 'set_last_session') return Promise.resolve(null)
            if (cmd === 'generate_prose') {
              // Simulate async prose generation — emit prose-updated after a tick.
              void Promise.resolve().then(() => {
                for (const cb2 of listeners.get('prose-updated') ?? []) {
                  cb2({
                    payload: {
                      text: '\\begin{theorem}[Mock]\nA mock prose proof.\n\\end{theorem}\n\n\\begin{proof}\nTrivial. $\\square$\n\\end{proof}',
                      hash: 'mock-hash',
                    },
                  })
                }
              })
              return Promise.resolve('mock prose')
            }
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
              void Promise.resolve().then(() => {
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

      // Patch invoke to handle assistant commands and auto-fire
      // assistant-complete.
      type TauriInvoke = (cmd: string, args?: unknown) => Promise<unknown>
      const tauri = window.__TAURI__ as { core: { invoke: TauriInvoke } }
      const originalInvoke: TauriInvoke = tauri.core.invoke.bind(tauri.core)
      tauri.core.invoke = function (cmd: string, args?: unknown) {
        if (cmd === 'send_message') {
          if (!noAutoReply) {
            const content = (args as { content?: string } | undefined)?.content ?? ''
            // Fire assistant-complete on the next tick (echo mock)
            void Promise.resolve().then(() => {
              for (const cb of listeners.get('assistant-complete') ?? []) {
                cb({
                  payload: {
                    role: 'assistant',
                    content: `[echo] ${content}`,
                    timestamp: Date.now(),
                  },
                })
              }
            })
          }
          return Promise.resolve(null)
        }
        if (cmd === 'get_transcript') {
          return Promise.resolve({ summary: null, turns: [] })
        }
        if (cmd === 'load_transcript') {
          return Promise.resolve(null)
        }
        return originalInvoke(cmd, args)
      }
    },
    { setupComplete, hasAutoSave, diagnostics, semanticTokens, completionItems, noAutoReply },
  )
}

// ---------------------------------------------------------------------------
// Custom test fixture — wraps base test with ready-app helpers
// ---------------------------------------------------------------------------

export interface AppFixtures {
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
          window.__tauriEmit(event, payload)
        },
        { event, payload },
      )
    })
  },
})

export { expect } from '@playwright/test'
