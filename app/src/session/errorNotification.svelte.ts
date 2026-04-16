/**
 * Lightweight error notification state for surfacing user-facing errors.
 *
 * Maintains a reactive stack of error messages. Errors persist until
 * explicitly dismissed by the user. Duplicate messages are ignored.
 */

const errors = $state<string[]>([])

/** Reactive read-only accessor for the current error messages. */
export const errorNotification = {
  get messages(): readonly string[] {
    return errors
  },
}

/** Show an error message. Duplicates are ignored. */
export function showError(message: string): void {
  if (!errors.includes(message)) {
    errors.push(message)
  }
}

/** Dismiss the error at the given index. */
export function dismissError(index: number): void {
  errors.splice(index, 1)
}

/** Dismiss all errors. */
export function dismissAllErrors(): void {
  errors.length = 0
}
