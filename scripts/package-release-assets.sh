#!/usr/bin/env bash
# Assemble platform release zips from a built app archive and an optional
# reusable model bundle.
#
# Output layouts (both platforms):
#   - app-only:    SnappyRef/{snappyref.exe | SnappyRef.app/}
#   - full install: above plus SnappyRef/models/
#
# Notes:
#   - The macOS app-artifact zip already has the `SnappyRef/SnappyRef.app`
#     layout produced by `package-macos-artifact.sh`. Windows app-artifact
#     zips are produced by the Build Windows App workflow with the binary at
#     `SnappyRef/snappyref.exe`.
#   - No script in this pipeline emits `UPDATE.txt`. Update instructions live
#     in the website and in-app release notes instead.
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
install_root="$package_root/SnappyRef"
mkdir -p "$app_root" "$models_root" "$install_root"

unzip -q "$app_archive" -d "$app_root"

if [[ "$include_models" -eq 1 ]]; then
  unzip -q "$models_bundle" -d "$models_root"
  scripts/check-model-bundle.sh "$models_bundle"
fi

stage_models_into_install_root() {
  if [[ "$include_models" -ne 1 ]]; then
    return
  fi
  rm -rf "$install_root/models"
  cp -R "$models_root/models" "$install_root/models"
}

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
    stage_models_into_install_root
    (cd "$package_root" && zip -9 -r "$output" SnappyRef)
    if [[ "$include_models" -eq 1 ]]; then
      scripts/check-release-archive.sh "$output" SnappyRef/snappyref.exe
    else
      zipinfo -1 "$output" | grep -Fxq "SnappyRef/snappyref.exe"
      if zipinfo -1 "$output" | grep -Fxq "SnappyRef/UPDATE.txt"; then
        echo "release archive must not contain SnappyRef/UPDATE.txt" >&2
        exit 1
      fi
      if zipinfo -1 "$output" | grep -E '^SnappyRef/models(/|$)' >/dev/null; then
        echo "app-only release archive must not contain SnappyRef/models/" >&2
        exit 1
      fi
    fi
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
    stage_models_into_install_root
    (cd "$package_root" && zip -9 -r "$output" SnappyRef)
    if [[ "$include_models" -eq 1 ]]; then
      scripts/check-release-archive.sh "$output" SnappyRef/SnappyRef.app/Contents/MacOS/snappyref
    else
      zipinfo -1 "$output" | grep -Fxq "SnappyRef/SnappyRef.app/Contents/MacOS/snappyref"
      if zipinfo -1 "$output" | grep -Fxq "SnappyRef/UPDATE.txt"; then
        echo "release archive must not contain SnappyRef/UPDATE.txt" >&2
        exit 1
      fi
      if zipinfo -1 "$output" | grep -E '^SnappyRef/models(/|$)' >/dev/null; then
        echo "app-only release archive must not contain SnappyRef/models/" >&2
        exit 1
      fi
    fi
    ;;
  *)
    echo "unsupported platform: $platform" >&2
    exit 2
    ;;
esac
