#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 2 ]]; then
  echo "usage: $0 <archive.zip> <binary-name>" >&2
  exit 2
fi

archive=$1
binary_name=$2
binary_dir=$(dirname "$binary_name")
if [[ "$binary_dir" == "." ]]; then
  model_prefix="models"
  update_entry="UPDATE.txt"
else
  model_prefix="$binary_dir/models"
  update_entry="$binary_dir/UPDATE.txt"
fi

if [[ ! -f "$archive" ]]; then
  echo "missing archive: $archive" >&2
  exit 1
fi

entries=$(zipinfo -1 "$archive" | sed 's#\\#/#g')

require_entry() {
  local entry=$1
  if ! grep -Fxq "$entry" <<<"$entries"; then
    echo "missing required app-owned path: $entry" >&2
    exit 1
  fi
}

forbid_path() {
  local path=$1
  while IFS= read -r entry; do
    case "$entry" in
      "$path"|"$path"/*)
        echo "archive includes forbidden user-owned path: $path" >&2
        exit 1
        ;;
    esac
  done <<<"$entries"
}

require_entry "$binary_name"
case "$binary_name" in
  *.app/Contents/MacOS/*)
    app_root="${binary_name%%.app/*}.app"
    model_prefix="$app_root/Contents/Resources/models"
    update_entry="UPDATE.txt"
    require_entry "$app_root/Contents/Info.plist"
    require_entry "$app_root/Contents/Resources/snappyref.icns"
    ;;
esac
require_entry "$update_entry"
require_entry "$model_prefix/siglip2_base_patch16_224_int8/tokenizer.json"
require_entry "$model_prefix/siglip2_base_patch16_224_int8/onnx/text_model_int8.onnx"
require_entry "$model_prefix/siglip2_base_patch16_224_int8/onnx/vision_model_int8.onnx"

forbid_path "library"
forbid_path "library.db"
forbid_path "originals"
forbid_path "thumbs"
if [[ "$binary_dir" != "." && "$binary_name" != *.app/Contents/MacOS/* ]]; then
  forbid_path "$binary_dir/library"
  forbid_path "$binary_dir/library.db"
  forbid_path "$binary_dir/originals"
  forbid_path "$binary_dir/thumbs"
fi
