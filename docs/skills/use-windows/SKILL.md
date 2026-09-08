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

The wrapper synchronizes the managed source files, including additions,
modifications, deletions, and renames. It preserves file bytes and unrelated
remote files, including `.git`, build caches, private configuration, and app data.

## Checks

Run targeted checks for platform-sensitive work and the full Windows suite before
release candidates. If the machine is unavailable, report that limitation.
Windows availability is not a mandatory pre-push gate.

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

## Native file and URL drops

Use this guidance when changing the Windows drop bridge in
[windows.rs](../../../src/platform/native_drop/windows.rs) or its winit fallback.
Registration ownership and generated COM ABI rationale live beside that code.

For native drop changes, edit on Mac, sync, then check on Windows. Verify
Explorer file import, cursor-following previews, and final placement. Verify
browser URL drops where the custom bridge can register. In-process inspect
scenarios do not exercise the COM calling convention or OS registration.

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
scripts/windows-launch-gui.sh
```

Verify that the process started:

```sh
scripts/windows-remote.sh 'Get-Process snappyref -ErrorAction SilentlyContinue | Select-Object Id,ProcessName,MainWindowTitle,StartTime | Format-List'
```

Notes:

- `/IT` requires an active logged-in user session. If nobody is logged in, the
  task may run but no visible window appears.
- The task name is `SnappyRefAgentLaunch`; the launcher deletes/recreates it,
  runs it, waits for SnappyRef to appear, then deletes the task before the
  one-shot scheduled time can fire a second GUI later.
- The launcher stops existing `snappyref` processes before starting a new one,
  so a manual verification request leaves exactly one GUI instance.
- Do not use this for automated checks. It is for human-visible GUI
  verification on the Windows machine.

For explicit update verification, use the canonical
[update smoke procedure](../../release-operations.md#update-smokes).

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
