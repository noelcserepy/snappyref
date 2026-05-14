---
name: use-windows
description: |
  Build, check, or test SnappyRef on a real Windows machine from the Mac development checkout. Use when asked to run Windows cargo checks/builds/tests, verify Windows-only behavior, or sync local Mac edits to the Windows checkout.
user-invocable: true
argument-hint: "[windows-check-or-build]"
---

# Use Windows

Use this skill when the Mac agent needs Windows-native feedback without GitHub
Actions.

## Contract

- Tracked docs define the workflow.
- Private machine details live only in `.env.windows`.
- The Mac checkout is the source of truth for edits.
- The Windows checkout is an execution target for native checks and builds.
- Do not develop or patch files on the Windows checkout. Make changes on Mac,
  sync to Windows, then verify there.

## Private Config

Windows scripts load private local values from:

```text
.env.windows
```

Use `.env.windows.example` as the tracked template. `.env.windows` is gitignored
and should contain only local machine values such as:

- `SNAPPYREF_WINDOWS_HOST`
- `SNAPPYREF_WINDOWS_REPO`

If the private config is missing, ask the user for:

- SSH alias or host for the Windows machine
- Windows checkout path

## Connection Model

- Use SSH over Tailscale or another private network path.
- Use key-based auth. Avoid password prompts in agent-run commands.
- Use PowerShell explicitly for remote Windows commands.
- Treat visible GUI launch as separate from build/test. SSH-launched GUI apps may
  run in a non-interactive Windows session and not appear on the desktop.

## Sync

Use the script wrapper:

```sh
scripts/windows-sync.sh
```

It uses tar-over-SSH. It is explicit, background-free, and preserves file bytes
exactly:

```sh
git ls-files -co --exclude-standard -z \
  | tar --null -T - -cf - \
  | ssh -o BatchMode=yes <windows-ssh-alias> "powershell -NoProfile -Command \"cd <windows-repo-path>; tar -xf -\""
```

This includes tracked and untracked non-ignored files. It does not convert line
endings. It does not delete files on Windows that were removed on Mac; if a
deletion matters, remove it on Windows explicitly or use Git on Windows for that
case.

## Checks

After syncing, run the cheapest Windows-native check first:

```sh
scripts/windows-check.sh
```

Use fuller checks only when needed:

```sh
scripts/windows-test.sh
scripts/windows-test.sh <filter>
scripts/windows-build-release-fast.sh
```

For one-off commands, use:

```sh
scripts/windows-remote.sh '<powershell-command>'
```

## Visible GUI Launch

SSH-launched Windows GUI apps may run in a non-interactive session and not
appear on the logged-in desktop. For manual app verification, build the Windows
binary through the repo wrapper, then launch it in the interactive desktop
session with a one-shot scheduled task.

Build the same Windows feature set used by the fast release smoke path:

```sh
scripts/windows-build-release-fast.sh
```

Then launch the GUI:

```sh
script='$launch = Join-Path $env:TEMP "launch-snappyref-gui.ps1"
$repo = (Get-Location).Path
$exe = Join-Path $repo "target\release-fast\snappyref.exe"
@"
Set-Location ''$repo''
Start-Process -FilePath ''$exe'' -WorkingDirectory ''$repo''
"@ | Set-Content -Encoding UTF8 $launch
$time = (Get-Date).AddMinutes(1).ToString("HH:mm")
schtasks /Delete /TN SnappyRefAgentLaunch /F 2>$null | Out-Null
schtasks /Create /TN SnappyRefAgentLaunch /TR "powershell.exe -NoProfile -ExecutionPolicy Bypass -File `"$launch`"" /SC ONCE /ST $time /F /IT
schtasks /Run /TN SnappyRefAgentLaunch
'
enc=$(printf '%s' "$script" | iconv -f UTF-8 -t UTF-16LE | base64 | tr -d '\n')
scripts/windows-remote.sh "powershell -NoProfile -EncodedCommand $enc"
```

Verify that the process started:

```sh
scripts/windows-remote.sh 'Get-Process snappyref -ErrorAction SilentlyContinue | Select-Object Id,ProcessName,MainWindowTitle,StartTime | Format-List'
```

Notes:

- `/IT` requires an active logged-in user session. If nobody is logged in, the
  task may run but no visible window appears.
- The task name is `SnappyRefAgentLaunch`; the command deletes/recreates it.
- Do not use this for automated checks. It is for human-visible GUI
  verification on the Windows machine.

## Workflow

1. Confirm `.env.windows` exists.
2. Make source changes only in the Mac checkout.
3. Test key-only SSH if connection state is uncertain.
4. Run `scripts/windows-sync.sh`.
5. Run the narrowest Windows check that answers the task.
6. Report the command result and any warnings that matter.

## Guardrails

- Do not use GitHub Actions for inner-loop Windows checks unless the user asks.
- Do not install background sync tools unless the user asks.
- Do not edit the Windows checkout directly unless the user explicitly asks for
  a one-off diagnostic experiment. Bring any real fix back to Mac before testing.
- Do not change Windows firewall, SSH, or Tailscale settings without explaining
  the security impact first.
- Keep private machine details in `.env.windows`, not tracked docs.
