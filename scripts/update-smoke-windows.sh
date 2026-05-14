#!/usr/bin/env bash
# Sync the Mac checkout to the Windows verification machine and run the
# published-manifest auto-update smoke there.
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

sync_first=1
if [[ "${1:-}" == "--no-sync" ]]; then
  sync_first=0
  shift
fi
if [[ $# -ne 0 ]]; then
  echo "usage: scripts/update-smoke-windows.sh [--no-sync]" >&2
  exit 2
fi

if [[ "$sync_first" -eq 1 ]]; then
  "$script_dir/windows-sync.sh"
fi

"$script_dir/windows-remote.sh" \
  "powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\update-smoke-windows.ps1"
