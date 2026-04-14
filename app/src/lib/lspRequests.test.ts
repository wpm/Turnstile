import { describe, it, expect, beforeEach, vi } from 'vitest'
import {
  lspHover,
  lspDefinition,
  lspCodeActions,
  lspResolveCodeAction,
  lspDocumentSymbols,
} from './lspRequests'

function mockTauri(): { core: { invoke: ReturnType<typeof vi.fn> } } {
  const mock = {
    core: { invoke: vi.fn() },
    event: { listen: vi.fn() },
  }
  Object.defineProperty(globalThis, 'window', {
    value: { __TAURI__: mock },
    writable: true,
    configurable: true,
  })
  return mock
}

describe('lspRequests', () => {
  let tauri: ReturnType<typeof mockTauri>

  beforeEach(() => {
    tauri = mockTauri()
  })

  it('lspHover passes 0-indexed line and character', async () => {
    tauri.core.invoke.mockResolvedValue({ contents: 'Nat → Nat' })
    const result = await lspHover(3, 7)
    expect(tauri.core.invoke).toHaveBeenCalledWith('lsp_hover', {
      line: 3,
      character: 7,
    })
    expect(result).toEqual({ contents: 'Nat → Nat' })
  })

  it('lspHover propagates null responses', async () => {
    tauri.core.invoke.mockResolvedValue(null)
    expect(await lspHover(0, 0)).toBeNull()
  })

  it('lspDefinition passes position and returns structured location', async () => {
    const location = {
      uri: 'file:///proof.lean',
      line: 5,
      character: 0,
      end_line: 5,
      end_character: 10,
    }
    tauri.core.invoke.mockResolvedValue(location)
    const result = await lspDefinition(10, 4)
    expect(tauri.core.invoke).toHaveBeenCalledWith('lsp_definition', {
      line: 10,
      character: 4,
    })
    expect(result).toEqual(location)
  })

  it('lspCodeActions passes start and end positions', async () => {
    tauri.core.invoke.mockResolvedValue([])
    await lspCodeActions(2, 0, 2, 5)
    expect(tauri.core.invoke).toHaveBeenCalledWith('lsp_code_actions', {
      startLine: 2,
      startCharacter: 0,
      endLine: 2,
      endCharacter: 5,
    })
  })

  it('lspResolveCodeAction wraps the action payload', async () => {
    tauri.core.invoke.mockResolvedValue(null)
    const action = { title: 'Try this', data: { token: 1 } }
    await lspResolveCodeAction(action)
    expect(tauri.core.invoke).toHaveBeenCalledWith('lsp_resolve_code_action', { action })
  })

  it('lspDocumentSymbols takes no arguments', async () => {
    tauri.core.invoke.mockResolvedValue([])
    await lspDocumentSymbols()
    expect(tauri.core.invoke).toHaveBeenCalledWith('lsp_document_symbols', undefined)
  })
})
