# ADR-0001: Proof Assistant configuration and failure surfacing

**Status:** Accepted
**Date:** 2026-06-18
**Deciders:** W.P. McNeill
**Epic:** [#56](https://github.com/wpm/Turnstile/issues/56)

## Context

The Proof Assistant talks to an Anthropic LLM backend (`AnthropicBackend`) constructed at startup from `ANTHROPIC_API_KEY`, read from the process environment. When the key could not be found, `setup()` silently fell back to a mock that echoes the user's message:

```rust
// src-tauri/src/lib.rs
match llm::AnthropicBackend::from_env() {
    Ok(b) => Arc::new(b),
    Err(_) => Arc::new(llm::MockBackend::echo()),
}
```

The status bar only reported Lean ("Lean: connected"), so a dead assistant produced `[echo] …` replies with no signal that anything was wrong.

The root cause is environmental, not a coding mistake: a Tauri app launched from Finder/Dock/Spotlight inherits launchd's minimal environment, **not** the user's interactive shell. Environment variables are per-process and inherited parent→child; a variable `export`ed in `.zshrc` lives only in shells that sourced it and their children. A GUI launch is a child of launchd, which never ran the shell, so `std::env::var("ANTHROPIC_API_KEY")` returns nothing — even when the key is set in the user's shell. The shipping app is almost always launched this way, so the silent fallback was the *normal* path, not an edge case. The same limitation already forced hardcoded `PATH` fallbacks for locating Lean tooling (`setup.rs`).

Forces at play: the app is cross-platform (macOS, Windows, Linux); the API key is a secret and must be protected at rest and never logged; the user should not have to edit shell config or perform launchd surgery; and a missing key must be a recoverable, clearly-communicated state rather than a silent degradation.

## Decision

1. **No mock, ever.** Remove the `mock-llm` Cargo feature and the entire `MockBackend`. The assistant is either connected to a real backend or explicitly *disconnected*; it never echoes and never fabricates a turn.
2. **Key from the OS secret store.** The API key is a setting persisted in the platform keychain (macOS Keychain / Windows Credential Manager / Linux Secret Service) via the `keyring` crate — encrypted at rest, never written to `settings.json`, never logged or serialized. A process `ANTHROPIC_API_KEY` env var is honored only as an optional dev/CI override.
3. **First-run onboarding.** On first launch with no stored key, a modal collects the key and the model choices and persists them, so the user is never bitten by a missing key.
4. **Explicit failure surface.** A disconnected assistant is shown three ways: a red, right-justified "Proof Assistant: disconnected" status indicator; a dismissible error toast that names the cause and the fix; and a disabled chat input.
5. **Fix PATH too.** Adopt `fix-path-env` so GUI launches get the user's shell `PATH`, removing the hardcoded fallback and heading off the same class of bug for Lean tooling.

## Options Considered

The contested decision was **how a Finder-launched app should obtain the key**. Failure-handling (silent mock vs. visible disconnected state) was decided in favor of visibility and is not re-litigated below.

### Option A: Shell environment variable (status quo)

| Dimension | Assessment |
|-----------|------------|
| Complexity | Low |
| Security at rest | N/A (not stored by the app) |
| Cross-platform | Uniform, but uniformly broken for GUI launches |
| User effort | Edit shell config + relaunch |

**Pros:** Trivial; parity with CLI tooling; works from a terminal launch and in CI.
**Cons:** Invisible to Finder/Dock launches — this *is* the bug. No recovery path inside the app.

### Option B: Login-shell environment capture

Spawn the user's login shell (`$SHELL -ilc 'printf %s "$ANTHROPIC_API_KEY"'`) so it sources rc files, then read the value — the technique `fix-path-env` uses for `PATH`.

| Dimension | Assessment |
|-----------|------------|
| Complexity | Medium (shell detection, timeouts, parsing) |
| Security at rest | N/A |
| Cross-platform | Fragile: zsh/bash via `-ilc` work; fish/nushell need bespoke handling |
| User effort | None if the var is exported in a sourced file |

**Pros:** Honors "from the environment" literally; no extra UI.
**Cons:** Shell-specific; adds a shell spawn at startup; only works if the var is exported in a file the login shell sources; the off-the-shelf Tauri crate carries `PATH` only, so the key path would be hand-rolled.

### Option C: OS keychain via the `keyring` crate (chosen)

| Dimension | Assessment |
|-----------|------------|
| Complexity | Medium (onboarding UI + secret hygiene) |
| Security at rest | Strong — OS-encrypted, keyed to the user login |
| Cross-platform | One API over Keychain / Credential Manager / Secret Service |
| User effort | Enter key once at first run |

**Pros:** Encryption at rest for free; no shell dependence; in-app recovery path (Settings); standard desktop pattern.
**Cons:** Requires onboarding + a Settings field; a Linux host with no Secret Service has no encrypted store; imposes a no-log / no-serialize discipline on the secret.

### Option D: Stronghold vault (`tauri-plugin-stronghold`)

**Pros:** Encrypted vault.
**Cons:** Needs an unlock password that then has to be stashed somewhere (often the keychain anyway); wrong shape for a single token; slated for removal in Tauri v3.

## Trade-off Analysis

Only Option C delivers encryption-at-rest **and** a non-shell, cross-platform, recoverable path. Options A and B both make the key's availability depend on launch context and shell configuration — exactly the fragility that produced the original bug — and neither protects the secret at rest. Option B's underlying technique is genuinely useful, but the maintained tooling only carries `PATH`; hand-rolling shell parsing for one secret is not worth the fragility, so we adopt `fix-path-env` for `PATH` specifically and keychain for the key. Option D is heavier than a single token warrants and is on a deprecation path.

The env var is retained as an optional override so terminal and CI runs keep working, but the keychain is the source of truth for the shipping app. The cost we accept is a no-encrypted-store gap on minimal Linux, handled by an explicit degraded path (require the env var or a clearly-labeled fallback — never a silent plaintext write).

## Consequences

- **Easier:** assistant failures are visible and actionable; the key is stored securely and never echoed back to the user as `[echo]`; `PATH` is correct for Lean tooling under GUI launches.
- **Harder:** new first-run modal and a Settings restructure (tabbed: Models / Font sizes / Prompts, with the key field in Models); a secret-hygiene burden (redacting `Debug`/`Display`, excluding the key from serialization and logs, plus a guard test); platform-specific keychain handling and a defined Linux-without-Secret-Service degraded path.
- **To revisit:** whether an existing Anthropic credential (e.g. from the Claude CLI) can be discovered to skip onboarding; the exact Linux fallback behavior; whether to keep the env override long-term.

## Action Items

Tracked under epic [#56](https://github.com/wpm/Turnstile/issues/56):

1. [ ] #57 — Remove `mock-llm` feature and `MockBackend`; eliminate the echo fallback
2. [ ] #63 — Keychain API-key storage (encrypted at rest, never logged)
3. [ ] #58 — Backend assistant connection status (the silent-swallow fix)
4. [ ] #59 — Right-justified "Proof Assistant" status indicator
5. [ ] #60 — Error toast naming the cause + disabled chat input
6. [ ] #64 — First-run onboarding modal (key + model choices)
7. [ ] #66 — Split Settings into Models / Font sizes / Prompts tabs
8. [ ] #65 — Adopt `fix-path-env` to fix `PATH` for GUI launches
9. [ ] #61 — Audit other swallowed errors for toast-worthiness
10. [ ] #62 — Update browser fake + e2e (no echo)
