<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { settings as settingsStore, type Settings } from "./appState";

  interface ModelInfo {
    id: string;
    display_name: string;
  }

  let { onClose }: { onClose: () => void } = $props();

  let models = $state<ModelInfo[]>([]);
  let draft = $state<Settings | null>(null);
  let defaultAssistantPrompt = $state("");
  let defaultTranslationPrompt = $state("");
  let saving = $state(false);
  let error = $state<string | null>(null);

  onMount(async () => {
    try {
      const [s, m, ap, tp] = await Promise.all([
        invoke<Settings>("get_settings"),
        invoke<ModelInfo[]>("get_available_models"),
        invoke<string>("get_default_assistant_prompt"),
        invoke<string>("get_default_translation_prompt"),
      ]);
      draft = { ...s };
      models = m;
      defaultAssistantPrompt = ap;
      defaultTranslationPrompt = tp;
    } catch (e) {
      error = String(e);
    }
  });

  async function save() {
    if (!draft) return;
    saving = true;
    error = null;
    try {
      // Empty prompt overrides mean "use the default".
      const cleaned: Settings = {
        ...draft,
        assistant_prompt: draft.assistant_prompt?.trim()
          ? draft.assistant_prompt
          : null,
        translation_prompt: draft.translation_prompt?.trim()
          ? draft.translation_prompt
          : null,
      };
      await invoke("save_settings", { settings: cleaned });
      settingsStore.set(cleaned);
      onClose();
    } catch (e) {
      error = String(e);
    } finally {
      saving = false;
    }
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") onClose();
  }
</script>

<svelte:window onkeydown={onKeydown} />

<div
  class="overlay"
  role="presentation"
  onclick={(e) => {
    if (e.target === e.currentTarget) onClose();
  }}
>
  <div class="dialog" role="dialog" aria-modal="true" aria-label="Settings">
    <div class="dialog-header">Settings</div>

    {#if draft}
      <div class="dialog-body">
        <fieldset>
          <legend>Models</legend>
          <label>
            <span>Assistant</span>
            <select bind:value={draft.assistant_model}>
              <option value={null}>Default</option>
              {#each models as m (m.id)}
                <option value={m.id}>{m.display_name}</option>
              {/each}
            </select>
          </label>
          <label>
            <span>Prose translation</span>
            <select bind:value={draft.translation_model}>
              <option value={null}>Default</option>
              {#each models as m (m.id)}
                <option value={m.id}>{m.display_name}</option>
              {/each}
            </select>
          </label>
        </fieldset>

        <fieldset>
          <legend>Font sizes (pt)</legend>
          <div class="font-grid">
            <label>
              <span>Editor</span>
              <input
                type="number"
                min="8"
                max="32"
                bind:value={draft.editor_font_size}
              />
            </label>
            <label>
              <span>Goal state</span>
              <input
                type="number"
                min="8"
                max="32"
                bind:value={draft.goal_state_font_size}
              />
            </label>
            <label>
              <span>Prose proof</span>
              <input
                type="number"
                min="8"
                max="32"
                bind:value={draft.prose_proof_font_size}
              />
            </label>
            <label>
              <span>Assistant</span>
              <input
                type="number"
                min="8"
                max="32"
                bind:value={draft.assistant_font_size}
              />
            </label>
          </div>
        </fieldset>

        <fieldset>
          <legend>Prompts</legend>
          <label class="prompt-label">
            <span>Assistant system prompt (leave empty for the default)</span>
            <textarea
              rows="5"
              placeholder={defaultAssistantPrompt.slice(0, 200) + "…"}
              bind:value={draft.assistant_prompt}
            ></textarea>
          </label>
          <label class="prompt-label">
            <span>Translation system prompt (leave empty for the default)</span>
            <textarea
              rows="5"
              placeholder={defaultTranslationPrompt.slice(0, 200) + "…"}
              bind:value={draft.translation_prompt}
            ></textarea>
          </label>
        </fieldset>

        {#if error}
          <div class="error">{error}</div>
        {/if}
      </div>

      <div class="dialog-footer">
        <button class="secondary" onclick={onClose}>Cancel</button>
        <button class="primary" disabled={saving} onclick={() => void save()}>
          {saving ? "Saving…" : "Save"}
        </button>
      </div>
    {:else if error}
      <div class="dialog-body"><div class="error">{error}</div></div>
    {:else}
      <div class="dialog-body">Loading…</div>
    {/if}
  </div>
</div>

<style>
  .overlay {
    position: fixed;
    inset: 0;
    background: rgb(0 0 0 / 40%);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 100;
  }

  .dialog {
    width: min(40rem, calc(100vw - 4rem));
    max-height: calc(100vh - 4rem);
    display: flex;
    flex-direction: column;
    background: var(--color-bg);
    color: var(--color-text);
    border: 1px solid var(--color-border);
    border-radius: 0.75rem;
    box-shadow: 0 20px 50px rgb(0 0 0 / 30%);
  }

  .dialog-header {
    padding: 0.75rem 1rem;
    font-size: 0.75rem;
    font-weight: 600;
    letter-spacing: 0.05em;
    text-transform: uppercase;
    color: var(--color-text-muted);
    border-bottom: 1px solid var(--color-border);
  }

  .dialog-body {
    padding: 1rem;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 1rem;
    font-size: 0.875rem;
  }

  .dialog-footer {
    display: flex;
    justify-content: flex-end;
    gap: 0.5rem;
    padding: 0.75rem 1rem;
    border-top: 1px solid var(--color-border);
  }

  fieldset {
    border: 1px solid var(--color-border);
    border-radius: 0.5rem;
    padding: 0.75rem;
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
  }

  legend {
    font-size: 0.6875rem;
    font-weight: 600;
    letter-spacing: 0.05em;
    text-transform: uppercase;
    color: var(--color-text-muted);
    padding: 0 0.375rem;
  }

  label {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.75rem;
  }

  label > span {
    color: var(--color-text-muted);
    font-size: 0.8125rem;
  }

  .font-grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 0.5rem 1.5rem;
  }

  .prompt-label {
    flex-direction: column;
    align-items: stretch;
  }

  select,
  input,
  textarea {
    background: var(--color-surface);
    color: var(--color-text);
    border: 1px solid var(--color-border);
    border-radius: 0.375rem;
    padding: 0.25rem 0.5rem;
    font-size: 0.8125rem;
    font-family: inherit;
  }

  input[type="number"] {
    width: 4.5rem;
  }

  textarea {
    resize: vertical;
    font-family: ui-monospace, monospace;
    font-size: 0.75rem;
  }

  .error {
    color: #dc2626;
    font-size: 0.8125rem;
  }

  button {
    padding: 0.375rem 1rem;
    border-radius: 0.5rem;
    font-size: 0.8125rem;
    transition: background 0.15s;
  }

  button.primary {
    background: var(--color-accent);
    color: var(--color-accent-text);
  }

  button.primary:hover:not(:disabled) {
    background: var(--color-accent-hover);
  }

  button.primary:disabled {
    background: var(--color-border);
  }

  button.secondary {
    background: var(--color-surface);
    color: var(--color-text);
    border: 1px solid var(--color-border);
  }
</style>
