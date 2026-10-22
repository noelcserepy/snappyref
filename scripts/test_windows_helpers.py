"""Exercise the SSH helper entry points without a private Windows machine."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]


class WindowsHelperTests(unittest.TestCase):
    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory()
        self.addCleanup(self.scratch.cleanup)
        self.root = Path(self.scratch.name)
        self.log = self.root / 'ssh.jsonl'
        self.env = dict(os.environ, SNAPPYREF_WINDOWS_ENV=str(self.root / 'env'),
                        SNAPPYREF_WINDOWS_HOST='windows-host-placeholder',
                        SNAPPYREF_WINDOWS_REPO=r'C:\checkout-placeholder',
                        SSH_LOG=str(self.log), PATH=f'{self.root}:{os.environ["PATH"]}')
        (self.root / 'ssh').write_text('''#!/usr/bin/env python3
import json, os, sys, time
args = sys.argv[1:]
with open(os.environ['SSH_LOG'], 'a') as log: log.write(json.dumps(args) + '\\n')
if os.environ.get('SSH_HANG'): time.sleep(30)
sys.exit(int(os.environ.get('SSH_RESULT', '0')))
''')
        (self.root / 'ssh').chmod(0o755)

    def run_script(self, name):
        return subprocess.run(['bash', str(ROOT / 'scripts' / name)], cwd=ROOT,
                              env=self.env, text=True, capture_output=True, timeout=30)

    def calls(self):
        return [json.loads(line) for line in self.log.read_text().splitlines()]

    def test_wake_runs_the_private_command_then_waits_for_ssh(self):
        del self.env['SNAPPYREF_WINDOWS_REPO']
        marker = self.root / 'woke'
        self.env['SNAPPYREF_WINDOWS_WAKE_CMD'] = f"echo 'wake args' > '{marker}'"
        result = self.run_script('windows-wake.sh')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(marker.read_text(), 'wake args\n')
        self.assertEqual(self.calls(), [['-n', '-o', 'BatchMode=yes', '-o', 'ConnectTimeout=5',
                                         '-o', 'ConnectionAttempts=1', '--',
                                         'windows-host-placeholder', 'exit']])
        self.assertIn('Windows SSH is reachable.', result.stdout)

    def test_wake_requires_a_command_and_bounds_unreachable_ssh(self):
        result = self.run_script('windows-wake.sh')
        self.assertEqual(result.returncode, 2, result.stderr)
        self.assertIn('Set SNAPPYREF_WINDOWS_WAKE_CMD in .env.windows', result.stderr)
        self.assertFalse(self.log.exists())
        # A failing command's own status, even 2, must not read as missing config.
        self.env['SNAPPYREF_WINDOWS_WAKE_CMD'] = 'exit 2'
        result = self.run_script('windows-wake.sh')
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn('The wake command failed.', result.stderr)
        self.assertFalse(self.log.exists())
        self.env['SNAPPYREF_WINDOWS_WAKE_CMD'] = 'true'
        self.env['SSH_RESULT'] = '255'
        self.env['SSH_HANG'] = '1'
        # Only the unreachable case needs a short bound; success returns on the first probe.
        self.env['SNAPPYREF_WINDOWS_WAKE_TIMEOUT_SECONDS'] = '1'
        result = self.run_script('windows-wake.sh')
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn('Windows SSH did not become reachable within 1 seconds.', result.stderr)
