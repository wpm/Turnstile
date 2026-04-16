import { render } from 'vitest-browser-svelte'
import { describe, it, expect, vi } from 'vitest'
import { userEvent } from 'vitest/browser'
import SymbolOutline from './SymbolOutline.svelte'
import type { DocumentSymbolInfo } from './lspRequests'

const MOCK_SYMBOLS: DocumentSymbolInfo[] = [
  {
    name: 'my_theorem',
    kind: 12, // function
    start_line: 0,
    start_character: 8,
    end_line: 0,
    end_character: 18,
    children: [
      {
        name: 'inner_lemma',
        kind: 12,
        start_line: 1,
        start_character: 10,
        end_line: 1,
        end_character: 22,
        children: [],
      },
    ],
  },
  {
    name: 'helper_lemma',
    kind: 12,
    start_line: 3,
    start_character: 8,
    end_line: 3,
    end_character: 20,
    children: [],
  },
]

interface Props {
  symbols: DocumentSymbolInfo[]
  onJump: (line: number, character: number) => void
  onClose: () => void
}

function renderOutline(overrides: Partial<Props> = {}): ReturnType<typeof render> {
  return render(SymbolOutline, {
    symbols: MOCK_SYMBOLS,
    onJump: vi.fn(),
    onClose: vi.fn(),
    ...overrides,
  })
}

describe('SymbolOutline', () => {
  describe('rendering', () => {
    it('renders as a dialog with search input focused', async () => {
      const screen = renderOutline()
      await expect.element(screen.getByRole('dialog')).toBeVisible()
      const input = screen.getByTestId('symbol-outline-input')
      await expect.element(input).toBeVisible()
    })

    it('displays all flattened symbols', () => {
      const screen = renderOutline()
      // MOCK_SYMBOLS flattens to 3: my_theorem, inner_lemma, helper_lemma
      const items = screen.getByTestId('symbol-outline-item')
      expect(items.elements().length).toBe(3)
    })

    it('displays symbol names and kind tags', async () => {
      const screen = renderOutline()
      const first = screen.getByTestId('symbol-outline-item').nth(0)
      await expect.element(first).toHaveAttribute('data-symbol-name', 'my_theorem')
      await expect.element(first).toHaveTextContent(/function/)
    })

    it('shows "No matching symbols" when symbols array is empty', async () => {
      const screen = renderOutline({ symbols: [] })
      await expect.element(screen.getByText('No matching symbols')).toBeVisible()
    })
  })

  describe('fuzzy filtering', () => {
    it('filters symbols as user types in the search input', async () => {
      const screen = renderOutline()
      await userEvent.fill(screen.getByTestId('symbol-outline-input').element(), 'help')
      // Only helper_lemma should match
      const items = screen.getByTestId('symbol-outline-item')
      expect(items.elements().length).toBe(1)
      await expect.element(items.first()).toHaveAttribute('data-symbol-name', 'helper_lemma')
    })

    it('shows "No matching symbols" when filter matches nothing', async () => {
      const screen = renderOutline()
      await userEvent.fill(screen.getByTestId('symbol-outline-input').element(), 'zzzzz')
      await expect.element(screen.getByText('No matching symbols')).toBeVisible()
      expect(screen.getByTestId('symbol-outline-item').query()).toBeNull()
    })
  })

  describe('keyboard navigation', () => {
    it('ArrowDown moves selection down through results', async () => {
      const screen = renderOutline()
      await userEvent.keyboard('{ArrowDown}')
      // Selection should move to index 1 (inner_lemma)
      const second = screen.getByTestId('symbol-outline-item').nth(1)
      // The selected item gets bg-accent class
      expect(second.element().classList.contains('bg-accent')).toBe(true)
    })

    it('ArrowUp wraps from first to last', async () => {
      const screen = renderOutline()
      // Selection starts at 0; ArrowUp should wrap to last (index 2)
      await userEvent.keyboard('{ArrowUp}')
      const last = screen.getByTestId('symbol-outline-item').nth(2)
      expect(last.element().classList.contains('bg-accent')).toBe(true)
    })

    it('ArrowDown wraps from last to first', async () => {
      const screen = renderOutline()
      // Move to last (press down 2 times from 0 -> 1 -> 2), then one more wraps to 0
      await userEvent.keyboard('{ArrowDown}')
      await userEvent.keyboard('{ArrowDown}')
      await userEvent.keyboard('{ArrowDown}')
      const first = screen.getByTestId('symbol-outline-item').nth(0)
      expect(first.element().classList.contains('bg-accent')).toBe(true)
    })

    it('Enter jumps to the selected symbol and closes', async () => {
      const onJump = vi.fn()
      const onClose = vi.fn()
      renderOutline({ onJump, onClose })

      // Move to helper_lemma (index 2)
      await userEvent.keyboard('{ArrowDown}')
      await userEvent.keyboard('{ArrowDown}')
      await userEvent.keyboard('{Enter}')

      expect(onJump).toHaveBeenCalledWith(3, 8)
      expect(onClose).toHaveBeenCalledOnce()
    })

    it('Enter on a filtered result jumps to the correct symbol', async () => {
      const onJump = vi.fn()
      const onClose = vi.fn()
      const screen = renderOutline({ onJump, onClose })

      await userEvent.fill(screen.getByTestId('symbol-outline-input').element(), 'inner')
      await userEvent.keyboard('{Enter}')

      expect(onJump).toHaveBeenCalledWith(1, 10)
      expect(onClose).toHaveBeenCalledOnce()
    })
  })

  describe('dismissal', () => {
    it('Escape calls onClose without jumping', async () => {
      const onJump = vi.fn()
      const onClose = vi.fn()
      renderOutline({ onJump, onClose })

      await userEvent.keyboard('{Escape}')
      expect(onClose).toHaveBeenCalledOnce()
      expect(onJump).not.toHaveBeenCalled()
    })

    it('clicking an item jumps and closes', async () => {
      const onJump = vi.fn()
      const onClose = vi.fn()
      const screen = renderOutline({ onJump, onClose })

      const third = screen.getByTestId('symbol-outline-item').nth(2)
      await third.click()

      expect(onJump).toHaveBeenCalledWith(3, 8)
      expect(onClose).toHaveBeenCalledOnce()
    })
  })

  describe('mouse interaction', () => {
    it('hovering an item highlights it', async () => {
      const screen = renderOutline()
      const second = screen.getByTestId('symbol-outline-item').nth(1)
      await second.hover()
      expect(second.element().classList.contains('bg-accent')).toBe(true)
    })
  })
})
