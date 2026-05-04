#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "usage: $0 <output.zip>" >&2
  exit 2
fi

zip_path=$1
package_root=$(mktemp -d "${RUNNER_TEMP:-/tmp}/snappyref-macos-package.XXXXXX")
app_root="$package_root/SnappyRef.app"
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
cp target/release/snappyref "$macos_root/"
chmod +x "$macos_root/snappyref"
cp assets/app-icon/snappyref.icns "$resources_root/"
cp UPDATE.txt "$package_root/"

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
  <string>dev.snappyref.SnappyRef</string>
  <key>CFBundleIconFile</key>
  <string>snappyref</string>
  <key>CFBundleName</key>
  <string>SnappyRef</string>
  <key>CFBundleDisplayName</key>
  <string>SnappyRef</string>
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

python3 scripts/fetch-semantic-model.py --target "$resources_root/models"
model_root="$resources_root/models/siglip2_base_patch16_224_int8"
test -s "$model_root/tokenizer.json"
test -s "$model_root/onnx/text_model_int8.onnx"
test -s "$model_root/onnx/vision_model_int8.onnx"

codesign --force --deep --sign - "$app_root"
codesign --verify --deep --strict "$app_root"

(cd "$package_root" && zip -9 -r "$zip_path" SnappyRef.app UPDATE.txt)
