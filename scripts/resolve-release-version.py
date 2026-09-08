#!/usr/bin/env python3
"""Resolve release tags from the selected source version and reject mismatches."""
import argparse
from pathlib import Path
import tomllib

ROOT = Path(__file__).resolve().parents[1]


def resolve(tag='', manifest=None):
    version = tomllib.loads(manifest if manifest is not None else (ROOT / 'Cargo.toml').read_text())['package']['version']
    if tag and tag != 'v' + version:
        raise ValueError(f'release tag {tag!r} does not match source version v{version}')
    return version, 'v' + version


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--tag', default='')
    parser.add_argument('--github-output', type=Path)
    args = parser.parse_args()
    version, tag = resolve(args.tag)
    output = f'version={version}\ntag={tag}\n'
    if args.github_output:
        with args.github_output.open('a') as target:
            target.write(output)
    else:
        print(output, end='')


if __name__ == '__main__':
    main()
