<script lang="ts">
  import { renderContent } from '../assistant/renderContent'

  /**
   * Prompt editor with Write / Preview sub-tabs.
   *
   * - **Write** shows the raw prompt in an editable `<textarea>`. Edits
   *   stream through `onInput` with the new text.
   * - **Preview** shows the same text rendered as Markdown via the assistant's
   *   `renderContent` pipeline (headings, lists, fenced code with Lean
   *   highlighting, KaTeX math). Preview is read-only.
   *
   * Each instance owns its own active-sub-tab state, so two editors on the
   * same screen operate independently.
   */

  interface Props {
    /** Current text shown in Write and rendered in Preview. */
    value: string
    /** Called whenever the user types in the Write textarea. */
    onInput: (text: string) => void
    /**
     * Prefix for stable testids — e.g. `'assistant-prompt'` yields
     * `'assistant-prompt-write-tab'`, `'assistant-prompt-preview'`, etc.
     */
    testidPrefix: string
    /** aria-label for the textarea, used by screen readers. */
    ariaLabel: string
  }

  let { value, onInput, testidPrefix, ariaLabel }: Props = $props()

  type Mode = 'write' | 'preview'
  let mode = $state<Mode>('write')

  // Rendered HTML is derived — any edit in Write reflects in Preview the next
  // time the user flips over, and flipping back to Write shows the current
  // text unchanged.
  const renderedHtml = $derived(value ? renderContent(value) : '')

  function subTabClass(active: boolean): string {
    return active
      ? 'bg-bg-tertiary text-text-primary'
      : 'text-text-secondary hover:text-text-primary'
  }
</script>

<div class="flex flex-col flex-1 min-h-0 gap-1">
  <!-- Sub-tablist: Write / Preview -->
  <div
    role="tablist"
    aria-label="Prompt editor mode"
    class="flex shrink-0 border-b border-border"
  >
    <button
      type="button"
      role="tab"
      aria-selected={mode === 'write'}
      class="px-3 py-1 text-[11px] uppercase tracking-wide rounded-t
        focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent focus-visible:ring-inset
        {subTabClass(mode === 'write')}"
      onclick={() => (mode = 'write')}
      data-testid="{testidPrefix}-write-tab"
    >
      Write
    </button>
    <button
      type="button"
      role="tab"
      aria-selected={mode === 'preview'}
      class="px-3 py-1 text-[11px] uppercase tracking-wide rounded-t
        focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent focus-visible:ring-inset
        {subTabClass(mode === 'preview')}"
      onclick={() => (mode = 'preview')}
      data-testid="{testidPrefix}-preview-tab"
    >
      Preview
    </button>
  </div>

  {#if mode === 'write'}
    <textarea
      aria-label={ariaLabel}
      {value}
      oninput={(e) => {
        onInput((e.target as HTMLTextAreaElement).value)
      }}
      class="flex-1 min-h-[8rem] w-full rounded border border-border bg-bg-secondary
        px-3 py-2 text-[12px] text-text-primary font-mono resize-none
        focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent"
      data-testid="{testidPrefix}-textarea"
    ></textarea>
  {:else}
    <div
      class="flex-1 min-h-[8rem] overflow-y-auto rounded border border-border bg-bg-secondary
        px-3 py-2 text-[12px] text-text-primary"
      data-testid="{testidPrefix}-preview"
      aria-label="{ariaLabel} preview"
    >
      {#if renderedHtml}
        <!-- eslint-disable-next-line svelte/no-at-html-tags -- renderContent output is trusted (internal markdown render) -->
        <div class="prompt-preview">{@html renderedHtml}</div>
      {:else}
        <p class="text-text-secondary opacity-60">Nothing to preview.</p>
      {/if}
    </div>
  {/if}
</div>

<style>
  .prompt-preview :global(h1),
  .prompt-preview :global(h2),
  .prompt-preview :global(h3),
  .prompt-preview :global(h4) {
    font-weight: 600;
    margin: 0.6em 0 0.3em;
  }
  .prompt-preview :global(h1) {
    font-size: 1.15em;
  }
  .prompt-preview :global(h2) {
    font-size: 1.08em;
  }
  .prompt-preview :global(h3),
  .prompt-preview :global(h4) {
    font-size: 1em;
  }
  .prompt-preview :global(p) {
    margin: 0.4em 0;
    line-height: 1.5;
  }
  .prompt-preview :global(ul),
  .prompt-preview :global(ol) {
    margin: 0.4em 0;
    padding-left: 1.2em;
  }
  .prompt-preview :global(li) {
    margin: 0.15em 0;
  }
  .prompt-preview :global(code) {
    font-family: var(--font-mono, monospace);
    background: var(--bg-tertiary, rgba(127, 127, 127, 0.1));
    padding: 0 0.25em;
    border-radius: 3px;
  }
  .prompt-preview :global(pre) {
    margin: 0.6em 0;
    padding: 0.6em 0.8em;
    border-radius: 4px;
    background: var(--bg-tertiary);
    overflow-x: auto;
  }
  .prompt-preview :global(pre code) {
    background: transparent;
    padding: 0;
  }
  .prompt-preview :global(blockquote) {
    border-left: 3px solid var(--border);
    margin: 0.5em 0;
    padding-left: 0.8em;
    color: var(--text-secondary);
  }
</style>
