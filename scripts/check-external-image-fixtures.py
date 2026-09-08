#!/usr/bin/env python3
import json
import pathlib
import re
import subprocess
import argparse
import os

root = pathlib.Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser(description="Check external fixture dimensions and expected decode failures.")
parser.add_argument('manifest', nargs='?', type=pathlib.Path, default=root / 'tests/fixtures/external/manifest.json')
parser.add_argument('--fixtures', type=pathlib.Path, default=os.environ.get('SNAPPYREF_EXTERNAL_FIXTURE_DIR', root / 'tests/fixtures/external/files'))
parser.add_argument('--binary', type=pathlib.Path, default=os.environ.get('SNAPPYREF_BIN', root / 'target/release/snappyref'))
args = parser.parse_args()
manifest_path = args.manifest
fixture_root = args.fixtures
snappyref_bin = args.binary
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
