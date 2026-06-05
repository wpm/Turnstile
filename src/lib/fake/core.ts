// Shim for `@tauri-apps/api/core` in fake-backend (e2e) mode.
import { fakeInvoke } from "./backend";

export function invoke<T>(
  cmd: string,
  args?: Record<string, unknown>,
): Promise<T> {
  return fakeInvoke(cmd, args) as Promise<T>;
}
