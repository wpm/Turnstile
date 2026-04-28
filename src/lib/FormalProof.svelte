<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import {
    Compartment,
    EditorState,
    StateEffect,
    StateField,
    type Text,
  } from "@codemirror/state";
  import {
    EditorView,
    keymap,
    lineNumbers,
    highlightActiveLine,
    tooltips,
    Decoration,
    type DecorationSet,
  } from "@codemirror/view";
  import {
    annotationField,
    diagnosticGutter,
    setAnnotations,
    type Annotation,
  } from "./annotations";
  import { lspHoverTooltip } from "./hover";
  import { defaultKeymap, history, historyKeymap } from "@codemirror/commands";
  import {
    syntaxHighlighting,
    defaultHighlightStyle,
  } from "@codemirror/language";
  import { oneDark } from "@codemirror/theme-one-dark";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";

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
  let view = $state<EditorView | undefined>(undefined);
  const themeCompartment = new Compartment();
  const editableCompartment = new Compartment();

  let unlistenProgress: (() => void) | undefined;
  let unlistenAnnotations: (() => void) | undefined;
  let unlistenSessionLoaded: (() => void) | undefined;

  type LspPosition = { line: number; character: number };
  type ContentChange = {
    range: { start: LspPosition; end: LspPosition };
    text: string;
  };

  function offsetToLspPosition(doc: Text, offset: number): LspPosition {
    const line = doc.lineAt(offset);
    return { line: line.number - 1, character: offset - line.from };
  }

  onMount(async () => {
    view = new EditorView({
      state: EditorState.create({
        extensions: [
          lineNumbers(),
          highlightActiveLine(),
          history(),
          keymap.of([...defaultKeymap, ...historyKeymap]),
          tooltips({ parent: document.body }),
          progressField,
          annotationField,
          ...diagnosticGutter,
          lspHoverTooltip,
          syntaxHighlighting(defaultHighlightStyle),
          editableCompartment.of(EditorView.editable.of(false)),
          themeCompartment.of(dark ? oneDark : []),
          EditorView.theme({
            "&": { height: "100%" },
            ".cm-scroller": { overflow: "auto" },
          }),
          EditorView.updateListener.of((update) => {
            if (!update.docChanged) return;
            const oldDoc = update.startState.doc;
            const changes: ContentChange[] = [];
            update.changes.iterChanges((fromA, toA, _fromB, _toB, inserted) => {
              changes.push({
                range: {
                  start: offsetToLspPosition(oldDoc, fromA),
                  end: offsetToLspPosition(oldDoc, toA),
                },
                text: inserted.toString(),
              });
            });
            void invoke("update_document", { changes });
          }),
        ],
      }),
      parent: container,
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

    unlistenSessionLoaded = await listen<{ proof_lean: string }>(
      "session-loaded",
      (e) => {
        if (!view) return;
        const newContent = e.payload.proof_lean;
        const currentContent = view.state.doc.toString();
        if (newContent === currentContent) return;
        view.dispatch({
          changes: { from: 0, to: view.state.doc.length, insert: newContent },
          // Clear stale annotations from the previous document immediately.
          effects: setAnnotations.of([]),
        });
      },
    );
  });

  onDestroy(() => {
    unlistenProgress?.();
    unlistenAnnotations?.();
    unlistenSessionLoaded?.();
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

<div bind:this={container} class="editor-host"></div>

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
</style>
