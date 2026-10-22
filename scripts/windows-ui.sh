#!/usr/bin/env bash
# Run a PowerShell step on the signed-in Windows desktop and bring back its
# window captures. Exits with the step's status, 124 on timeout.
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=scripts/windows-env.sh
source "$script_dir/windows-env.sh"
: "${SNAPPYREF_WINDOWS_REPO:?Set SNAPPYREF_WINDOWS_REPO in .env.windows or the environment.}"

usage='usage: scripts/windows-ui.sh <step.ps1> [--timeout <seconds>]'
step="${1:-}"
timeout=120
[[ -f "$step" ]] || { echo "$usage" >&2; exit 2; }
shift
while [[ $# -gt 0 ]]; do
  case "$1" in
    --timeout) [[ "${2:-}" =~ ^[0-9]+$ ]] || { echo "$usage" >&2; exit 2; }; timeout="$2"; shift 2 ;;
    *) echo "$usage" >&2; exit 2 ;;
  esac
done

run_id="$(date +%Y%m%d-%H%M%S)-$$"
# Relative to the SSH account's home, which is where scp and ssh start.
remote_dir="AppData/Local/Temp/snappyref-ui-$run_id"
# Outside every checkout, so a capture never lands in a commit by accident.
captures="${TMPDIR:-/tmp}/snappyref-windows-ui/$run_id"
stage="$(mktemp -d)"

finish() {
  mkdir -p "$(dirname "$captures")"
  scp -rq "$SNAPPYREF_WINDOWS_HOST:$remote_dir/captures" "$captures" 2>/dev/null || true
  ssh -o BatchMode=yes "$SNAPPYREF_WINDOWS_HOST" \
    "powershell -NoProfile -Command \"Remove-Item -Recurse -Force -LiteralPath $remote_dir\"" \
    >/dev/null 2>&1 || true
  rm -rf "$stage"
  if [[ -n "$(ls -A "$captures" 2>/dev/null)" ]]; then
    echo "Captures: $captures"
  fi
}
trap finish EXIT

cp "$script_dir"/windows-ui/{run,host,app}.ps1 "$stage/"
cp "$step" "$stage/step.ps1"
printf '%s' "$SNAPPYREF_WINDOWS_REPO" >"$stage/repo.txt"
scp -rq "$stage" "$SNAPPYREF_WINDOWS_HOST:$remote_dir"

# The account's SSH shell is PowerShell, which keeps the status only when told.
ssh -o BatchMode=yes "$SNAPPYREF_WINDOWS_HOST" \
  "powershell -NoProfile -ExecutionPolicy Bypass -File $remote_dir/run.ps1 -TimeoutSeconds $timeout; exit \$LASTEXITCODE"
