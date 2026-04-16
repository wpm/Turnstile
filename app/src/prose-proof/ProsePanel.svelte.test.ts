import { render } from 'vitest-browser-svelte'
import { describe, it, expect } from 'vitest'
import ProsePanel from './ProsePanel.svelte'

interface Props {
  proseHtml: string
  generating: boolean
  fontSize: number
}

function renderPanel(overrides: Partial<Props> = {}): ReturnType<typeof render> {
  return render(ProsePanel, {
    proseHtml: '',
    generating: false,
    fontSize: 14,
    ...overrides,
  })
}

describe('ProsePanel', () => {
  it('shows placeholder text when proseHtml is empty', async () => {
    const screen = renderPanel()
    await expect.element(screen.getByText(/Toggle to Formal Proof/)).toBeVisible()
  })

  it('does not render prose-content when proseHtml is empty', async () => {
    const screen = renderPanel()
    const panel = screen.getByTestId('prose-panel')
    await expect.element(panel).toBeVisible()
    // .prose-content should not exist
    expect(panel.element().querySelector('.prose-content')).toBeNull()
  })

  it('renders HTML content when proseHtml is provided', async () => {
    const screen = renderPanel({
      proseHtml: '<p>A <strong>theorem</strong> statement.</p>',
    })
    await expect.element(screen.getByText('theorem')).toBeVisible()
    // Placeholder should be gone
    expect(screen.getByText(/Toggle to Formal Proof/).query()).toBeNull()
  })

  it('renders generating overlay when generating is true', async () => {
    const screen = renderPanel({
      proseHtml: '<p>content</p>',
      generating: true,
    })
    await expect.element(screen.getByTestId('prose-generating-overlay')).toBeInTheDocument()
  })

  it('does not render generating overlay when generating is false', async () => {
    const screen = renderPanel({
      proseHtml: '<p>content</p>',
      generating: false,
    })
    await expect.element(screen.getByTestId('prose-generating-overlay')).not.toBeInTheDocument()
  })

  it('applies fontSize to placeholder text', () => {
    const screen = renderPanel({ fontSize: 18 })
    const p = screen.getByText(/Toggle to Formal Proof/).element()
    expect(p.style.fontSize).toBe('18px')
  })

  it('applies fontSize to prose content', () => {
    const screen = renderPanel({
      proseHtml: '<p>text</p>',
      fontSize: 20,
    })
    const content = screen.getByTestId('prose-panel').element().querySelector('.prose-content')
    expect(content).not.toBeNull()
    expect((content as HTMLElement).style.fontSize).toBe('20px')
  })

  it('updates content when props change via rerender', async () => {
    const screen = renderPanel()
    await expect.element(screen.getByText(/Toggle to Formal Proof/)).toBeVisible()

    await screen.rerender({
      proseHtml: '<p>New content</p>',
      generating: false,
      fontSize: 14,
    })
    await expect.element(screen.getByText('New content')).toBeVisible()
    expect(screen.getByText(/Toggle to Formal Proof/).query()).toBeNull()
  })
})
