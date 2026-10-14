"""MCP setup failures publish evidence before any real app can start."""
import builtins
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

from test_mcp_drive import ROOT, drive


def cli_fixture(root):
    script = root / 'docs/skills/inspect/scripts/mcp-drive.py'
    script.parent.mkdir(parents=True)
    shutil.copy2(ROOT / 'docs/skills/inspect/scripts/mcp-drive.py', script)
    exe = root / 'fake-app'
    exe.write_text(f'#!{sys.executable}\nfrom pathlib import Path\n'
                   "Path(__file__).with_name('launched').write_text('started')\n")
    exe.chmod(0o755)
    calls = root / 'calls.json'
    calls.write_text('[]')
    (root / 'tmp').mkdir()
    return script, exe, calls


class McpSetupTests(unittest.TestCase):
    def test_invalid_calls_file_publishes_failure_and_replaces_the_previous_latest(self):
        with tempfile.TemporaryDirectory() as scratch:
            root = Path(scratch).resolve()
            script, exe, calls = cli_fixture(root)
            calls.write_text('not json')
            scenario = root / 'target/snappyref-inspect/mcp-drive'
            previous = scenario / 'run-previous'
            previous.mkdir(parents=True)
            (scenario / 'latest.txt').write_text(str(previous))
            result = subprocess.run([sys.executable, str(script), str(calls), '--exe', str(exe)],
                                    env={**os.environ, 'TMPDIR': str(root / 'tmp')}, capture_output=True, text=True)
            self.assertEqual(result.returncode, 1, result.stderr)
            artifact = Path((scenario / 'latest.txt').read_text())
            self.assertNotEqual(artifact, previous)
            self.assertEqual(artifact.parent, scenario)
            failure = {'type': 'assertion', 'passed': False,
                       'name': 'the drive finishes: Expecting value: line 1 column 1 (char 0)'}
            report = json.loads((artifact / 'report.json').read_text())
            self.assertEqual(report, {
                'scenario': 'mcp-drive', 'status': 'fail', 'artifact_dir': str(artifact),
                'first_failure': failure['name'], 'assertions': [failure], 'frames': [],
            })
            self.assertIs(report['assertions'][0]['passed'], False)
            self.assertEqual([json.loads(line) for line in (artifact / 'events.ndjson').read_text().splitlines()],
                             [failure])
            self.assertFalse((root / 'launched').exists())

    def test_setup_and_failure_logging_errors_still_publish_the_failed_report(self):
        for stage in ('frames', 'data-root', 'events'):
            with self.subTest(stage=stage), tempfile.TemporaryDirectory() as scratch:
                root = Path(scratch).resolve()
                _, exe, calls = cli_fixture(root)
                mkdir, open_file = Path.mkdir, builtins.open

                def create_directory(path, *args, **kwargs):
                    if stage == 'frames' and path.name == 'frames':
                        raise OSError('frame directory unavailable')
                    return mkdir(path, *args, **kwargs)

                def open_event(path, *args, **kwargs):
                    if stage == 'events' and Path(path).name == 'events.ndjson':
                        raise OSError('event log unavailable')
                    return open_file(path, *args, **kwargs)

                temp_root = patch.object(drive.tempfile, 'mkdtemp', side_effect=OSError('data root unavailable')) \
                    if stage == 'data-root' else patch.object(drive, 'grant_full_access', side_effect=RuntimeError('startup failed'))
                with patch.object(drive, 'ROOT', root), patch.object(Path, 'mkdir', create_directory), \
                        patch('builtins.open', open_event), temp_root, \
                        patch('sys.argv', ['mcp-drive', str(calls), '--exe', str(exe)]):
                    self.assertEqual(drive.main(), 1)
                artifact = Path((root / 'target/snappyref-inspect/mcp-drive/latest.txt').read_text())
                failure = {'frames': 'frame directory unavailable', 'data-root': 'data root unavailable',
                           'events': 'startup failed'}[stage]
                assertions = [{'type': 'assertion', 'passed': False, 'name': f'the drive finishes: {failure}'}]
                if stage == 'events':
                    assertions.append({'type': 'assertion', 'passed': False,
                                       'name': 'events are recorded: event log unavailable'})
                report = json.loads((artifact / 'report.json').read_text())
                self.assertEqual(report['status'], 'fail')
                self.assertEqual(report['first_failure'], f'the drive finishes: {failure}')
                self.assertEqual(report['assertions'], assertions)
                for assertion in report['assertions']:
                    self.assertIs(assertion['passed'], False)
                self.assertEqual(report['frames'], [])

    def test_nonpositive_or_nonfinite_deadlines_are_usage_errors_before_startup(self):
        for value in ('0', '-1', 'nan', 'inf', '-inf'):
            with self.subTest(timeout=value), tempfile.TemporaryDirectory() as scratch:
                root = Path(scratch).resolve()
                script, exe, calls = cli_fixture(root)
                process = subprocess.Popen([sys.executable, str(script), str(calls), '--exe', str(exe),
                                            f'--timeout={value}'], start_new_session=True,
                                           env={**os.environ, 'TMPDIR': str(root / 'tmp')},
                                           stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
                try:
                    _, errors = process.communicate(timeout=10)
                except subprocess.TimeoutExpired:
                    os.killpg(process.pid, signal.SIGKILL)
                    _, errors = process.communicate()
                self.assertEqual(process.returncode, 2, errors)
                self.assertIn('--timeout', errors)
                self.assertFalse((root / 'launched').exists())


if __name__ == '__main__':
    unittest.main()
