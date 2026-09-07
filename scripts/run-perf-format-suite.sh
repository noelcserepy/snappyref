#!/usr/bin/env bash
set -euo pipefail
repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
formats=("$@")
if [[ ${#formats[@]} -eq 0 ]]; then
  formats=(jpg png webp gif svg tiff bmp avif jp2)
fi
scenarios=()
for format in "${formats[@]}"; do
  scenarios+=("format:$format")
done
exec python3 "$repo_root/scripts/perf_eval.py" run "${scenarios[@]}"
