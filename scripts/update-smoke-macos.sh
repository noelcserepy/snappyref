#!/usr/bin/env bash
# Exercise the real macOS auto-update path against the published manifest.
#
# This installs the current local build into target/update-smoke/macos/SnappyRef,
# lies that it is version 0.0.0, lets the app fetch/stage the latest GitHub
# release, then verifies the updater helper swapped the scratch app bundle.
set -euo pipefail

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "update-smoke-macos.sh only runs on macOS" >&2
  exit 2
fi

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$repo_root"

smoke_root=${SNAPPYREF_UPDATE_SMOKE_ROOT:-"$repo_root/target/update-smoke/macos"}
mkdir -p "$smoke_root"
smoke_root=$(cd "$smoke_root" && pwd)
case "$smoke_root" in
  ""|"/"|"$HOME"|"$repo_root")
    echo "refusing unsafe smoke root: $smoke_root" >&2
    exit 2
    ;;
esac
install_root="$smoke_root/SnappyRef"
zip_path="$smoke_root/current-local.zip"
status_path="$smoke_root/status.txt"
app_log="$smoke_root/app.log"
timeout_secs=${SNAPPYREF_UPDATE_SMOKE_TIMEOUT_SECS:-300}
helper_timeout_secs=${SNAPPYREF_UPDATE_SMOKE_HELPER_TIMEOUT_SECS:-120}

mkdir -p "$smoke_root"
rm -rf "$smoke_root"/*

cargo build --release --locked --bins
"$repo_root/scripts/package-macos-artifact.sh" "$zip_path"
ditto -x -k "$zip_path" "$smoke_root"

app_bin="$install_root/SnappyRef.app/Contents/MacOS/snappyref"
if [[ ! -x "$app_bin" ]]; then
  echo "packaged app binary missing at $app_bin" >&2
  exit 1
fi

SNAPPYREF_UPDATE_CURRENT_VERSION="${SNAPPYREF_UPDATE_CURRENT_VERSION:-0.0.0}" \
SNAPPYREF_INSTALL_ROOT="$install_root" \
SNAPPYREF_LIBRARY_ROOT="$smoke_root/library" \
SNAPPYREF_UPDATE_SMOKE_STATUS="$status_path" \
SNAPPYREF_UPDATE_SMOKE_TIMEOUT_SECS="$timeout_secs" \
  "$app_bin" --update-smoke-install --no-startup-focus >"$app_log" 2>&1 &
app_pid=$!

if ! wait "$app_pid"; then
  echo "smoke app exited non-zero; log: $app_log" >&2
  tail -80 "$app_log" >&2 || true
  exit 1
fi

deadline=$((SECONDS + helper_timeout_secs))
while (( SECONDS < deadline )); do
  if [[ -f "$status_path" ]] && grep -q '^failed:' "$status_path"; then
    echo "update smoke failed: $(cat "$status_path")" >&2
    tail -80 "$app_log" >&2 || true
    exit 1
  fi
  if [[ -d "$install_root/SnappyRef.app" && -d "$install_root/SnappyRef.app.previous" ]]; then
    break
  fi
  sleep 1
done

if [[ ! -d "$install_root/SnappyRef.app" ]]; then
  echo "updated app bundle missing at $install_root/SnappyRef.app" >&2
  exit 1
fi
if [[ ! -d "$install_root/SnappyRef.app.previous" ]]; then
  echo "backup app bundle missing at $install_root/SnappyRef.app.previous" >&2
  tail -80 "$app_log" >&2 || true
  exit 1
fi

if [[ ! -x "$app_bin" ]]; then
  echo "updated app binary is not executable at $app_bin" >&2
  exit 1
fi

if pgrep -f "$app_bin" >/dev/null 2>&1; then
  pkill -f "$app_bin" >/dev/null 2>&1 || true
fi

echo "macOS update smoke passed"
echo "scratch install: $install_root"
