#!/usr/bin/env python3
import pathlib
import shutil
import subprocess
import tempfile

from PIL import Image, ImageDraw


ROOT = pathlib.Path(__file__).resolve().parents[1]
ICON_ROOT = ROOT / "assets" / "app-icon"
SOURCE_SVG = ICON_ROOT / "snappyref.svg"


def draw_icon(size: int) -> Image.Image:
    scale = size / 128
    image = Image.new("RGBA", (size, size), "#0A0A0A")
    draw = ImageDraw.Draw(image)

    def rect(bounds, radius, fill):
        scaled = tuple(round(value * scale) for value in bounds)
        draw.rounded_rectangle(scaled, radius=round(radius * scale), fill=fill)

    rect((-100, 27, 36, 165), 8, "#E4CEB0")
    rect((-88, 39, 24, 153), 4, "#0A0A0A")
    rect((48, -78, 184, 60), 8, "#E4CEB0")
    rect((60, -66, 172, 48), 4, "#0A0A0A")
    rect((48, 72, 184, 210), 8, "#E4CEB0")
    rect((60, 84, 172, 198), 4, "#0A0A0A")
    return image


def main() -> None:
    ICON_ROOT.mkdir(parents=True, exist_ok=True)
    if not SOURCE_SVG.exists():
        raise SystemExit(f"missing source SVG: {SOURCE_SVG}")

    png_128 = draw_icon(128)
    png_128.save(ICON_ROOT / "snappyref-128.png")

    png_512 = draw_icon(512)
    png_512.save(ICON_ROOT / "snappyref-512.png")

    ico_sizes = [16, 24, 32, 48, 64, 128, 256]
    ico_images = [draw_icon(size) for size in ico_sizes]
    ico_images[-1].save(
        ICON_ROOT / "snappyref.ico",
        sizes=[(size, size) for size in ico_sizes],
        append_images=ico_images[:-1],
    )

    iconutil = shutil.which("iconutil")
    if iconutil is not None:
        with tempfile.TemporaryDirectory() as temp_dir:
            iconset = pathlib.Path(temp_dir) / "snappyref.iconset"
            iconset.mkdir()
            for size in [16, 32, 128, 256, 512]:
                draw_icon(size).save(iconset / f"icon_{size}x{size}.png")
                if size < 512:
                    draw_icon(size * 2).save(iconset / f"icon_{size}x{size}@2x.png")
            subprocess.run(
                [iconutil, "-c", "icns", str(iconset), "-o", str(ICON_ROOT / "snappyref.icns")],
                check=True,
            )


if __name__ == "__main__":
    main()
