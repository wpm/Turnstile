// Ambient declarations for globals injected by the Tauri mock in tests.
interface Window {
  __tauriEmit: (event: string, payload: unknown) => void
  __resolveLsp: () => void
  __TAURI__: unknown
}
