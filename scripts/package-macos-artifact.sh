#!/usr/bin/env bash
# Build the macOS app-artifact zip used by downstream packaging.
#
# Output layout: a zip whose single top-level entry is `SnappyRef/SnappyRef.app/`.
# Models are no longer bundled inside `SnappyRef.app`; Semantic Search assets
# are distributed separately as `snappyref-semantic-search.zip`.
# SNAPPYREF_BUNDLE_ID and SNAPPYREF_BUNDLE_NAME override the bundle identity,
# so a QA build does not share LaunchServices state with an installed app.
set -euo pipefail

if [[ $# -lt 1 || $# -gt 2 ]]; then
  echo "usage: $0 <output.zip> [binary-directory]" >&2
  exit 2
fi
zip_path=$(python3 -c 'import os,sys; print(os.path.abspath(sys.argv[1]))' "$1")
binary_dir=${2:-target/release}
bundle_id=${SNAPPYREF_BUNDLE_ID:-dev.snappyref.SnappyRef}
bundle_name=${SNAPPYREF_BUNDLE_NAME:-SnappyRef}
package_root=$(mktemp -d "${RUNNER_TEMP:-/tmp}/snappyref-macos-package.XXXXXX")
trap 'rm -rf "$package_root"' EXIT
install_root="$package_root/SnappyRef"
app_root="$install_root/SnappyRef.app"
contents_root="$app_root/Contents"
macos_root="$contents_root/MacOS"
resources_root="$contents_root/Resources"
app_version=$(python3 - <<'PY'
import pathlib
import tomllib

manifest = tomllib.loads(pathlib.Path("Cargo.toml").read_text())
print(manifest["package"]["version"])
PY
)

mkdir -p "$macos_root" "$resources_root"
cp "$binary_dir/snappyref" "$macos_root/"
chmod +x "$macos_root/snappyref"
# Ship the auto-update helper alongside the main binary at
# SnappyRef.app/Contents/MacOS/snappyref-updater.
cp "$binary_dir/snappyref-updater" "$macos_root/"
chmod +x "$macos_root/snappyref-updater"
cp assets/app-icon/snappyref.icns "$resources_root/"
python3 scripts/third_party_notices.py "$resources_root/THIRD_PARTY_NOTICES.txt"

cat > "$contents_root/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key>
  <string>en</string>
  <key>CFBundleExecutable</key>
  <string>snappyref</string>
  <key>CFBundleIdentifier</key>
  <string>$bundle_id</string>
  <key>CFBundleIconFile</key>
  <string>snappyref</string>
  <key>CFBundleName</key>
  <string>$bundle_name</string>
  <key>CFBundleDisplayName</key>
  <string>$bundle_name</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleShortVersionString</key>
  <string>$app_version</string>
  <key>CFBundleVersion</key>
  <string>$app_version</string>
  <key>LSMinimumSystemVersion</key>
  <string>14.0</string>
  <key>NSHighResolutionCapable</key>
  <true/>
</dict>
</plist>
PLIST

codesign --force --deep --sign - "$app_root"
codesign --verify --deep --strict "$app_root"

# Guard against accidental bundled models: the bundle must never include a
# Resources/models directory now that the Local Model Store lives outside.
if [[ -e "$resources_root/models" ]]; then
  echo "SnappyRef.app must not contain Contents/Resources/models" >&2
  exit 1
fi

(cd "$package_root" && zip -9 -r "$zip_path" SnappyRef)
