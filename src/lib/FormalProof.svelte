<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import {
    Compartment,
    EditorState,
    StateEffect,
    StateField,
  } from "@codemirror/state";
  import {
    EditorView,
    keymap,
    lineNumbers,
    highlightActiveLine,
    Decoration,
    type DecorationSet,
  } from "@codemirror/view";
  import {
    annotationField,
    diagnosticGutter,
    setAnnotations,
    type Annotation,
  } from "./annotations";
  import { defaultKeymap, history, historyKeymap } from "@codemirror/commands";
  import {
    syntaxHighlighting,
    defaultHighlightStyle,
  } from "@codemirror/language";
  import { oneDark } from "@codemirror/theme-one-dark";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { applyTextEdits, type TextEdit } from "./textEdits";

  const setProgressLines = StateEffect.define<{ from: number; to: number }[]>();

  const progressDecoration = Decoration.line({ class: "cm-elaborating" });

  const progressField = StateField.define<DecorationSet>({
    create: () => Decoration.none,
    update(deco, tr) {
      try {
        for (const effect of tr.effects) {
          if (effect.is(setProgressLines)) {
            if (effect.value.length === 0) return Decoration.none;
            const marks = effect.value.flatMap(({ from, to }) => {
              const ranges = [];
              for (let line = Math.max(1, from); line <= to; line++) {
                if (line > tr.state.doc.lines) break;
                ranges.push(
                  progressDecoration.range(tr.state.doc.line(line).from),
                );
              }
              return ranges;
            });
            return Decoration.set(marks, true);
          }
        }
        return tr.docChanged ? deco.map(tr.changes) : deco;
      } catch {
        return Decoration.none;
      }
    },
    provide: (f) => EditorView.decorations.from(f),
  });

  let {
    dark = false,
    lspReady = false,
  }: { dark?: boolean; lspReady?: boolean } = $props();

  let container: HTMLDivElement;
  let view: EditorView | undefined;
  const themeCompartment = new Compartment();
  const editableCompartment = new Compartment();

  let debounceTimer: ReturnType<typeof setTimeout> | undefined;
  let unlistenElaboration: (() => void) | undefined;
  let unlistenProgress: (() => void) | undefined;
  let unlistenAnnotations: (() => void) | undefined;
  // True while applying LSP format edits to suppress the update listener.
  let applyingFormat = false;
  let isFormatting = $state(false);

  async function formatDocument() {
    if (!view) return;
    isFormatting = true;
    view.dispatch({ effects: setProgressLines.of([]) });
    try {
      const edits = await invoke<TextEdit[]>("lsp_format_document");
      if (edits.length > 0) {
        applyTextEdits(
          view,
          edits,
          () => {
            applyingFormat = true;
          },
          () => {
            applyingFormat = false;
          },
        );
      }
    } finally {
      isFormatting = false;
    }
  }

  onMount(async () => {
    view = new EditorView({
      state: EditorState.create({
        extensions: [
          lineNumbers(),
          highlightActiveLine(),
          history(),
          keymap.of([...defaultKeymap, ...historyKeymap]),
          progressField,
          annotationField,
          ...diagnosticGutter,
          syntaxHighlighting(defaultHighlightStyle),
          editableCompartment.of(EditorView.editable.of(false)),
          themeCompartment.of(dark ? oneDark : []),
          EditorView.theme({
            "&": { height: "100%" },
            ".cm-scroller": { overflow: "auto" },
          }),
          EditorView.updateListener.of((update) => {
            if (!update.docChanged || applyingFormat) return;
            clearTimeout(debounceTimer);
            const content = update.state.doc.toString();
            debounceTimer = setTimeout(() => {
              void invoke("update_document", { content });
            }, 300);
          }),
        ],
      }),
      parent: container,
    });

    // Format whenever the LSP finishes elaborating — this is the correct
    // moment: the server has fully processed the latest didChange.
    unlistenElaboration = await listen("lsp-elaboration-done", () => {
      void formatDocument();
    });

    unlistenProgress = await listen<{ start_line: number; end_line: number }[]>(
      "lsp-file-progress",
      (e) => {
        if (!view) return;
        view.dispatch({
          effects: setProgressLines.of(
            e.payload.map((r) => ({ from: r.start_line, to: r.end_line })),
          ),
        });
      },
    );

    unlistenAnnotations = await listen<Annotation[]>(
      "annotations-updated",
      (e) => {
        if (!view) return;
        view.dispatch({ effects: setAnnotations.of(e.payload) });
      },
    );
  });

  onDestroy(() => {
    clearTimeout(debounceTimer);
    unlistenElaboration?.();
    unlistenProgress?.();
    unlistenAnnotations?.();
    view?.destroy();
  });

  $effect(() => {
    if (!view) return;
    view.dispatch({
      effects: themeCompartment.reconfigure(dark ? oneDark : []),
    });
  });

  $effect(() => {
    if (!view) return;
    view.dispatch({
      effects: editableCompartment.reconfigure(
        EditorView.editable.of(lspReady),
      ),
    });
  });
</script>

<div
  bind:this={container}
  class="editor-host"
  class:is-formatting={isFormatting}
></div>

<style>
  .editor-host {
    height: 100%;
    overflow: hidden;
    position: relative;
  }

  .editor-host :global(.cm-editor) {
    height: 100%;
    font-size: 0.875rem;
  }

  .editor-host :global(.cm-editor.cm-focused) {
    outline: none;
  }

  .editor-host :global(.cm-elaborating) {
    animation: elaborating-pulse 1.2s ease-in-out infinite;
  }

  @keyframes elaborating-pulse {
    0%,
    100% {
      background-color: transparent;
    }
    50% {
      background-color: color-mix(
        in srgb,
        var(--color-accent) 12%,
        transparent
      );
    }
  }

  .editor-host.is-formatting::after {
    content: "Formatting\2026";
    position: absolute;
    bottom: 0.5rem;
    right: 0.75rem;
    font-size: 0.6875rem;
    font-family: system-ui, sans-serif;
    color: var(--color-text-muted);
    pointer-events: none;
    animation: fade-in 80ms ease-out;
  }

  @keyframes fade-in {
    from {
      opacity: 0;
    }
    to {
      opacity: 1;
    }
  }
</style>
