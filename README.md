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
# development app
pnpm tauri dev
# release bundle
pnpm tauri build
```

## Testing

```sh
# the whole gate, in order (frontend, Rust, e2e)
pnpm verify

# frontend unit tests (vitest)
pnpm test
# svelte-check
pnpm check
# eslint
pnpm lint
# cargo fmt --check, clippy -D warnings, cargo test
pnpm verify:rust
# Playwright against the real UI + fake backend
pnpm test:e2e
# vitest suite driving a real lean --server (√2 proof)
pnpm test:lean-server
```

### Test types and launch points

Three kinds of test, two runners (vitest and Playwright), two config files.
There's no single launcher because Playwright is a separate test runner from
vitest; the two vitest types share one config via vitest **projects**.

| Type          | Files                           | Runner & config                                      | Script                  |
|---------------|---------------------------------|------------------------------------------------------|-------------------------|
| Unit          | `src/**/*.test.ts`              | vitest — `unit` project in `vitest.config.ts`        | `pnpm test`             |
| LSP contract  | `e2e/lean-server/**/*.test.mjs` | vitest — `lean-server` project in `vitest.config.ts` | `pnpm test:lean-server` |
| Browser (e2e) | `e2e/browser/**/*.spec.ts`      | Playwright — `playwright.config.ts`                  | `pnpm test:e2e`         |

- **`vitest.config.ts`** defines both vitest types as projects, selected with
  `--project unit` / `--project lean-server`. A bare `vitest run` (no
  `--project`) runs **both** — so it requires a Lean binary; the `pnpm test`
  script pins `--project unit` to keep the fast path Lean-free. The
  `lean-server` project carries the long timeouts (60 s / 300 s) the live
  server needs; `unit` does not.
- **`playwright.config.ts`** owns the browser suite: `testDir: e2e/browser`,
  and it starts its own dev server (`vite dev --mode e2e`).
- The three file globs are disjoint, which is what lets the runners coexist
  without picking up each other's specs.

The two suites under `e2e/` are the end-to-end layer (see
`e2e/lean-server/README.md`). `e2e/browser/` runs the unmodified frontend in
a browser against an in-browser simulation of the backend (`vite dev --mode
e2e` serves the same thing for manual poking); `e2e/lean-server/` checks the
protocol contracts the backend encodes against a live Lean server.

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
