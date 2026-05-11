# Repo Inspection Guide

## Startup

Read these before normal work:

1. `CONTEXT.md`, if it exists.
2. `docs/adr/*.md`, if any exist.
3. `docs/skills/app-inspection/SKILL.md`, if it exists.
4. `docs/skills/perf-profiling/SKILL.md`, if it exists.

## Project Memory Maintenance

Keep `CONTEXT.md` and `docs/adr/` current as part of normal work.

Use `CONTEXT.md` for domain language: terms, relationships, examples, and resolved ambiguities. If a task introduces or changes project language, update `CONTEXT.md` in the same change.

Use `docs/adr/` for durable decisions only. Create an ADR when a decision is hard to reverse, surprising without context, and the result of a real trade-off.

Use `grill-with-docs` when language, boundaries, lifecycle, storage, release/update behavior, or architecture decisions are unclear. Ask one question at a time; if the repo answers the question, inspect the repo instead of asking.

## Repo-Local Skills

Use [app-inspection](docs/skills/app-inspection/SKILL.md) when inspecting the app, verifying a UI state, capturing a screenshot, adding a temporary debug probe, or setting deterministic in-app UI state for capture.

Use [perf-profiling](docs/skills/perf-profiling/SKILL.md) when measuring or improving startup, import, embedding, idle CPU, or memory performance.

## Repo Principles

- Optimize for speed first. Prefer changes that keep startup and core interactions fast.
- Keep interactions shortcut-driven and low-friction.
- Use sensible defaults so repeated flows happen automatically.
- Keep the code and UI simple. If a solution feels complex, look for a smaller one first. If it still looks too complicated, review it with the `simplify` skill before implementing.

## UI Work

Read `DESIGN_SYSTEM.md` before planning or editing any UI.

## Testing Notes

- Use `cargo check` after probe or UI changes before trusting a screenshot.

## Pre-push Checks

- CI runs `cargo fmt --check` and will fail on unformatted code. Always run `cargo fmt` before pushing.
- Run `cargo check` (and any tests you touched) locally before pushing so CI does not catch basic regressions.

## Agent skills

### Issue tracker

GitHub Issues are the canonical implementation backlog. Private draft backlog notes can live in `.scratch/backlog/` until they are ready to become GitHub issues. See `docs/agents/issue-tracker.md`.

### Triage labels

Use the standard Matt Pocock triage state labels plus `bug` / `enhancement` category labels. See `docs/agents/triage-labels.md`.

### Domain docs

This is a single-context repo: use root `CONTEXT.md` plus `docs/adr/`. See `docs/agents/domain.md`.
