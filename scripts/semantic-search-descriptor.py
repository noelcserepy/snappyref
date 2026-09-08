#!/usr/bin/env python3
"""Resolve build overrides or verify the descriptors attached to selected app builds."""
import argparse
import os
from pathlib import Path
import re

DEFAULT = Path(__file__).resolve().parents[1] / 'assets/semantic-search-descriptor.env'
KEYS = ('semantic_search_url', 'semantic_search_sha256', 'semantic_search_size')


def read(path):
    values = {}
    for line in path.read_text(encoding='utf-8-sig').splitlines():
        key, separator, value = line.partition('=')
        if not separator or key in values:
            raise ValueError(f'invalid descriptor line in {path}')
        values[key] = value
    validate(values)
    return values


def validate(values):
    if set(values) != set(KEYS):
        raise ValueError('descriptor requires exactly URL, SHA256 and size')
    url = values[KEYS[0]]
    rest = next((url[len(prefix):] for prefix in ('http://', 'https://') if url.startswith(prefix)), '')
    if not re.split(r'[/\?#]', rest, maxsplit=1)[0] or any(char.isspace() for char in url):
        raise ValueError('descriptor requires HTTP URL')
    if not re.fullmatch(r'[0-9a-fA-F]{64}', values[KEYS[1]]):
        raise ValueError('descriptor requires SHA256')
    if not re.fullmatch(r'[1-9][0-9]*', values[KEYS[2]]) or int(values[KEYS[2]]) > 2**64 - 1:
        raise ValueError('descriptor requires positive u64 byte size')
    if any('\n' in value or '\r' in value for value in values.values()):
        raise ValueError('descriptor values must be single lines')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest='command', required=True)
    resolve = commands.add_parser('resolve')
    resolve.add_argument('--output', required=True, type=Path)
    resolve.add_argument('--github-env', type=Path)
    verify = commands.add_parser('verify')
    verify.add_argument('descriptors', nargs='+', type=Path)
    args = parser.parse_args()
    if args.command == 'verify':
        descriptors = [read(path) for path in args.descriptors]
        if any(value != descriptors[0] for value in descriptors[1:]):
            raise ValueError('selected app builds have different Semantic Search descriptors')
        return
    values = read(DEFAULT)
    for key in KEYS:
        override = os.environ.get('SNAPPYREF_' + key.upper())
        if override:
            values[key] = override
    validate(values)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(''.join(f'{key}={values[key]}\n' for key in KEYS))
    if args.github_env:
        with args.github_env.open('a') as target:
            target.write(''.join(f'SNAPPYREF_{key.upper()}={values[key]}\n' for key in KEYS))


if __name__ == '__main__':
    main()
