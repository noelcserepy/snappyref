"""Inspect success requires evidence from the invocation that just ran."""
import json
from pathlib import Path
import tempfile
import time
import unittest
from unittest.mock import patch

from inspect_scenarios import check_artifact, main, SCENARIOS


class InspectEvidenceTests(unittest.TestCase):
    def test_missing_stale_failed_and_incomplete_reports_fail(self):
        with tempfile.TemporaryDirectory() as scratch:
            root = Path(scratch)
            started = time.time()
            with self.assertRaises(RuntimeError):
                check_artifact('sample', started, root)
            directory = root / 'target/snappyref-inspect/sample'
            directory.mkdir(parents=True)
            frame = directory / 'frame.png'
            frame.write_bytes(b'fixture')
            (directory / 'latest.txt').write_text(str(directory))
            report = {'status': 'pass', 'frames': [str(frame)], 'assertions': [{'passed': True}]}
            (directory / 'report.json').write_text(json.dumps(report))
            self.assertEqual(check_artifact('sample', started, root)['assertions'], 1)
            with self.assertRaises(RuntimeError):
                check_artifact('sample', time.time() + 1, root)
            for change in ({'status': 'fail'}, {'frames': []}, {'assertions': []}, {'assertions': [{'passed': False}]}):
                (directory / 'report.json').write_text(json.dumps({**report, **change}))
                with self.assertRaises(RuntimeError):
                    check_artifact('sample', started, root)

    def test_zero_cargo_matches_cannot_report_success(self):
        with patch('sys.argv', ['inspect', 'sample-image-import']), patch('inspect_scenarios.subprocess.run') as cargo, patch('inspect_scenarios.check_artifact', side_effect=RuntimeError('missing artifact')):
            cargo.return_value.returncode = 0
            self.assertEqual(main(), 1)

    def test_catalogue_maps_to_existing_tests_and_preserves_refactor_coverage(self):
        root = Path(__file__).resolve().parents[1]
        source = '\n'.join(path.read_text() for path in (root / 'src').rglob('*.rs'))
        self.assertEqual(sum(item[2] for item in SCENARIOS.values()), 12)
        for test, _, _, _ in SCENARIOS.values():
            self.assertIn('fn ' + test + '(', source)
