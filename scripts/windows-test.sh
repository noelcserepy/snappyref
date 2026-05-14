#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

if [[ $# -gt 0 ]]; then
  filter="$1"
  shift
  "$script_dir/windows-remote.sh" \
    "cargo test --locked --no-default-features --features semantic-search '$filter' $*"
else
  "$script_dir/windows-remote.sh" \
    "cargo test --locked --no-default-features --features semantic-search"
fi
