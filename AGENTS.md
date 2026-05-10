# Repo Inspection Guide

## Startup

Read these before normal work:

1. `.agents/start-context.md`, if it exists.
2. `CONTEXT.md`, if it exists.
3. `docs/adr/*.md`, if any exist.
4. `/Users/noelcserepy/.agents/skills/grill-with-docs/SKILL.md`.
5. `docs/skills/app-inspection/SKILL.md`, if it exists.

Repo-local `.agents` must be gitignored and treated as private local context.

## Project Memory Maintenance

Keep `CONTEXT.md` and `docs/adr/` current as part of normal work.

Use `CONTEXT.md` for domain language: terms, relationships, examples, and resolved ambiguities. If a task introduces or changes project language, update `CONTEXT.md` in the same change.

Use `docs/adr/` for durable decisions only. Create an ADR when a decision is hard to reverse, surprising without context, and the result of a real trade-off.

Use `grill-with-docs` when language, boundaries, lifecycle, storage, release/update behavior, or architecture decisions are unclear. Ask one question at a time; if the repo answers the question, inspect the repo instead of asking.

## Repo-Local Skills

Use [app-inspection](docs/skills/app-inspection/SKILL.md) when inspecting the app, verifying a UI state, capturing a screenshot, or adding a temporary debug probe.

## Repo Principles

- Optimize for speed first. Prefer changes that keep startup and core interactions fast.
- Favor layout over decoration. Use spacing, alignment, and positioning before borders or fills.
- Keep interactions shortcut-driven and low-friction.
- Use sensible defaults so repeated flows happen automatically.
- Keep the code and UI simple. If a solution feels complex, look for a smaller one first.

## Testing Notes

- Use `cargo check` after probe or UI changes before trusting a screenshot.

## Pre-push Checks

- CI runs `cargo fmt --check` and will fail on unformatted code. Always run `cargo fmt` before pushing.
- Run `cargo check` (and any tests you touched) locally before pushing so CI does not catch basic regressions.
