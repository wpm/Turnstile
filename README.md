# Turnstile

_A Lean proof assistant pairing formal proofs with prose._

Turnstile is a desktop app for developing Lean 4 proofs alongside a
textbook-style prose rendering of the same argument. The left pane is a
CodeMirror editor backed by a live `lean --server`; below it sits the
current goal state or the generated prose proof. The right pane is a
Proof Assistant — a Claude-backed collaborator that reads your source,
goal state, and diagnostics through tools, and keeps the prose in step
with the formalism.

The Lean code is the mathematics; the prose hints at the meaning. The
turnstile (⊢) separates what you have from what you must show.

## Running

Requirements: [pnpm](https://pnpm.io), the [Rust
toolchain](https://rustup.rs), and an `ANTHROPIC_API_KEY` in the
environment (without one the assistant falls back to a mock that echoes).
On first launch Turnstile installs elan, the pinned Lean toolchain, and
the Mathlib cache automatically — the status bar tracks progress.

```sh
pnpm install
pnpm tauri dev      # development app
pnpm tauri build    # release bundle
```

## Testing

```sh
pnpm verify         # the whole gate, in order (frontend, Rust, e2e, praxis)

pnpm test           # frontend unit tests (vitest)
pnpm check          # svelte-check
pnpm lint           # eslint
pnpm verify:rust    # cargo fmt --check, clippy -D warnings, cargo test
pnpm test:e2e       # Playwright against the real UI + fake backend
pnpm test:praxis    # vitest suite driving a real lean --server (√2 proof)
```

The last two are the praxis layer (see `praxis/README.md`): the
Playwright suite runs the unmodified frontend in a browser against an
in-browser simulation of the backend (`vite dev --mode e2e` serves the
same thing for manual poking), and the LSP harness checks the protocol
contracts the backend encodes against a live Lean server.

## Architecture notes

- `src-tauri/src/lean/` — the LSP client: `server.rs` spawns and wires
  `lean --server` (async-lsp), `protocol.rs` owns the document version
  counter and staleness filters, `messages/` translates LSP traffic into
  `TurnstileMessage`s emitted to the frontend on a single
  `turnstile-message` event.
- `src-tauri/src/assistant/` + `llm/` — the Proof Assistant: transcript,
  tool dispatch (read source/goals/prose/diagnostics, update prose), and
  the streaming Anthropic backend.
- `src-tauri/src/proof/` — the central `Proof` type (formal + prose +
  goal state) and the prose translator.
- `src-tauri/src/session/` — `.turn` session files (ZIP of source,
  prose, transcript, metadata), autosave and recovery.
- `src/lib/` — Svelte 5 frontend; `turnstile_messages.ts` is the single
  listener that fans messages out to stores.
- `docs/` — protocol spec and FSM diagrams.

Sessions are saved as `.turn` files via the File menu; settings
(models, prompts, font sizes) live under Settings (⌘,).
