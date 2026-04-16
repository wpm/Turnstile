<script lang="ts">
  import { onMount, tick } from 'svelte'

  interface Option {
    value: string | number
    label: string
  }

  interface Props {
    id: string
    value: string | number
    options: Option[]
    onchange: (value: string | number) => void
    'data-testid'?: string
  }

  let { id, value, options, onchange, 'data-testid': testid }: Props = $props()

  let open = $state(false)
  let triggerEl = $state<HTMLButtonElement | null>(null)
  let listEl = $state<HTMLUListElement | null>(null)
  let localActiveIndex = $state(-1)

  const selectedLabel = $derived(options.find((o) => o.value === value)?.label ?? String(value))
  const listboxId = $derived(`${id}-listbox`)
  const activeId = $derived(
    open && localActiveIndex >= 0 ? `${id}-option-${String(localActiveIndex)}` : undefined,
  )

  async function openDropdown(): Promise<void> {
    localActiveIndex = options.findIndex((o) => o.value === value)
    if (localActiveIndex < 0) localActiveIndex = 0
    open = true
    await tick()
    listEl?.focus()
  }

  function closeDropdown(): void {
    open = false
    triggerEl?.focus()
  }

  function selectOption(option: Option): void {
    onchange(option.value)
    open = false
    triggerEl?.focus()
  }

  function onTriggerKeydown(e: KeyboardEvent): void {
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault()
      if (open) {
        localActiveIndex =
          e.key === 'ArrowDown'
            ? (localActiveIndex + 1) % options.length
            : (localActiveIndex - 1 + options.length) % options.length
      } else {
        void openDropdown()
      }
    } else if (e.key === 'Enter' || e.key === ' ') {
      e.preventDefault()
      if (open) {
        const opt = options[localActiveIndex]
        if (opt) selectOption(opt)
      } else {
        void openDropdown()
      }
    } else if (e.key === 'Escape') {
      open = false
    } else if (e.key === 'Tab') {
      // Let Tab move focus naturally; close the dropdown as it leaves.
      open = false
    }
  }

  function onListKeydown(e: KeyboardEvent): void {
    if (e.key === 'ArrowDown') {
      e.preventDefault()
      localActiveIndex = (localActiveIndex + 1) % options.length
    } else if (e.key === 'ArrowUp') {
      e.preventDefault()
      localActiveIndex = (localActiveIndex - 1 + options.length) % options.length
    } else if (e.key === 'Enter' || e.key === ' ') {
      e.preventDefault()
      const opt = options[localActiveIndex]
      if (opt) selectOption(opt)
    } else if (e.key === 'Escape') {
      closeDropdown()
    } else if (e.key === 'Tab') {
      // Let Tab move focus naturally; close the dropdown as it leaves.
      open = false
    }
  }

  // Scroll the active option into view when navigating by keyboard.
  $effect(() => {
    if (!open || !listEl) return
    const el = listEl.querySelector<HTMLElement>(`[id="${id}-option-${String(localActiveIndex)}"]`)
    el?.scrollIntoView({ block: 'nearest' })
  })

  // Close on outside mousedown — the only reliable cross-environment signal
  // that focus is leaving the component. onblur is not used because WKWebView
  // fires blur before mousedown handlers resolve, causing the dropdown to close
  // before click events on <li> items can fire.
  onMount(() => {
    function onDocMousedown(e: MouseEvent): void {
      if (!triggerEl?.contains(e.target as Node) && !listEl?.contains(e.target as Node)) {
        open = false
      }
    }
    document.addEventListener('mousedown', onDocMousedown)
    return () => {
      document.removeEventListener('mousedown', onDocMousedown)
    }
  })
</script>

<div class="relative inline-block" {id}>
  <button
    bind:this={triggerEl}
    type="button"
    role="combobox"
    aria-haspopup="listbox"
    aria-expanded={open}
    aria-controls={listboxId}
    aria-activedescendant={activeId}
    id="{id}-trigger"
    data-testid={testid}
    class="flex items-center gap-1.5 rounded border border-border bg-bg-secondary px-2 py-1
      text-[13px] text-text-primary
      focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent"
    onclick={() => {
      if (open) {
        closeDropdown()
      } else {
        void openDropdown()
      }
    }}
    onkeydown={onTriggerKeydown}
  >
    <span>{selectedLabel}</span>
    <svg
      xmlns="http://www.w3.org/2000/svg"
      viewBox="0 0 16 16"
      fill="currentColor"
      class="w-3 h-3 text-text-secondary shrink-0"
      aria-hidden="true"
    >
      <path
        fill-rule="evenodd"
        d="M4.22 6.22a.75.75 0 0 1 1.06 0L8 8.94l2.72-2.72a.75.75 0 1 1 1.06 1.06l-3.25 3.25a.75.75 0 0 1-1.06 0L4.22 7.28a.75.75 0 0 1 0-1.06Z"
        clip-rule="evenodd"
      />
    </svg>
  </button>

  {#if open}
    <ul
      bind:this={listEl}
      id={listboxId}
      role="listbox"
      aria-label="Options"
      tabindex="-1"
      class="absolute right-0 z-10 mt-1 max-h-48 min-w-full overflow-y-auto rounded border border-border
        bg-bg-primary shadow-lg focus:outline-none"
      onkeydown={onListKeydown}
    >
      {#each options as option, i (option.value)}
        <!-- svelte-ignore a11y_click_events_have_key_events -->
        <li
          id="{id}-option-{i}"
          role="option"
          aria-selected={option.value === value}
          class="cursor-default px-3 py-1.5 text-[13px] select-none
            {i === localActiveIndex
            ? 'bg-accent text-white'
            : 'text-text-primary hover:bg-bg-secondary'}"
          onclick={() => {
            selectOption(option)
          }}
        >
          {option.label}
        </li>
      {/each}
    </ul>
  {/if}
</div>
