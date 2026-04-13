import { describe, it, expect, beforeEach, vi } from 'vitest'
import { syncSaveMenuState } from './saveIndicator'

function mockTauri(): { core: { invoke: ReturnType<typeof vi.fn> } } {
  const mock = {
    core: { invoke: vi.fn().mockResolvedValue(undefined) },
    event: { listen: vi.fn() },
  }
  Object.defineProperty(globalThis, 'window', {
    value: { __TAURI__: mock },
    writable: true,
    configurable: true,
  })
  return mock
}

describe('syncSaveMenuState', () => {
  let tauri: ReturnType<typeof mockTauri>

  beforeEach(() => {
    tauri = mockTauri()
  })

  it('enables Save menu item when dirty', async () => {
    await syncSaveMenuState(true)
    expect(tauri.core.invoke).toHaveBeenCalledWith('set_menu_item_enabled', {
      id: 'save_session',
      enabled: true,
    })
  })

  it('disables Save menu item when clean', async () => {
    await syncSaveMenuState(false)
    expect(tauri.core.invoke).toHaveBeenCalledWith('set_menu_item_enabled', {
      id: 'save_session',
      enabled: false,
    })
  })
})
