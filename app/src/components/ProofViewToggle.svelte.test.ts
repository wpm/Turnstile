import { render } from 'vitest-browser-svelte'
import { describe, it, expect, vi } from 'vitest'
import ProofViewToggle from './ProofViewToggle.svelte'

describe('ProofViewToggle', () => {
  it("advertises a switch to 'Prose Proof' when viewing the formal proof", async () => {
    const screen = render(ProofViewToggle, { view: 'formal', onToggle: vi.fn() })
    await expect
      .element(screen.getByRole('button', { name: 'Switch to Prose Proof' }))
      .toBeVisible()
  })

  it("advertises a switch to 'Formal Proof' when viewing the prose proof", async () => {
    const screen = render(ProofViewToggle, { view: 'prose', onToggle: vi.fn() })
    await expect
      .element(screen.getByRole('button', { name: 'Switch to Formal Proof' }))
      .toBeVisible()
  })

  it('fires onToggle when the button is clicked', async () => {
    const onToggle = vi.fn()
    const screen = render(ProofViewToggle, { view: 'formal', onToggle })
    await screen.getByRole('button').click()
    expect(onToggle).toHaveBeenCalledTimes(1)
  })
})
