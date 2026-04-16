import { render } from 'vitest-browser-svelte'
import { describe, it, expect } from 'vitest'
import SetupOverlay from './SetupOverlay.svelte'

interface Props {
  message: string
  progress: number
  visible: boolean
  isError: boolean
}

function renderOverlay(overrides: Partial<Props> = {}): ReturnType<typeof render> {
  return render(SetupOverlay, {
    message: 'Setting up',
    progress: 0,
    visible: true,
    isError: false,
    ...overrides,
  })
}

describe('SetupOverlay', () => {
  it('renders nothing when not visible', async () => {
    const screen = renderOverlay({ visible: false })
    await expect.element(screen.getByRole('status')).not.toBeInTheDocument()
  })

  it('shows the setup message and an indeterminate progress bar when progress is 0', async () => {
    const screen = renderOverlay({ message: 'Downloading toolchain' })
    await expect.element(screen.getByText('Downloading toolchain')).toBeVisible()
    await expect
      .element(screen.getByRole('progressbar'))
      .toHaveAttribute('aria-valuetext', 'Starting…')
  })

  it('reports determinate progress via aria-valuenow', async () => {
    const screen = renderOverlay({ progress: 42 })
    const bar = screen.getByRole('progressbar')
    await expect.element(bar).toHaveAttribute('aria-valuenow', '42')
    await expect.element(bar).toHaveAttribute('aria-valuetext', '42%')
  })

  it('shows the error fallback and no progress bar when isError is true', async () => {
    const screen = renderOverlay({ message: 'Something broke', isError: true })
    await expect.element(screen.getByText('Something broke')).toBeVisible()
    await expect.element(screen.getByText(/Setup failed/)).toBeVisible()
    await expect.element(screen.getByRole('progressbar')).not.toBeInTheDocument()
  })
})
