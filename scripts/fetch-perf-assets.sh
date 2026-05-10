#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
manifest=${1:-"$repo_root/perf-assets/manifest.json"}
target_root=${SNAPPYREF_PERF_ASSET_DIR:-"$repo_root/perf-assets/images"}

python3 - "$manifest" "$target_root" <<'PY'
import json
import pathlib
import subprocess
import sys
import time

manifest_path = pathlib.Path(sys.argv[1])
target_root = pathlib.Path(sys.argv[2])
assets = json.loads(manifest_path.read_text())
target_root.mkdir(parents=True, exist_ok=True)

def derive(asset, target):
    spec = asset["derive"]
    operation = spec["operation"]
    if operation == "svg-basic":
        target.write_text(
            """<svg xmlns="http://www.w3.org/2000/svg" width="960" height="640" viewBox="0 0 960 640">
  <rect width="960" height="640" fill="#f8fafc"/>
  <path d="M120 510 L310 130 L500 510 Z" fill="#2563eb"/>
  <circle cx="650" cy="300" r="170" fill="#16a34a"/>
  <path d="M565 300 H735 M650 215 V385" stroke="#f8fafc" stroke-width="54" stroke-linecap="round"/>
</svg>
""",
            encoding="utf-8",
        )
        return

    try:
        from PIL import Image, ImageSequence
    except Exception as error:
        raise SystemExit(f"Pillow is required for derived perf assets: {error}") from error

    source = target_root / spec["from"]
    if not source.exists():
        raise SystemExit(f"cannot derive {asset['file']}: missing source {spec['from']}")

    if operation in {"webp-lossy", "webp-lossless", "tiff-lzw"}:
        image = Image.open(source)
        image.load()
        max_side = spec.get("max_side")
        if max_side:
            image.thumbnail((max_side, max_side), Image.Resampling.LANCZOS)
        if operation == "webp-lossy":
            image.save(target, "WEBP", quality=spec.get("quality", 80), method=6)
        elif operation == "webp-lossless":
            image.save(target, "WEBP", lossless=True, method=6)
        else:
            image.save(target, "TIFF", compression="tiff_lzw")
        return

    if operation == "gif-animated":
        image = Image.open(source)
        frames = []
        durations = []
        for frame in ImageSequence.Iterator(image):
            frames.append(frame.convert("P", palette=Image.Palette.ADAPTIVE))
            durations.append(frame.info.get("duration", 80))
        if not frames:
            raise SystemExit(f"cannot derive {asset['file']}: source has no frames")
        frames[0].save(
            target,
            "GIF",
            save_all=True,
            append_images=frames[1:],
            duration=durations,
            loop=0,
            optimize=False,
        )
        return

    raise SystemExit(f"unknown perf asset derive operation: {operation}")

for asset in assets:
    rel = pathlib.Path(asset["file"])
    target = target_root / rel
    target.parent.mkdir(parents=True, exist_ok=True)
    if target.exists() and target.stat().st_size > 0:
        print(f"exists {rel}")
        continue

    if "derive" in asset:
        source = asset["derive"].get("from", "local")
        print(f"derive {rel} <- {source}")
        derive(asset, target)
        continue

    url = asset.get("download_url")
    if not url:
        raise SystemExit(f"asset has neither download_url nor derive: {rel}")
    print(f"fetch {rel} <- {url}")
    try:
        subprocess.run(
            [
                "curl",
                "--fail",
                "--location",
                "--retry",
                "5",
                "--retry-delay",
                "3",
                "--connect-timeout",
                "15",
                "--max-time",
                "180",
                "--user-agent",
                "SnappyRefPerfAssetFetcher/0.1 local-dev",
                "--output",
                str(target),
                url,
            ],
            check=True,
        )
    except subprocess.CalledProcessError:
        target.unlink(missing_ok=True)
        raise
    time.sleep(1.0)
PY
