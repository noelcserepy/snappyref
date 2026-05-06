#!/usr/bin/env bash
set -euo pipefail

usage() {
  echo "usage: $0 --platform <windows|macos> --app-archive <zip> --output <zip> [--models-bundle <zip>] [--include-models]" >&2
}

platform=""
app_archive=""
models_bundle=""
output=""
include_models=0

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
    --models-bundle)
      models_bundle=${2:-}
      shift 2
      ;;
    --output)
      output=${2:-}
      shift 2
      ;;
    --include-models)
      include_models=1
      shift
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

if [[ "$include_models" -eq 1 && -z "$models_bundle" ]]; then
  echo "--models-bundle is required with --include-models" >&2
  exit 2
fi

work_root=$(mktemp -d "${RUNNER_TEMP:-/tmp}/snappyref-package.XXXXXX")
app_root="$work_root/app"
models_root="$work_root/models"
package_root="$work_root/package"
mkdir -p "$app_root" "$models_root" "$package_root"

unzip -q "$app_archive" -d "$app_root"

if [[ "$include_models" -eq 1 ]]; then
  unzip -q "$models_bundle" -d "$models_root"
  scripts/check-model-bundle.sh "$models_bundle"
fi

case "$platform" in
  windows)
    mkdir -p "$package_root/SnappyRef"
    if [[ -d "$app_root/SnappyRef" ]]; then
      cp -R "$app_root/SnappyRef/." "$package_root/SnappyRef/"
    else
      cp "$app_root/snappyref.exe" "$package_root/SnappyRef/"
      cp "$app_root/UPDATE.txt" "$package_root/SnappyRef/"
    fi
    if [[ "$include_models" -eq 1 ]]; then
      cp -R "$models_root/models" "$package_root/SnappyRef/models"
    fi
    (cd "$package_root" && zip -9 -r "$output" SnappyRef)
    if [[ "$include_models" -eq 1 ]]; then
      scripts/check-release-archive.sh "$output" SnappyRef/snappyref.exe
    else
      zipinfo -1 "$output" | grep -Fxq "SnappyRef/snappyref.exe"
      zipinfo -1 "$output" | grep -Fxq "SnappyRef/UPDATE.txt"
    fi
    ;;
  macos)
    if [[ ! -d "$app_root/SnappyRef.app" ]]; then
      echo "missing SnappyRef.app in app archive" >&2
      exit 1
    fi
    cp -R "$app_root/SnappyRef.app" "$package_root/SnappyRef.app"
    cp "$app_root/UPDATE.txt" "$package_root/UPDATE.txt"
    if [[ "$include_models" -ne 1 ]]; then
      echo "macOS packages require --include-models because models live inside SnappyRef.app" >&2
      exit 1
    fi
    mkdir -p "$package_root/SnappyRef.app/Contents/Resources"
    rm -rf "$package_root/SnappyRef.app/Contents/Resources/models"
    cp -R "$models_root/models" "$package_root/SnappyRef.app/Contents/Resources/models"
    (cd "$package_root" && zip -9 -r "$output" SnappyRef.app UPDATE.txt)
    scripts/check-release-archive.sh "$output" SnappyRef.app/Contents/MacOS/snappyref
    ;;
  *)
    echo "unsupported platform: $platform" >&2
    exit 2
    ;;
esac
