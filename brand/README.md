# Turnstile brand

This directory is the single, authoritative home for Turnstile's visual
identity. The website and the app icons derive from what's here — if you're
touching anything brand-related, **start in this file**, then use the assets
and tokens it documents. Don't reinvent colors, fonts, or the mark elsewhere.

## Contents

| File            | What it is                                                         |
| --------------- | ------------------------------------------------------------------ |
| `turnstile.svg` | The primary mark — a turnstile (⊢) in a ring. **Master.**          |
| `tokens.css`    | Named CSS custom properties: palette, type scale, font stacks.     |
| `fonts/`        | IBM Plex Serif faces used to render the wordmark / social card.    |
| `generate.py`   | Regenerates every derived icon and favicon from the masters.       |
| `app-icon.png`  | Generated 1024² icon master fed to `tauri icon` (don't hand-edit). |
| `README.md`     | This document.                                                     |

## The mark

`turnstile.svg` is a purpose-drawn turnstile (⊢) enclosed in a ring, in an
even-weight, round-capped style. The turnstile is Turnstile: it's the symbol
that "separates what you have from what you must show," and it's the glyph the
app is named for.

The mark is **single-color**. It carries `fill="currentColor"` and no hardcoded
colors, so one file serves every context — set the surrounding text `color` and
the mark follows:

- **Black on white** — `color: var(--ts-ink)` on a `--ts-paper` ground.
- **White on black (dark mode)** — `color: #fff` on `--ts-slate-900`. Inverts
  cleanly; no separate dark asset.
- **Favicon, social card, dock icon** — the same file, recolored by context.

It is drawn on a 256×256 viewBox, optically centered, and holds its read down
to favicon sizes because the strokes are a single even weight.

### Clearspace & minimum size

- **Clearspace:** keep padding of at least **¼ of the mark's width** clear on
  all sides. Nothing — text, edges, other marks — intrudes into that margin.
- **Minimum size:** **16px** (favicon floor). Below that the ring muddies; use
  a solid square knockout instead.

### Do / don't

- **Do** recolor via `currentColor` (set `color`). **Don't** edit fills into
  the SVG.
- **Do** keep the ring and bars at their drawn even weight. **Don't** restroke,
  add a second color, add gradients, or add a drop shadow.
- **Do** preserve the aspect ratio and clearspace. **Don't** stretch, rotate,
  or crop the ring.
- **Don't** place the mark on a busy photographic background — it needs a flat
  ground for the ring to read.

## Palette

The palette mirrors the running app (`src/app.css`): a Tailwind **slate**
neutral ramp on paper white, with **blue-600** as the accent. Brand and app are
deliberately identical — the website should look like the app.

| Token                   | Light                 | Dark (`:root.dark`)   |
| ----------------------- | --------------------- | --------------------- |
| `--ts-color-bg`         | `#ffffff` (paper)     | `#0f172a` (slate-900) |
| `--ts-color-surface`    | `#f8fafc` (slate-50)  | `#1e293b` (slate-800) |
| `--ts-color-header-bg`  | `#f1f5f9` (slate-100) | `#1e293b` (slate-800) |
| `--ts-color-border`     | `#e2e8f0` (slate-200) | `#334155` (slate-700) |
| `--ts-color-text`       | `#0f172a` (slate-900) | `#f1f5f9` (slate-100) |
| `--ts-color-text-muted` | `#64748b` (slate-500) | `#94a3b8` (slate-400) |
| `--ts-accent`           | `#2563eb` (blue-600)  | `#2563eb` (blue-600)  |
| `--ts-accent-hover`     | `#1d4ed8` (blue-700)  | `#1d4ed8` (blue-700)  |

The full `--ts-slate-50…900` ramp is defined in `tokens.css` for any shade not
covered by a semantic token. These token **names** intentionally match the
app's `--color-*` variables, so the website can map them 1:1.

> **Accent sign-off:** blue-600 is the accent. It was the open taste decision;
> it's settled by adopting the color the app already uses, rather than
> introducing a new one. The dark mode keeps blue-600 (as the app does);
> blue-500 (`#3b82f6`) is noted in `tokens.css` as the fallback if contrast on
> slate-900 ever needs lifting.

## Type

Turnstile pairs formal proofs with prose, so the type pairing leans into that
duality — and uses the **IBM Plex** superfamily, one designed family whose
serif, sans, and mono share metrics. That alignment matters because prose and
Lean code sit in adjacent panes. All three are SIL OFL 1.1 (free to self-host);
available from Google Fonts.

| Role                      | Family         | Token             |
| ------------------------- | -------------- | ----------------- |
| Wordmark, headings, prose | IBM Plex Serif | `--ts-font-serif` |
| UI, labels, buttons       | IBM Plex Sans  | `--ts-font-sans`  |
| Lean code, monospace      | IBM Plex Mono  | `--ts-font-mono`  |

Type scale is a 1.250 (major third) ramp on a 16px base, exposed as
`--ts-text-xs … --ts-text-3xl`. Weights in use: 400 regular, 500 medium, 600
semibold (`--ts-weight-*`). Line heights: `--ts-leading-tight` for headings,
`--ts-leading-snug` for UI, `--ts-leading-prose` for body.

### Wordmark

The wordmark is the mark followed by **"Turnstile" in IBM Plex Serif SemiBold
(600)**, baseline-aligned, with a gap of roughly half the cap height between
them. There's no separate wordmark file yet — compose it from the mark plus
type. Add one here if a locked lockup is needed.

## Consuming this

- **Website (`#site-scaffold`):** import `tokens.css`, self-host the three IBM
  Plex families, use `turnstile.svg` for the logo, favicon, and social card.
- **App icons (`#app-icons`):** regenerate from `turnstile.svg` — see below.
  For platform icons that need a filled tile, knock the mark out of a
  `--ts-color-bg` / `--ts-slate-900` square; for maskable icons honor the ¼
  clearspace.

## Regenerating the derived assets

Every icon and favicon is **generated** from the masters in this directory —
none are hand-edited, so they can't drift from the mark. Regenerate them all
with one command from the repo root:

```sh
pnpm brand:assets
```

That runs `brand/generate.py`, which rasterizes `turnstile.svg` into a 1024×1024
`brand/app-icon.png`, feeds it to Tauri's generator (`pnpm tauri icon
brand/app-icon.png`) to rebuild the whole `src-tauri/icons/*` set, and emits the
website favicon set and social card into `static/`. Re-run it whenever
`turnstile.svg`, the palette, or the icon recipe changes, then commit the
results.

### What it emits

| Output                        | What it is                                                                |
| ----------------------------- | ------------------------------------------------------------------------- |
| `brand/app-icon.png`          | 1024² master fed to `tauri icon` (white mark on the blue-600 accent tile) |
| `src-tauri/icons/*`           | Full Tauri desktop + Windows Store icon set                               |
| `static/favicon.svg`          | Vector favicon; ink in light mode, slate-100 in dark                      |
| `static/favicon.ico`          | Multi-size `.ico` (16/32/48)                                              |
| `static/favicon-96x96.png`    | Modern PNG favicon                                                        |
| `static/apple-touch-icon.png` | 180² opaque iOS home-screen tile                                          |
| `static/og-image.png`         | 1200×630 Open Graph / social card                                         |

Wiring these into the site's `<head>` is the site-scaffold's job, not this one.

### Prerequisites

- Node deps installed (`pnpm install`) — provides `tauri icon`.
- The Python toolchain the script imports:

  ```sh
  pip install cairosvg Pillow
  ```

- `brand/fonts/IBMPlexSerif-{SemiBold,Regular}.ttf` — the wordmark faces used to
  render the social card, vendored here so generation is offline-reproducible.

### The app-icon recipe

The desktop/store icons are the mark knocked out **white** of a **blue-600
(`--ts-accent`) rounded-square tile** — the most brand-identifying, highest-
contrast pairing, and it holds its read down to taskbar/dock/store-tile sizes.
To switch to a slate-900 or paper tile instead, change `ICON_TILE` / `ICON_MARK`
at the top of `generate.py` and re-run.
