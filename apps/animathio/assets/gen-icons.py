#!/usr/bin/env python3
"""Regenerate the placeholder AniMathIO application icons.

The sources are checked in (`assets/icons/*.png`, `icon.ico`, `icon.icns`) so a
packaging run never needs Python or Pillow; this script only documents how they
were produced and keeps them reproducible:

    python3 apps/animathio/assets/gen-icons.py

The artwork is a placeholder: a rounded indigo/sky gradient tile with a white
summation glyph. Replace it with the real brand asset before 2.0 ships; keep the
file names and sizes so `[package.metadata.packager]` keeps working.
"""

from __future__ import annotations

import pathlib

from PIL import Image, ImageDraw

MASTER = 1024
OUT = pathlib.Path(__file__).resolve().parent / "icons"

# Sizes Debian/AppImage packagers map to `usr/share/icons/hicolor/<n>x<n>`.
PNG_SIZES = (32, 64, 128, 256, 512, 1024)
# Windows .ico: everything Windows Explorer picks between.
ICO_SIZES = (16, 24, 32, 48, 64, 128, 256)
# macOS .icns types accepted by both Pillow and cargo-packager.
ICNS_SIZES = (16, 32, 64, 128, 256, 512, 1024)

TOP = (79, 70, 229)  # indigo-600
BOTTOM = (14, 165, 233)  # sky-500
RADIUS = 0.22  # corner radius as a fraction of the tile

# Sigma polyline in unit coordinates: top bar right to left, diagonal in to the
# waist, diagonal back out, bottom bar left to right.
SIGMA = ((0.71, 0.27), (0.29, 0.27), (0.55, 0.50), (0.29, 0.73), (0.71, 0.73))
STROKE = 0.085  # stroke width as a fraction of the tile


def render() -> Image.Image:
    """Draw the icon at 4x the master size, then downsample for smooth edges."""
    scale = 4
    size = MASTER * scale
    # Vertical gradient.
    gradient = Image.new("RGB", (1, size))
    for y in range(size):
        t = y / (size - 1)
        gradient.putpixel(
            (0, y),
            tuple(round(a + (b - a) * t) for a, b in zip(TOP, BOTTOM)),
        )
    tile = gradient.resize((size, size), Image.Resampling.NEAREST).convert("RGBA")

    # Rounded corners via a mask.
    mask = Image.new("L", (size, size), 0)
    ImageDraw.Draw(mask).rounded_rectangle(
        (0, 0, size - 1, size - 1), radius=round(RADIUS * size), fill=255
    )
    icon = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    icon.paste(tile, (0, 0), mask)

    # Summation glyph, with round caps (PIL has no cap style, so draw circles).
    draw = ImageDraw.Draw(icon)
    width = round(STROKE * size)
    points = [(round(x * size), round(y * size)) for x, y in SIGMA]
    draw.line(points, fill=(255, 255, 255, 255), width=width, joint="curve")
    for x, y in points:
        draw.ellipse(
            (x - width // 2, y - width // 2, x + width // 2, y + width // 2),
            fill=(255, 255, 255, 255),
        )

    return icon.resize((MASTER, MASTER), Image.Resampling.LANCZOS)


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    master = render()
    master.save(OUT / "icon.png")
    for size in PNG_SIZES:
        master.resize((size, size), Image.Resampling.LANCZOS).save(
            OUT / f"{size}x{size}.png"
        )

    frames = [
        master.resize((size, size), Image.Resampling.LANCZOS) for size in ICO_SIZES
    ]
    frames[-1].save(
        OUT / "icon.ico",
        format="ICO",
        sizes=[(size, size) for size in ICO_SIZES],
        append_images=frames[:-1],
    )
    master.save(
        OUT / "icon.icns",
        format="ICNS",
        sizes=[(size, size) for size in ICNS_SIZES],
    )
    for path in sorted(OUT.iterdir()):
        print(f"wrote {path.relative_to(OUT.parent.parent)} ({path.stat().st_size} B)")


if __name__ == "__main__":
    main()
