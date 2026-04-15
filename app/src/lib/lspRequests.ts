/**
 * Thin typed wrappers around the LSP Tauri commands.
 *
 * All positions are 0-indexed (LSP convention). The backend owns the current
 * document URI, so callers do not pass it.
 */

import { invoke } from './tauri'

export interface HoverInfo {
  contents: string
  /** Markup kind from LSP — determines rendering (rich markdown vs. preformatted text). */
  kind: 'markdown' | 'plaintext'
}

export interface DefinitionLocation {
  uri: string
  line: number
  character: number
  end_line: number
  end_character: number
}

export interface TextEditDto {
  start_line: number
  start_character: number
  end_line: number
  end_character: number
  new_text: string
}

export interface WorkspaceEditDto {
  /** Flat list of `(uri, edits)` pairs — matches the Rust DTO shape. */
  changes: [string, TextEditDto[]][]
}

export interface CodeActionInfo {
  title: string
  kind: string | null
  edit: WorkspaceEditDto | null
  /** Opaque payload for `codeAction/resolve`. May be any LSP-defined shape. */
  resolve_data: unknown
}

export interface DocumentSymbolInfo {
  name: string
  /** LSP SymbolKind numeric code. */
  kind: number
  start_line: number
  start_character: number
  end_line: number
  end_character: number
  children: DocumentSymbolInfo[]
}

/** Request `textDocument/hover` at the given 0-indexed position. */
export async function lspHover(line: number, character: number): Promise<HoverInfo | null> {
  return invoke<HoverInfo | null>('lsp_hover', { line, character })
}

/** Request `textDocument/definition` at the given 0-indexed position. */
export async function lspDefinition(
  line: number,
  character: number,
): Promise<DefinitionLocation | null> {
  return invoke<DefinitionLocation | null>('lsp_definition', { line, character })
}

/** Request `textDocument/codeAction` over the given 0-indexed range. */
export async function lspCodeActions(
  startLine: number,
  startCharacter: number,
  endLine: number,
  endCharacter: number,
): Promise<CodeActionInfo[]> {
  return invoke<CodeActionInfo[]>('lsp_code_actions', {
    startLine,
    startCharacter,
    endLine,
    endCharacter,
  })
}

/** Resolve a lazily-resolved code action via `codeAction/resolve`. */
export async function lspResolveCodeAction(action: unknown): Promise<WorkspaceEditDto | null> {
  return invoke<WorkspaceEditDto | null>('lsp_resolve_code_action', { action })
}

/** Request `textDocument/documentSymbol` for the current file. */
export async function lspDocumentSymbols(): Promise<DocumentSymbolInfo[]> {
  return invoke<DocumentSymbolInfo[]>('lsp_document_symbols')
}
