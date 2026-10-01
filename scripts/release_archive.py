#!/usr/bin/env python3
"""Validate app-only release archives and copy accepted producer bytes unchanged."""
import argparse
import shutil
import stat
import zipfile
from pathlib import Path


def validate_archive(path: Path, platform: str) -> None:
    if platform not in ('windows', 'macos'):
        raise ValueError(f'unsupported platform: {platform}')
    with zipfile.ZipFile(path) as archive:
        names = set()
        for entry in archive.infolist():
            name = entry.filename
            parts = name.rstrip('/').split('/')
            if (parts[0] != 'SnappyRef' or any(part in ('', '.', '..') for part in parts)
                    or '\\' in name or ':' in name or name in names
                    or stat.S_ISLNK(entry.external_attr >> 16)):
                raise ValueError(f'unsafe or non-current archive path: {name!r}')
            names.add(name)
        if archive.testzip() is not None:
            raise ValueError('corrupt release archive')

    required = ['SnappyRef/snappyref.exe', 'SnappyRef/THIRD_PARTY_NOTICES.txt']
    forbidden = ['SnappyRef/UPDATE.txt', 'SnappyRef/models', 'SnappyRef/library',
                 'SnappyRef/library.db', 'SnappyRef/originals', 'SnappyRef/thumbs']
    if platform == 'macos':
        # Notices live inside the bundle so the macOS update swap carries them.
        prefix = 'SnappyRef/SnappyRef.app/Contents/'
        required = [prefix + name for name in ('MacOS/snappyref', 'MacOS/snappyref-updater', 'Info.plist',
                                               'Resources/snappyref.icns', 'Resources/THIRD_PARTY_NOTICES.txt')]
        forbidden.append(prefix + 'Resources/models')
    else:
        forbidden.append('SnappyRef/snappyref-updater.exe')
    for name in required:
        if name not in names:
            raise ValueError(f'missing required app-owned path: {name}')
    for path in forbidden:
        if any(name == path or name.startswith(path + '/') for name in names):
            raise ValueError(f'archive contains forbidden path: {path}')


def package_archive(source: Path, platform: str, output: Path) -> None:
    validate_archive(source, platform)
    shutil.copyfile(source, output)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--platform', choices=('windows', 'macos'), required=True)
    parser.add_argument('--app-archive', type=Path, required=True)
    parser.add_argument('--output', type=Path)
    args = parser.parse_args()
    if args.output:
        package_archive(args.app_archive, args.platform, args.output)
    else:
        validate_archive(args.app_archive, args.platform)


if __name__ == '__main__':
    main()
