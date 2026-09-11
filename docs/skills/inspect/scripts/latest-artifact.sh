#!/usr/bin/env bash
set -u

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd -- "$script_dir/../../../.." && pwd)"

scenario="${1:-}"
target_dir="${CARGO_TARGET_DIR:-target}"
[[ "$target_dir" = /* ]] || target_dir="$repo_root/$target_dir"
artifact_root="$target_dir/snappyref-inspect"

if [[ -n "$scenario" ]]; then
  latest_file="$artifact_root/$scenario/latest.txt"
  if [[ -f "$latest_file" ]]; then
    cat "$latest_file"
    echo
    exit 0
  fi
  echo "no artifact found for scenario: $scenario" >&2
  exit 1
fi

latest_file="$(find "$artifact_root" -name latest.txt -type f -print 2>/dev/null | xargs ls -t 2>/dev/null | head -n 1 || true)"
if [[ -z "$latest_file" ]]; then
  echo "no inspect artifact found" >&2
  exit 1
fi

cat "$latest_file"
echo
