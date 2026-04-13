<script lang="ts">
  import { onMount } from 'svelte'
  import { FONT_SIZE_OPTIONS, settings, createDraft, updateSetting } from '../lib/settings.svelte'
  import { invoke } from '../lib/tauri'
  import { showError } from '../lib/errorNotification.svelte'
  import { theme } from '../lib/theme'
  import type { ThemePreference } from '../lib/theme'
  import SelectField from './SelectField.svelte'

  const THEME_OPTIONS: { value: ThemePreference; label: string }[] = [
    { value: 'auto', label: 'Auto (follow system)' },
    { value: 'light', label: 'Light' },
    { value: 'dark', label: 'Dark' },
  ]

  const { onClose }: { onClose: () => void } = $props()

  const TABS = [
    { id: 'appearance', label: 'Appearance' },
    { id: 'model', label: 'Model' },
  ]

  const FONT_FIELDS = [
    { id: 'editor', label: 'Editor', key: 'editorFontSize' as const },
    { id: 'prose', label: 'Prose', key: 'proseFontSize' as const },
    { id: 'chat', label: 'Chat', key: 'chatFontSize' as const },
  ]

  const appearanceDraft = createDraft(['editorFontSize', 'proseFontSize', 'chatFontSize'])
  const modelDraft = createDraft(['model'], {
    afterApply: async (values) => {
      if (values.model) await invoke('set_model', { modelId: values.model })
    },
  })

  let activeTab = $state('appearance')

  function tabClass(id: string): string {
    return activeTab === id
      ? 'bg-accent text-white'
      : 'text-text-secondary hover:bg-bg-tertiary hover:text-text-primary'
  }

  let windowEl = $state<HTMLElement | null>(null)
  let posX = $state<number | null>(null)
  let posY = $state<number | null>(null)
  let dragCleanup: (() => void) | null = null

  // The element that had focus before the modal opened — restored on close.
  let triggerEl: Element | null = null

  // All focusable elements inside the modal window.
  function getFocusable(): HTMLElement[] {
    if (!windowEl) return []
    return Array.from(
      windowEl.querySelectorAll<HTMLElement>(
        'button:not([disabled]), select:not([disabled]), [tabindex]:not([tabindex="-1"])',
      ),
    ).filter((el) => el.offsetParent !== null)
  }

  // First focusable element inside the active (visible) tab panel.
  function getFirstPanelFocusable(): HTMLElement | null {
    if (!windowEl) return null
    const panel = windowEl.querySelector<HTMLElement>(
      `[role="tabpanel"][id="settings-panel-${activeTab}"]`,
    )
    if (!panel) return null
    return (
      Array.from(
        panel.querySelectorAll<HTMLElement>(
          'button:not([disabled]), select:not([disabled]), [tabindex]:not([tabindex="-1"])',
        ),
      ).find((el) => el.offsetParent !== null) ?? null
    )
  }

  function isTabButton(el: Element | null): boolean {
    return TABS.some((t) => el?.getAttribute('data-testid') === `settings-tab-${t.id}`)
  }

  onMount(() => {
    triggerEl = document.activeElement

    // Move focus to the active tab button on open.
    const activeTabBtn = windowEl?.querySelector<HTMLElement>(
      `[data-testid="settings-tab-${activeTab}"]`,
    )
    if (activeTabBtn) activeTabBtn.focus()

    window.addEventListener('keydown', handleKeydown)
    return () => {
      window.removeEventListener('keydown', handleKeydown)
      dragCleanup?.()
      // Return focus to the trigger when the modal unmounts.
      if (triggerEl instanceof HTMLElement) triggerEl.focus()
    }
  })

  const selectedModelId = $derived(
    modelDraft.model ??
      (settings.availableModels.length > 0 ? (settings.availableModels[0]?.id ?? '') : ''),
  )

  const windowStyle = $derived(
    posX === null
      ? ''
      : `position: fixed; left: ${String(posX)}px; top: ${String(posY ?? 0)}px; transform: none;`,
  )

  function handleKeydown(e: KeyboardEvent): void {
    if (e.key === 'Escape') {
      onClose()
      return
    }

    // Focus trap: keep Tab/Shift+Tab inside the modal.
    if (e.key === 'Tab') {
      const focusable = getFocusable()
      if (focusable.length === 0) return
      const first = focusable[0]
      const last = focusable[focusable.length - 1]
      if (!first || !last) return

      // Tab from a tab button jumps into the panel, skipping other tab buttons.
      // This matches the ARIA tabs pattern: Tab exits the tablist into the tabpanel.
      if (!e.shiftKey && isTabButton(document.activeElement)) {
        const firstInPanel = getFirstPanelFocusable()
        if (firstInPanel) {
          e.preventDefault()
          firstInPanel.focus()
          return
        }
      }

      // Shift+Tab from first focusable wraps to last; Tab from last wraps to first.
      if (e.shiftKey && document.activeElement === first) {
        e.preventDefault()
        last.focus()
      } else if (!e.shiftKey && document.activeElement === last) {
        e.preventDefault()
        first.focus()
      }
      return
    }

    // Up/Down arrow keys switch between tabs when a tab button holds focus.
    // Home/End jump to first/last tab. (Vertical tablist per APG.)
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp' || e.key === 'Home' || e.key === 'End') {
      if (!isTabButton(document.activeElement)) return
      const tabIds = TABS.map((t) => t.id)
      const currentIndex = tabIds.indexOf(activeTab)
      if (currentIndex === -1) return

      let nextIndex: number
      if (e.key === 'Home') {
        nextIndex = 0
      } else if (e.key === 'End') {
        nextIndex = tabIds.length - 1
      } else {
        nextIndex =
          e.key === 'ArrowDown'
            ? (currentIndex + 1) % tabIds.length
            : (currentIndex - 1 + tabIds.length) % tabIds.length
      }

      const nextTabId = tabIds[nextIndex]
      if (!nextTabId) return
      e.preventDefault()
      activeTab = nextTabId
      windowEl?.querySelector<HTMLElement>(`[data-testid="settings-tab-${nextTabId}"]`)?.focus()
    }
  }

  function handleBackdropClick(e: MouseEvent): void {
    if (e.target === e.currentTarget) {
      onClose()
    }
  }

  function onTitleMousedown(e: MouseEvent): void {
    if (e.button !== 0 || (e.target as Element).closest('button')) {
      return
    }
    e.preventDefault()

    if (!windowEl) return
    const rect = windowEl.getBoundingClientRect()
    const startX = rect.left
    const startY = rect.top
    const mouseStartX = e.clientX
    const mouseStartY = e.clientY

    function onMousemove(me: MouseEvent): void {
      posX = startX + (me.clientX - mouseStartX)
      posY = startY + (me.clientY - mouseStartY)
    }
    function onMouseup(): void {
      dragCleanup = null
      window.removeEventListener('mousemove', onMousemove)
      window.removeEventListener('mouseup', onMouseup)
    }
    dragCleanup = () => {
      window.removeEventListener('mousemove', onMousemove)
      window.removeEventListener('mouseup', onMouseup)
    }
    window.addEventListener('mousemove', onMousemove)
    window.addEventListener('mouseup', onMouseup)
  }
</script>

<!-- Backdrop -->
<div
  role="dialog"
  aria-modal="true"
  aria-labelledby="settings-title"
  tabindex="-1"
  class="fixed inset-0 z-50 flex items-center justify-center bg-black/50"
  onclick={handleBackdropClick}
  onkeydown={(e) => {
    if (e.key === 'Escape') onClose()
  }}
  data-testid="settings-modal"
>
  <!-- Modal window -->
  <div
    bind:this={windowEl}
    style="width: 660px; height: 460px; min-width: 420px; min-height: 300px;
      resize: both; overflow: hidden; {windowStyle}"
    class="flex flex-col rounded-lg border border-border bg-bg-primary shadow-2xl"
    onclick={(e) => {
      e.stopPropagation()
    }}
    role="presentation"
  >
    <!-- Title bar -->
    <div
      class="flex shrink-0 cursor-move items-center justify-between border-b border-border px-4 py-3 select-none"
      onmousedown={onTitleMousedown}
      role="presentation"
    >
      <span id="settings-title" class="text-[13px] font-semibold text-text-primary">Settings</span>
      <button
        class="cursor-default text-text-secondary hover:text-text-primary text-lg leading-none px-1
          rounded focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent"
        aria-label="Close settings"
        onclick={onClose}
      >
        ×
      </button>
    </div>

    <!-- Body: sidebar + content -->
    <div class="flex flex-1 overflow-hidden">
      <!-- Tab sidebar: vertical tablist per WAI-ARIA tabs pattern -->
      <!-- svelte-ignore a11y_no_noninteractive_element_to_interactive_role -->
      <nav
        role="tablist"
        aria-label="Settings sections"
        aria-orientation="vertical"
        class="w-40 shrink-0 border-r border-border bg-bg-secondary py-2"
      >
        {#each TABS as tab (tab.id)}
          <button
            role="tab"
            aria-selected={activeTab === tab.id}
            aria-controls="settings-panel-{tab.id}"
            id="settings-tab-{tab.id}"
            class="w-full px-4 py-1.5 text-left text-[12px] transition-colors
              focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent focus-visible:ring-inset
              {tabClass(tab.id)}"
            onclick={() => (activeTab = tab.id)}
            data-testid="settings-tab-{tab.id}"
          >
            {tab.label}
          </button>
        {/each}
      </nav>

      <!-- Tab panels -->
      {#each TABS as tab (tab.id)}
        <div
          role="tabpanel"
          id="settings-panel-{tab.id}"
          aria-labelledby="settings-tab-{tab.id}"
          hidden={activeTab !== tab.id}
          class="settings-tab-panel flex flex-1 flex-col overflow-y-auto p-5 gap-5"
        >
          {#if tab.id === 'appearance'}
            <h3 class="text-[11px] font-semibold uppercase tracking-widest text-text-secondary">
              Theme
            </h3>

            <div class="flex items-center justify-between">
              <span id="theme-select-label" class="text-[13px] text-text-primary">Appearance</span>
              <SelectField
                id="theme-select"
                value={settings.theme}
                options={THEME_OPTIONS}
                onchange={(v) => {
                  const pref = v as ThemePreference
                  theme.set(pref)
                  void updateSetting('theme', pref).catch((err: unknown) => {
                    const msg = err instanceof Error ? err.message : String(err)
                    showError(`Failed to save setting: ${msg}`)
                  })
                }}
                data-testid="theme-select"
              />
            </div>

            <h3 class="text-[11px] font-semibold uppercase tracking-widest text-text-secondary">
              Font Sizes
            </h3>

            {#each FONT_FIELDS as field (field.id)}
              <div class="flex items-center justify-between">
                <span id="{field.id}-font-size-label" class="text-[13px] text-text-primary">
                  {field.label}
                </span>
                <SelectField
                  id="{field.id}-font-size"
                  value={appearanceDraft[field.key]}
                  options={FONT_SIZE_OPTIONS.map((s) => ({ value: s, label: `${String(s)}px` }))}
                  onchange={(v) => {
                    appearanceDraft.set(field.key, Number(v))
                  }}
                  data-testid="{field.id}-font-size-select"
                />
              </div>
            {/each}

            <div class="flex-1"></div>

            <div class="flex items-center justify-between border-t border-border pt-4">
              <button
                class="rounded border border-border bg-bg-secondary px-3 py-1.5
                  text-[12px] text-text-secondary hover:bg-bg-tertiary hover:text-text-primary
                  focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent"
                onclick={() => {
                  appearanceDraft.fillDefaults()
                }}
                data-testid="restore-defaults-button"
              >
                Restore Defaults
              </button>
              <button
                disabled={!appearanceDraft.dirty}
                class="rounded px-3 py-1.5 text-[12px]
                  focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent
                  {appearanceDraft.dirty
                  ? 'bg-accent text-white hover:bg-accent/90'
                  : 'bg-bg-secondary text-text-secondary/50 cursor-default'}"
                onclick={() => {
                  void appearanceDraft.apply().catch((err: unknown) => {
                    const msg = err instanceof Error ? err.message : String(err)
                    showError(`Failed to save settings: ${msg}`)
                  })
                }}
                data-testid="apply-appearance-button"
              >
                Apply
              </button>
            </div>
          {:else if tab.id === 'model'}
            <h3 class="text-[11px] font-semibold uppercase tracking-widest text-text-secondary">
              Language Model
            </h3>

            <div class="flex items-center justify-between">
              <span id="model-select-label" class="text-[13px] text-text-primary">Model</span>
              <SelectField
                id="model-select"
                value={selectedModelId}
                options={settings.availableModels.map((m) => ({
                  value: m.id,
                  label: m.display_name,
                }))}
                onchange={(v) => {
                  modelDraft.set('model', String(v))
                }}
                data-testid="model-select"
              />
            </div>

            <p class="text-[12px] text-text-secondary leading-relaxed">
              The selected model is used for all proof assistant conversations. More capable models
              produce better proofs but may respond more slowly.
            </p>

            <div class="flex-1"></div>

            <div class="flex items-center justify-end border-t border-border pt-4">
              <button
                disabled={!modelDraft.dirty}
                class="rounded px-3 py-1.5 text-[12px]
                  focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent
                  {modelDraft.dirty
                  ? 'bg-accent text-white hover:bg-accent/90'
                  : 'bg-bg-secondary text-text-secondary/50 cursor-default'}"
                onclick={() => {
                  void modelDraft.apply().catch((err: unknown) => {
                    const msg = err instanceof Error ? err.message : String(err)
                    showError(`Failed to save settings: ${msg}`)
                  })
                }}
                data-testid="apply-model-button"
              >
                Apply
              </button>
            </div>
          {/if}
        </div>
      {/each}
    </div>
  </div>
</div>
