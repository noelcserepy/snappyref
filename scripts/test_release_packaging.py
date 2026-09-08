"""Exercise both packaging entry points with local builds and no release/network calls."""
import os
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
                        archive.writestr('SnappyRef/snappyref.exe', b'fixture executable')
                    else:
                        for name in ('MacOS/snappyref', 'MacOS/snappyref-updater', 'Info.plist'):
                            archive.writestr('SnappyRef/SnappyRef.app/Contents/' + name, b'fixture')
                        archive.write(ROOT / 'assets/app-icon/snappyref.icns', 'SnappyRef/SnappyRef.app/Contents/Resources/snappyref.icns')
                (root / f'snappyref-{platform}-semantic-search-descriptor.env').write_bytes((ROOT / 'assets/semantic-search-descriptor.env').read_bytes())
            command = ['python3', str(ROOT / 'scripts/package-selected-release.py'), '--windows-tag', 'existing-windows', '--macos-tag', 'existing-macos', '--release-tag', 'vfixture', '--handoff-dir', str(root)]
            result = subprocess.run([*command, '--output', str(root / 'valid')], capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            with zipfile.ZipFile(root / 'valid/snappyref-windows-vfixture.zip') as archive:
                self.assertEqual(archive.read('SnappyRef/snappyref.exe'), b'fixture executable')
            with zipfile.ZipFile(root / 'valid/snappyref-mac-vfixture.zip') as archive:
                self.assertEqual(archive.read('SnappyRef/SnappyRef.app/Contents/Resources/snappyref.icns'), (ROOT / 'assets/app-icon/snappyref.icns').read_bytes())
            descriptor = root / 'snappyref-macos-semantic-search-descriptor.env'
            original = descriptor.read_text()
            for invalid in (original.replace('semantic_search_size=253732373', 'semantic_search_size=1'), original.replace('semantic_search_sha256=', 'unexpected=')):
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


if __name__ == '__main__':
    unittest.main()
