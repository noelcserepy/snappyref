"""Drive windows-ui.sh against a stand-in PC: ssh runs the command in a local
PowerShell, as the PC's SSH shell does, scp copies under a fake home, and the
scheduled task starts host.ps1 as a real background process."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]

ADAPTERS = r'''
function Add-Type {}
class SnappyRefUi { static [bool] SetProcessDPIAware() { return $true } }
function Set-Location { param($Path) }
function Join-Path {
    param($Path, $ChildPath)
    if ($Path -like 'C:\*') { "$Path\$ChildPath" }
    else { Microsoft.PowerShell.Management\Join-Path $Path $ChildPath }
}
function Get-CimInstance {
    param($ClassName)
    [pscustomobject]@{Name='powershell.exe'; ProcessId=$PID; SessionId=0; Owner='S-1-account'}
    switch ($env:SESSION_STATE) {
        'other-owner' { [pscustomobject]@{Name='explorer.exe'; ProcessId=1; SessionId=1; Owner='S-1-other'} }
        'other-process' { [pscustomobject]@{Name='other.exe'; ProcessId=1; SessionId=1; Owner='S-1-account'} }
        'session-zero' { [pscustomobject]@{Name='explorer.exe'; ProcessId=1; SessionId=0; Owner='S-1-account'} }
        'signed-in' { [pscustomobject]@{Name='explorer.exe'; ProcessId=1; SessionId=1; Owner='S-1-account'} }
    }
}
function Invoke-CimMethod { param($InputObject, $MethodName) [pscustomobject]@{Sid=$InputObject.Owner} }
function Get-Process {
    param($Name, $ErrorAction)
    if (Test-Path $env:LAUNCH_LOG) { [pscustomobject]@{Id=1; MainWindowHandle=[IntPtr]1} }
}
function Stop-Process { param([Parameter(ValueFromPipeline)]$InputObject, [switch]$Force) process {} }
function Start-Process {
    param($FilePath, $WorkingDirectory)
    @{executable=$FilePath; directory=$WorkingDirectory} | ConvertTo-Json |
        Microsoft.PowerShell.Management\Set-Content $env:LAUNCH_LOG
}
function New-ScheduledTaskAction { param($Execute, $Argument) [pscustomobject]@{Execute=$Execute; Argument=$Argument} }
function New-ScheduledTaskPrincipal { param($UserId, $LogonType) [pscustomobject]@{LogonType=$LogonType} }
function Register-ScheduledTask {
    param($TaskName, $Action, $Principal, [switch]$Force)
    $global:task = $Action
    Add-Content $env:TASK_LOG "register $TaskName $($Principal.LogonType)"
}
function Start-ScheduledTask {
    param($TaskName)
    $global:taskProcess = [Diagnostics.Process]::Start($global:task.Execute, $global:task.Argument)
}
function Get-ScheduledTask {
    param($TaskName, $ErrorAction)
    if ($env:TASK_BUSY) { return [pscustomobject]@{State='Running'} }
    if ($global:task) {
        $running = $global:taskProcess -and -not $global:taskProcess.HasExited
        [pscustomobject]@{State=$(if ($running) { 'Running' } else { 'Ready' })}
    }
}
function Stop-ScheduledTask { param($TaskName) $global:taskProcess.Kill($true); Add-Content $env:TASK_LOG "stop $TaskName" }
function Unregister-ScheduledTask { param($TaskName, $Confirm) $global:task = $null; Add-Content $env:TASK_LOG "unregister $TaskName" }
'''

POWERSHELL = r'''#!/usr/bin/env python3
import os, subprocess, sys
args = sys.argv[1:]
prelude = f". '{os.environ['PS_ADAPTERS']}'"
if '-File' in args:
    at = args.index('-File')
    script = os.path.abspath(args[at + 1])
    body = f"{prelude}; & '{script}' {' '.join(args[at + 2:])}; exit $LASTEXITCODE"
else:
    body = f"{prelude}; {args[args.index('-Command') + 1]}"
sys.exit(subprocess.run([os.environ['REAL_PWSH'], '-NoProfile', '-Command', body]).returncode)
'''

SSH = r'''#!/usr/bin/env python3
import os, subprocess, sys
sys.exit(subprocess.run([os.environ['REAL_PWSH'], '-NoProfile', '-Command', sys.argv[-1]],
                        cwd=os.environ['FAKE_HOME']).returncode)
'''

SCP = r'''#!/usr/bin/env python3
import os, subprocess, sys
host = os.environ['SNAPPYREF_WINDOWS_HOST'] + ':'
paths = [os.path.join(os.environ['FAKE_HOME'], a[len(host):]) if a.startswith(host) else a
         for a in sys.argv[1:] if not a.startswith('-')]
sys.exit(subprocess.run(['cp', '-R', *paths], stderr=subprocess.DEVNULL).returncode)
'''


class WindowsUiTests(unittest.TestCase):
    def setUp(self):
        powershell = os.environ.get('SNAPPYREF_TEST_PWSH') or shutil.which('pwsh')
        if not powershell:
            self.skipTest('PowerShell runtime not installed; set SNAPPYREF_TEST_PWSH')
        scratch = tempfile.TemporaryDirectory()
        self.addCleanup(scratch.cleanup)
        self.root = Path(scratch.name)
        self.home = self.root / 'home'
        (self.home / 'AppData' / 'Local' / 'Temp').mkdir(parents=True)
        (self.root / 'tmp').mkdir()
        bin_dir = self.root / 'bin'
        bin_dir.mkdir()
        for name, body in [('powershell', POWERSHELL), ('ssh', SSH), ('scp', SCP)]:
            (bin_dir / name).write_text(body)
            (bin_dir / name).chmod(0o755)
        (bin_dir / 'powershell.exe').symlink_to(bin_dir / 'powershell')
        (self.root / 'adapters.ps1').write_text(ADAPTERS)
        self.launch_log = self.root / 'launch.json'
        self.task_log = self.root / 'tasks.log'
        self.env = dict(os.environ, PATH=f'{bin_dir}:{os.environ["PATH"]}', REAL_PWSH=powershell,
                        PS_ADAPTERS=str(self.root / 'adapters.ps1'), FAKE_HOME=str(self.home),
                        TMPDIR=str(self.root / 'tmp'), LAUNCH_LOG=str(self.launch_log),
                        TASK_LOG=str(self.task_log), SESSION_STATE='signed-in',
                        SNAPPYREF_WINDOWS_ENV=str(self.root / 'no-env-file'),
                        SNAPPYREF_WINDOWS_HOST='windows-host-placeholder',
                        SNAPPYREF_WINDOWS_REPO=r'C:\checkout-placeholder',
                        USERNAME='account-placeholder', USERDOMAIN='WORKGROUP')

    def run_ui(self, *args):
        return subprocess.run(['bash', str(ROOT / 'scripts' / args[0]), *args[1:]], cwd=ROOT,
                              env=self.env, text=True, capture_output=True, timeout=60)

    def step(self, body):
        path = self.root / 'step.ps1'
        path.write_text(body)
        return str(path)

    def tasks(self):
        return self.task_log.read_text().splitlines() if self.task_log.exists() else []

    def assert_nothing_left_on_the_pc(self):
        self.assertEqual(list((self.home / 'AppData' / 'Local' / 'Temp').iterdir()), [])

    def test_launch_needs_this_accounts_desktop_and_a_free_one(self):
        for state, message in [
            ('absent', 'Sign in at the PC first.'),
            ('other-owner', 'Sign in at the PC first.'),
            ('other-process', 'Sign in at the PC first.'),
            ('session-zero', 'Sign in at the PC first.'),
            ('busy', 'Another step is running on the desktop.'),
        ]:
            with self.subTest(state=state):
                busy = state == 'busy'
                self.env.update(SESSION_STATE='signed-in' if busy else state, TASK_BUSY='1' if busy else '')
                result = self.run_ui('windows-launch-gui.sh')
                self.assertEqual(result.returncode, 1, result.stderr)
                self.assertIn(message, result.stderr)
                self.assertFalse(self.launch_log.exists())
                self.assertEqual(self.tasks(), [])
                self.assert_nothing_left_on_the_pc()

        # USERDOMAIN reads WORKGROUP over SSH, as on the real PC; the account still matches.
        self.env.update(SESSION_STATE='signed-in', TASK_BUSY='')
        result = self.run_ui('windows-launch-gui.sh')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads(self.launch_log.read_text()), {
            'executable': r'C:\checkout-placeholder\target\release-fast\snappyref.exe',
            'directory': r'C:\checkout-placeholder',
        })
        self.assertEqual(self.tasks(), ['register SnappyRefAgentUi Interactive',
                                        'unregister SnappyRefAgentUi'])
        self.assert_nothing_left_on_the_pc()

    def test_returns_the_steps_status_log_and_captures(self):
        result = self.run_ui('windows-ui.sh', self.step(
            '"step says hi"\n'
            'Set-Content (Join-Path $env:SNAPPYREF_UI_CAPTURES "board.png") "pixels"\n'
            'exit 5\n'))

        self.assertEqual(result.returncode, 5, result.stderr)
        self.assertIn('step says hi', result.stdout)
        captures = Path(result.stdout.split('Captures: ')[1].strip())
        self.assertEqual(captures.parent, self.root / 'tmp' / 'snappyref-windows-ui')
        self.assertEqual((captures / 'board.png').read_text(), 'pixels\n')
        self.assert_nothing_left_on_the_pc()

        result = self.run_ui('windows-ui.sh', self.step('"before"\nthrow "boom"\n'))

        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn('before', result.stdout)
        self.assertIn('ERROR: boom', result.stdout)
        self.assertNotIn('Captures:', result.stdout)
        self.assert_nothing_left_on_the_pc()

    def test_stops_a_stalled_step_at_the_timeout(self):
        result = self.run_ui('windows-ui.sh', self.step('Start-Sleep -Seconds 30\n'),
                             '--timeout', '2')

        self.assertEqual(result.returncode, 124, result.stderr)
        self.assertIn('Step timed out after 2 s.', result.stderr)
        self.assertEqual(self.tasks(), ['register SnappyRefAgentUi Interactive',
                                        'stop SnappyRefAgentUi', 'unregister SnappyRefAgentUi'])
        self.assert_nothing_left_on_the_pc()


if __name__ == '__main__':
    unittest.main()
