<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import {
    Annotation,
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
    EMPTY_ANNOTATIONS,
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
  import {
    fileProgress,
    annotations as annotationsStore,
  } from "./turnstile_messages";
  import { progressHighlightLines } from "./progress";
  import type { FileProgressRange } from "./FileProgressRange";
  import { cursorPosition, proofSource, settings, wordWrap } from "./appState";

  const setProgressLines = StateEffect.define<FileProgressRange[]>();

  // Marks a transaction as a programmatic document replacement (e.g. restoring
  // the editor on session load). The backend already holds the authoritative
  // source for these, so the update listener must NOT forward them back as
  // incremental LSP changes — doing so applies a diff on top of the
  // already-set source and corrupts it (duplicated/concatenated text).
  const programmaticUpdate = Annotation.define<boolean>();

  const progressDecoration = Decoration.line({ class: "cm-elaborating" });

  const progressField = StateField.define<DecorationSet>({
    create: () => Decoration.none,
    update(deco, tr) {
      try {
        for (const effect of tr.effects) {
          if (effect.is(setProgressLines)) {
            if (effect.value.length === 0) return Decoration.none;
            const marks = progressHighlightLines(
              effect.value,
              tr.state.doc.lines,
            ).map((line) =>
              progressDecoration.range(tr.state.doc.line(line).from),
            );
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
  const wrapCompartment = new Compartment();
  const fontCompartment = new Compartment();

  function fontTheme(size: number) {
    return EditorView.theme({
      "&": { fontSize: `${String(size)}pt` },
    });
  }

  let unsubscribeProgress: (() => void) | undefined;
  let unsubscribeAnnotations: (() => void) | undefined;
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
          wrapCompartment.of([]),
          fontCompartment.of([]),
          themeCompartment.of(dark ? oneDark : []),
          EditorView.theme({
            "&": { height: "100%" },
            ".cm-scroller": { overflow: "auto" },
          }),
          EditorView.updateListener.of((update) => {
            if (update.selectionSet || update.docChanged) {
              const head = update.state.selection.main.head;
              const line = update.state.doc.lineAt(head);
              cursorPosition.set({
                line: line.number - 1,
                col: head - line.from,
              });
            }
            if (!update.docChanged) return;
            const fullText = update.state.doc.toString();
            proofSource.set(fullText);
            // A programmatic replacement (session load) is already reflected in
            // the backend source; forwarding it would echo the load back into
            // the edit path. Keep the UI store above in sync, but stop here.
            if (
              update.transactions.some((tr) =>
                tr.annotation(programmaticUpdate),
              )
            ) {
              return;
            }
            // Route (3): the backend is the source of truth. Send the whole
            // document for it to assign verbatim (correct by construction), and
            // the incremental changes for it to forward to the LSP as a ranged
            // didChange — the LSP's native protocol.
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
            void invoke("update_document", { fullText, changes });
          }),
        ],
      }),
      parent: container,
    });

    unsubscribeProgress = fileProgress.subscribe((ranges) => {
      if (!view) return;
      view.dispatch({ effects: setProgressLines.of(ranges) });
    });

    unsubscribeAnnotations = annotationsStore.subscribe((anns) => {
      if (!view) return;
      view.dispatch({ effects: setAnnotations.of(anns) });
    });

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
          effects: setAnnotations.of(EMPTY_ANNOTATIONS),
          // The backend already set its source to this content during the load;
          // tag the transaction so the update listener doesn't echo it back as
          // an incremental change applied on top of the already-set source.
          annotations: programmaticUpdate.of(true),
        });
      },
    );
  });

  onDestroy(() => {
    unsubscribeProgress?.();
    unsubscribeAnnotations?.();
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

  $effect(() => {
    if (!view) return;
    view.dispatch({
      effects: wrapCompartment.reconfigure(
        $wordWrap ? EditorView.lineWrapping : [],
      ),
    });
  });

  $effect(() => {
    if (!view) return;
    const size = $settings?.editor_font_size;
    view.dispatch({
      effects: fontCompartment.reconfigure(size ? fontTheme(size) : []),
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
