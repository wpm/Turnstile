<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { settings as settingsStore, type Settings } from "./appState";

  interface ModelInfo {
    id: string;
    display_name: string;
  }

  let {
    onClose,
    firstRun = false,
  }: { onClose: () => void; firstRun?: boolean } = $props();

  let models = $state<ModelInfo[]>([]);
  let draft = $state<Settings | null>(null);
  let defaultAssistantPrompt = $state("");
  let defaultTranslationPrompt = $state("");
  let saving = $state(false);
  let error = $state<string | null>(null);

  // API key handling. We never read the key back from the backend (it lives in
  // the OS keychain, #63) — we only learn whether one is present, and let the
  // user enter a new one. A blank field on save leaves the stored key untouched.
  let keyPresent = $state(false);
  let apiKey = $state("");

  // On first run we require a key before the dialog can be dismissed, so the
  // user comes up connected without a relaunch.
  const needsKey = $derived(firstRun && !keyPresent && !apiKey.trim());

  // Tabbed layout. The three groups (Models & API key, Font sizes, Prompts)
  // each get their own panel; the shared `draft` keeps edits alive when the
  // user switches tabs. The API key lives in the Models tab (#66).
  const tabs = [
    { id: "models", label: "Models & API key" },
    { id: "fonts", label: "Font sizes" },
    { id: "prompts", label: "Prompts" },
  ] as const;
  type TabId = (typeof tabs)[number]["id"];
  let activeTab = $state<TabId>("models");
  let tabRefs = $state<HTMLButtonElement[]>([]);

  // Roving-tabindex arrow navigation: only the active tab is in the tab order,
  // and arrow/Home/End keys move focus (and selection) between tabs.
  function onTabKeydown(e: KeyboardEvent, index: number) {
    let next: number;
    switch (e.key) {
      case "ArrowRight":
      case "ArrowDown":
        next = (index + 1) % tabs.length;
        break;
      case "ArrowLeft":
      case "ArrowUp":
        next = (index - 1 + tabs.length) % tabs.length;
        break;
      case "Home":
        next = 0;
        break;
      case "End":
        next = tabs.length - 1;
        break;
      default:
        return;
    }
    e.preventDefault();
    activeTab = tabs[next].id;
    tabRefs[next]?.focus();
  }

  onMount(async () => {
    try {
      const [s, m, ap, tp, hasKey] = await Promise.all([
        invoke<Settings>("get_settings"),
        invoke<ModelInfo[]>("get_available_models"),
        invoke<string>("get_default_assistant_prompt"),
        invoke<string>("get_default_translation_prompt"),
        invoke<boolean>("has_api_key"),
      ]);
      draft = { ...s };
      models = m;
      defaultAssistantPrompt = ap;
      defaultTranslationPrompt = tp;
      keyPresent = hasKey;
    } catch (e) {
      error = String(e);
    }
  });

  async function save() {
    if (!draft) return;
    saving = true;
    error = null;
    try {
      const trimmedKey = apiKey.trim();
      // Store a newly entered key first; a blank field leaves the existing key
      // untouched (the user need not re-type it to change other settings).
      if (trimmedKey) {
        await invoke("set_api_key", { key: trimmedKey });
        keyPresent = true;
        apiKey = "";
      }

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

  // On first run with no key, the dialog can't be dismissed without entering
  // one — there's no usable app state to return to.
  function requestClose() {
    if (needsKey) return;
    onClose();
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") requestClose();
  }
</script>

<svelte:window onkeydown={onKeydown} />

<div
  class="overlay"
  role="presentation"
  onclick={(e) => {
    if (e.target === e.currentTarget) requestClose();
  }}
>
  <div
    class="dialog"
    role="dialog"
    aria-modal="true"
    aria-label={firstRun ? "Welcome to Turnstile" : "Settings"}
  >
    <div class="dialog-header">
      {firstRun ? "Welcome to Turnstile" : "Settings"}
    </div>

    {#if draft}
      <div class="dialog-body">
        {#if firstRun}
          <p class="intro">
            To use the Proof Assistant, add your Anthropic API key. You can
            change it and the model choices any time from Settings.
          </p>
        {/if}

        <div class="tablist" role="tablist" aria-label="Settings sections">
          {#each tabs as tab, i (tab.id)}
            <button
              bind:this={tabRefs[i]}
              type="button"
              role="tab"
              id="settings-tab-{tab.id}"
              aria-controls="settings-panel-{tab.id}"
              aria-selected={activeTab === tab.id}
              tabindex={activeTab === tab.id ? 0 : -1}
              class="tab"
              class:active={activeTab === tab.id}
              onclick={() => {
                activeTab = tab.id;
              }}
              onkeydown={(e) => {
                onTabKeydown(e, i);
              }}
            >
              {tab.label}
            </button>
          {/each}
        </div>

        <div
          role="tabpanel"
          id="settings-panel-models"
          aria-labelledby="settings-tab-models"
          tabindex="0"
          hidden={activeTab !== "models"}
        >
          <fieldset>
            <legend>Anthropic API key</legend>
            <label class="key-label">
              <span>{keyPresent ? "Replace key" : "API key"}</span>
              <input
                type="password"
                autocomplete="off"
                placeholder={keyPresent
                  ? "A key is already stored — leave blank to keep it"
                  : "sk-ant-…"}
                bind:value={apiKey}
              />
            </label>
            <p class="hint">
              Stored encrypted in your OS keychain — never written to disk in
              plaintext or logged.
            </p>
          </fieldset>

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
        </div>

        <div
          role="tabpanel"
          id="settings-panel-fonts"
          aria-labelledby="settings-tab-fonts"
          tabindex="0"
          hidden={activeTab !== "fonts"}
        >
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
        </div>

        <div
          role="tabpanel"
          id="settings-panel-prompts"
          aria-labelledby="settings-tab-prompts"
          tabindex="0"
          hidden={activeTab !== "prompts"}
        >
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
              <span
                >Translation system prompt (leave empty for the default)</span
              >
              <textarea
                rows="5"
                placeholder={defaultTranslationPrompt.slice(0, 200) + "…"}
                bind:value={draft.translation_prompt}
              ></textarea>
            </label>
          </fieldset>
        </div>

        {#if error}
          <div class="error">{error}</div>
        {/if}
      </div>

      <div class="dialog-footer">
        {#if !firstRun}
          <button class="secondary" onclick={requestClose}>Cancel</button>
        {/if}
        <button
          class="primary"
          disabled={saving || needsKey}
          onclick={() => void save()}
        >
          {saving ? "Saving…" : firstRun ? "Get started" : "Save"}
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

  .tablist {
    display: flex;
    gap: 0.25rem;
    border-bottom: 1px solid var(--color-border);
  }

  .tab {
    padding: 0.375rem 0.75rem;
    border-radius: 0.375rem 0.375rem 0 0;
    background: transparent;
    color: var(--color-text-muted);
    font-size: 0.8125rem;
    border-bottom: 2px solid transparent;
    margin-bottom: -1px;
  }

  .tab:hover {
    color: var(--color-text);
  }

  .tab.active {
    color: var(--color-text);
    border-bottom-color: var(--color-accent);
  }

  [role="tabpanel"] {
    display: flex;
    flex-direction: column;
    gap: 1rem;
  }

  /* An explicit `display` would otherwise win over the `hidden` attribute's
     UA `display: none`, leaving inactive panels visible. */
  [role="tabpanel"][hidden] {
    display: none;
  }

  [role="tabpanel"]:focus-visible {
    outline: none;
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

  .intro {
    margin: 0;
    font-size: 0.8125rem;
    line-height: 1.5;
    color: var(--color-text-muted);
  }

  .hint {
    margin: 0;
    font-size: 0.75rem;
    line-height: 1.4;
    color: var(--color-text-muted);
  }

  .key-label {
    flex-direction: column;
    align-items: stretch;
    gap: 0.375rem;
  }

  .key-label input {
    width: 100%;
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
