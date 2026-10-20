#!/usr/bin/env bash
# Read-only: says whether scenario runs from this checkout will build and
# report this checkout's code. Exits 1 on a problem worth fixing first.
set -u

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd -- "$script_dir/../../../.." && pwd)"
target_dir="${CARGO_TARGET_DIR:-target}"
[[ "$target_dir" = /* ]] || target_dir="$repo_root/$target_dir"
status=0

echo "checkout=$repo_root"
echo "head=$(git -C "$repo_root" rev-parse --short HEAD)$(git -C "$repo_root" diff --quiet HEAD || echo ' (uncommitted changes)')"
echo "artifacts=$target_dir/snappyref-inspect"

# inspect_root owns which targets this checkout may build into.
problem="$(python3 -c '
import sys
sys.path.insert(0, sys.argv[1])
from inspect_scenarios import inspect_root
try:
    inspect_root()
except RuntimeError as error:
    print(error)
' "$repo_root/scripts")"
if [[ -n "$problem" ]]; then
  echo "problem: $problem"
  status=1
fi

# Test binaries, and the app `mcp-drive.py` starts.
running="$(pgrep -fl "$target_dir/debug/(deps/snappyref-|snappyref( |$))" || true)"
if [[ -n "$running" ]]; then
  echo "busy: a test binary or app from this checkout is running. Wait for it, or stop it only if you started it:"
  echo "$running"
fi

if latest="$("$script_dir/latest-artifact.sh" 2>/dev/null)"; then
  echo "latest_artifact=$latest"
  echo "latest_status=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["status"])' "$latest/report.json" 2>/dev/null || echo missing)"
else
  echo "latest_artifact=none"
fi

exit "$status"
