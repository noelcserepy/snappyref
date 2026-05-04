#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 1 ]]; then
  echo "usage: $0 /path/to/source-image" >&2
  exit 2
fi

source_image=$1
if [[ ! -f "$source_image" ]]; then
  echo "source image not found: $source_image" >&2
  exit 2
fi

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
snappyref_bin=${SNAPPYREF_BIN:-"$repo_root/target/release/snappyref"}

if [[ ! -x "$snappyref_bin" ]]; then
  echo "release binary not found: $snappyref_bin" >&2
  echo "run: cargo build --release" >&2
  exit 2
fi

work_dir=${SNAPPYREF_DECODE_BENCH_DIR:-$(mktemp -d)}
cleanup_work_dir=0
if [[ -z "${SNAPPYREF_DECODE_BENCH_DIR:-}" ]]; then
  cleanup_work_dir=1
fi
cleanup() {
  if [[ "$cleanup_work_dir" == "1" ]]; then
    rm -rf "$work_dir"
  fi
}
trap cleanup EXIT

mkdir -p "$work_dir"

generated=()

copy_source() {
  local extension
  extension=${source_image##*.}
  local target="$work_dir/source.$extension"
  cp "$source_image" "$target"
  generated+=("$target")
}

generate_with_ffmpeg() {
  local extension=$1
  local target="$work_dir/source.$extension"
  if ffmpeg -hide_banner -loglevel error -y -i "$source_image" -frames:v 1 "$target"; then
    generated+=("$target")
  else
    echo "skipped .$extension: ffmpeg conversion failed" >&2
    rm -f "$target"
  fi
}

generate_tiff() {
  local target="$work_dir/source.tiff"
  if ffmpeg -hide_banner -loglevel error -y -i "$source_image" -frames:v 1 -pix_fmt rgba "$target"; then
    generated+=("$target")
  else
    echo "skipped .tiff: ffmpeg conversion failed" >&2
    rm -f "$target"
  fi
}

generate_webp() {
  local target="$work_dir/source.webp"
  if command -v cwebp >/dev/null 2>&1 && cwebp -quiet "$source_image" -o "$target"; then
    generated+=("$target")
  else
    generate_with_ffmpeg webp
  fi
}

generate_qoi() {
  generate_with_ffmpeg qoi
}

generate_exr() {
  local target="$work_dir/source.exr"
  if ffmpeg -hide_banner -loglevel error -y -i "$source_image" -frames:v 1 -pix_fmt rgba64le "$target"; then
    generated+=("$target")
  else
    echo "skipped .exr: ffmpeg conversion failed" >&2
    rm -f "$target"
  fi
}

generate_pnm_family() {
  generate_with_ffmpeg pbm
  generate_with_ffmpeg pgm
  generate_with_ffmpeg ppm
}

generate_optional_ffmpeg() {
  local extension=$1
  if ffmpeg -hide_banner -loglevel error -encoders 2>/dev/null | awk '{print $2}' | grep -qx "$extension"; then
    generate_with_ffmpeg "$extension"
  else
    generate_with_ffmpeg "$extension"
  fi
}

copy_source
generate_with_ffmpeg png
generate_with_ffmpeg bmp
generate_tiff
generate_with_ffmpeg tga
generate_webp
generate_qoi
generate_exr
generate_pnm_family
generate_optional_ffmpeg avif
generate_optional_ffmpeg jp2
generate_optional_ffmpeg jxl

printf 'decode benchmark source=%s work_dir=%s\n' "$source_image" "$work_dir"
for image in "${generated[@]}"; do
  if ! "$snappyref_bin" --perf-decode "$image"; then
    printf 'decode_failed source=%s\n' "$image"
  fi
done
