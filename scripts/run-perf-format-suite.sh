#!/usr/bin/env bash
#
# Run `format:<format>` perf scenarios for every format in the corpus and
# emit a per-format comparison table.
#
# This wraps `scripts/run-perf-scenario.sh` for each corpus format and
# then calls `scripts/summarize-perf-by-format.py` on the resulting run
# directories. Use it when a mixed-corpus import result needs a per-format
# breakdown before attributing a regression to a decoder or source format.
#
# Usage:
#
#     scripts/run-perf-format-suite.sh                  # all corpus formats
#     scripts/run-perf-format-suite.sh jpg png webp     # only these
#     SNAPPYREF_PERF_BINARY=./target/release/snappyref scripts/run-perf-format-suite.sh
#
# Optional env vars are passed through to `run-perf-scenario.sh`:
# `SNAPPYREF_PERF_BINARY`, `SNAPPYREF_PERF_ASSET_DIR`,
# `SNAPPYREF_PERF_IDLE_HOLD_SECS`, `SNAPPYREF_PERF_MODEL_UNLOAD_AFTER_IDLE_MS`.

set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
formats=("$@")
if [[ ${#formats[@]} -eq 0 ]]; then
  # Derive distinct formats from the manifest using python so we do not
  # depend on jq being installed.
  formats=( $(python3 -c "
import json, sys, pathlib
manifest = json.loads(pathlib.Path('$repo_root/perf-assets/manifest.json').read_text())
print(' '.join(sorted({entry['format'] for entry in manifest})))
") )
fi

run_dirs=()
for fmt in "${formats[@]}"; do
  echo "== running format:$fmt =="
  out=$(bash "$repo_root/scripts/run-perf-scenario.sh" "format:$fmt")
  echo "$out"
  run_dir=$(printf '%s\n' "$out" | awk -F= '/^run_dir=/ {print $2}')
  if [[ -n "$run_dir" ]]; then
    run_dirs+=("$run_dir")
  fi
done

echo
echo "== per-format summary =="
python3 "$repo_root/scripts/summarize-perf-by-format.py" "${run_dirs[@]}"
