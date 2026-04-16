import { describe, it, expect, beforeEach } from 'vitest'
import {
  errorNotification,
  showError,
  dismissError,
  dismissAllErrors,
} from './errorNotification.svelte'

describe('errorNotification', () => {
  beforeEach(() => {
    dismissAllErrors()
  })

  it('starts with no messages', () => {
    expect(errorNotification.messages).toEqual([])
  })

  it('shows an error message', () => {
    showError('Something went wrong')
    expect(errorNotification.messages).toEqual(['Something went wrong'])
  })

  it('stacks multiple errors', () => {
    showError('First error')
    showError('Second error')
    expect(errorNotification.messages).toEqual(['First error', 'Second error'])
  })

  it('ignores duplicate messages', () => {
    showError('Same error')
    showError('Same error')
    expect(errorNotification.messages).toEqual(['Same error'])
  })

  it('dismisses an error by index', () => {
    showError('First error')
    showError('Second error')
    showError('Third error')
    dismissError(1)
    expect(errorNotification.messages).toEqual(['First error', 'Third error'])
  })

  it('dismissAllErrors clears all messages', () => {
    showError('First error')
    showError('Second error')
    dismissAllErrors()
    expect(errorNotification.messages).toEqual([])
  })
})
