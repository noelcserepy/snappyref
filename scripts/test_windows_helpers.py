"""Exercise the SSH helper entry points without a private Windows machine."""
import json
import shutil
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
import json, os, shlex, subprocess, sys, time
args = sys.argv[1:]
with open(os.environ['SSH_LOG'], 'a') as log: log.write(json.dumps(args) + '\\n')
if 'wake-relay-placeholder' in args:
    sys.exit(subprocess.run(shlex.split(args[-1]), input=sys.stdin.read(), text=True).returncode)
if os.environ.get('SSH_HANG'): time.sleep(30)
sys.exit(int(os.environ.get('SSH_RESULT', '0')))
''')
        (self.root / 'ssh').chmod(0o755)

    def run_script(self, name):
        return subprocess.run(['bash', str(ROOT / 'scripts' / name)], cwd=ROOT,
                              env=self.env, text=True, capture_output=True, timeout=30)

    def calls(self):
        return [json.loads(line) for line in self.log.read_text().splitlines()]

    def test_signed_out_launcher_fails_before_launch_and_signed_in_launches(self):
        powershell = os.environ.get('SNAPPYREF_TEST_PWSH') or shutil.which('pwsh')
        if not powershell:
            self.skipTest('PowerShell runtime not installed; set SNAPPYREF_TEST_PWSH')
        self.env.update(REAL_PWSH=powershell, USERNAME='account-placeholder',
                        USERDOMAIN='EXAMPLE', TEMP=str(self.root),
                        LAUNCH_LOG=str(self.root / 'launch.json'))
        prelude = self.root / 'adapters.ps1'
        prelude.write_text(r'''function Set-Location { param($Path) }
function Get-Location { [pscustomobject]@{Path = 'C:\checkout-placeholder'} }
function Join-Path {
    param($Path, $ChildPath)
    if ($Path -eq $env:TEMP) {
        Microsoft.PowerShell.Management\Join-Path $Path $ChildPath
    } else { "$Path\$ChildPath" }
}
function Get-CimInstance {
    param($ClassName)
    switch ($env:SESSION_STATE) {
        'absent' { }
        'other-owner' { [pscustomobject]@{Name='explorer.exe'; SessionId=1; Owner='other-account'} }
        'other-domain' { [pscustomobject]@{Name='explorer.exe'; SessionId=1; Owner=$env:USERNAME} }
        'other-process' { [pscustomobject]@{Name='other.exe'; SessionId=1; Owner=$env:USERNAME} }
        'session-zero' { [pscustomobject]@{Name='explorer.exe'; SessionId=0; Owner=$env:USERNAME} }
        'signed-in' { [pscustomobject]@{Name='explorer.exe'; SessionId=1; Owner=$env:USERNAME} }
    }
}
function Invoke-CimMethod {
    param($InputObject, $MethodName)
    $domain = if ($env:SESSION_STATE -eq 'other-domain') { 'OTHER' } else { 'EXAMPLE' }
    [pscustomobject]@{User=$InputObject.Owner; Domain=$domain}
}
function Get-Process { param($Name, $ErrorAction) }
function Stop-Process { param([switch]$Force) }
function Start-Sleep { param($Milliseconds) }
function Start-Process {
    param($FilePath, $WorkingDirectory)
    @{executable=$FilePath; directory=$WorkingDirectory} | ConvertTo-Json |
        Microsoft.PowerShell.Management\Set-Content $env:LAUNCH_LOG
}
function schtasks {
    switch ($args[0]) {
        '/Create' { $global:taskCommand = $args[[array]::IndexOf($args, '/TR') + 1] }
        '/Run' { Invoke-Expression $global:taskCommand }
        '/Query' { $global:LASTEXITCODE = 1; return }
    }
    $global:LASTEXITCODE = 0
}
''')
        self.env['PS_ADAPTERS'] = str(prelude)
        (self.root / 'powershell').write_text('''#!/usr/bin/env python3
import base64, os, subprocess, sys
from pathlib import Path
args = sys.argv[1:]
if '-EncodedCommand' in args:
    body = base64.b64decode(args[args.index('-EncodedCommand') + 1]).decode('utf-16le')
elif '-File' in args:
    body = Path(args[args.index('-File') + 1]).read_text(encoding='utf-8-sig')
else:
    body = args[args.index('-Command') + 1]
prelude = Path(os.environ['PS_ADAPTERS']).read_text()
sys.exit(subprocess.run([os.environ['REAL_PWSH'], '-NoProfile', '-Command', prelude + '\\n' + body]).returncode)
''')
        (self.root / 'powershell').chmod(0o755)
        (self.root / 'powershell.exe').symlink_to(self.root / 'powershell')
        (self.root / 'ssh').write_text('''#!/usr/bin/env python3
import shlex, subprocess, sys
sys.exit(subprocess.run(shlex.split(sys.argv[-1])).returncode)
''')
        launch_log = self.root / 'launch.json'
        for state in ['absent', 'other-owner', 'other-domain', 'other-process', 'session-zero']:
            with self.subTest(state=state):
                self.env['SESSION_STATE'] = state
                result = self.run_script('windows-launch-gui.sh')
                self.assertEqual(result.returncode, 1, result.stderr)
                self.assertIn('Sign in at the PC first.', result.stderr)
                self.assertFalse(launch_log.exists())
        self.env['SESSION_STATE'] = 'signed-in'
        result = self.run_script('windows-launch-gui.sh')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads(launch_log.read_text()), {
            'executable': r'C:\checkout-placeholder\target\release-fast\snappyref.exe',
            'directory': r'C:\checkout-placeholder',
        })

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
