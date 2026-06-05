// Shim for `@tauri-apps/api/event` in fake-backend (e2e) mode.
import { fakeListen } from "./backend";

export interface Event<T> {
  payload: T;
}

export function listen<T>(
  event: string,
  cb: (event: Event<T>) => void,
): Promise<() => void> {
  return Promise.resolve(
    fakeListen(event, cb as (event: { payload: unknown }) => void),
  );
}
