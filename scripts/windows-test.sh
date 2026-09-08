#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Match the canonical behavior gate: native rendering tests run serially.
# An explicit libtest --test-threads option still overrides this default.
if [[ $# -gt 0 ]]; then
  filter="$1"
  shift
  "$script_dir/windows-remote.sh" \
    "Set-Item Env:RUST_TEST_THREADS 1; cargo test --locked '$filter' $*"
else
  "$script_dir/windows-remote.sh" \
    "Set-Item Env:RUST_TEST_THREADS 1; cargo test --locked"
fi
