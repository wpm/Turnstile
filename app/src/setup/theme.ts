import { writable } from 'svelte/store'

/** User's stored preference — may be explicit or follow the OS. */
export type ThemePreference = 'dark' | 'light' | 'auto'

/** The concrete theme applied to the UI (never 'auto'). */
export type ResolvedTheme = 'dark' | 'light'

/** The user's persisted theme preference. */
export const theme = writable<ThemePreference>('auto')

/**
 * Tracks the OS-level color scheme via `matchMedia`.
 * Updated by a listener initialised in App.svelte.
 */
export const systemTheme = writable<ResolvedTheme>('dark')

/** Flip a resolved theme to its opposite (for the two-state toggle button). */
export function toggleTheme(current: ResolvedTheme): ResolvedTheme {
  return current === 'dark' ? 'light' : 'dark'
}

/**
 * Turn a preference + the current system theme into a concrete dark/light value.
 */
export function resolveTheme(pref: ThemePreference, system: ResolvedTheme): ResolvedTheme {
  if (pref === 'auto') return system
  return pref
}
