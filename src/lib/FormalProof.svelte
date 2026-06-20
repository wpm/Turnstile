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
  import { unicodeAbbreviations } from "./unicodeAbbreviations";
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
    goalCache,
  } from "./turnstile_messages";
  import { progressHighlightLines } from "./progress";
  import type { FileProgressRange } from "./FileProgressRange";
  import { cursorPosition, proofSource, settings, wordWrap } from "./appState";
  import { get } from "svelte/store";
  import GoalCard from "./GoalCard.svelte";
  import type { GoalEntry } from "./GoalEntry";

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
  let hostWrap = $state<HTMLDivElement | undefined>(undefined);
  let view = $state<EditorView | undefined>(undefined);

  // ── Cursor-row goal card (ADR-0004) ──────────────────────────────────
  // Display is a pure function of (cursor row, goal cache): the card renders
  // iff the cursor row's cached goal is `fresh`. No Lean query on the read path.
  let cardGoals = $state<GoalEntry[]>([]);
  let cardTop = $state(0);
  let cardVisible = $state(false);
  let cardDimmed = $state(false);
  let unsubscribeGoalCache: (() => void) | undefined;
  let dimTimer: ReturnType<typeof setTimeout> | undefined;

  /**
   * Recompute the cursor-row card from the cache and the editor geometry. Pure
   * lookup — never touches Lean. Hides the card unless the cursor row is
   * `fresh`; positions it just below the cursor line (so it never underlaps the
   * line's own text) with its right edge pinned to the rail.
   */
  function refreshCard() {
    if (!view || !hostWrap) {
      cardVisible = false;
      return;
    }
    const head = view.state.selection.main.head;
    const line = view.state.doc.lineAt(head);
    const entry = get(goalCache).get(line.number); // cache rows are 1-indexed
    if (!entry || entry.status !== "fresh" || entry.goals.length === 0) {
      cardVisible = false;
      return;
    }
    const coords = view.coordsAtPos(line.from);
    if (!coords) {
      cardVisible = false;
      return;
    }
    cardGoals = entry.goals;
    cardTop = coords.bottom - hostWrap.getBoundingClientRect().top;
    cardVisible = true;
  }
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
  let unlistenFormalSourceUpdated: (() => void) | undefined;

  // Replace the editor document with backend-authoritative content, clearing
  // stale annotations and tagging the transaction programmatic so the update
  // listener doesn't echo it back as an incremental edit. Shared by the
  // session-load and Proof-Assistant-write paths (both set the backend source
  // first; CodeMirror is a follower). No-op when the content already matches.
  function applyBackendSource(newContent: string) {
    if (!view) return;
    if (newContent === view.state.doc.toString()) return;
    view.dispatch({
      changes: { from: 0, to: view.state.doc.length, insert: newContent },
      effects: setAnnotations.of(EMPTY_ANNOTATIONS),
      annotations: programmaticUpdate.of(true),
    });
  }

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
          // Unicode `\`-escape glyph completions, composed (no `override`) so
          // the future Lean LSP completion source stacks alongside it (#98).
          // `autocompletion` registers its own keymap at Prec.highest, so Enter
          // accepts the glyph when the dropdown is open and falls through to the
          // editor bindings (newline, etc.) when it isn't.
          unicodeAbbreviations,
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
            // Cursor movement, edits, and viewport/geometry changes can all move
            // the card or change which row it reflects — recompute from cache.
            if (
              update.selectionSet ||
              update.docChanged ||
              update.geometryChanged
            ) {
              refreshCard();
            }
            // While typing on the cursor row, fade the card rather than let it
            // fight the text being entered under it (the row will go stale and
            // hide shortly after, once the backend re-elaborates).
            if (update.docChanged) {
              cardDimmed = true;
              if (dimTimer) clearTimeout(dimTimer);
              dimTimer = setTimeout(() => {
                cardDimmed = false;
              }, 400);
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

    // A new cache snapshot can show, hide, or change the card. Scrolling moves
    // the cursor line, so the card must follow it.
    unsubscribeGoalCache = goalCache.subscribe(() => {
      refreshCard();
    });
    view.scrollDOM.addEventListener("scroll", refreshCard, { passive: true });

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
        // The backend already set its source to this content during the load;
        // applyBackendSource tags the transaction so the update listener doesn't
        // echo it back as an incremental change on top of the already-set source.
        applyBackendSource(e.payload.proof_lean);
      },
    );

    // The Proof Assistant's update_lean_source tool replaced the backend source;
    // follow it in the editor, the same way a session load does.
    unlistenFormalSourceUpdated = await listen<string>(
      "formal-source-updated",
      (e) => {
        applyBackendSource(e.payload);
      },
    );
  });

  onDestroy(() => {
    unsubscribeProgress?.();
    unsubscribeAnnotations?.();
    unsubscribeGoalCache?.();
    unlistenSessionLoaded?.();
    unlistenFormalSourceUpdated?.();
    if (dimTimer) clearTimeout(dimTimer);
    view?.scrollDOM.removeEventListener("scroll", refreshCard);
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

<div bind:this={hostWrap} class="editor-host">
  <div bind:this={container} class="editor-mount"></div>
  {#if cardVisible}
    <GoalCard goals={cardGoals} top={cardTop} dimmed={cardDimmed} />
  {/if}
</div>

<style>
  .editor-host {
    height: 100%;
    overflow: hidden;
    position: relative;
  }

  .editor-mount {
    height: 100%;
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
