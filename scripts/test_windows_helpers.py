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
                        SNAPPYREF_WINDOWS_WAKE_RELAY='wake-relay-placeholder',
                        SNAPPYREF_WINDOWS_MAC='02:00:00:00:00:01',
                        SNAPPYREF_WINDOWS_WAKE_TIMEOUT_SECONDS='1',
                        SSH_LOG=str(self.log), PATH=f'{self.root}:{os.environ["PATH"]}')
        (self.root / 'socket.py').write_text('''import json, os
from pathlib import Path
AF_INET, SOCK_DGRAM, SOL_SOCKET, SO_BROADCAST = 2, 2, 1, 6
class socket:
    def __enter__(self): return self
    def __exit__(self, *args): pass
    def __init__(self, *args): pass
    def setsockopt(self, *args):
        assert args == (SOL_SOCKET, SO_BROADCAST, 1)
    def sendto(self, packet, address):
        Path(os.environ['PACKET_LOG']).write_text(json.dumps([packet.hex(), address]))
''')
        self.env['PACKET_LOG'] = str(self.root / 'packet.json')
        self.env['PYTHONPATH'] = str(self.root)
        (self.root / 'ssh').write_text('''#!/usr/bin/env python3
import base64, json, os, shlex, subprocess, sys, time
from pathlib import Path
args = sys.argv[1:]
with open(os.environ['SSH_LOG'], 'a') as log: log.write(json.dumps(args) + '\\n')
if 'wake-relay-placeholder' in args:
    sys.exit(subprocess.run(shlex.split(args[-1]), input=sys.stdin.read(), text=True).returncode)
if 'EncodedCommand' in args[-1]:
    script = base64.b64decode(args[-1].split()[-1].rstrip(chr(34))).decode('utf-16le')
    if 'Get-CimInstance Win32_Process' not in script: sys.exit(0)
if os.environ.get('SSH_HANG'): time.sleep(30)
sys.exit(int(os.environ.get('SSH_RESULT', '0')))
''')
        (self.root / 'ssh').chmod(0o755)

    def run_script(self, name):
        return subprocess.run(['bash', str(ROOT / 'scripts' / name)], cwd=ROOT,
                              env=self.env, text=True, capture_output=True, timeout=10)

    def calls(self):
        return [json.loads(line) for line in self.log.read_text().splitlines()]

    def test_signed_out_launcher_fails_before_launch_and_signed_in_launches(self):
        self.env['SSH_RESULT'] = '3'
        result = self.run_script('windows-launch-gui.sh')
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn('Sign in at the PC first.', result.stderr)
        self.assertEqual(len(self.calls()), 1)
        self.log.unlink()
        self.env['SSH_RESULT'] = '0'
        result = self.run_script('windows-launch-gui.sh')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(len(self.calls()), 2)
        self.assertIn('-EncodedCommand ', self.calls()[1][-1])

    def test_wake_relay_sends_magic_packet_then_waits_for_ssh(self):
        del self.env['SNAPPYREF_WINDOWS_REPO']
        result = self.run_script('windows-wake.sh')
        self.assertEqual(result.returncode, 0, result.stderr)
        packet, address = json.loads((self.root / 'packet.json').read_text())
        self.assertEqual(bytes.fromhex(packet), b'\xff' * 6 + b'\x02\x00\x00\x00\x00\x01' * 16)
        self.assertEqual(address, ['255.255.255.255', 9])
        self.assertEqual(len(self.calls()), 2)
        self.assertIn('wake-relay-placeholder', self.calls()[0])
        self.assertIn('windows-host-placeholder', self.calls()[1])

    def test_wake_requires_private_config_and_bounds_unreachable_ssh(self):
        self.env['SNAPPYREF_WINDOWS_MAC'] = ''
        result = self.run_script('windows-wake.sh')
        self.assertEqual(result.returncode, 2, result.stderr)
        self.assertIn('Set SNAPPYREF_WINDOWS_WAKE_RELAY and SNAPPYREF_WINDOWS_MAC', result.stderr)
        self.assertFalse(self.log.exists())
        self.env['SNAPPYREF_WINDOWS_MAC'] = '02:00:00:00:00:01'
        self.env['SSH_RESULT'] = '255'
        self.env['SSH_HANG'] = '1'
        result = self.run_script('windows-wake.sh')
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn('Windows SSH did not become reachable within 1 seconds.', result.stderr)
