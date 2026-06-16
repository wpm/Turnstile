# Lean LSP-contract tests

Theory says what the code _should_ do; these tests check it against reality.
This suite exercises one of Turnstile's external realities — the Lean LSP
server — on a real problem: proving the irrationality of √2 (no Mathlib,
infinite descent on naturals). The browser specs that drive the real UI live
alongside it in [`../browser/`](../browser/).

## The √2 scenario

`lean-lsp.mjs` drives a real `lean --server` over stdio exactly the way
the Rust backend does: didOpen a draft ending in `sorry`, read the goal
at the sorry with `$/lean/plainGoal`, "type" the completed descent
argument with an incremental didChange, and confirm clean
re-elaboration. Two consumers:

```sh
pnpm test:lean-server   # the assertions, as a vitest suite
node e2e/lean-server/lean-lsp-harness.mjs \
  --record e2e/lean-server/recordings/out.json
                        # ad-hoc CLI; records notification fixtures
```

The suite skips (loudly) when no working lean binary is found — it
probes `$TURNSTILE_LSP_CMD` then `~/.elan/bin/lean` with `--version`,
because an elan shim with no installed toolchain would hang rather than
fail. Each assertion names the backend contract it checks. Findings
from live runs that shaped the backend:

- `$/lean/fileProgress` carries its version **inside** `textDocument`
  (`VersionedTextDocumentIdentifier` with optional version), not at the
  top level. `lean::server::FileProgressParams` deserializes this shape.
- Processing ranges are end-exclusive, but they do **not** always end at
  character 0 — Lean reports mid-line end positions while elaborating.
  `parse_file_progress` therefore trims the final line only when the end
  character is 0.
- Elaboration completion is an **empty** `processing` array; the backend
  folds it into `TurnstileMessage::ElaborationDone` and the frontend
  clears the progress highlights on it.
- `$/lean/plainGoal` answers at the _start_ of a tactic token (character
  4 of `    sorry`), and may return nothing for positions inside the
  token — relevant to how goal positions are chosen.

## recordings/

Notification logs captured by `--record`: real Lean traffic, usable as
fixtures. `sqrt2-session.json` is the √2 session described above.

- Whether a progress range ends at character 0 (whole-line) or
  mid-line varies run to run — it is an observation, not a contract,
  and no test may assert a particular mix.
- Lean can publish the final diagnostics _after_ the empty fileProgress
  that signals elaboration done (observed ~1.5 s late), so consumers
  must wait for diagnostics, never assume they precede ElaborationDone.

## Browser specs

The Playwright suite in [`../browser/`](../browser/) drives the built UI
in a real browser against the fake backend (`src/lib/fake/`), which
simulates the Rust side's event contracts. Run with `pnpm test:e2e`.

## One command

`pnpm verify` runs the entire gate in order: unit tests, svelte-check,
eslint, prettier, `cargo fmt --check` + clippy (lint set in
`src-tauri/Cargo.toml [lints]`) + `cargo test`, the browser suite, and
this Lean LSP-contract suite.
