#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "usage: $0 <model-bundle.zip>" >&2
  exit 2
fi

archive=$1
model_prefix="models/siglip2_base_patch16_224_int8"

if [[ ! -f "$archive" ]]; then
  echo "missing archive: $archive" >&2
  exit 1
fi

entries=$(zipinfo -1 "$archive" | sed 's#\\#/#g')

require_entry() {
  local entry=$1
  if ! grep -Fxq "$entry" <<<"$entries"; then
    echo "missing required model path: $entry" >&2
    exit 1
  fi
}

require_entry "MODEL_BUNDLE.txt"
require_entry "$model_prefix/tokenizer.json"
require_entry "$model_prefix/onnx/text_model_int8.onnx"
require_entry "$model_prefix/onnx/vision_model_int8.onnx"
