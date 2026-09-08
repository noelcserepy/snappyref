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

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
case "$platform" in
  windows) binary="SnappyRef/snappyref.exe" ;;
  macos) binary="SnappyRef/SnappyRef.app/Contents/MacOS/snappyref" ;;
  *) echo "unsupported platform: $platform" >&2; exit 2 ;;
esac
"$script_dir/check-release-archive.sh" "$app_archive" "$binary"
cp "$app_archive" "$output"
