// Shim for `@tauri-apps/api/core` in fake-backend (e2e) mode.
import { fakeInvoke } from "./backend";

export function invoke<T>(
  cmd: string,
  args?: Record<string, unknown>,
): Promise<T> {
  return fakeInvoke(cmd, args) as Promise<T>;
}

// The browser-driven e2e harness is not a Tauri runtime, so Tauri-only paths
// (e.g. the auto-updater flow, #53) must opt out. Returning false here keeps
// them from running — and from pulling in the native updater/process plugins —
// under Playwright.
export function isTauri(): boolean {
  return false;
}

// The updater/dialog plugins import `Channel` and `Resource` from core. With
// `@tauri-apps/api/core` aliased to this shim in e2e mode, those named exports
// must exist for the bundle to link, even though the code that uses them never
// runs (isTauri() is false). Inert stubs are enough.
export class Channel {
  onmessage: ((message: unknown) => void) | null = null;
}

export class Resource {
  get rid(): number {
    return 0;
  }
  close(): Promise<void> {
    return Promise.resolve();
  }
}
