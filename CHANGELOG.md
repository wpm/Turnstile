# Changelog

## 1.0.0 — 2026-06-05

First stable release.

### Fixed

- Processing highlights no longer flash on the wrong lines. Three
  cooperating defects: `$/lean/fileProgress` staleness versions were
  read from the wrong field (Lean nests them in `textDocument`), the
  document version counter used for staleness filtering was never
  advanced by editor edits, and the frontend never cleared highlights
  when elaboration finished.
- Processing ranges are treated as end-exclusive: a range ending at
  character 0 no longer highlights one line too many.
- The LSP document now opens with the session's source when a session
  is restored before the server comes up, instead of opening empty.
- The assistant system prompt described a `read_tactic_state` tool that
  did not exist; it now documents the real `read_goal_state` tool.

### Added

- The Proof Assistant chat is wired to the backend: transcript loading,
  streaming replies with a thinking indicator, Markdown + KaTeX
  rendering, and error bubbles.
- The File menu works: New/Open/Save/Save As, autosave every 30s, and
  an autosave-recovery prompt on launch.
- A Settings dialog (⌘,): assistant and translation models, font sizes,
  and system-prompt overrides.
- View → Toggle Word Wrap.
- A status bar showing Lean server state and first-run setup progress.
- A fake backend (`vite dev --mode e2e`) that runs the full UI in a
  plain browser, a Playwright end-to-end suite over it, and a Lean
  LSP-contract suite that drives a real `lean --server` through an
  irrationality proof of √2.

### Changed

- Version 1.0.0 across package.json, Cargo.toml, and tauri.conf.json;
  product name is now "Turnstile".
