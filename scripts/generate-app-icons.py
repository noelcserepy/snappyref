#!/usr/bin/env python3
import pathlib
import shutil
import subprocess
import tempfile

from io import BytesIO

import cairosvg
from PIL import Image


ROOT = pathlib.Path(__file__).resolve().parents[1]
ICON_ROOT = ROOT / "assets" / "app-icon"
SOURCE_SVG = ICON_ROOT / "snappyref.svg"


def render_icon(size: int) -> Image.Image:
    """Render the canonical SVG. Requires Pillow and CairoSVG for conversion only."""
    png = cairosvg.svg2png(url=str(SOURCE_SVG), output_width=size, output_height=size)
    return Image.open(BytesIO(png)).convert("RGBA")


def main() -> None:
    ICON_ROOT.mkdir(parents=True, exist_ok=True)
    if not SOURCE_SVG.exists():
        raise SystemExit(f"missing source SVG: {SOURCE_SVG}")

    png_128 = render_icon(128)
    png_128.save(ICON_ROOT / "snappyref-128.png")

    ico_sizes = [16, 24, 32, 48, 64, 128, 256]
    ico_images = [render_icon(size) for size in ico_sizes]
    ico_images[-1].save(
        ICON_ROOT / "snappyref.ico",
        sizes=[(size, size) for size in ico_sizes],
        append_images=ico_images[:-1],
    )

    iconutil = shutil.which("iconutil")
    if iconutil is None:
        raise SystemExit("iconutil is required to generate all packaging formats; run on macOS")
    if iconutil is not None:
        with tempfile.TemporaryDirectory() as temp_dir:
            iconset = pathlib.Path(temp_dir) / "snappyref.iconset"
            iconset.mkdir()
            for size in [16, 32, 128, 256, 512]:
                render_icon(size).save(iconset / f"icon_{size}x{size}.png")
                if size < 512:
                    render_icon(size * 2).save(iconset / f"icon_{size}x{size}@2x.png")
            subprocess.run(
                [iconutil, "-c", "icns", str(iconset), "-o", str(ICON_ROOT / "snappyref.icns")],
                check=True,
            )


if __name__ == "__main__":
    main()
