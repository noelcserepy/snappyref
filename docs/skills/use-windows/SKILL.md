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
