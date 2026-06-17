# Releasing Turnstile

This is the end-to-end runbook for cutting a Turnstile release. Followed top
to bottom, it should let a contributor who has never shipped before produce a
signed, notarized, public GitHub Release with working website download links.

A release is a tag, a workflow, and a few manual checks:

1. A version tag (`v*`) is pushed to GitHub.
2. The release workflow builds the signed/notarized installer matrix and
   publishes a **draft** Release with all assets attached.
3. A human verifies the draft on real hardware, then **un-drafts** it.
4. The website's "latest download" links resolve against the published
   Release.

```
bump versions ─► green CI on main ─► tag v1.0.0-rc.1 ─► release workflow
   ─► draft Release ─► smoke-test artifacts ─► publish (un-draft) ─► verify site
```

The work that builds this machinery is tracked under the
[1.0.0 release epic (#19)][epic]; individual steps cross-reference the issue
that owns them so you can dig into the rationale.

[epic]: https://github.com/wpm/Turnstile/issues/19

---

## Prerequisites

These are one-time setup items. They rarely change between releases, but
confirm them before the *first* release and whenever a secret or certificate
might have rotated/expired.

### Apple Developer ID

macOS builds are signed with a **Developer ID Application** certificate and
notarized through Apple so Gatekeeper accepts them with no right-click-open
workaround. See [#29] for the full verification procedure. In short, the
certificate must be a valid, unexpired *Developer ID Application* identity
**with its private key** present in the login keychain:

```sh
security find-identity -v -p codesigning
# look for: Developer ID Application: <Name> (<TEAMID>)
```

We use the **Apple ID + app-specific password** notarization path (not the
App Store Connect API-key path). Mathdoku needed no entitlements file; whether
Turnstile's embedded `lean --server` needs hardened-runtime exceptions is
tracked in [#22] and confirmed during the macOS smoke test ([#33]).

### GitHub Actions secrets

The release workflow reads six `APPLE_*` secrets. They are added under
**Settings → Secrets and variables → Actions** in `wpm/Turnstile` and must
match these names exactly. Full retrieval instructions (how to export the
`.p12`, generate the app-specific password, etc.) live in [#30].

| Secret                       | Where it comes from                                              |
| ---------------------------- | --------------------------------------------------------------- |
| `APPLE_CERTIFICATE`          | base64 of the Developer ID Application `.p12` export            |
| `APPLE_CERTIFICATE_PASSWORD` | password chosen when exporting the `.p12`                       |
| `APPLE_SIGNING_IDENTITY`     | `Developer ID Application: <Name> (<TEAMID>)`, exactly          |
| `APPLE_ID`                   | Apple ID email of the developer account                         |
| `APPLE_PASSWORD`             | app-specific password for that Apple ID (not the account one)   |
| `APPLE_TEAM_ID`              | 10-character team ID (also inside the identity string)          |

> Secrets must never pass through Claude, a PR, or shell history — only Bill
> can add them. Set them with `gh secret set <NAME> --repo wpm/Turnstile`,
> which prompts for the value rather than taking it on the command line.

Turnstile has **no auto-updater**, so Mathdoku's `TAURI_SIGNING_PRIVATE_KEY`
(updater/minisign) secret is *not* needed here. `CODECOV_TOKEN` is used by CI
but is unrelated to releases.

### Repo settings

Confirm once (details in [#31]):

- **Actions → General → Workflow permissions** allow the release workflow to
  create Releases and upload assets (the workflow declares
  `permissions: contents: write`).
- Actions is enabled and **tag-triggered** workflows are allowed.
- **Settings → Pages** source is the `gh-pages` branch — `pages.yml` deploys
  the marketing site (including the download links) there.

---

## Pre-flight

Do all of this on `main` (or a PR that merges to `main`) *before* tagging.

### 1. Bump the version in all three files

Turnstile keeps the version in three places that must agree. CI enforces this
([#25]), and the release workflow refuses to build if the tag disagrees with
the source version, so get it right here:

| File                         | Field             |
| ---------------------------- | ----------------- |
| `package.json`               | `version`         |
| `src-tauri/tauri.conf.json`  | `version`         |
| `src-tauri/Cargo.toml`       | `package.version` |

For a release candidate, set the **base** version (e.g. `1.0.0`) in these
files — the `-rc.N` suffix lives only on the git tag, not in the source. The
final release reuses the same `1.0.0` source version; only the tag changes
(`v1.0.0-rc.1` → `v1.0.0`).

After editing, `src-tauri/Cargo.lock` will update its `turnstile` entry on the
next build — commit that too.

### 2. Update the CHANGELOG

Add or finalize the section for this version in `CHANGELOG.md` (template and
scope guidance in [#27]). The release notes shown on the GitHub Release are
curated from this section for 1.0.0; later releases may switch to
auto-generated notes.

### 3. Run the full gate locally — on a Mac

```sh
pnpm verify
```

This runs `test + check + lint + format:check + verify:rust + e2e +
lean-server`. **Run it on macOS** ([#32]): `verify:rust` includes `cargo test`,
and the `lean-server` suite drives a real `lean --server` against Mathlib,
which is the path that must be green before tagging. First run provisions the
Lean toolchain + Mathlib cache and is slow; later runs are fast.

### 4. Confirm green CI on `main`

The version bump + CHANGELOG must be merged to `main` and the **CI** workflow
green there before you tag. The release builds from the tagged commit, so a
red `main` means a red release. On push to `main`, CI also builds and
headlessly launches the Linux packages ([#37]) and runs the unsigned
packaging check ([#21]) — those passing on `main` is your pre-tag evidence
that the bundler config is sound.

---

## Cut the release

Tag the release commit and push the tag. The tag name is `v` + the version,
with the RC suffix for candidates:

```sh
git checkout main && git pull          # be on the green, version-bumped commit
git tag v1.0.0-rc.1
git push origin v1.0.0-rc.1
```

Pushing a `v*` tag triggers the release workflow ([#20]). It:

- Builds the **macOS** universal bundle (`universal-apple-darwin`, Intel +
  Apple Silicon) → `.dmg`, signed with the Developer ID cert, then **notarized
  and stapled** via the `APPLE_*` secrets.
- Builds the **Linux** x86_64 bundles → `.deb`, `.rpm`, `.AppImage` (the
  Linux leg does not need and does not fail on missing Apple secrets).
- Renames assets to stable, version-less filenames and generates
  `SHA256SUMS` ([#24]).
- Creates a **draft** GitHub Release with all assets attached.

Expected assets on the draft:

- `Turnstile-macOS-universal.dmg`
- `Turnstile-linux-x86_64.deb`
- `Turnstile-linux-x86_64.rpm`
- `Turnstile-linux-x86_64.AppImage`
- `SHA256SUMS`

**Duration:** budget roughly 20–40 minutes. The macOS leg dominates —
compiling both arch slices plus the **notarization wait** (Apple's service is
asynchronous and can take anywhere from a couple of minutes to ~20+). Re-runs
on the same tag update the existing draft rather than duplicating assets, so
a failed leg can be re-run from the Actions tab without re-tagging.

---

## Verify the draft

Don't publish until the artifacts are confirmed good. This is [#34]'s
checklist; the platform-specific smoke tests are [#33] (macOS) and [#37]
(Linux).

1. **Review the draft Release** on GitHub: correct tag, all five assets
   present, release notes render.
2. **Checksums:** download the assets and confirm they match `SHA256SUMS`
   (`shasum -a 256 -c SHA256SUMS`).
3. **macOS smoke test on real hardware** ([#33]) — Gatekeeper/notarization
   can only be verified on a real Mac:
   ```sh
   # install the .dmg to /Applications, then:
   spctl -a -vvv /Applications/Turnstile.app
   #   → accepted, source=Notarized Developer ID
   codesign --verify --deep --strict /Applications/Turnstile.app
   ```
   Launch the app, confirm first-launch Lean toolchain + Mathlib provisioning
   completes and a proof elaborates under the hardened runtime.
4. **Linux** is verified automatically in CI on push to `main` ([#37], build +
   `xvfb` headless launch), so it needs no manual hardware step. If you want a
   spot check, install the `.deb`/`.rpm` or run the `.AppImage` on a clean box
   and confirm it launches and provisions Lean.

If any check fails, **do not publish** — fix it and re-cut (see Rollback).
File macOS signing/entitlement failures against [#22].

---

## Publish

Once the draft is verified:

1. On the GitHub Release page, **un-draft** (Edit → uncheck "Set as a
   draft" / "Publish release"). Mark it a **pre-release** for `-rc.N` tags;
   leave that unchecked for the final.
2. Publishing makes the `releases/latest/download/...` URLs resolve. Confirm
   the website's Releases section links work ([#26]) — they point at stable
   asset names on `github.com`, e.g.:

   ```
   https://github.com/wpm/Turnstile/releases/latest/download/Turnstile-macOS-universal.dmg
   https://github.com/wpm/Turnstile/releases/latest/download/Turnstile-linux-x86_64.deb
   https://github.com/wpm/Turnstile/releases/latest/download/Turnstile-linux-x86_64.rpm
   https://github.com/wpm/Turnstile/releases/latest/download/Turnstile-linux-x86_64.AppImage
   https://github.com/wpm/Turnstile/releases/latest/download/SHA256SUMS
   ```

   > `latest/download/` resolves to the newest **non-prerelease, non-draft**
   > Release. While only `-rc.N` pre-releases exist, those `latest` links will
   > 404 — that's expected until the final `1.0.0` is published. The website
   > notes this pre-release caveat.

---

## Rollback / re-cut

If a draft is bad, or a published RC needs to be replaced:

1. **Delete the GitHub Release** (the draft or pre-release) from its page —
   "Delete" removes the release and its uploaded assets.
2. **Delete the tag**, locally and on the remote:
   ```sh
   git tag -d v1.0.0-rc.1
   git push origin :refs/tags/v1.0.0-rc.1
   ```
3. Land the fix on `main`, get CI green, then **re-cut**. If the fix is purely
   in the release machinery (not the app), you can re-use the same version but
   bump the RC: tag `v1.0.0-rc.2`. If you must re-use the exact same tag name,
   delete it first (step 2) — never move a tag that anyone may have already
   pulled.

Re-running the workflow on an *existing* tag (Actions → release →
`workflow_dispatch`) is the lighter option when the tagged commit is fine and
only a transient build/notarization step failed — it updates the draft in
place.

---

## Versioning convention

- **Tag format:** `v` + semver, e.g. `v1.0.0`. The leading `v` is required —
  it's what the release workflow triggers on (`tags: ["v*"]`).
- **Source version:** the three version files carry the base version
  (`1.0.0`) with **no** `-rc` suffix. The candidate suffix lives only on the
  tag. The release workflow checks the tag's base version against the source
  version, so `v1.2.0` against a `1.0.0` tree fails fast ([#25]).
- **Release candidates:** `vX.Y.Z-rc.N`, starting at `-rc.1` and incrementing
  the `N` for each re-cut (`-rc.2`, …). Publish these as GitHub **pre-releases**
  so they don't become `latest`.
- **Final release:** when an RC is validated end to end, tag `vX.Y.Z` (no
  suffix) from the same/validated commit and publish it as a normal release.
  The final **supersedes** the RC: it becomes `latest`, and the website's
  `releases/latest/download/...` links resolve to it. The `-rc` pre-releases
  can be left in the Releases list for history or deleted.
- **Bump size** follows Conventional Commits, per `CLAUDE.md`: `fix` → patch,
  `feat` → minor, `!`/`BREAKING CHANGE` → breaking.

---

## Cross-references

| Area                                   | Issue |
| -------------------------------------- | ----- |
| Release epic / design decisions        | [#19] |
| Release workflow (`release.yml`)       | [#20] |
| Unsigned packaging check (push/PR)     | [#21] |
| macOS signing / hardened runtime       | [#22] |
| Linux bundle metadata                  | [#23] |
| Stable artifact names + `SHA256SUMS`   | [#24] |
| Version-consistency check in CI        | [#25] |
| Website download links                 | [#26] |
| CHANGELOG + release-notes template     | [#27] |
| Apple Developer ID / notarization path | [#29] |
| Signing/notarization secrets           | [#30] |
| Repo settings (Actions, Pages)         | [#31] |
| `pnpm verify` on Mac                   | [#32] |
| macOS installer smoke test             | [#33] |
| Tag + publish the Release              | [#34] |
| Linux verify in CI on `main`           | [#37] |

[#19]: https://github.com/wpm/Turnstile/issues/19
[#20]: https://github.com/wpm/Turnstile/issues/20
[#21]: https://github.com/wpm/Turnstile/issues/21
[#22]: https://github.com/wpm/Turnstile/issues/22
[#23]: https://github.com/wpm/Turnstile/issues/23
[#24]: https://github.com/wpm/Turnstile/issues/24
[#25]: https://github.com/wpm/Turnstile/issues/25
[#26]: https://github.com/wpm/Turnstile/issues/26
[#27]: https://github.com/wpm/Turnstile/issues/27
[#29]: https://github.com/wpm/Turnstile/issues/29
[#30]: https://github.com/wpm/Turnstile/issues/30
[#31]: https://github.com/wpm/Turnstile/issues/31
[#32]: https://github.com/wpm/Turnstile/issues/32
[#33]: https://github.com/wpm/Turnstile/issues/33
[#34]: https://github.com/wpm/Turnstile/issues/34
[#37]: https://github.com/wpm/Turnstile/issues/37
