import { describe, it, expect, vi } from 'vitest'
import { handleMenuEvent, MENU_IDS, type MenuActions } from './menu'

function makeActions(): MenuActions & Record<string, ReturnType<typeof vi.fn>> {
  return {
    newSession: vi.fn(),
    openSession: vi.fn(),
    saveSession: vi.fn(),
    saveSessionAs: vi.fn(),
    openSettings: vi.fn(),
    toggleWordWrap: vi.fn(),
  }
}

describe('handleMenuEvent', () => {
  it.each([
    ['new_session', 'newSession'],
    ['open_session', 'openSession'],
    ['save_session', 'saveSession'],
    ['save_session_as', 'saveSessionAs'],
    ['settings', 'openSettings'],
    ['toggle_word_wrap', 'toggleWordWrap'],
  ] as const)('dispatches "%s" to %s', (id, actionName) => {
    const actions = makeActions()
    const handled = handleMenuEvent(id, actions)

    expect(handled).toBe(true)
    expect(actions[actionName]).toHaveBeenCalledOnce()
  })

  it('returns false for unknown IDs', () => {
    const actions = makeActions()
    const handled = handleMenuEvent('unknown_id', actions)

    expect(handled).toBe(false)
    for (const fn of Object.values(actions)) {
      expect(fn).not.toHaveBeenCalled()
    }
  })
})

describe('MENU_IDS', () => {
  it('contains all required menu item IDs', () => {
    expect(MENU_IDS.NEW_SESSION).toBe('new_session')
    expect(MENU_IDS.OPEN_SESSION).toBe('open_session')
    expect(MENU_IDS.SAVE_SESSION).toBe('save_session')
    expect(MENU_IDS.SAVE_SESSION_AS).toBe('save_session_as')
    expect(MENU_IDS.SETTINGS).toBe('settings')
    expect(MENU_IDS.TOGGLE_WORD_WRAP).toBe('toggle_word_wrap')
  })

  it('has exactly 6 entries', () => {
    expect(Object.keys(MENU_IDS)).toHaveLength(6)
  })
})
