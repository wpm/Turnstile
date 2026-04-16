/**
 * Theme type shared across UI modules.
 *
 * Turnstile follows the OS light/dark preference automatically; there is no
 * in-app toggle and no persisted theme setting. This module exists so CM6
 * extensions and other consumers can type their `theme` prop without pulling
 * in the (deleted) preference store.
 */

/** The concrete theme currently applied to the UI. */
export type ResolvedTheme = 'dark' | 'light'
