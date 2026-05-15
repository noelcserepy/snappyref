#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
tmp_dir=$(mktemp -d)
trap 'rm -rf "$tmp_dir"' EXIT

make_zip() {
  local archive=$1
  shift
  (
    cd "$tmp_dir/root"
    zip -qr "$archive" "$@"
  )
}

mkdir -p "$tmp_dir/root/SnappyRef/SnappyRef.app/Contents/MacOS"
mkdir -p "$tmp_dir/root/SnappyRef/SnappyRef.app/Contents/Resources"
touch "$tmp_dir/root/SnappyRef/SnappyRef.app/Contents/MacOS/snappyref"
touch "$tmp_dir/root/SnappyRef/SnappyRef.app/Contents/MacOS/snappyref-updater"
touch "$tmp_dir/root/SnappyRef/SnappyRef.app/Contents/Info.plist"
touch "$tmp_dir/root/SnappyRef/SnappyRef.app/Contents/Resources/snappyref.icns"
make_zip "$tmp_dir/mac-ok.zip" SnappyRef

"$repo_root/scripts/check-release-archive.sh" \
  "$tmp_dir/mac-ok.zip" \
  "SnappyRef/SnappyRef.app/Contents/MacOS/snappyref"

mkdir -p "$tmp_dir/root/SnappyRef/SnappyRef.app/Contents/Resources/models/siglip2"
touch "$tmp_dir/root/SnappyRef/SnappyRef.app/Contents/Resources/models/siglip2/tokenizer.json"
make_zip "$tmp_dir/mac-models.zip" SnappyRef

if "$repo_root/scripts/check-release-archive.sh" \
  "$tmp_dir/mac-models.zip" \
  "SnappyRef/SnappyRef.app/Contents/MacOS/snappyref" 2>"$tmp_dir/mac-models.err"; then
  echo "expected mac archive with in-bundle models to fail" >&2
  exit 1
fi
grep -q "in-bundle models directory" "$tmp_dir/mac-models.err"

rm -rf "$tmp_dir/root"
mkdir -p "$tmp_dir/root/SnappyRef"
touch "$tmp_dir/root/SnappyRef/snappyref.exe"
make_zip "$tmp_dir/windows-ok.zip" SnappyRef

"$repo_root/scripts/check-release-archive.sh" \
  "$tmp_dir/windows-ok.zip" \
  "SnappyRef/snappyref.exe"

touch "$tmp_dir/root/SnappyRef/snappyref-updater.exe"
make_zip "$tmp_dir/windows-helper.zip" SnappyRef

if "$repo_root/scripts/check-release-archive.sh" \
  "$tmp_dir/windows-helper.zip" \
  "SnappyRef/snappyref.exe" 2>"$tmp_dir/windows-helper.err"; then
  echo "expected windows archive with updater helper to fail" >&2
  exit 1
fi
grep -q "snappyref-updater.exe" "$tmp_dir/windows-helper.err"

rm -rf "$tmp_dir/root"
mkdir -p "$tmp_dir/root/SnappyRef/models/siglip2"
touch "$tmp_dir/root/SnappyRef/snappyref.exe"
touch "$tmp_dir/root/SnappyRef/models/siglip2/tokenizer.json"
make_zip "$tmp_dir/windows-models.zip" SnappyRef

if "$repo_root/scripts/check-release-archive.sh" \
  "$tmp_dir/windows-models.zip" \
  "SnappyRef/snappyref.exe" 2>"$tmp_dir/windows-models.err"; then
  echo "expected windows archive with Local Model Store files to fail" >&2
  exit 1
fi
grep -q "models directory" "$tmp_dir/windows-models.err"
