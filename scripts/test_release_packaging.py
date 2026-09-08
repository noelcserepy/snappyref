"""Exercise both packaging entry points with local builds and no release/network calls."""
import os
import re
import plistlib
import struct
from pathlib import Path
import subprocess
import tempfile
import unittest
import zipfile

ROOT = Path(__file__).resolve().parents[1]


class ReleasePackagingTests(unittest.TestCase):
    def test_selected_builds_package_and_reject_mismatched_or_invalid_descriptors(self):
        with tempfile.TemporaryDirectory() as scratch:
            root = Path(scratch)
            for platform in ('windows', 'macos'):
                with zipfile.ZipFile(root / f'snappyref-{platform}-app.zip', 'w') as archive:
                    if platform == 'windows':
                        archive.writestr('SnappyRef/snappyref.exe', b'fixture executable' + struct.pack('<IIII', 0xFEEF04BD, 0x10000, 2, 0))
                    else:
                        for name in ('MacOS/snappyref', 'MacOS/snappyref-updater'):
                            archive.writestr('SnappyRef/SnappyRef.app/Contents/' + name, b'fixture')
                        archive.writestr('SnappyRef/SnappyRef.app/Contents/Info.plist', plistlib.dumps(dict(CFBundleVersion='0.2.0', CFBundleShortVersionString='0.2.0')))
                        archive.write(ROOT / 'assets/app-icon/snappyref.icns', 'SnappyRef/SnappyRef.app/Contents/Resources/snappyref.icns')
                (root / f'snappyref-{platform}-semantic-search-descriptor.env').write_bytes((ROOT / 'assets/semantic-search-descriptor.env').read_bytes())
            command = ['python3', str(ROOT / 'scripts/package-selected-release.py'), '--windows-tag', 'existing-windows', '--macos-tag', 'existing-macos', '--release-tag', 'v0.2.0', '--handoff-dir', str(root)]
            result = subprocess.run([*command, '--output', str(root / 'valid')], capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            with zipfile.ZipFile(root / 'valid/snappyref-windows-v0.2.0.zip') as archive:
                self.assertTrue(archive.read('SnappyRef/snappyref.exe').startswith(b'fixture executable'))
            with zipfile.ZipFile(root / 'valid/snappyref-mac-v0.2.0.zip') as archive:
                self.assertEqual(archive.read('SnappyRef/SnappyRef.app/Contents/Resources/snappyref.icns'), (ROOT / 'assets/app-icon/snappyref.icns').read_bytes())
            descriptor = root / 'snappyref-macos-semantic-search-descriptor.env'
            original = descriptor.read_text()
            for invalid in (re.sub(r'(?m)^semantic_search_size=(\d+)$', lambda match: 'semantic_search_size=' + str(int(match[1]) + 1), original), original.replace('semantic_search_sha256=', 'unexpected=')):
                descriptor.write_text(invalid)
                result = subprocess.run([*command, '--output', str(root / 'rejected')], capture_output=True, text=True)
                self.assertNotEqual(result.returncode, 0)
                self.assertFalse((root / 'rejected').exists())

    def test_build_override_is_written_to_both_descriptor_and_compiler_environment(self):
        with tempfile.TemporaryDirectory() as scratch:
            root = Path(scratch)
            env = {**os.environ, 'SNAPPYREF_SEMANTIC_SEARCH_URL': 'https://example.com/model.zip', 'SNAPPYREF_SEMANTIC_SEARCH_SHA256': 'a' * 64, 'SNAPPYREF_SEMANTIC_SEARCH_SIZE': '42'}
            subprocess.run(['python3', str(ROOT / 'scripts/semantic-search-descriptor.py'), 'resolve', '--output', str(root / 'descriptor.env'), '--github-env', str(root / 'github.env')], env=env, check=True)
            values = dict(line.split('=', 1) for line in (root / 'descriptor.env').read_text().splitlines())
            compiled = dict(line.split('=', 1) for line in (root / 'github.env').read_text().splitlines())
            self.assertEqual(compiled, {'SNAPPYREF_' + key.upper(): value for key, value in values.items()})
            self.assertEqual(values['semantic_search_size'], '42')


class DescriptorUrlTests(unittest.TestCase):
    def test_python_and_rust_share_narrow_url_contract(self):
        import json
        import runpy
        import shutil
        validate = runpy.run_path(str(ROOT / 'scripts/semantic-search-descriptor.py'))['validate']
        cases = json.loads((ROOT / 'tests/fixtures/semantic-descriptor-urls.json').read_text())
        for case in cases:
            values = dict(semantic_search_url=case['url'], semantic_search_sha256='a' * 64, semantic_search_size='1')
            if case['valid']:
                validate(values)
            else:
                with self.assertRaises(ValueError, msg=case['url']):
                    validate(values)
        if not shutil.which('rustc'):
            self.skipTest('rustc unavailable for shared Rust URL cases')
        source = (ROOT / 'build.rs').read_text()
        start = source.index('fn valid_descriptor_url(')
        function = source[start:source.index('\n}', start) + 2]
        assertions = '\n'.join('assert_eq!(valid_descriptor_url(' + json.dumps(case['url'], ensure_ascii=False) + '), ' + str(case['valid']).lower() + ');' for case in cases)
        with tempfile.TemporaryDirectory() as scratch:
            path = Path(scratch)
            (path / 'urls.rs').write_text(function + '\nfn main() {\n' + assertions + '\n}\n')
            subprocess.run(['rustc', str(path / 'urls.rs'), '-o', str(path / 'urls')], check=True)
            subprocess.run([str(path / 'urls')], check=True)

    def test_final_archive_bytes_are_preserved_and_obsolete_layouts_rejected(self):
        with tempfile.TemporaryDirectory() as scratch:
            root = Path(scratch)
            archive = root / 'input.zip'
            output = root / 'output.zip'
            for names, valid in ((['SnappyRef/snappyref.exe'], True), (['snappyref.exe'], False),
                                 (['SnappyRef/snappyref.exe', 'SnappyRef/UPDATE.txt'], False),
                                 (['SnappyRef/snappyref.exe', 'SnappyRef/../outside'], False)):
                with zipfile.ZipFile(archive, 'w') as target:
                    for name in names:
                        target.writestr(name, b'fixture')
                output.unlink(missing_ok=True)
                result = subprocess.run([str(ROOT / 'scripts/package-release-assets.sh'), '--platform', 'windows', '--app-archive', str(archive), '--output', str(output)], capture_output=True)
                self.assertEqual(result.returncode == 0, valid, result.stderr)
                if valid:
                    self.assertEqual(archive.read_bytes(), output.read_bytes())
                else:
                    self.assertFalse(output.exists())

if __name__ == '__main__':
    unittest.main()
