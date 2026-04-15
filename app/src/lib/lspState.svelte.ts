/**
 * LSP event state: diagnostics, semantic tokens, file progress, goal state,
 * and symbol-outline results.
 *
 * Exposes a reactive singleton (`lspState`) whose fields are populated by
 * Tauri event listeners registered via `setupLspListeners`. Listener setup
 * is a single awaited Promise.all so the caller can enforce
 * register-before-start_lsp ordering.
 */

import { listen } from './tauri'
import type { DiagnosticInfo, FileProgressRange, SemanticToken } from './tauri'
import type { DocumentSymbolInfo } from './lspRequests'

let diagnostics = $state<DiagnosticInfo[] | null>(null)
let semanticTokens = $state<SemanticToken[] | null>(null)
let fileProgress = $state<FileProgressRange[] | null>(null)
let goalText = $state('')
let goalLineToProofLine = $state<(number | null)[]>([])
let outlineSymbols = $state<DocumentSymbolInfo[]>([])

export const lspState = {
  get diagnostics(): DiagnosticInfo[] | null {
    return diagnostics
  },
  get semanticTokens(): SemanticToken[] | null {
    return semanticTokens
  },
  get fileProgress(): FileProgressRange[] | null {
    return fileProgress
  },
  get goalText(): string {
    return goalText
  },
  get goalLineToProofLine(): (number | null)[] {
    return goalLineToProofLine
  },
  get outlineSymbols(): DocumentSymbolInfo[] {
    return outlineSymbols
  },
}

export function setOutlineSymbols(symbols: DocumentSymbolInfo[]): void {
  outlineSymbols = symbols
}

/**
 * Register all LSP-event listeners (diagnostics, semantic tokens, file
 * progress, goal state). Returns a single unlisten closure that detaches
 * all four.
 *
 * Callers must await this (and any other listener-setup calls) BEFORE
 * invoking `start_lsp`, so events emitted immediately after start are not
 * missed.
 */
export async function setupLspListeners(): Promise<() => void> {
  const [unlistenDiag, unlistenTokens, unlistenProgress, unlistenGoalState] = await Promise.all([
    listen<DiagnosticInfo[]>('lsp-diagnostics', (diags) => {
      diagnostics = diags
    }),
    listen<SemanticToken[]>('lsp-semantic-tokens', (tokens) => {
      semanticTokens = tokens
    }),
    listen<FileProgressRange[]>('lsp-file-progress', (ranges) => {
      fileProgress = ranges
    }),
    listen<{ full: string; panel_line_to_source_line: (number | null)[] }>(
      'goal-state-updated',
      ({ full, panel_line_to_source_line }) => {
        goalText = full
        goalLineToProofLine = panel_line_to_source_line
      },
    ),
  ])

  return () => {
    unlistenDiag()
    unlistenTokens()
    unlistenProgress()
    unlistenGoalState()
  }
}
