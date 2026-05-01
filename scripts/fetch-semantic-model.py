#!/usr/bin/env python3
import argparse
import pathlib
import urllib.request


MODEL_KEY = "siglip2_base_patch16_224_int8"
REPO_ID = "onnx-community/siglip2-base-patch16-224-ONNX"
FILES = [
    "tokenizer.json",
    "onnx/text_model_int8.onnx",
    "onnx/vision_model_int8.onnx",
]


def fetch_file(url: str, target: pathlib.Path) -> None:
    target.parent.mkdir(parents=True, exist_ok=True)
    if target.exists() and target.stat().st_size > 0:
        print(f"exists {target}")
        return

    print(f"fetch {url} -> {target}")
    with urllib.request.urlopen(url, timeout=120) as response:
        target.write_bytes(response.read())

    if target.stat().st_size == 0:
        target.unlink(missing_ok=True)
        raise SystemExit(f"downloaded empty model file: {target}")


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Fetch semantic-search model assets for release packaging."
    )
    parser.add_argument(
        "--target",
        type=pathlib.Path,
        default=pathlib.Path("models"),
        help="Directory that will contain the model-key subdirectory.",
    )
    args = parser.parse_args()

    model_root = args.target / MODEL_KEY
    for relative in FILES:
        url = f"https://huggingface.co/{REPO_ID}/resolve/main/{relative}"
        fetch_file(url, model_root / relative)

    for relative in FILES:
        path = model_root / relative
        if not path.is_file() or path.stat().st_size == 0:
            raise SystemExit(f"missing packaged model asset: {path}")

    print(f"semantic model ready: {model_root}")


if __name__ == "__main__":
    main()
