---
name: use-windows
description: |
  Windows clippy, check, build, or test for SnappyRef on a real Windows machine from the Mac checkout. Use for any Windows cargo command (cross-compiling from the Mac fails on ring's C build), Windows-only behavior, or syncing Mac edits to the Windows checkout.
user-invocable: true
argument-hint: "[windows-check-or-build]"
---

# Use Windows

Use this skill when the Mac agent needs Windows-native feedback without GitHub
Actions.

The scripts assume the maintainer's own Windows machine reached over SSH.
Contributors without that setup can run the same cargo commands in a Windows
checkout directly, or dispatch the `Build Windows App` workflow on a branch.

## Ownership and connection

Edit on Mac, then sync to the Windows execution checkout. Do not patch Windows
files unless the user requests a one-off diagnostic experiment; bring real fixes
back to Mac before testing. Use native checks before GitHub Actions unless the
user asks otherwise.

Scripts load private machine values from gitignored `.env.windows`, using
`.env.windows.example` as the tracked template. If missing, ask for the SSH
alias or host and Windows checkout path. Keep those details out of tracked docs.

Use key-based SSH over Tailscale or another private network, with explicit
PowerShell commands. Test key-only SSH if connection state is uncertain. Do not
install background sync tools unless asked. Explain security impact before
changing firewall, SSH, or Tailscale settings.

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

Normal checks and tests use shipping default features. The Windows user needs
libjpeg-turbo and the explicit `TURBOJPEG_*` environment settings used by the
[CI install action](../../../.github/actions/install-turbojpeg-windows/action.yml). Keep local
installation paths in that machine's user environment. The test wrapper runs
serially, matching the canonical behavior gate; an explicit `--test-threads`
option overrides that default.

```sh
scripts/windows-test.sh
scripts/windows-test.sh <filter>
```

For one-off commands, use:

```sh
scripts/windows-remote.sh '<powershell-command>'
```

## Native file and URL drops

Use this guidance when changing the Windows drop bridge in
[windows.rs](../../../src/platform/native_drop/windows.rs) or its winit fallback.
Registration ownership and generated COM ABI rationale live beside that code.

For native drop changes, verify Explorer file import, cursor-following previews,
and final placement on Windows. Verify
browser URL drops where the custom bridge can register. In-process inspect
scenarios do not exercise the COM calling convention or OS registration.

## Visible GUI Launch

SSH-launched Windows GUI apps may run in a non-interactive session and not
appear on the logged-in desktop. For manual app verification, build the Windows
binary through the repo wrapper, then launch it in the interactive desktop
session with a one-shot scheduled task.

For quick development GUI checks, build `release-fast` with reduced features.
This build does not verify the shipping feature set:

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

Report the commands, results, and any native behavior that remains unverified.
