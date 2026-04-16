/**
 * Session state: editor content, prose, proof view, and dirty flag.
 *
 * Owns `sessionDirty` — the single source of truth for "session has
 * unsaved changes." It is set by editor edits, prose-updated events, and
 * word-wrap toggles (via `markDirty()` imported by `layoutState`), and
 * cleared by successful saves, new/open session, and `applySessionLoaded`.
 *
 * Also owns the session commands (new/open/save/save-as/auto-save) and
 * the `session-loaded` + `prose-updated` listener setup, so the whole
 * session lifecycle lives in one module.
 */

import { invoke, listen } from './tauri'
import type { SessionState } from './tauri'
import { renderContent } from '../assistant/renderContent'
import { getTheoremTitle, titleToFilename } from './theoremName'
import { showError } from './errorNotification.svelte'
import { setAssistantWidthPct, setGoalPanelPct, setWordWrap } from './layoutState.svelte'
import { layoutState } from './layoutState.svelte'

let editorContent = $state('')
let proseText = $state('')
let proseHash = $state<string | null>(null)
let sessionDirty = $state(false)
let proofView = $state<'formal' | 'prose'>('formal')
let proseGenerating = $state(false)

export const sessionState = {
  get editorContent(): string {
    return editorContent
  },
  get proseText(): string {
    return proseText
  },
  get proseHash(): string | null {
    return proseHash
  },
  get sessionDirty(): boolean {
    return sessionDirty
  },
  get proofView(): 'formal' | 'prose' {
    return proofView
  },
  get proseGenerating(): boolean {
    return proseGenerating
  },
  get theoremTitle(): string {
    return getTheoremTitle(proseText, editorContent)
  },
  get renderedProseHtml(): string {
    return renderContent(proseText)
  },
}

// ── Simple setters ────────────────────────────────────────────────────

export function setProofView(view: 'formal' | 'prose'): void {
  proofView = view
}

export function setProseGenerating(flag: boolean): void {
  proseGenerating = flag
}

export function markDirty(): void {
  sessionDirty = true
}

/**
 * Record an editor edit: update local state, mark dirty, and push the
 * content to the LSP (ignoring errors during startup).
 */
export function setEditorContent(content: string): void {
  editorContent = content
  sessionDirty = true
  invoke('update_document', { content }).catch(() => {
    /* LSP not yet connected */
  })
}

// ── session-loaded fan-out ────────────────────────────────────────────

export interface SessionSetupDeps {
  /** Bridge to `editorRef.setContent(...)` from App.svelte. */
  setEditorText: (content: string) => void
}

let deps: SessionSetupDeps | null = null

/**
 * Apply a session payload to the reactive stores, restoring editor
 * content, prose, and layout fields. Clears the dirty flag.
 */
function applySessionLoaded(session: SessionState): void {
  deps?.setEditorText(session.proof_lean)
  proseText = session.prose.text
  proseHash = session.prose.tactic_state_hash
  setAssistantWidthPct(session.meta.assistant_width_pct || 25)
  setGoalPanelPct(session.meta.goal_panel_pct ?? 30)
  proofView = session.meta.proof_view === 'prose' ? 'prose' : 'formal'
  setWordWrap(session.meta.word_wrap ?? false)
  sessionDirty = false
}

// ── Meta builder ──────────────────────────────────────────────────────

interface SessionMetaShape {
  format_version: number
  created_at: string
  saved_at: string
  cursor_line: number
  cursor_col: number
  editor_scroll_top: number
  assistant_width_pct: number
  proof_view: string
  goal_panel_pct: number
  word_wrap: boolean
}

function buildSessionMeta(): SessionMetaShape {
  return {
    format_version: 1,
    created_at: '',
    saved_at: '',
    cursor_line: 0,
    cursor_col: 0,
    editor_scroll_top: 0,
    assistant_width_pct: layoutState.assistantWidthPct,
    proof_view: proofView,
    goal_panel_pct: layoutState.goalPanelPct,
    word_wrap: layoutState.wordWrap,
  }
}

// ── Session commands ──────────────────────────────────────────────────

/** Generate prose if it doesn't exist yet (called before saving). */
async function ensureProse(): Promise<void> {
  if (proseText || !editorContent.trim()) return
  proseGenerating = true
  try {
    await invoke('generate_prose')
  } catch (err) {
    const msg = err instanceof Error ? err.message : String(err)
    showError(`Prose generation failed: ${msg}`)
  } finally {
    proseGenerating = false
  }
}

export async function newSession(): Promise<void> {
  await invoke('new_session')
  sessionDirty = false
}

export async function openSession(path: string | null = null): Promise<void> {
  await invoke('open_session', { path })
  sessionDirty = false
}

export async function saveSession(): Promise<void> {
  try {
    await ensureProse()
    const title = getTheoremTitle(proseText, editorContent)
    const suggestedName = title !== 'New Theorem' ? titleToFilename(title) : null
    await invoke('save_session', {
      proofLean: editorContent,
      proseText,
      proseHash,
      meta: buildSessionMeta(),
      suggestedName,
    })
    sessionDirty = false
  } catch (err) {
    const msg = err instanceof Error ? err.message : String(err)
    showError(`Save failed: ${msg}`)
  }
}

export async function saveSessionAs(): Promise<void> {
  try {
    await ensureProse()
    const title = getTheoremTitle(proseText, editorContent)
    const suggestedName = title !== 'New Theorem' ? titleToFilename(title) : null
    await invoke('save_session_as', {
      proofLean: editorContent,
      proseText,
      proseHash,
      meta: buildSessionMeta(),
      suggestedName,
    })
    sessionDirty = false
  } catch (err) {
    const msg = err instanceof Error ? err.message : String(err)
    showError(`Save As failed: ${msg}`)
  }
}

export async function autoSave(): Promise<void> {
  if (!sessionDirty) return
  try {
    await invoke('auto_save_session', {
      proofLean: editorContent,
      proseText,
      proseHash,
      meta: buildSessionMeta(),
    })
  } catch (err) {
    const msg = err instanceof Error ? err.message : String(err)
    showError(`Auto-save failed: ${msg}`)
  }
}

/**
 * Reopen the last-saved session, if any. Silently ignores missing files
 * (the user's last session may have been moved or deleted).
 */
export async function reopenLastSession(): Promise<void> {
  const lastPath = await invoke<string | null>('get_last_session').catch(() => null)
  if (lastPath) {
    await invoke('open_session', { path: lastPath }).catch(() => {
      /* silently ignore */
    })
  }
}

// ── Listener setup ────────────────────────────────────────────────────

/**
 * Register the prose-updated and session-loaded listeners. The editor
 * bridge is captured so `applySessionLoaded` can push content into the
 * CodeMirror instance owned by App.svelte.
 */
export async function setupSessionListeners(bridgeDeps: SessionSetupDeps): Promise<() => void> {
  deps = bridgeDeps
  const [unlistenProse, unlistenSession] = await Promise.all([
    listen<{ text: string; hash: string | null }>('prose-updated', (data) => {
      proseText = data.text
      proseHash = data.hash
      sessionDirty = true
    }),
    listen<SessionState>('session-loaded', (session) => {
      applySessionLoaded(session)
    }),
  ])

  return () => {
    unlistenProse()
    unlistenSession()
    deps = null
  }
}
