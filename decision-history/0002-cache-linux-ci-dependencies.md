# ADR-0002: Cache Linux CI system dependencies via a shared composite action

**Status:** Accepted
**Date:** 2026-06-18
**Deciders:** @wpm

## Context

Turnstile is a Tauri app, so every Linux CI job must install the WebKitGTK/GTK build
stack before it can compile or package. The same block appears in four workflows
(`ci.yml`, `release.yml`, `package-check.yml`, `linux-bundle.yml`):

```
sudo apt-get update
sudo apt-get install -y libwebkit2gtk-4.1-dev libgtk-3-dev libsoup-3.0-dev \
  libjavascriptcoregtk-4.1-dev librsvg2-dev libayatana-appindicator3-dev \
  libxdo-dev libssl-dev build-essential file wget curl
```

None of these packages are preinstalled on `ubuntu-latest`, and nothing is cached
between runs. Each run therefore pays the full `apt-get update` + download + unpack
cost (~1–2 min). The duplicated block is also a maintenance hazard: the package list
has to be kept in sync across four files by hand.

Forces at play:

- Install time is paid on every push/PR, so it dominates short jobs like the package check.
- The dependency list is stable and identical across workflows (modulo `xvfb` for the
  bundle job).
- This is a small project; we prefer low operational overhead over maximal speed.

## Decision

Introduce an in-repo composite action at `.github/actions/linux-deps/action.yml` that
installs and caches the dependency set using
[`awalsh128/cache-apt-pkgs-action`](https://github.com/awalsh128/cache-apt-pkgs-action),
and have all four workflows call it. The package list lives in one place; the bundle job
passes `xvfb` via an input.

On a cache hit the action restores the packages from the Actions cache and skips
`apt-get update` and the downloads, dropping the step from ~1–2 min to ~10–20s.

## Options Considered

### Option A: Cache apt packages via composite action (chosen)

| Dimension        | Assessment                                       |
| ---------------- | ------------------------------------------------ |
| Complexity       | Low — one composite action, swap four call sites |
| Cost             | Free (GitHub Actions cache)                      |
| Speed            | ~10–20s on cache hit                             |
| Team familiarity | High — plain Actions YAML                        |
| Maintenance      | Low — single package list                        |

**Pros:** Big speedup for minimal change; de-duplicates the package list; no external
infrastructure to own.
**Cons:** Relies on a third-party action; restores files without re-running package
post-install scripts (see Consequences).

### Option B: Prebuilt container image

| Dimension        | Assessment                                     |
| ---------------- | ---------------------------------------------- |
| Complexity       | Medium/High — build, publish, version an image |
| Cost             | ghcr storage; rebuild pipeline                 |
| Speed            | Fastest, fully deterministic                   |
| Team familiarity | Medium                                         |
| Maintenance      | Higher — own an image and its rebuild cadence  |

**Pros:** Fastest and most reproducible; no per-run apt at all.
**Cons:** New artifact to maintain and keep current; overkill for current CI volume.

### Option C: Status quo (inline apt-get in each workflow)

**Pros:** Nothing to change; no third-party dependency.
**Cons:** Slow on every run; four copies of the list to keep in sync.

## Trade-off Analysis

Option B is the fastest and most deterministic but adds a maintained artifact and a
rebuild pipeline that isn't justified at this project's CI volume. Option C keeps things
simple but leaves the recurring time cost and the four-way duplication in place. Option A
captures most of B's speed benefit with roughly C's level of effort, and additionally
fixes the duplication. The main thing we give up is some robustness: the caching action
restores files rather than performing a full install (see caveat below).

## Consequences

What becomes easier:

- Linux CI jobs finish noticeably faster on cache hits.
- The dependency list is maintained in exactly one place.

What becomes harder / needs care:

- **`cache-apt-pkgs-action` restores files but does not re-run package post-install
  scripts.** For these GTK/WebKit **dev** libraries (headers, `.so` files, pkg-config
  metadata) this is fine, because they don't register services or run meaningful setup
  hooks. This assumption must be re-checked if the list later grows to include a package
  that does real post-install work (e.g. registers a daemon, creates users, or compiles
  caches). If that happens, exclude that package from the cached set or revisit Option B.
- The cache is keyed on the package list; **bump the action's `version:` input to bust
  the cache** when the list changes, otherwise stale contents may be restored.
- We take on a third-party action as a CI dependency; pin it to a major version.

What we'll need to revisit:

- If CI volume grows or cache hit rates disappoint, reconsider Option B (prebuilt image).

## Action Items

1. [ ] Add `.github/actions/linux-deps/action.yml` wrapping `cache-apt-pkgs-action`.
2. [ ] Replace the inline install step in `ci.yml`, `release.yml`, `package-check.yml`,
       and `linux-bundle.yml` (pass `extra-packages: xvfb` for the bundle job).
3. [ ] Confirm a second run on the same package list shows a cache hit and a shorter
       install step.

## References

- Issue: wpm/Turnstile#69
- [awalsh128/cache-apt-pkgs-action](https://github.com/awalsh128/cache-apt-pkgs-action)
