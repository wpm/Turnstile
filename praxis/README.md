# Praxis

Theory says what the code *should* do; praxis checks it against reality.
This directory holds harnesses that exercise Turnstile's two external
realities — the Lean LSP server and a browser driving the real UI — on a
real problem: proving the irrationality of √2 (no Mathlib, infinite
descent on naturals).

## lean-lsp-harness.mjs

Drives a real `lean --server` over stdio exactly the way the Rust backend
does: didOpen a draft ending in `sorry`, read the goal at the sorry with
`$/lean/plainGoal`, "type" the completed descent argument with an
incremental didChange, and confirm clean re-elaboration.

```sh
TURNSTILE_LSP_CMD=~/.elan/bin/lean node praxis/lean-lsp-harness.mjs \
  [--record praxis/recordings/out.json]
```

Each assertion maps to a backend contract (see the file header). Findings
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
- `$/lean/plainGoal` answers at the *start* of a tactic token (character
  4 of `    sorry`), and may return nothing for positions inside the
  token — relevant to how goal positions are chosen.

## recordings/

Notification logs captured by `--record`: real Lean traffic, usable as
fixtures. `sqrt2-session.json` is the √2 session described above.

## Frontend praxis

The Playwright suite in `e2e/` drives the built UI in a real browser
against the fake backend (`src/lib/fake/`), which simulates the Rust
side's event contracts. Run with `pnpm test:e2e`.
