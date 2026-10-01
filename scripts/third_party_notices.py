#!/usr/bin/env python3
"""Write THIRD_PARTY_NOTICES.txt for an app archive from Cargo.lock and the non-crate components.

Release packaging runs this after the build, so the notices always match the locked crates.
"""
import argparse
import json
from pathlib import Path
import re
import subprocess
import textwrap

import semantic_model

ROOT = Path(__file__).resolve().parents[1]
LICENSES = ROOT / 'assets/licenses'
LICENSE_FILE = re.compile(r'(licen[cs]e|copying|notice|unlicense|copyright)', re.I)
# License files for native code or fonts that a crate carries below its root.
BUNDLED = {
    'epaint_default_fonts': 'fonts/*.txt',
    'onig_sys': 'oniguruma/COPYING',
    'turbojpeg-sys': 'libjpeg-turbo/LICENSE.md',
}
RULE = '\n\n' + '=' * 78 + '\n\n'
FONT_AUTHORS = re.compile(r'^Copyright .*? The (.+?) Project Authors', re.M)


def shipped_packages(metadata: dict) -> list[dict]:
    """Every package reachable from SnappyRef through normal or build edges, on any platform."""
    packages = {package['id']: package for package in metadata['packages']}
    nodes = {node['id']: node for node in metadata['resolve']['nodes']}
    root = metadata['resolve']['root']
    seen, pending = set(), [root]
    while pending:
        node = pending.pop()
        if node not in seen:
            seen.add(node)
            pending += [dep['pkg'] for dep in nodes[node]['deps']
                        if any(kind['kind'] != 'dev' for kind in dep['dep_kinds'])]
    seen.discard(root)
    return sorted((packages[node] for node in seen), key=lambda package: (package['name'], package['version']))


def license_texts(package: dict) -> list[str]:
    """The crate's own license files, or the standard text of a license it offers when it ships none."""
    crate = Path(package['manifest_path']).parent
    # Sort by name: Windows paths compare case-insensitively, which would reorder the notices.
    files = sorted((path for path in crate.iterdir() if path.is_file() and LICENSE_FILE.match(path.name)),
                   key=lambda path: path.name)
    if not files:
        offered = re.findall(r'[\w.+-]+', package['license'] or '')
        standard = [LICENSES / f'{name}.txt' for name in offered if (LICENSES / f'{name}.txt').is_file()]
        if not standard:
            raise ValueError(f'{package["name"]} {package["version"]} ships no license file and offers '
                             f'no license in {LICENSES}: {package["license"]}')
        files = standard[:1]
    if package['name'] in BUNDLED:
        files += sorted(crate.glob(BUNDLED[package['name']]), key=lambda path: path.name)
    return [path.read_text(encoding='utf-8', errors='replace').strip() for path in files]


def onnx_runtime_section(packages: list[dict]) -> str:
    """ort-sys statically links a prebuilt ONNX Runtime whose version its download list pins."""
    ort_sys = next(package for package in packages if package['name'] == 'ort-sys')
    dist = (Path(ort_sys['manifest_path']).parent / 'build/download/dist.txt').read_text(encoding='utf-8')
    versions = set(re.findall(r'/ms@([\d.]+)/', dist))
    if len(versions) != 1:
        raise ValueError(f'expected one ONNX Runtime version in ort-sys {ort_sys["version"]}, found {versions}')
    version = versions.pop()
    vendored = LICENSES / f'onnxruntime-{version}'
    if not vendored.is_dir():
        raise ValueError(f'ort-sys {ort_sys["version"]} links ONNX Runtime {version}, but {vendored} is missing. '
                         f'Replace the onnxruntime-* folder with LICENSE and ThirdPartyNotices.txt '
                         f'from microsoft/onnxruntime v{version}.')
    texts = [(vendored / name).read_text(encoding='utf-8').strip() for name in ('LICENSE', 'ThirdPartyNotices.txt')]
    return f'ONNX Runtime {version}, statically linked through the ort crate\n\n' + '\n\n'.join(texts)


def font_sections() -> list[str]:
    """One section per bundled font license, headed by the font name from its copyright line."""
    sections = []
    for path in sorted(ROOT.glob('assets/fonts/*-OFL.txt'), key=lambda path: path.name):
        text = path.read_text(encoding='utf-8').strip()
        authors = FONT_AUTHORS.search(text)
        if not authors:
            raise ValueError(f'{path} has no "Copyright ... The <font> Project Authors" line')
        sections.append(f'{authors[1]}\n\n{text}')
    return sections


def notices(metadata: dict) -> str:
    packages = shipped_packages(metadata)
    users: dict[str, list[str]] = {}
    texts: dict[str, str] = {}
    for package in packages:
        # Whitespace-insensitive keys merge reflowed copies of the same license text.
        for key, text in {' '.join(text.split()): text for text in license_texts(package)}.items():
            texts.setdefault(key, text)
            users.setdefault(key, []).append(f'{package["name"]} {package["version"]}')
    crates = '\n'.join(f'{package["name"]} {package["version"]}: {package["license"]}' for package in packages)
    sections = [
        'SnappyRef third-party notices\n\n'
        'SnappyRef includes the third-party software listed below. Where a crate offers a choice of\n'
        'licenses, SnappyRef uses a permissive one. self_cell (Apache-2.0 OR GPL-2.0-only) is used\n'
        'under Apache-2.0.',
        onnx_runtime_section(packages),
        'libjpeg-turbo, statically linked through the turbojpeg crate\n\n'
        'This software is based in part on the work of the Independent JPEG Group.\n'
        'Its license is listed with turbojpeg-sys below.',
        *font_sections(),
        'SigLIP 2 model, downloaded by Semantic Search and not part of this archive\n\n'
        + semantic_model.NOTICE.strip() + '\n\n' + (LICENSES / 'Apache-2.0.txt').read_text(encoding='utf-8').strip(),
        f'Rust crates\n\n{crates}',
        *(textwrap.fill('Used by: ' + ', '.join(users[key]), 98, break_on_hyphens=False)
          + '\n\n' + texts[key] for key in texts),
    ]
    return RULE.join(sections) + '\n'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('output', type=Path)
    args = parser.parse_args()
    metadata = json.loads(subprocess.run(['cargo', 'metadata', '--locked', '--format-version', '1'],
                                         cwd=ROOT, check=True, capture_output=True, encoding='utf-8').stdout)
    args.output.write_text(notices(metadata), encoding='utf-8')


if __name__ == '__main__':
    main()
