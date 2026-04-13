import { describe, it, expect, beforeEach } from 'vitest'
import { errorNotification, showError, dismissError } from './errorNotification.svelte'

describe('errorNotification', () => {
  beforeEach(() => {
    dismissError()
  })

  it('starts with null message', () => {
    expect(errorNotification.message).toBeNull()
  })

  it('shows an error message', () => {
    showError('Something went wrong')
    expect(errorNotification.message).toBe('Something went wrong')
  })

  it('replaces a previous error', () => {
    showError('First error')
    showError('Second error')
    expect(errorNotification.message).toBe('Second error')
  })

  it('dismissError clears the message immediately', () => {
    showError('Error to dismiss')
    dismissError()
    expect(errorNotification.message).toBeNull()
  })
})
