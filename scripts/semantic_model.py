#!/usr/bin/env python3
"""Fetch, package, and validate the standalone Semantic Search model."""
import argparse
import hashlib
from pathlib import Path
import tempfile
import urllib.request
import zipfile

ROOT = Path(__file__).resolve().parents[1]
MODEL_KEY = 'siglip2_base_patch16_224_int8'
REPO_ID = 'onnx-community/siglip2-base-patch16-224-ONNX'
REVISION = 'ba1f3b0843f24bc5417d38e19c37b287d719b2f4'
FILES = {
    'tokenizer': 'tokenizer.json',
    'text_model': 'onnx/text_model_int8.onnx',
    'vision_model': 'onnx/vision_model_int8.onnx',
}
NOTICE = f'''SigLIP 2 base patch16-224, int8 ONNX export

The model weights are google/siglip2-base-patch16-224 by Google, released under
the Apache License, Version 2.0.
https://huggingface.co/google/siglip2-base-patch16-224

The ONNX conversion and int8 quantization are unmodified files from
{REPO_ID} at revision {REVISION}.
https://huggingface.co/{REPO_ID}
'''


def fetch(target: Path) -> Path:
    model_root = target / MODEL_KEY
    for relative in FILES.values():
        path = model_root / relative
        if path.is_file() and path.stat().st_size:
            continue
        path.parent.mkdir(parents=True, exist_ok=True)
        url = f'https://huggingface.co/{REPO_ID}/resolve/{REVISION}/{relative}'
        with urllib.request.urlopen(url, timeout=120) as response:
            path.write_bytes(response.read())
        if not path.stat().st_size:
            path.unlink()
            raise ValueError(f'downloaded empty model file: {path}')
    return model_root


def check(archive_path: Path) -> None:
    with zipfile.ZipFile(archive_path) as archive:
        provenance = dict(line.split('=', 1) for line in archive.read('MODEL_BUNDLE.txt').decode().splitlines())
        if provenance.get('model_key') != MODEL_KEY:
            raise ValueError('unexpected model key')
        for label, relative in FILES.items():
            path = f'models/{MODEL_KEY}/{relative}'
            data = archive.read(path)
            if not data or hashlib.sha256(data).hexdigest() != provenance.get(f'{label}_sha256'):
                raise ValueError(f'missing, empty, or mismatched model asset: {path}')


def bundle(output: Path) -> None:
    with tempfile.TemporaryDirectory(prefix='snappyref-model-bundle-') as scratch:
        model_root = fetch(Path(scratch) / 'models')
        provenance = [f'model_key={MODEL_KEY}']
        with zipfile.ZipFile(output, 'w', compression=zipfile.ZIP_DEFLATED, compresslevel=9) as archive:
            for label, relative in FILES.items():
                path = model_root / relative
                provenance.append(f'{label}_sha256={hashlib.sha256(path.read_bytes()).hexdigest()}')
                archive.write(path, f'models/{MODEL_KEY}/{relative}')
            archive.write(ROOT / 'assets/licenses/Apache-2.0.txt', f'models/{MODEL_KEY}/LICENSE')
            archive.writestr(f'models/{MODEL_KEY}/NOTICE', NOTICE)
            archive.writestr('MODEL_BUNDLE.txt', '\n'.join(provenance) + '\n')
        check(output)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest='command', required=True)
    commands.add_parser('fetch').add_argument('--target', type=Path, default=Path('models'))
    commands.add_parser('bundle').add_argument('output', type=Path)
    commands.add_parser('check').add_argument('archive', type=Path)
    args = parser.parse_args()
    if args.command == 'fetch':
        fetch(args.target)
    elif args.command == 'bundle':
        bundle(args.output)
    else:
        check(args.archive)


if __name__ == '__main__':
    main()
