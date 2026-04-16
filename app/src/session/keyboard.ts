/**
 * Global keyboard shortcuts for session + settings + symbol outline.
 *
 * Installs a single `window.keydown` handler that routes Cmd/Ctrl-prefixed
 * chords to the provided handlers and returns a cleanup closure.
 */

interface ShortcutHandlers {
  newSession: () => void
  openSession: () => void
  saveSession: () => void
  saveSessionAs: () => void
  openSettings: () => void
  openSymbolOutline: () => void
}

export function installKeyboardShortcuts(handlers: ShortcutHandlers): () => void {
  const handleKeydown = (e: KeyboardEvent): void => {
    const meta = e.metaKey || e.ctrlKey
    if (!meta) return

    // Cmd/Ctrl+Shift+O — symbol outline command palette
    if ((e.key === 'o' || e.key === 'O') && e.shiftKey) {
      e.preventDefault()
      handlers.openSymbolOutline()
      return
    }

    if (e.key === ',') {
      e.preventDefault()
      handlers.openSettings()
    } else if (e.key === 'n' || e.key === 'N') {
      e.preventDefault()
      handlers.newSession()
    } else if (e.key === 'o' || e.key === 'O') {
      e.preventDefault()
      handlers.openSession()
    } else if ((e.key === 's' || e.key === 'S') && e.shiftKey) {
      e.preventDefault()
      handlers.saveSessionAs()
    } else if (e.key === 's' || e.key === 'S') {
      e.preventDefault()
      handlers.saveSession()
    }
  }

  window.addEventListener('keydown', handleKeydown)
  return () => {
    window.removeEventListener('keydown', handleKeydown)
  }
}
