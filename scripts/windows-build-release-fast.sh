#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
"$script_dir/windows-remote.sh" \
  "cargo build --profile release-fast --locked --no-default-features --features semantic-search --bins"
