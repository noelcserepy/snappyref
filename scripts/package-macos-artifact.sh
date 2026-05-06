#!/usr/bin/env bash
set -euo pipefail

model_mode=fetch
models_dir=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --skip-models)
      model_mode=skip
      shift
      ;;
    --models-dir)
      if [[ $# -lt 2 ]]; then
        echo "missing value for --models-dir" >&2
        exit 2
      fi
      model_mode=copy
      models_dir=$2
      shift 2
      ;;
    --*)
      echo "unknown option: $1" >&2
      exit 2
      ;;
    *)
      break
      ;;
  esac
done

if [[ $# -ne 1 ]]; then
  echo "usage: $0 [--skip-models|--models-dir <models-dir>] <output.zip>" >&2
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

case "$model_mode" in
  fetch)
    python3 scripts/fetch-semantic-model.py --target "$resources_root/models"
    ;;
  copy)
    if [[ ! -d "$models_dir" ]]; then
      echo "missing models directory: $models_dir" >&2
      exit 1
    fi
    mkdir -p "$resources_root"
    cp -R "$models_dir" "$resources_root/models"
    ;;
  skip)
    ;;
esac

if [[ "$model_mode" != "skip" ]]; then
  model_root="$resources_root/models/siglip2_base_patch16_224_int8"
  test -s "$model_root/tokenizer.json"
  test -s "$model_root/onnx/text_model_int8.onnx"
  test -s "$model_root/onnx/vision_model_int8.onnx"
fi

codesign --force --deep --sign - "$app_root"
codesign --verify --deep --strict "$app_root"

(cd "$package_root" && zip -9 -r "$zip_path" SnappyRef.app UPDATE.txt)
