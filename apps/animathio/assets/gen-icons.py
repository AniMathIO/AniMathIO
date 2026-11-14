#!/usr/bin/env python3
"""Regenerate the original AniMathIO application icons.

The sources are checked in (`assets/icons/*.png`, `icon.ico`, `icon.icns`) so a
packaging run never needs Python or Pillow; this script only documents how they
were produced and keeps them reproducible:

    python3 apps/animathio/assets/gen-icons.py

The canonical brand.svg embeds the preserved 1.x artwork at high resolution.
Read that source for every packaging format; keep the file names and sizes so
`[package.metadata.packager]` keeps working.
"""

from __future__ import annotations

import base64
import io
import pathlib
import xml.etree.ElementTree as ET

from PIL import Image

MASTER = 1024
OUT = pathlib.Path(__file__).resolve().parent / "icons"

# Sizes Debian/AppImage packagers map to `usr/share/icons/hicolor/<n>x<n>`.
PNG_SIZES = (32, 64, 128, 256, 512, 1024)
# Windows .ico: everything Windows Explorer picks between.
ICO_SIZES = (16, 24, 32, 48, 64, 128, 256)
# macOS .icns types accepted by both Pillow and cargo-packager.
ICNS_SIZES = (16, 32, 64, 128, 256, 512, 1024)


def render() -> Image.Image:
    """Read the preserved original artwork embedded in the canonical brand SVG."""
    root = ET.parse(OUT.parent / "brand.svg").getroot()
    artwork = root.find("{http://www.w3.org/2000/svg}image")
    assert artwork is not None, "brand.svg must contain the original artwork"
    uri = artwork.attrib["{http://www.w3.org/1999/xlink}href"]
    assert uri.startswith("data:image/png;base64,")
    image = Image.open(io.BytesIO(base64.b64decode(uri.split(",", 1)[1])))
    return image.convert("RGBA").resize((MASTER, MASTER), Image.Resampling.LANCZOS)


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
