// Types matching the Rust structs serialized by the Tauri backend.

export interface SetupProgressPayload {
  phase: string // "checking" | "installing-elan" | "creating-project" | "fetching-mathlib" | "downloading-cache" | "ready" | "error"
  message: string
  progress_pct: number
}

export interface DiagnosticInfo {
  start_line: number // 1-indexed (backend converts from 0-indexed LSP)
  start_col: number
  end_line: number
  end_col: number
  severity: number // 1=error, 2=warning, 3=info, 4=hint
  message: string
}

export interface SemanticToken {
  line: number // 1-indexed (matches backend convention)
  col: number
  length: number
  token_type: string
  token_modifiers: string[]
}

export interface CompletionItem {
  label: string
  detail: string | null
  insert_text: string | null
}

export interface FileProgressRange {
  start_line: number // 1-indexed
  end_line: number // 1-indexed
}

export interface AssistantTurn {
  role: 'user' | 'assistant'
  content: string
  timestamp: number
}

export interface SessionMeta {
  format_version: number
  created_at: string
  saved_at: string
  cursor_line: number
  cursor_col: number
  editor_scroll_top: number
  assistant_width_pct: number
  proof_view?: string
  goal_panel_pct?: number
  word_wrap?: boolean
}

export interface SessionState {
  meta: SessionMeta
  proof_lean: string
  prose: { text: string; tactic_state_hash: string | null }
  turns: AssistantTurn[]
  summary: string | null
}

// window.__TAURI__ is injected by Tauri when withGlobalTauri: true
declare global {
  interface Window {
    __TAURI__: {
      core: { invoke: (cmd: string, args?: unknown) => Promise<unknown> }
      event: {
        listen: (event: string, cb: (e: { payload: unknown }) => void) => Promise<() => void>
      }
    }
  }
}

export function invoke<T>(cmd: string, args?: unknown): Promise<T> {
  return window.__TAURI__.core.invoke(cmd, args) as Promise<T>
}

// eslint-disable-next-line @typescript-eslint/no-unnecessary-type-parameters -- T narrows the untyped Tauri global
export function listen<T>(event: string, cb: (payload: T) => void): Promise<() => void> {
  return window.__TAURI__.event.listen(event, (e) => {
    cb(e.payload as T)
  })
}
