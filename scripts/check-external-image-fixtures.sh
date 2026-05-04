#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
manifest=${1:-"$repo_root/tests/fixtures/external/manifest.json"}
fixture_root=${SNAPPYREF_EXTERNAL_FIXTURE_DIR:-"$repo_root/tests/fixtures/external/files"}
snappyref_bin=${SNAPPYREF_BIN:-"$repo_root/target/release/snappyref"}

if [[ ! -f "$manifest" ]]; then
  echo "fixture manifest not found: $manifest" >&2
  exit 2
fi

if [[ ! -x "$snappyref_bin" ]]; then
  echo "release binary not found: $snappyref_bin" >&2
  echo "run: cargo build --release" >&2
  exit 2
fi

python3 - "$manifest" "$fixture_root" "$snappyref_bin" <<'PY'
import json
import pathlib
import re
import subprocess
import sys

manifest_path = pathlib.Path(sys.argv[1])
fixture_root = pathlib.Path(sys.argv[2])
snappyref_bin = pathlib.Path(sys.argv[3])
fixtures = json.loads(manifest_path.read_text())

failed = False
for fixture in fixtures:
    rel = fixture["file"]
    expected_width = int(fixture.get("width", 0))
    expected_height = int(fixture.get("height", 0))
    expect_decode_failure = bool(fixture.get("expect_decode_failure", False))
    path = fixture_root / rel
    if not path.exists():
        print(f"missing {rel}: run scripts/fetch-image-fixtures.sh first")
        failed = True
        continue

    result = subprocess.run(
        [str(snappyref_bin), "--perf-decode", str(path)],
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
    )
    output = result.stdout.strip()
    if expect_decode_failure:
        if result.returncode == 0:
            print(f"unexpected_decode_success {rel}: {output}")
            failed = True
        else:
            print(f"ok_expected_failure {rel}: {output}")
        continue

    if result.returncode != 0:
        print(f"decode_failed {rel}: {output}")
        failed = True
        continue

    match = re.search(r"dims=(\d+)x(\d+)", output)
    if not match:
        print(f"decode_output_unparseable {rel}: {output}")
        failed = True
        continue

    width = int(match.group(1))
    height = int(match.group(2))
    if expected_width and expected_height and (width, height) != (expected_width, expected_height):
        print(
            f"dimension_mismatch {rel}: expected {expected_width}x{expected_height}, got {width}x{height}"
        )
        failed = True
        continue

    print(f"ok {rel} {width}x{height}")

if failed:
    raise SystemExit(1)
PY
