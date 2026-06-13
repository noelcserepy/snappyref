# Repo Inspection Guide

## Startup

Read these before normal work:

1. `CONTEXT.md`, if it exists.
2. `docs/adr/*.md`, if any exist.
3. `docs/skills/inspect/SKILL.md`, if it exists.
4. `docs/skills/perf-profiling/SKILL.md`, if it exists.
5. `docs/skills/patchnotes/SKILL.md`, if it exists.

## Project Memory Maintenance

Keep `CONTEXT.md` and `docs/adr/` current as part of normal work.

Use `CONTEXT.md` for domain language: terms, relationships, examples, and resolved ambiguities. If a task introduces or changes project language, update `CONTEXT.md` in the same change.

Use `docs/adr/` for durable decisions only. Create an ADR when a decision is hard to reverse, surprising without context, and the result of a real trade-off.

Use `grill-with-docs` when language, boundaries, lifecycle, storage, release/update behavior, or architecture decisions are unclear. Ask one question at a time; if the repo answers the question, inspect the repo instead of asking.

## Repo-Local Skills

Use [inspect](docs/skills/inspect/SKILL.md) for UI/app behavior verification. Prefer `docs/skills/inspect/scripts/run-scenario.sh <scenario>` and inspect artifacts over manual app launches, OS mouse automation, ad hoc screenshots, or debug flags.

Use [perf-profiling](docs/skills/perf-profiling/SKILL.md) when measuring or improving startup, import, embedding, idle CPU, or memory performance.

Use [patchnotes](docs/skills/patchnotes/SKILL.md) when writing user-facing SnappyRef patchnotes, release notes, changelog copy, or a release summary from PRs since the last version release.

Use [use-windows](docs/skills/use-windows/SKILL.md) when checking, building, testing, or syncing SnappyRef on a real Windows machine from the Mac development checkout.

## Repo Principles

- Optimize for speed first. Prefer changes that keep startup and core interactions fast.
- Keep interactions shortcut-driven and low-friction.
- Use sensible defaults so repeated flows happen automatically.
- Keep the code and UI simple. If a solution feels complex, look for a smaller one first. If it still looks too complicated, review it with the `simplify` skill before implementing.

## UI Work

Use the design-system skill before planning or editing any UI, visual semantics, shared components, tokens, styling, layout, motion, or design-system behavior.

There is no standalone design-system document. Treat the code as the canonical visual vocabulary:

- start with `src/ui/theme.rs` for shared tokens, spacing, radius, type, color, and motion constants
- read `src/ui/motion.rs` for shared motion semantics
- read the relevant component/module before changing a pattern
- use inspect scenarios/artifacts as runtime visual evidence
- when visual semantics change, update the owning token/component comment, API name, test, or inspect scenario in the same change

## Testing Notes

- Use `cargo check` after UI/app-inspection changes before trusting screenshots. For behavior verification, prefer an inspect scenario and artifact.
- Use `scripts/update-smoke-macos.sh` or `scripts/update-smoke-windows.sh` only for explicit auto-update smoke checks. They build a scratch install, force the running app to report an old version, fetch the published GitHub manifest, stage the real latest release for that platform, and verify the platform update apply path. Details live in [release-operations](docs/release-operations.md). They are intentionally manual, networked, and not a pre-push gate.
- Use [use-windows](docs/skills/use-windows/SKILL.md) for Windows-native checks before GitHub Actions when a change touches Windows behavior, release packaging assumptions, path handling, updater behavior, native drop/clipboard/input behavior, or platform-specific code.
- Windows verification follows one invariant: make changes on Mac, run `scripts/windows-sync.sh`, then verify on Windows. Do not develop directly in the Windows checkout.
- Do not make Windows verification a mandatory pre-push gate; the Windows machine may be unavailable. Run targeted Windows checks for platform-sensitive work and the full Windows suite before release candidates.

## Pre-push Checks

The pre-push gate (fmt + `cargo check` under shipping features + the
`update_preservation_smoke` test) lives in `.githooks/pre-push` and runs
automatically on `git push` once the hook path is configured. One-time setup on
a fresh clone:

```sh
git config core.hooksPath .githooks
```

The hook is the local backstop for formatting and cargo checks.
Bypass with `git push --no-verify` when you genuinely need to (e.g. debugging
branches that you know will not merge).

## Agent skills

### Issue tracker

GitHub Issues are the canonical implementation backlog. Private draft backlog notes can live in `.scratch/backlog/` until they are ready to become GitHub issues. See `docs/agents/issue-tracker.md`.

### Triage labels

Use the standard Matt Pocock triage state labels plus `bug` / `enhancement` category labels. See `docs/agents/triage-labels.md`.

### Domain docs

This is a single-context repo: use root `CONTEXT.md` plus `docs/adr/`. See `docs/agents/domain.md`.
