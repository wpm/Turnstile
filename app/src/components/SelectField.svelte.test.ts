import { render } from 'vitest-browser-svelte'
import { describe, it, expect, vi } from 'vitest'
import { userEvent } from 'vitest/browser'
import SelectField from './SelectField.svelte'

const TEST_OPTIONS = [
  { value: 'alpha', label: 'Alpha' },
  { value: 'beta', label: 'Beta' },
  { value: 'gamma', label: 'Gamma' },
]

interface Props {
  id: string
  value: string | number
  options: { value: string | number; label: string }[]
  onchange: (value: string | number) => void
  'data-testid'?: string
}

function renderSelect(overrides: Partial<Props> = {}): ReturnType<typeof render> {
  return render(SelectField, {
    id: 'test-select',
    value: 'alpha',
    options: TEST_OPTIONS,
    onchange: vi.fn(),
    ...overrides,
  } as Props)
}

describe('SelectField', () => {
  describe('rendering and ARIA', () => {
    it('renders a combobox trigger showing the selected label', async () => {
      const screen = renderSelect({ value: 'beta' })
      const trigger = screen.getByRole('combobox')
      await expect.element(trigger).toBeVisible()
      await expect.element(trigger).toHaveTextContent('Beta')
      await expect.element(trigger).toHaveAttribute('aria-expanded', 'false')
      await expect.element(trigger).toHaveAttribute('aria-haspopup', 'listbox')
    })

    it('shows the raw value when no option matches', async () => {
      const screen = renderSelect({ value: 'unknown' })
      await expect.element(screen.getByRole('combobox')).toHaveTextContent('unknown')
    })

    it('does not render the listbox when closed', () => {
      const screen = renderSelect()
      expect(screen.getByRole('listbox').query()).toBeNull()
    })
  })

  describe('mouse interaction', () => {
    it('opens the dropdown on click and shows all options', async () => {
      const screen = renderSelect()
      await screen.getByRole('combobox').click()
      await expect.element(screen.getByRole('listbox')).toBeVisible()
      const options = screen.getByRole('option')
      expect(options.elements().length).toBe(3)
      await expect.element(screen.getByRole('combobox')).toHaveAttribute('aria-expanded', 'true')
    })

    it('selects an option on click and closes the dropdown', async () => {
      const onchange = vi.fn()
      const screen = renderSelect({ onchange })
      await screen.getByRole('combobox').click()
      await screen.getByRole('option', { name: 'Gamma' }).click()
      expect(onchange).toHaveBeenCalledWith('gamma')
      expect(screen.getByRole('listbox').query()).toBeNull()
    })

    it('toggles closed when trigger is clicked while open', async () => {
      const screen = renderSelect()
      const trigger = screen.getByRole('combobox')
      await trigger.click()
      await expect.element(screen.getByRole('listbox')).toBeVisible()
      await trigger.click()
      expect(screen.getByRole('listbox').query()).toBeNull()
    })
  })

  describe('keyboard navigation from trigger', () => {
    it('ArrowDown opens the dropdown', async () => {
      const screen = renderSelect()
      const trigger = screen.getByRole('combobox')
      await trigger.click() // focus
      // Close it so we can test ArrowDown opening
      await userEvent.keyboard('{Escape}')
      expect(screen.getByRole('listbox').query()).toBeNull()

      await userEvent.keyboard('{ArrowDown}')
      await expect.element(screen.getByRole('listbox')).toBeVisible()
    })

    it('ArrowUp opens the dropdown', async () => {
      const screen = renderSelect()
      screen.getByRole('combobox').element().focus()
      await userEvent.keyboard('{ArrowUp}')
      await expect.element(screen.getByRole('listbox')).toBeVisible()
    })

    it('Enter selects the active option and closes', async () => {
      const onchange = vi.fn()
      const screen = renderSelect({ value: 'alpha', onchange })
      screen.getByRole('combobox').element().focus()

      // Open with ArrowDown, then move to Beta, then Enter
      await userEvent.keyboard('{ArrowDown}')
      await userEvent.keyboard('{ArrowDown}')
      await userEvent.keyboard('{Enter}')
      expect(onchange).toHaveBeenCalledWith('beta')
      expect(screen.getByRole('listbox').query()).toBeNull()
    })

    it('Space selects the active option and closes', async () => {
      const onchange = vi.fn()
      const screen = renderSelect({ value: 'alpha', onchange })
      screen.getByRole('combobox').element().focus()

      await userEvent.keyboard('{ArrowDown}')
      await userEvent.keyboard('{ArrowDown}')
      await userEvent.keyboard('{ }')
      expect(onchange).toHaveBeenCalledWith('beta')
    })

    it('Escape closes without selecting', async () => {
      const onchange = vi.fn()
      const screen = renderSelect({ onchange })
      await screen.getByRole('combobox').click()
      await expect.element(screen.getByRole('listbox')).toBeVisible()
      await userEvent.keyboard('{Escape}')
      expect(screen.getByRole('listbox').query()).toBeNull()
      expect(onchange).not.toHaveBeenCalled()
    })

    it('Tab closes the dropdown without selecting', async () => {
      const onchange = vi.fn()
      const screen = renderSelect({ onchange })
      await screen.getByRole('combobox').click()
      await expect.element(screen.getByRole('listbox')).toBeVisible()
      await userEvent.keyboard('{Tab}')
      expect(screen.getByRole('listbox').query()).toBeNull()
      expect(onchange).not.toHaveBeenCalled()
    })
  })

  describe('keyboard navigation wrapping', () => {
    it('ArrowDown wraps from last to first option', async () => {
      const screen = renderSelect({ value: 'gamma' })
      screen.getByRole('combobox').element().focus()
      await userEvent.keyboard('{ArrowDown}')
      // Opens at gamma (index 2), ArrowDown wraps to alpha (index 0)
      await userEvent.keyboard('{ArrowDown}')
      await userEvent.keyboard('{Enter}')
      // Should have wrapped to alpha
    })

    it('ArrowUp wraps from first to last option', async () => {
      const screen = renderSelect({ value: 'alpha' })
      screen.getByRole('combobox').element().focus()
      await userEvent.keyboard('{ArrowDown}')
      // Opens at alpha (index 0), ArrowUp wraps to gamma (index 2)
      await userEvent.keyboard('{ArrowUp}')
      await userEvent.keyboard('{Enter}')
    })
  })

  describe('keyboard navigation from listbox', () => {
    it('Enter on listbox selects and closes', async () => {
      const onchange = vi.fn()
      const screen = renderSelect({ onchange })
      await screen.getByRole('combobox').click()
      // The listbox gets focus via openDropdown -> listEl.focus()
      const listbox = screen.getByRole('listbox')
      await expect.element(listbox).toBeVisible()

      await userEvent.keyboard('{ArrowDown}')
      await userEvent.keyboard('{Enter}')
      expect(onchange).toHaveBeenCalledOnce()
      expect(screen.getByRole('listbox').query()).toBeNull()
    })

    it('Escape from listbox closes and returns focus to trigger', async () => {
      const screen = renderSelect()
      await screen.getByRole('combobox').click()
      await expect.element(screen.getByRole('listbox')).toBeVisible()

      await userEvent.keyboard('{Escape}')
      expect(screen.getByRole('listbox').query()).toBeNull()
      // Focus should return to the combobox trigger
      expect(document.activeElement?.getAttribute('role')).toBe('combobox')
    })
  })

  describe('aria-activedescendant', () => {
    it('reflects the currently highlighted option ID', async () => {
      const screen = renderSelect()
      screen.getByRole('combobox').element().focus()
      await userEvent.keyboard('{ArrowDown}')
      // Opens at index 0 (alpha), then ArrowDown moves to index 1
      await userEvent.keyboard('{ArrowDown}')
      await expect
        .element(screen.getByRole('combobox'))
        .toHaveAttribute('aria-activedescendant', 'test-select-option-1')
    })
  })
})
