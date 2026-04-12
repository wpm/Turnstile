<script lang="ts">
  import { onMount } from 'svelte'
  import {
    FONT_SIZE_OPTIONS,
    settings,
    updateSetting,
    resetToDefaults,
  } from '../lib/settings.svelte'
  import { invoke } from '../lib/tauri'

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

  let activeTab = $state('appearance')
  let windowEl = $state<HTMLElement | null>(null)

  // Position state — null means "use CSS centering" on first open.
  let posX = $state<number | null>(null)
  let posY = $state<number | null>(null)

  // Track in-flight drag listeners so they can be canceled on unmount.
  let dragCleanup: (() => void) | null = null

  onMount(() => {
    window.addEventListener('keydown', handleKeydown)
    return () => {
      window.removeEventListener('keydown', handleKeydown)
      dragCleanup?.()
    }
  })

  const selectedModelId = $derived(
    settings.model ??
      (settings.availableModels.length > 0 ? (settings.availableModels[0]?.id ?? '') : ''),
  )

  // Reactive style string — recomputed only when posX/posY change.
  const windowStyle = $derived(
    posX === null
      ? ''
      : `position: fixed; left: ${String(posX)}px; top: ${String(posY ?? 0)}px; transform: none;`,
  )

  function handleModelChange(e: Event): void {
    const id = (e.target as HTMLSelectElement).value
    updateSetting('model', id)
    invoke('set_model', { modelId: id }).catch((err: unknown) => {
      console.error('set_model failed:', err)
    })
  }

  function handleKeydown(e: KeyboardEvent): void {
    if (e.key === 'Escape') {
      onClose()
    }
  }

  function handleBackdropClick(e: MouseEvent): void {
    if (e.target === e.currentTarget) {
      onClose()
    }
  }

  function onTitleMousedown(e: MouseEvent): void {
    // Only drag on primary button; ignore clicks on the close button.
    if (e.button !== 0 || (e.target as Element).closest('button')) {
      return
    }
    e.preventDefault()

    // Capture the modal's actual screen position at drag start — works whether
    // the modal is still CSS-centered or already at an explicit position.
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
  aria-label="Settings"
  tabindex="-1"
  class="fixed inset-0 z-50 flex items-center justify-center bg-black/50"
  onclick={handleBackdropClick}
  onkeydown={(e) => {
    if (e.key === 'Escape') onClose()
  }}
  data-testid="settings-modal"
>
  <!-- Modal window: fixed default size, user-resizable -->
  <div
    bind:this={windowEl}
    style="width: 660px; height: 460px; min-width: 420px; min-height: 300px;
      resize: both; overflow: hidden; {windowStyle}"
    class="flex flex-col rounded-lg border border-border-default bg-surface shadow-2xl"
    onclick={(e) => {
      e.stopPropagation()
    }}
    role="presentation"
  >
    <!-- Title bar — drag handle -->
    <div
      class="flex shrink-0 cursor-move items-center justify-between border-b border-border-default px-5 py-3 select-none"
      onmousedown={onTitleMousedown}
      role="presentation"
    >
      <span class="text-[13px] font-semibold text-on-surface">Settings</span>
      <button
        class="text-lg leading-none px-1 cursor-default text-on-surface-secondary hover:text-on-surface transition-colors"
        aria-label="Close settings"
        onclick={onClose}
      >
        ×
      </button>
    </div>

    <!-- Body: sidebar + content -->
    <div class="flex flex-1 overflow-hidden">
      <!-- Tab sidebar -->
      <nav class="w-40 shrink-0 border-r border-border-default bg-surface-secondary py-3">
        {#each TABS as tab (tab.id)}
          <button
            class="w-full px-4 py-2 text-left text-[13px] transition-colors"
            class:bg-accent={activeTab === tab.id}
            class:text-on-accent={activeTab === tab.id}
            class:text-on-surface-secondary={activeTab !== tab.id}
            class:hover:bg-surface-tertiary={activeTab !== tab.id}
            class:hover:text-on-surface={activeTab !== tab.id}
            onclick={() => (activeTab = tab.id)}
            data-testid="settings-tab-{tab.id}"
          >
            {tab.label}
          </button>
        {/each}
      </nav>

      <!-- Tab content -->
      <div class="flex flex-1 flex-col overflow-y-auto p-6 gap-5">
        {#if activeTab === 'appearance'}
          <h3 class="text-[11px] font-semibold uppercase tracking-widest text-on-surface-secondary">
            Font Sizes
          </h3>

          {#each FONT_FIELDS as field (field.id)}
            <div class="flex items-center justify-between py-0.5">
              <label class="text-[13px] text-on-surface" for="{field.id}-font-size">
                {field.label}
              </label>
              <select
                id="{field.id}-font-size"
                class="rounded-md border px-3 py-1.5 text-[13px] focus:outline-none
                  bg-surface-tertiary border-border-default text-on-surface
                  focus:border-border-active transition-colors"
                value={settings[field.key]}
                onchange={(e) => {
                  updateSetting(field.key, Number((e.target as HTMLSelectElement).value))
                }}
                data-testid="{field.id}-font-size-select"
              >
                {#each FONT_SIZE_OPTIONS as size (size)}
                  <option value={size}>{size}px</option>
                {/each}
              </select>
            </div>
          {/each}

          <div class="flex-1"></div>

          <div class="border-t border-border-default pt-4">
            <button
              class="rounded-md border px-3 py-1.5 text-[12px]
                border-border-default bg-surface-tertiary text-on-surface-secondary
                hover:bg-surface-secondary hover:text-on-surface transition-colors"
              onclick={resetToDefaults}
              data-testid="restore-defaults-button"
            >
              Restore Defaults
            </button>
          </div>
        {:else if activeTab === 'model'}
          <h3 class="text-[11px] font-semibold uppercase tracking-widest text-on-surface-secondary">
            Language Model
          </h3>

          <div class="flex items-center justify-between py-0.5">
            <label class="text-[13px] text-on-surface" for="model-select">Model</label>
            <select
              id="model-select"
              class="rounded-md border px-3 py-1.5 text-[13px] focus:outline-none
                bg-surface-tertiary border-border-default text-on-surface
                focus:border-border-active transition-colors"
              value={selectedModelId}
              onchange={handleModelChange}
              data-testid="model-select"
            >
              {#each settings.availableModels as m (m.id)}
                <option value={m.id}>{m.display_name}</option>
              {/each}
            </select>
          </div>

          <div class="flex-1"></div>
        {/if}
      </div>
    </div>
  </div>
</div>
