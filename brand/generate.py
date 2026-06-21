#!/usr/bin/env python3
"""Regenerate every derived brand asset from the masters in this directory.

Single source of truth:
  - brand/turnstile.svg  — the mark (single-color, `currentColor`)
  - brand/tokens.css     — the palette (mirrored as PALETTE below)
  - brand/fonts/*.ttf     — IBM Plex Serif used for the wordmark / social card

What this script emits (nothing here is hand-edited — always regenerate):
  - brand/app-icon.png            high-res master fed to `tauri icon`
  - src-tauri/icons/*             full Tauri desktop/store icon set
  - static/favicon.svg            adaptive (light/dark) favicon, vector
  - static/favicon.ico            multi-size .ico (16/32/48)
  - static/favicon-96x96.png      modern PNG favicon
  - static/apple-touch-icon.png   opaque iOS home-screen tile
  - static/og-image.png           1200x630 Open Graph / social card

Run it with the one-liner documented in brand/README.md.
"""

from __future__ import annotations

import io
import shutil
import subprocess
import sys
from pathlib import Path

try:
    import cairosvg
    from PIL import Image, ImageDraw, ImageFont
except ImportError as exc:  # pragma: no cover - guidance only
    sys.exit(
        f"Missing dependency: {exc.name}.\n"
        "Install the generator toolchain with:\n"
        "    pip install cairosvg Pillow\n"
    )

BRAND = Path(__file__).resolve().parent
ROOT = BRAND.parent
ICONS_OUT = ROOT / "src-tauri" / "icons"
STATIC = ROOT / "static"
FONTS = BRAND / "fonts"

MASTER_SVG = BRAND / "turnstile.svg"
APP_ICON = BRAND / "app-icon.png"

# Mirrors brand/tokens.css — keep in sync with the semantic tokens there.
PALETTE = {
    "paper": "#ffffff",
    "ink": "#0f172a",        # slate-900
    "slate-100": "#f1f5f9",
    "slate-500": "#64748b",
    "slate-900": "#0f172a",
    "accent": "#2563eb",     # blue-600 — the brand accent
}

# --- App-icon recipe -------------------------------------------------------
# Per brand/README.md "Consuming this": for platform icons that need a filled
# tile, knock the mark out of a brand square. We use a paper (white) tile with
# the ink (black) mark — matching the website's light-mode rendering, the
# highest-contrast pairing, and it holds its read down to taskbar / dock /
# store-tile sizes.
ICON_TILE = PALETTE["paper"]
ICON_MARK = PALETTE["ink"]
ICON_SIZE = 1024
ICON_RADIUS = round(0.2237 * ICON_SIZE)  # iOS-style rounded-square corner
ICON_MARK_SCALE = 0.66  # mark box as a fraction of the tile (leaves clearspace)


def render_mark(color: str, size: int) -> Image.Image:
    """Rasterize turnstile.svg at `size`px square, recolored to `color`."""
    svg = MASTER_SVG.read_text(encoding="utf-8").replace(
        'fill="currentColor"', f'fill="{color}"'
    )
    png = cairosvg.svg2png(
        bytestring=svg.encode("utf-8"),
        output_width=size,
        output_height=size,
    )
    return Image.open(io.BytesIO(png)).convert("RGBA")


def rounded_tile(size: int, radius: int, color: str) -> Image.Image:
    """An opaque rounded-square tile on a transparent canvas."""
    tile = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    draw = ImageDraw.Draw(tile)
    draw.rounded_rectangle([0, 0, size - 1, size - 1], radius=radius, fill=color)
    return tile


def make_tile_icon(size: int, *, rounded: bool) -> Image.Image:
    """Mark knocked out of a filled brand tile (rounded for app icons)."""
    if rounded:
        radius = round(0.2237 * size)
        img = rounded_tile(size, radius, ICON_TILE)
    else:
        img = Image.new("RGBA", (size, size), ICON_TILE)
    mark_px = round(size * ICON_MARK_SCALE)
    mark = render_mark(ICON_MARK, mark_px)
    offset = (size - mark_px) // 2
    img.alpha_composite(mark, (offset, offset))
    return img


def build_app_icon() -> None:
    icon = make_tile_icon(ICON_SIZE, rounded=True)
    icon.save(APP_ICON)
    print(f"  brand/{APP_ICON.name}  ({ICON_SIZE}x{ICON_SIZE})")


def run_tauri_icon() -> None:
    """Feed the master into Tauri's generator: `tauri icon <source>`."""
    cmd = ["pnpm", "exec", "tauri", "icon", str(APP_ICON), "-o", str(ICONS_OUT)]
    print("  $ " + " ".join(cmd))
    try:
        subprocess.run(cmd, cwd=ROOT, check=True)
    except FileNotFoundError:
        sys.exit("pnpm not found — run `pnpm install` first.")

    # Turnstile ships desktop-only (see tauri.conf.json bundle targets), so drop
    # the mobile sets `tauri icon` also emits — they'd be unused binaries.
    for mobile in ("android", "ios"):
        shutil.rmtree(ICONS_OUT / mobile, ignore_errors=True)


def build_favicon_svg() -> None:
    """Adaptive vector favicon: ink in light mode, slate-100 in dark mode.

    Built from the master's own shapes so it can never drift from the mark.
    """
    src = MASTER_SVG.read_text(encoding="utf-8")
    # Pull the drawn shapes (everything between the first <path and </svg>),
    # dropping <title>/<desc>, and let CSS drive the fill instead of currentColor.
    start = src.index("<path")
    end = src.rindex("</svg>")
    shapes = src[start:end].strip()
    light, dark = PALETTE["ink"], PALETTE["slate-100"]
    svg = (
        '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 256 256" '
        'role="img" aria-label="Turnstile">\n'
        "  <style>\n"
        f"    :root {{ fill: {light}; }}\n"
        f"    @media (prefers-color-scheme: dark) {{ :root {{ fill: {dark}; }} }}\n"
        "  </style>\n"
        f"  {shapes}\n"
        "</svg>\n"
    )
    (STATIC / "favicon.svg").write_text(svg, encoding="utf-8")
    print("  static/favicon.svg  (adaptive light/dark)")


def build_favicons() -> None:
    # Transparent ink mark, rendered large then downsampled for crisp small sizes.
    base = render_mark(PALETTE["ink"], 256)

    ico_sizes = [(16, 16), (32, 32), (48, 48)]
    base.save(STATIC / "favicon.ico", sizes=ico_sizes)
    print("  static/favicon.ico  (16/32/48)")

    base.resize((96, 96), Image.LANCZOS).save(STATIC / "favicon-96x96.png")
    print("  static/favicon-96x96.png")

    # Apple touch icon: opaque tile (iOS ignores transparency).
    make_tile_icon(180, rounded=False).save(STATIC / "apple-touch-icon.png")
    print("  static/apple-touch-icon.png  (180x180)")


def _font(name: str, size: int) -> ImageFont.FreeTypeFont:
    return ImageFont.truetype(str(FONTS / name), size)


def build_social_card() -> None:
    """1200x630 Open Graph card: mark + IBM Plex Serif wordmark + tagline."""
    W, H = 1200, 630
    card = Image.new("RGBA", (W, H), PALETTE["paper"])
    draw = ImageDraw.Draw(card)

    # Mark in the accent, left-aligned within a generous margin.
    mark_px = 300
    mark = render_mark(PALETTE["accent"], mark_px)
    mark_x, mark_y = 110, (H - mark_px) // 2
    card.alpha_composite(mark, (mark_x, mark_y))

    text_x = mark_x + mark_px + 70
    wordmark = _font("IBMPlexSerif-SemiBold.ttf", 132)
    tagline = _font("IBMPlexSerif-Regular.ttf", 40)

    # Vertically center the wordmark + tagline block against the mark.
    word = "Turnstile"
    line = "Formal proofs, paired with prose."
    w_box = draw.textbbox((0, 0), word, font=wordmark)
    t_box = draw.textbbox((0, 0), line, font=tagline)
    w_h = w_box[3] - w_box[1]
    t_h = t_box[3] - t_box[1]
    gap = 28
    block_h = w_h + gap + t_h
    top = (H - block_h) // 2

    draw.text((text_x, top - w_box[1]), word, font=wordmark, fill=PALETTE["ink"])
    draw.text(
        (text_x, top + w_h + gap - t_box[1]),
        line,
        font=tagline,
        fill=PALETTE["slate-500"],
    )

    card.convert("RGB").save(STATIC / "og-image.png")
    print("  static/og-image.png  (1200x630)")


def main() -> None:
    STATIC.mkdir(exist_ok=True)
    ICONS_OUT.mkdir(parents=True, exist_ok=True)

    print("Brand master -> app-icon master:")
    build_app_icon()

    print("Tauri desktop/store icon set:")
    run_tauri_icon()

    print("Website favicons + social card:")
    build_favicon_svg()
    build_favicons()
    build_social_card()

    print("\nDone. Visually verify the rendered icons before committing.")


if __name__ == "__main__":
    main()
