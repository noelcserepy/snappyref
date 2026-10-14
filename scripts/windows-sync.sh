#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=scripts/windows-env.sh
source "$script_dir/windows-env.sh"
: "${SNAPPYREF_WINDOWS_REPO:?Set SNAPPYREF_WINDOWS_REPO in .env.windows or the environment.}"

exec python3 "$script_dir/windows-sync.py"
