#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
manifest=${1:-"$repo_root/tests/fixtures/external/manifest.json"}
target_root=${SNAPPYREF_EXTERNAL_FIXTURE_DIR:-"$repo_root/tests/fixtures/external/files"}

if [[ ! -f "$manifest" ]]; then
  cat >&2 <<EOF
fixture manifest not found: $manifest

Create $manifest as a JSON array:
[
  {
    "file": "sample.heic",
    "url": "https://example.com/sample.heic",
    "sha256": "expected lowercase hex digest",
    "license": "license text or URL",
    "format": "heic",
    "width": 0,
    "height": 0
  }
]
EOF
  exit 2
fi

mkdir -p "$target_root"

python3 - "$manifest" "$target_root" <<'PY'
import hashlib
import json
import pathlib
import sys
import urllib.request

manifest_path = pathlib.Path(sys.argv[1])
target_root = pathlib.Path(sys.argv[2])
fixtures = json.loads(manifest_path.read_text())

for fixture in fixtures:
    rel = fixture["file"]
    url = fixture["url"]
    expected = fixture.get("sha256", "").lower()
    target = target_root / rel
    target.parent.mkdir(parents=True, exist_ok=True)

    if not target.exists():
        print(f"fetch {url} -> {target}")
        with urllib.request.urlopen(url, timeout=60) as response:
            target.write_bytes(response.read())
    else:
        print(f"exists {target}")

    digest = hashlib.sha256(target.read_bytes()).hexdigest()
    if expected and digest != expected:
        target.unlink(missing_ok=True)
        raise SystemExit(
            f"sha256 mismatch for {target}: expected {expected}, got {digest}"
        )
    print(f"ok {rel} sha256={digest}")
PY
