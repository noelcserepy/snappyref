#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=scripts/windows-env.sh
source "$script_dir/windows-env.sh"

if [[ $# -eq 0 ]]; then
  echo "usage: scripts/windows-remote.sh <powershell-command>" >&2
  exit 2
fi

remote_command="$*"
ssh -o BatchMode=yes "$SNAPPYREF_WINDOWS_HOST" \
  "powershell -NoProfile -Command \"cd '$SNAPPYREF_WINDOWS_REPO'; $remote_command\""
