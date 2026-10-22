"""Inspect success requires evidence from the invocation that just ran."""
import json
from pathlib import Path
import re
import subprocess
import tempfile
import time
import struct
import zlib
import unittest
from unittest.mock import patch

from inspect_scenarios import check_artifact, main, SCENARIOS


def png_bytes():
    def chunk(kind, data):
        return struct.pack('>I', len(data)) + kind + data + struct.pack('>I', zlib.crc32(kind + data))
    return (b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', 1, 1, 8, 6, 0, 0, 0))
            + chunk(b'IDAT', zlib.compress(b'\0\xff\0\0\xff')) + chunk(b'IEND', b''))


def missing_doc_paths(root, top_level):
    """Backticked repo paths in the inspect docs that do not exist, as `doc: path`.

    A token counts as a repo path when its first segment is in `top_level`, the
    repo's tracked top-level entries, so `scripts/x.py` is checked while
    `target/...` and `frames/` are not.
    """
    missing = []
    for doc in sorted((root / 'docs/skills/inspect').rglob('*.md')):
        for code in re.findall(r'`([^`\n]+)`', doc.read_text()):
            for token in code.split():
                path = re.sub(r':\d+(-\d+)?$', '', token)
                if '/' in path and path.split('/')[0] in top_level and not (root / path).exists():
                    missing.append(f'{doc.relative_to(root)}: {path}')
    return missing


class InspectEvidenceTests(unittest.TestCase):
    def test_missing_stale_failed_and_incomplete_reports_fail(self):
        with tempfile.TemporaryDirectory() as scratch, patch.dict('os.environ', {'CARGO_TARGET_DIR': 'build'}):
            root = Path(scratch)
            started = time.time()
            with self.assertRaises((RuntimeError, ValueError)):
                check_artifact('sample', started, root)
            directory = root / 'build/snappyref-inspect/sample'
            directory.mkdir(parents=True)
            frame = directory / 'frame.png'
            frame.write_bytes(png_bytes())
            (directory / 'latest.txt').write_text(str(directory))
            report = {'scenario': 'sample', 'artifact_dir': str(directory), 'status': 'pass',
                      'frames': [str(frame)], 'assertions': [{'name': 'image visible', 'passed': True}]}
            (directory / 'report.json').write_text(json.dumps(report))
            self.assertEqual(check_artifact('sample', started, root),
                             {'scenario': 'sample', 'artifact': str(directory), 'assertions': 1})
            with self.assertRaises((RuntimeError, ValueError)):
                check_artifact('sample', time.time() + 1, root)
            for change in ({'status': 'fail'}, {'frames': []}, {'assertions': []}, {'assertions': [{'name': 'image visible', 'passed': False}]}, {'scenario': 'other'},
                           {'artifact_dir': str(root)}, {'assertions': [{'name': 'image visible', 'passed': 'yes'}]}):
                (directory / 'report.json').write_text(json.dumps({**report, **change}))
                with self.assertRaises((RuntimeError, ValueError)):
                    check_artifact('sample', started, root)

            (directory / 'report.json').write_text(json.dumps(report))
            frame.write_bytes(b'corrupt PNG')
            with self.assertRaises(ValueError):
                check_artifact('sample', started, root)

    def test_zero_cargo_matches_cannot_report_success(self):
        with tempfile.TemporaryDirectory() as scratch, patch.dict('os.environ', {'CARGO_TARGET_DIR': 'target/test-inspect-no-evidence'}), patch('sys.argv', ['inspect', 'sample-image-import']), patch('inspect_scenarios.subprocess.run') as cargo:
            cargo.return_value.returncode = 0
            self.assertEqual(main(), 1)

    def test_catalogue_maps_to_existing_tests(self):
        root = Path(__file__).resolve().parents[1]
        source = '\n'.join(path.read_text() for path in (root / 'src').rglob('*.rs'))
        for test, _, _ in SCENARIOS.values():
            self.assertIn('fn ' + test + '(', source)

    def test_inspect_docs_name_only_existing_repo_paths(self):
        root = Path(__file__).resolve().parents[1]
        tracked = subprocess.run(['git', 'ls-files'], cwd=root, capture_output=True, text=True, check=True).stdout
        self.assertEqual(missing_doc_paths(root, {line.split('/')[0] for line in tracked.splitlines()}), [])

    def test_missing_doc_path_is_reported(self):
        with tempfile.TemporaryDirectory() as scratch:
            root = Path(scratch)
            (root / 'docs/skills/inspect/scripts').mkdir(parents=True)
            (root / 'docs/skills/inspect/scripts/mcp-drive.py').write_text('')
            (root / 'docs/skills/inspect/map.md').write_text(
                'Run `docs/skills/inspect/scripts/mcp-drive.py scripts/mcp-drive.py` '
                'and read `docs/skills/inspect/map.md:3`, `target/run/` and `frames/`.\n')
            self.assertEqual(missing_doc_paths(root, {'docs', 'scripts'}),
                             ['docs/skills/inspect/map.md: scripts/mcp-drive.py'])
