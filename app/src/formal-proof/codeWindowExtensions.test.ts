import { describe, it, expect } from 'vitest'
import { EditorState } from '@codemirror/state'
import { EditorView } from '@codemirror/view'
import type { SemanticToken } from '../session/tauri'
import {
  semanticTokensField,
  setSemanticTokensEffect,
  themeExtension,
  baseTheme,
} from './codeWindowExtensions'

function makeView(doc: string): EditorView {
  const parent = document.createElement('div')
  return new EditorView({
    state: EditorState.create({
      doc,
      extensions: [semanticTokensField, baseTheme, themeExtension('dark')],
    }),
    parent,
  })
}

describe('semanticTokensField', () => {
  it('starts with no decorations', () => {
    const view = makeView('theorem t := by\nrfl')
    const decos = view.state.field(semanticTokensField)
    expect(decos.size).toBe(0)
    view.destroy()
  })

  it('applies decorations when setSemanticTokensEffect is dispatched', () => {
    const view = makeView('theorem t := by\nrfl')
    const tokens: SemanticToken[] = [
      { line: 1, col: 0, length: 7, token_type: 'keyword', token_modifiers: [] },
    ]
    view.dispatch({ effects: setSemanticTokensEffect.of(tokens) })

    // After dispatch, the field holds at least one mark decoration.
    const decos = view.state.field(semanticTokensField)
    expect(decos.size).toBeGreaterThan(0)
    view.destroy()
  })

  it('replaces the decoration set on each dispatch', () => {
    const view = makeView('theorem t := by\nrfl')
    view.dispatch({
      effects: setSemanticTokensEffect.of([
        { line: 1, col: 0, length: 7, token_type: 'keyword', token_modifiers: [] },
      ]),
    })
    // Second dispatch with no tokens clears the field.
    view.dispatch({ effects: setSemanticTokensEffect.of([]) })
    const decos = view.state.field(semanticTokensField)
    expect(decos.size).toBe(0)
    view.destroy()
  })

  it('ignores out-of-range tokens', () => {
    const view = makeView('short')
    view.dispatch({
      effects: setSemanticTokensEffect.of([
        { line: 99, col: 0, length: 3, token_type: 'keyword', token_modifiers: [] },
      ]),
    })
    const decos = view.state.field(semanticTokensField)
    expect(decos.size).toBe(0)
    view.destroy()
  })
})

describe('themeExtension', () => {
  it('returns a dark extension for "dark"', () => {
    const ext = themeExtension('dark')
    expect(ext).toBeDefined()
  })

  it('returns a light extension for "light"', () => {
    const ext = themeExtension('light')
    expect(ext).toBeDefined()
  })

  it('produces distinct extensions for dark and light', () => {
    expect(themeExtension('dark')).not.toBe(themeExtension('light'))
  })
})
