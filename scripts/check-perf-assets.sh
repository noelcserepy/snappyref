#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
manifest=${1:-"$repo_root/perf-assets/manifest.json"}
asset_root=${SNAPPYREF_PERF_ASSET_DIR:-"$repo_root/perf-assets/images"}

python3 - "$manifest" "$asset_root" <<'PY'
import hashlib
import json
import pathlib
import subprocess
import sys

manifest_path = pathlib.Path(sys.argv[1])
asset_root = pathlib.Path(sys.argv[2])
assets = json.loads(manifest_path.read_text())
missing = False

for asset in assets:
    rel = pathlib.Path(asset["file"])
    path = asset_root / rel
    if not path.exists():
        print(f"missing {rel}: run scripts/fetch-perf-assets.sh")
        missing = True
        continue

    data = path.read_bytes()
    digest = hashlib.sha256(data).hexdigest()
    file_type = subprocess.run(
        ["file", "-b", str(path)],
        check=True,
        capture_output=True,
        text=True,
    ).stdout.strip()
    print(
        f"{rel}\tformat={asset['format']}\tbytes={len(data)}\tsha256={digest}\t{file_type}"
    )

if missing:
    sys.exit(1)
PY
