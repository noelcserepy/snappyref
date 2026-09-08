#!/usr/bin/env python3
"""Package selected existing app builds. Never creates or publishes a release."""
import argparse
import plistlib
import struct
import zipfile
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]


def archive_version(path, platform):
    with zipfile.ZipFile(path) as archive:
        if platform == 'macos':
            info = plistlib.loads(archive.read('SnappyRef/SnappyRef.app/Contents/Info.plist'))
            version = info['CFBundleShortVersionString']
            if info['CFBundleVersion'] != version:
                raise ValueError('macOS bundle version fields disagree')
            return version
        data = archive.read('SnappyRef/snappyref.exe')
        # VS_FIXEDFILEINFO records the Cargo version in the shipped PE resource.
        signature = b'\xbd\x04\xef\xfe\x00\x00\x01\x00'
        first = data.find(signature)
        if first < 0 or data.find(signature, first + 1) >= 0:
            raise ValueError('Windows app requires one fixed version resource')
        major_minor, patch_build = struct.unpack_from('<II', data, first + 8)
        if patch_build & 0xffff:
            raise ValueError('unexpected Windows app build version')
        return f'{major_minor >> 16}.{major_minor & 0xffff}.{patch_build >> 16}'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--windows-tag')
    parser.add_argument('--macos-tag')
    parser.add_argument('--release-tag', required=True)
    parser.add_argument('--output', required=True, type=Path)
    parser.add_argument('--handoff-dir', type=Path, help='Use local archive/descriptor fixtures without downloading')
    args = parser.parse_args()
    selected = [(platform, tag) for platform, tag in [('windows', args.windows_tag), ('macos', args.macos_tag)] if tag]
    if not selected:
        parser.error('select at least one platform build')
    if '/' in args.release_tag or '\\' in args.release_tag or args.release_tag in ('.', '..'):
        parser.error('release tag must be a filename component')
    with tempfile.TemporaryDirectory(prefix='snappyref-release-handoff-') as scratch:
        handoff = args.handoff_dir.resolve() if args.handoff_dir else Path(scratch)
        descriptors = []
        for platform, tag in selected:
            archive = f'snappyref-{platform}-app.zip'
            descriptor = f'snappyref-{platform}-semantic-search-descriptor.env'
            if not args.handoff_dir:
                subprocess.run(['gh', 'release', 'download', tag, '--repo', 'noelcserepy/snappyref', '--pattern', archive, '--pattern', descriptor,
                                '--dir', str(handoff), '--clobber'], cwd=ROOT, check=True)
            if not (handoff / archive).is_file():
                raise ValueError(f'missing app archive: {archive}')
            version = archive_version(handoff / archive, platform)
            if args.release_tag != 'v' + version:
                raise ValueError(f'{platform} build version {version} does not match release tag {args.release_tag}')
            descriptors.append(str(handoff / descriptor))
        subprocess.run(['python3', str(ROOT / 'scripts/semantic-search-descriptor.py'), 'verify', *descriptors], check=True)
        args.output.mkdir(parents=True, exist_ok=True)
        for platform, _ in selected:
            output_name = 'mac' if platform == 'macos' else platform
            subprocess.run([str(ROOT / 'scripts/package-release-assets.sh'), '--platform', platform,
                            '--app-archive', str(handoff / f'snappyref-{platform}-app.zip'),
                            '--output', str(args.output.resolve() / f'snappyref-{output_name}-{args.release_tag}.zip')], cwd=ROOT, check=True)


if __name__ == '__main__':
    main()
