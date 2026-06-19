# ADR-0003: Backend as single source of truth for formal-proof text and annotations

**Status:** Accepted
**Date:** 2026-06-19
**Deciders:** W.P. McNeill
**Epic:** [#56](https://github.com/wpm/Turnstile/issues/56)
**Tracking:** [#81](https://github.com/wpm/Turnstile/issues/81)

## Context

The formal proof exists simultaneously in three places:

- **CodeMirror's document** (`view.state.doc` in `src/lib/FormalProof.svelte`) — the editor buffer the user sees and edits.
- **The backend `Proof`** (`Proof.formal.source`, held in `AppState` behind `Arc<Mutex<Proof>>`) — the copy every consumer reads: the Proof Assistant's `read_lean_source` tool, prose translation, and session serialization.
- **The Lean LSP's internal document** — the copy `lean --server` elaborates to produce diagnostics, semantic tokens, and goal state.

These three are kept in agreement by convention, not by a designated authority, and the convention silently changes direction depending on where a change originates:

- **Typing** originates at CodeMirror, which sends an _incremental_ diff (`ContentChange[]`) to the backend; the backend reconstructs its copy by replaying the diff (`apply_content_changes` in `src-tauri/src/lib.rs`) and forwards a ranged `didChange` to the LSP. Here CodeMirror is the de-facto authority.
- **Loading** (open a `.turn` file, restore autosave, new session) originates at the backend, which sets `Proof.formal.source` directly (`src-tauri/src/session/mod.rs`) and emits `session-loaded` so CodeMirror catches up. Here the backend is the authority.

A bug exposed the cost of this ambiguity. After the user edited a theorem and then opened a saved `.turn` file, the Proof Assistant's `read_lean_source` returned the loaded theorem concatenated with leftover fragments of the edited one — two theorems glued together — while the editor pane displayed a single clean theorem. The load set the backend source directly **and** the load-induced document replacement tripped CodeMirror's update listener, which echoed the load back as an incremental diff. That diff — computed against the editor's pre-load buffer — was applied on top of the source the backend had just set, double-applying and corrupting it. The load (backend-authoritative) had re-entered the edit path (CodeMirror-authoritative). A targeted fix is already in the tree: a `programmaticUpdate` transaction annotation that stops the load-induced echo (`src/lib/FormalProof.svelte`), plus an e2e regression test (`e2e/browser/turnstile.spec.ts`).

That fix closes the immediate bug but does not resolve the underlying weaknesses it revealed:

1. **No single source of truth.** The authoritative replica flips between typing and loading, and nothing enforces or documents the contract. The bug was the two directions colliding.

2. **Text and annotations have opposite, undocumented granularities.** Text flows downstream as incremental diffs (CodeMirror → backend → LSP). Annotations flow upstream as whole-document snapshots (LSP → backend → CodeMirror): `publishDiagnostics` and the (delta-_encoded_ but whole-document) semantic-token array are each a complete set; `set_tokens` / `set_diagnostics` replace a whole category; `AnnotationsUpdated` carries a full clone; the frontend store does `annotations.set(...)`. The annotation half is snapshot-in / snapshot-out and cannot drift; the text half reconstructs from diffs and can, and did.

3. **The annotation intermediate representation matches neither end.** The LSP produces two distinct atom types on two schedules: `SemanticToken` and `DiagnosticInfo` (`src-tauri/src/lean/messages/turnstile.rs`). CodeMirror consumes three derived facets — decorations, gutter markers, and hover hits (`src/lib/annotations.ts`) — where diagnostics fan out to all three and tokens feed only one. The current `Annotation` sum type fuses the two producers into a single `Vec<Annotation>`, and all three consumers then re-split it by `kind` on every render. Neither the LSP nor CodeMirror asked for the single list; it is a self-imposed middle.

Forces at play: the backend outlives the editor window and runs operations (the LSP subprocess, autosave-on-exit, assistant tool calls) when no window may be mounted; the backend is the only component every consumer already reads from and the only one testable in pure Rust without a browser; positional annotations are meaningful only relative to a specific version of the text, so text and annotations are one coupled fact; and incrementalism is the native protocol of both interfaces we sit between (CodeMirror's `ChangeSet`, the LSP's ranged `didChange`).

## Decision

1. **The backend is the single source of truth** for the formal proof — both its text and its visual annotations. CodeMirror and the LSP are followers. Every consumer already reads the backend copy; this makes that the _defining_ authority rather than an incidental one.

2. **`Proof.formal.source` is set by whole-string assignment, never by replaying our own diff.** Both entry points — typing and reload — assign a complete, authoritative string. This removes the fallible `apply_content_changes` re-derivation from the authoritative path; that re-derivation was the class the bug belonged to.

3. **Live text sync uses route (3): whole document for our copy, incremental to the LSP.** On a CodeMirror edit the frontend sends the whole document (`update.state.doc.toString()`, already computed today for the UI store) for the backend to assign to `source`, _and_ the incremental delta for the backend to forward to the LSP as a ranged `didChange`. The backend stops applying the delta to its own copy. Two incrementors remain in the system — CodeMirror's and the LSP's — which is acceptable precisely because incrementalism is the native language of both interfaces we mediate; the one incrementor we remove is _ours_.

4. **Reload is a backend-authoritative push, not a simulated keystroke.** On load the backend assigns the loaded text to `source` directly, then pushes the whole document to both followers: CodeMirror silently (tagged so its update listener does not echo a diff — the `programmaticUpdate` fix already does this) and the LSP via a full-document `didChange` (`replace_document`). Routing a reload "through CodeMirror as if typed" is explicitly rejected: that round-trip echo is the original bug. Typing and reload thus become two entry points into one mechanism — a whole-string assignment to the authoritative `source` followed by a push to followers — differing only in which side originated the string.

5. **`LSPAnnotation` replaces `Annotations` as the single source of truth for formal-proof visuals.** It holds the two producer-shaped lists (`Vec<SemanticToken>`, `Vec<DiagnosticInfo>`) and exposes four methods — `syntax_coloring`, `underlines`, `gutter`, `hover` — each returning its own CodeMirror-dialect, serializable structure using CM offset coordinates resolved in Rust against `formal.source`. A single product type wraps the four outputs for the wire. The `line/col → offset` resolution moves from TypeScript (`src/lib/annotations.ts`) into Rust; this is possible only because the text and its annotations are co-located in the backend.

6. **Annotations nest under the formal proof:** `FormalProof { text, annotations: LSPAnnotation }`, rather than hoisting annotations to `Proof`'s top level. The annotations index into `text`, so co-locating them makes "text plus its annotations are one coupled unit" structural rather than conventional.

## Options Considered

### Source-of-truth owner

| Option                         | Authority                        | Assessment                                                                                                                                                                                               |
| ------------------------------ | -------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Editor-as-writer               | CodeMirror; backend follows      | Authority lives in the least-available, least-testable component; the backend's copy becomes correct only after a UI round-trip; "route reload through CodeMirror as typed" reintroduces the echo bug.   |
| **Backend-as-writer (chosen)** | Backend; CodeMirror + LSP follow | Authority lives where every consumer already reads, where it is testable in pure Rust, and where it survives the window. Cost: the load path must push to followers and the editor must follow silently. |

### Text-sync granularity

| Option                                                         | Our copy set by     | To LSP             | Assessment                                                                                                |
| -------------------------------------------------------------- | ------------------- | ------------------ | --------------------------------------------------------------------------------------------------------- |
| (1) Whole-doc everywhere                                       | whole-string assign | full `didChange`   | Simplest; eliminates all re-derivation. Cost: a full-document re-read by Lean on every keystroke.         |
| (2) Incremental everywhere (today)                             | replay our diff     | ranged `didChange` | Fastest, but keeps our fallible applier in the authoritative path — the bug class.                        |
| **(3) `to_string` for our copy + incremental to LSP (chosen)** | whole-string assign | ranged `didChange` | Authoritative copy correct-by-construction; LSP still gets cheap diffs from its proper protocol producer. |

### Annotation intermediate representation

| Option                                 | Shape                                        | Assessment                                                                                            |
| -------------------------------------- | -------------------------------------------- | ----------------------------------------------------------------------------------------------------- |
| One fused list (today)                 | `Vec<Annotation>` (sum type)                 | Matches neither end; producers fuse two-into-one, consumers re-split three times.                     |
| Three consumer-shaped sets             | decorations / gutter / hover, stored         | Matches CodeMirror, but stores what should be _derived_; couples storage to one consumer's rendering. |
| **Two producer-shaped lists (chosen)** | `Vec<SemanticToken>` + `Vec<DiagnosticInfo>` | Matches the LSP producers; the three CM facets are computed on demand as pure derivations.            |

## Trade-off Analysis

Route (3) dominates the text-sync choice. It costs ~nothing over (2) — the frontend already computes `update.state.doc.toString()` for the UI store — and ~nothing over (1) — the LSP still receives cheap ranged diffs. What it buys is decisive: the backend's authoritative copy is no longer the output of _our_ offset math (`lsp_pos_to_offset` + `replace_range`) re-deriving the document from a base plus a diff; it is the verbatim string CodeMirror is displaying. The diff that still flows onward is applied only by the LSP, the canonical protocol consumer designed to track a document via versioned `didChange`. We stop competing with it.

Crucially, route (3) requires **no new synchronization**. Within a single edit, `to_string()` and the deltas are the same CodeMirror transaction expressed two ways — CodeMirror guarantees `state.doc === startState.doc.apply(changes)` — so they are a fan-out of one fact and cannot disagree. Across edits, ordering is already enforced by the `Protocol::next_version` counter (`src-tauri/src/lean/client.rs`) plus the `keep_notification` stale-filter that drops out-of-version LSP notifications. The authoritative `source` is immune to delta ordering altogether because it is a whole-string assignment (last-writer-wins). The only property to _preserve_ (not build) is keeping "store source, then forward delta" inside one critical section in `update_document`, so a later edit's source-write cannot interleave ahead of an earlier edit's `didChange`.

Backend-as-writer is preferred over editor-as-writer because the editor cannot be the authority for a value that must exist before and after the window does, and because making the editor authoritative is exactly what reopens the echo bug on reload. The two-list annotation representation is preferred because it matches the producer side, and the consumer-side derivations (decoration / gutter / hover) are computed regardless and do not want to be stored — moving them into Rust, beside the text they index, is what lets the offset resolution be correct against the authoritative source.

## Consequences

- **Easier:** the formal proof has one authority; `read_lean_source`, prose, and serialization read a copy that cannot drift; the load path and the typing path collapse into one mechanism (whole-string assign + push to followers); the annotation pipeline matches both the LSP producers and the CodeMirror consumers, eliminating the fuse-then-re-split middle; offset resolution happens once, in Rust, against the authoritative text.
- **Harder:** a refactor across the Rust/TypeScript boundary — `Annotations` → `LSPAnnotation` with four serializable CM-dialect derivations, the `AnnotationsUpdated` wire shape, the `ts-rs` types, and the frontend reduced to a thin renderer; the load path must push a full-document `didChange` to the LSP (`replace_document`) in addition to setting `source`; the frontend must send `to_string()` for the backend copy while still emitting the delta for the LSP.
- **To revisit:** the LSP's own incremental copy could in principle drift from CodeMirror's (diagnostics/goal-state wrong while `source` stays right). This risk is contained to the LSP's well-trodden `didChange` machinery and is detectable via the version counter; only route (1) drives system-wide re-derivation risk to zero, at the per-keystroke re-read cost. Whether to add a periodic hash-based resync, or a version stamp pairing each annotation set with the source version it was computed against, is left open.

## Action Items

Tracked under [#81](https://github.com/wpm/Turnstile/issues/81), filed beneath epic [#56](https://github.com/wpm/Turnstile/issues/56).
