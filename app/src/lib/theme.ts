import { writable } from 'svelte/store'

export type Theme = 'mocha' | 'latte'

export const theme = writable<Theme>('mocha')

export function toggleTheme(current: Theme): Theme {
  return current === 'mocha' ? 'latte' : 'mocha'
}
