#!/usr/bin/env bash
# Build a release SnappyRef QA.app with its own bundle id, so computer-use QA
# never attaches to an installed dev.snappyref.SnappyRef. Prints the app path.
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$repo_root"
target_dir=${CARGO_TARGET_DIR:-$repo_root/target}
qa_root="$target_dir/qa"
app_path="$qa_root/SnappyRef QA.app"

cargo build --locked --release --bins
rm -rf "$qa_root"
mkdir -p "$qa_root"
SNAPPYREF_BUNDLE_ID=dev.snappyref.SnappyRef.QA SNAPPYREF_BUNDLE_NAME="SnappyRef QA" \
  scripts/package-macos-artifact.sh "$qa_root/qa.zip" "$target_dir/release" >/dev/null
ditto -x -k "$qa_root/qa.zip" "$qa_root"
mv "$qa_root/SnappyRef/SnappyRef.app" "$app_path"
rm -rf "$qa_root/SnappyRef" "$qa_root/qa.zip"
codesign --verify --deep --strict "$app_path"
echo "$app_path"
