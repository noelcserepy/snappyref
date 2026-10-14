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
                        SNAPPYREF_WINDOWS_WAKE_TIMEOUT_SECONDS='1',
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
        result = self.run_script('windows-wake.sh')
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn('Windows SSH did not become reachable within 1 seconds.', result.stderr)
