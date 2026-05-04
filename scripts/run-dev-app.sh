#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
app_root="$repo_root/target/SnappyRefDev.app"
contents="$app_root/Contents"
macos="$contents/MacOS"
binary="$repo_root/target/debug/snappyref"

cargo build

rm -rf "$app_root"
mkdir -p "$macos"
cp "$binary" "$macos/SnappyRef"

cat > "$contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key>
  <string>en</string>
  <key>CFBundleExecutable</key>
  <string>SnappyRef</string>
  <key>CFBundleIdentifier</key>
  <string>dev.noelcserepy.snappyref.dev</string>
  <key>CFBundleName</key>
  <string>SnappyRef</string>
  <key>CFBundleDisplayName</key>
  <string>SnappyRef</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleShortVersionString</key>
  <string>0.1.0-dev</string>
  <key>CFBundleVersion</key>
  <string>1</string>
  <key>LSMinimumSystemVersion</key>
  <string>12.0</string>
</dict>
</plist>
PLIST

open -n "$app_root" --args "$@"
