#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "usage: $0 <output.zip>" >&2
  exit 2
fi

zip_path=$1
bundle_root=$(mktemp -d "${RUNNER_TEMP:-/tmp}/snappyref-model-bundle.XXXXXX")

python3 scripts/fetch-semantic-model.py --target "$bundle_root/models"

model_root="$bundle_root/models/siglip2_base_patch16_224_int8"
test -s "$model_root/tokenizer.json"
test -s "$model_root/onnx/text_model_int8.onnx"
test -s "$model_root/onnx/vision_model_int8.onnx"

{
  echo "model_key=siglip2_base_patch16_224_int8"
  echo "tokenizer_sha256=$(shasum -a 256 "$model_root/tokenizer.json" | awk '{print $1}')"
  echo "text_model_sha256=$(shasum -a 256 "$model_root/onnx/text_model_int8.onnx" | awk '{print $1}')"
  echo "vision_model_sha256=$(shasum -a 256 "$model_root/onnx/vision_model_int8.onnx" | awk '{print $1}')"
} > "$bundle_root/MODEL_BUNDLE.txt"

(cd "$bundle_root" && zip -9 -r "$zip_path" models MODEL_BUNDLE.txt)
scripts/check-model-bundle.sh "$zip_path"
