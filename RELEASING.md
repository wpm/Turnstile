# Releasing Turnstile

This is the end-to-end runbook for cutting a Turnstile release. Followed top to
bottom, it should let a contributor who has never shipped before produce a
signed, notarized, public GitHub Release with working website download links.

A release starts in the GitHub UI. You create and publish a Release; publishing
triggers a workflow that builds the signed/notarized installers and uploads
them onto the Release you just published:

1. Bump the version on `main` and confirm CI is green.
2. **Create a new Release** in the GitHub UI (tag, notes, pre-release flag) and
   publish it.
3. Publishing triggers the release workflow, which builds the installer matrix
   and **uploads the assets onto that Release** a few minutes later.
4. Verify the published assets; the website's "latest download" links resolve
   against the published (non-prerelease) Release.

```
bump version (package.json) ─► green CI on main ─► create + publish Release in the UI
   ─► workflow builds & attaches installers (a few min later) ─► verify ─► done
```

> **The assets lag the publish.** Creating the Release makes it live
> immediately, but the download files don't appear until the build +
> notarization finish — usually 20–40 minutes later. During that window the
> Release page shows your notes with no binaries attached. This is expected,
> and it's the tradeoff for driving releases from the UI: a _draft_ Release
> doesn't trigger the build, so there's no way to attach binaries before
> publishing.

---

## Prerequisites

One-time setup. These rarely change between releases, but confirm them before
the _first_ release and whenever a secret or certificate might have rotated or
expired.

### Apple Developer ID

macOS builds are signed with a **Developer ID Application** certificate and
notarized through Apple so Gatekeeper accepts them with no right-click-open
workaround. The certificate must be a valid, unexpired _Developer ID
Application_ identity **with its private key** present in the login keychain:

```sh
security find-identity -v -p codesigning
# look for: Developer ID Application: <Name> (<TEAMID>)
```

We use the **Apple ID + app-specific password** notarization path (not the App
Store Connect API-key path). No entitlements file is required: the embedded
`lean --server` runs as a separately-executed child process and works under the
default hardened runtime (verified on a signed local build and on the notarized
release).

### GitHub Actions secrets

The release workflow reads six `APPLE_*` secrets, added under **Settings →
Secrets and variables → Actions** and matching these names exactly:

| Secret                       | Where it comes from                                           |
| ---------------------------- | ------------------------------------------------------------- |
| `APPLE_CERTIFICATE`          | base64 of the Developer ID Application `.p12` export          |
| `APPLE_CERTIFICATE_PASSWORD` | password chosen when exporting the `.p12`                     |
| `APPLE_SIGNING_IDENTITY`     | `Developer ID Application: <Name> (<TEAMID>)`, exactly        |
| `APPLE_ID`                   | Apple ID email of the developer account                       |
| `APPLE_PASSWORD`             | app-specific password for that Apple ID (not the account one) |
| `APPLE_TEAM_ID`              | 10-character team ID (also inside the identity string)        |

To produce them: export the Developer ID certificate **with its private key**
from Keychain Access as a `.p12` (the export password becomes
`APPLE_CERTIFICATE_PASSWORD`), then `base64 -i cert.p12` for `APPLE_CERTIFICATE`;
generate the app-specific password at account.apple.com → Sign-In and Security →
App-Specific Passwords.

> Never commit a secret, paste one into a PR, or leave one in shell history. Add
> them with `gh secret set <NAME> --repo wpm/Turnstile`, which prompts for the
> value rather than taking it on the command line.

### Updater signing key

Turnstile auto-updates via the Tauri updater (#50). The release workflow has
`bundle.createUpdaterArtifacts` enabled, so `tauri build` signs each update
bundle (macOS `.app.tar.gz`, Linux `.AppImage.tar.gz`) with a **minisign** key
and emits its `.sig`; the `attach` job assembles those into a signed
`latest.json` on the Release. This key is **separate from the Apple Developer
ID** and irrecoverable — losing or rotating it forces every install to
re-download once.

| Secret                      | Where it comes from                                                                   |
| --------------------------- | ------------------------------------------------------------------------------------- |
| `TAURI_SIGNING_PRIVATE_KEY` | full contents of `turnstile-updater.key` (`cargo tauri signer generate`, no password) |

The key has no password, so `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` is left empty
in the workflow. The matching public key is embedded in `tauri.conf.json` under
`plugins.updater.pubkey` for in-app verification. Because the key is required to
build updater artifacts, the main-only Linux bundle job (`linux-bundle.yml`)
reads it too. See #51 for generating and storing the key.

`CODECOV_TOKEN` is used by CI but is unrelated to releases.

### Repo settings

Confirm once:

- **Actions → General → Workflow permissions** allow the workflow to upload
  release assets (it declares `permissions: contents: write`).
- Actions is enabled.
- **Settings → Pages** source is the `gh-pages` branch — the marketing site
  (including the download links) deploys there.

---

## Pre-flight

Do all of this on `main` (or a PR that merges to `main`) _before_ creating the
Release.

### 1. Bump the version

The version is single-sourced from **`package.json`** — bump it there and
nowhere else:

```jsonc
// package.json
"version": "1.0.0"
```

`src-tauri/tauri.conf.json` reads the version from `package.json`
(`"version": "../package.json"`), and `src-tauri/Cargo.toml` carries a fixed
`0.0.0` placeholder (the app crate is never published, so its version is not the
app version). For a release candidate, keep the **base** version (`1.0.0`) here —
the `-rc.N` suffix lives only on the release tag, not in the source.
`src-tauri/Cargo.lock` updates on the next build; commit that too if it changes.

### 2. Update the CHANGELOG

Add or finalize the section for this version in `CHANGELOG.md`. The release
notes you paste into the GitHub Release are curated from this section.

### 3. Run the full gate locally — on a Mac

```sh
pnpm verify
```

This runs `test + check + lint + format:check + verify:rust + e2e +
lean-server`. **Run it on macOS:** `verify:rust` includes `cargo test`, and the
`lean-server` suite drives a real `lean --server` against Mathlib — the path
that must be green before releasing. The first run provisions the Lean toolchain

- Mathlib cache and is slow; later runs are fast.

### 4. Confirm green CI on `main`

The version bump + CHANGELOG must be merged to `main` and CI green there before
you create the Release, because the Release builds from that commit. On push to
`main`, CI also builds and headlessly launches the Linux packages and runs the
unsigned packaging check — those passing is your pre-release evidence that the
bundler config is sound.

---

## Cut the release

You create the Release in the GitHub UI; publishing it triggers the build.

1. Go to **Releases → Draft a new release**.
2. **Choose a tag:** type `v` + the version, with the RC suffix for candidates
   (`v1.0.0-rc.1`, or `v1.0.0` for the final). GitHub creates the tag when you
   publish. Set **Target** to the green, version-bumped commit on `main`.
3. **Title and notes:** paste the curated notes from `CHANGELOG.md`.
4. **Pre-release:** tick **Set as a pre-release** for `-rc.N` tags; leave it
   unchecked for the final.
5. **Publish release.**

Publishing fires the release workflow. It:

- Checks the tag's base version against `package.json` and **fails fast** if
  they disagree (so a typo'd tag can't ship mislabeled binaries).
- Builds the **macOS** universal bundle (`universal-apple-darwin`, Intel + Apple
  Silicon) → `.dmg`, signed with the Developer ID cert, then **notarized and
  stapled** via the `APPLE_*` secrets.
- Builds the **Linux** x86_64 bundles → `.deb`, `.rpm`, `.AppImage`.
- Generates `SHA256SUMS` and uploads everything onto the Release under stable,
  version-less names.

**The assets appear a few minutes after you publish.** Budget roughly 20–40
minutes; the macOS leg dominates (compiling both arch slices plus the
asynchronous notarization wait, which can run from a couple of minutes to ~20+).
Until it finishes, the published Release shows your notes with **no downloads
attached** — that's expected. Watch progress under the **Actions** tab.

Expected assets once the workflow completes:

- `Turnstile-macOS-universal.dmg`
- `Turnstile-linux-x86_64.deb`
- `Turnstile-linux-x86_64.rpm`
- `Turnstile-linux-x86_64.AppImage`
- `SHA256SUMS`

If a build leg fails, **re-run it from the Actions tab** — the Release already
exists, so the assets re-attach in place (no re-tagging, no duplicate releases).

---

## Verify the published release

Because the build runs _after_ you publish, verification happens on the live
Release. For a `-rc.N` **pre-release** that's the intended flow — RCs exist to be
tested. Validate the RC's binaries before you ever cut the final `v1.0.0`, so the
final ships the same, already-trusted artifacts.

1. **Review the Release:** correct tag, all five assets attached once the
   workflow finishes, notes render.
2. **Checksums:** download the assets and confirm they match
   (`shasum -a 256 -c SHA256SUMS`).
3. **macOS smoke test on real hardware** — Gatekeeper/notarization can only be
   verified on a real Mac:
   ```sh
   # install the .dmg to /Applications, then:
   spctl -a -vvv /Applications/Turnstile.app
   #   → accepted, source=Notarized Developer ID
   codesign --verify --deep --strict /Applications/Turnstile.app
   ```
   Launch the app; confirm first-launch Lean toolchain + Mathlib provisioning
   completes and a proof elaborates under the hardened runtime.
4. **Linux** is verified automatically in CI on push to `main` (build + `xvfb`
   headless launch), so it needs no manual hardware step. For a spot check,
   install the `.deb`/`.rpm` or run the `.AppImage` on a clean box.

If any check fails, treat it as a re-cut (see Rollback) rather than shipping the
final on top of it.

### Website download links

For a published **non-prerelease** Release, the marketing site's download links
resolve against it:

```
https://github.com/wpm/Turnstile/releases/latest/download/Turnstile-macOS-universal.dmg
https://github.com/wpm/Turnstile/releases/latest/download/Turnstile-linux-x86_64.deb
https://github.com/wpm/Turnstile/releases/latest/download/Turnstile-linux-x86_64.rpm
https://github.com/wpm/Turnstile/releases/latest/download/Turnstile-linux-x86_64.AppImage
https://github.com/wpm/Turnstile/releases/latest/download/SHA256SUMS
```

> `latest/download/` resolves to the newest **non-prerelease, non-draft**
> Release. While only `-rc.N` pre-releases exist, those `latest` links 404 —
> expected until the final `1.0.0` is published.

---

## Rollback / re-cut

If a published Release is bad (failed smoke test, wrong build):

1. **Delete the Release** from its page — this removes the Release and its
   uploaded assets, but **not** the underlying tag.
2. **Delete the tag**, locally and on the remote:
   ```sh
   git tag -d v1.0.0-rc.1
   git push origin :refs/tags/v1.0.0-rc.1
   ```
3. Land the fix on `main`, get CI green, then create a new Release. If the fix is
   purely in the release machinery, bump the RC (`v1.0.0-rc.2`) rather than
   reusing a tag anyone may have pulled.

If only a transient build/notarization step failed and the commit is fine, you
don't need to re-cut — just **re-run the failed run** from the Actions tab; it
re-attaches the assets to the existing Release.

---

## Versioning convention

- **Tag format:** `v` + semver, e.g. `v1.0.0`. The leading `v` is required. You
  set the tag when creating the Release; GitHub creates it on publish.
- **Source version:** `package.json` carries the base version (`1.0.0`) with
  **no** `-rc` suffix; the candidate suffix lives only on the tag. The workflow
  checks the tag's base version against `package.json` and fails fast on a
  mismatch.
- **Release candidates:** `vX.Y.Z-rc.N`, starting at `-rc.1` and incrementing
  `N` for each re-cut. Publish these as GitHub **pre-releases** so they don't
  become `latest`.
- **Final release:** once an RC is validated end to end, create `vX.Y.Z` (no
  suffix) from the same validated commit and publish it as a normal release. The
  final **supersedes** the RC: it becomes `latest`, and the website's
  `releases/latest/download/...` links resolve to it. The `-rc` pre-releases can
  be left for history or deleted.
- **Bump size** follows Conventional Commits, per `CLAUDE.md`: `fix` → patch,
  `feat` → minor, `!`/`BREAKING CHANGE` → breaking.
