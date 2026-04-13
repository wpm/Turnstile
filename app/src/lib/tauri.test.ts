import { describe, it, expect, beforeEach, vi } from 'vitest'
import { invoke, listen } from './tauri'

// Shared mock for window.__TAURI__ — reusable across test files that need it.
function mockTauri(): {
  core: { invoke: ReturnType<typeof vi.fn> }
  event: { listen: ReturnType<typeof vi.fn> }
} {
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

describe('invoke', () => {
  let tauri: ReturnType<typeof mockTauri>

  beforeEach(() => {
    tauri = mockTauri()
  })

  it('delegates to window.__TAURI__.core.invoke with cmd and args', async () => {
    tauri.core.invoke.mockResolvedValue('ok')
    await invoke('my_cmd', { key: 'value' })
    expect(tauri.core.invoke).toHaveBeenCalledWith('my_cmd', { key: 'value' })
  })

  it('returns the promise from the underlying call', async () => {
    tauri.core.invoke.mockResolvedValue(42)
    const result = await invoke<number>('get_count')
    expect(result).toBe(42)
  })
})

describe('listen', () => {
  let tauri: ReturnType<typeof mockTauri>

  beforeEach(() => {
    tauri = mockTauri()
  })

  it('delegates to window.__TAURI__.event.listen', async () => {
    const unlisten = vi.fn()
    tauri.event.listen.mockResolvedValue(unlisten)
    const cb = vi.fn()
    await listen('my-event', cb)
    expect(tauri.event.listen).toHaveBeenCalledWith('my-event', expect.any(Function))
  })

  it('unwraps e.payload before passing to callback', async () => {
    tauri.event.listen.mockResolvedValue(vi.fn())

    const cb = vi.fn()
    await listen<string>('my-event', cb)

    // Retrieve the internal handler that listen() registered and invoke it
    // eslint-disable-next-line @typescript-eslint/no-non-null-assertion
    const internalHandler = tauri.event.listen.mock.calls[0]![1] as (e: {
      payload: unknown
    }) => void
    internalHandler({ payload: 'hello' })
    expect(cb).toHaveBeenCalledWith('hello')
  })
})
