/**
 * Lightweight error notification state for surfacing user-facing errors.
 *
 * Exposes a single reactive error message string (or null when no error is
 * active). Errors persist until explicitly dismissed by the user.
 */

let errorMessage = $state<string | null>(null)

/** Reactive read-only accessor for the current error message. */
export const errorNotification = {
  get message(): string | null {
    return errorMessage
  },
}

/** Show an error message. Persists until dismissed. */
export function showError(message: string): void {
  errorMessage = message
}

/** Immediately dismiss the current error. */
export function dismissError(): void {
  errorMessage = null
}
