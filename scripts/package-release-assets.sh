#!/usr/bin/env bash
# Assemble platform release zips from a built app archive.
#
# Output layouts (both platforms):
#   - SnappyRef/{snappyref.exe | SnappyRef.app/}
#
# Notes:
#   - The macOS app-artifact zip already has the `SnappyRef/SnappyRef.app`
#     layout produced by `package-macos-artifact.sh`. Windows app-artifact
#     zips are produced by the Build Windows App workflow with the binary at
#     `SnappyRef/snappyref.exe`.
#   - Release zips are app-only. Semantic Search assets are distributed as the
#     long-lived snappyref-semantic-search.zip asset on snappyref-dist.
#   - No script in this pipeline emits `UPDATE.txt`. Update instructions live in
#     the website and in-app release notes instead.
set -euo pipefail

usage() {
  echo "usage: $0 --platform <windows|macos> --app-archive <zip> --output <zip>" >&2
}

platform=""
app_archive=""
output=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --platform)
      platform=${2:-}
      shift 2
      ;;
    --app-archive)
      app_archive=${2:-}
      shift 2
      ;;
    --output)
      output=${2:-}
      shift 2
      ;;
    *)
      usage
      exit 2
      ;;
  esac
done

if [[ -z "$platform" || -z "$app_archive" || -z "$output" ]]; then
  usage
  exit 2
fi

work_root=$(mktemp -d "${RUNNER_TEMP:-/tmp}/snappyref-package.XXXXXX")
app_root="$work_root/app"
package_root="$work_root/package"
install_root="$package_root/SnappyRef"
mkdir -p "$app_root" "$install_root"

unzip -q "$app_archive" -d "$app_root"

case "$platform" in
  windows)
    if [[ -d "$app_root/SnappyRef" ]]; then
      cp -R "$app_root/SnappyRef/." "$install_root/"
    else
      cp "$app_root/snappyref.exe" "$install_root/"
    fi
    # The app-artifact must no longer carry an UPDATE.txt; strip a stray one
    # so we never emit it from this pipeline even if upstream is stale.
    rm -f "$install_root/UPDATE.txt"
    (cd "$package_root" && zip -9 -r "$output" SnappyRef)
    scripts/check-release-archive.sh "$output" SnappyRef/snappyref.exe
    ;;
  macos)
    if [[ -d "$app_root/SnappyRef/SnappyRef.app" ]]; then
      cp -R "$app_root/SnappyRef/." "$install_root/"
    elif [[ -d "$app_root/SnappyRef.app" ]]; then
      # Backwards-compatible path for app-artifact zips produced before this
      # change shipped: the .app sat at the archive root.
      cp -R "$app_root/SnappyRef.app" "$install_root/SnappyRef.app"
    else
      echo "missing SnappyRef.app in app archive" >&2
      exit 1
    fi
    rm -f "$install_root/UPDATE.txt"
    # The bundle must never carry an in-bundle models directory after the move
    # to the Local Model Store.
    rm -rf "$install_root/SnappyRef.app/Contents/Resources/models"
    (cd "$package_root" && zip -9 -r "$output" SnappyRef)
    scripts/check-release-archive.sh "$output" SnappyRef/SnappyRef.app/Contents/MacOS/snappyref
    ;;
  *)
    echo "unsupported platform: $platform" >&2
    exit 2
    ;;
esac
