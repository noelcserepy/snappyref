# Repo Inspection Guide

## Project Language Maintenance

There is no standalone project glossary. Treat the codebase as the canonical domain language:

- use domain names in modules, types, functions, tests, and inspect scenarios
- add short local comments only where naming cannot carry the rationale
- keep legacy persistence or compatibility vocabulary inside adapter modules

Put product contracts in the owning specs and implementation choices in the
owning module interface, tests, inspect scenarios, or repo-local skills.

When language, boundaries, lifecycle, storage, release/update behavior, or architecture decisions are unclear, ask one question at a time; if the repo answers the question, inspect the repo instead of asking.

## Repo-Local Skills

Use [inspect](docs/skills/inspect/SKILL.md) for UI/app behavior verification. Prefer `docs/skills/inspect/scripts/run-scenario.sh <scenario>` and inspect artifacts over manual app launches, OS mouse automation, ad hoc screenshots, or debug flags.

Use [perf-profiling](docs/skills/perf-profiling/SKILL.md) when measuring or improving startup, import, embedding, idle CPU, or memory performance.

Use [patchnotes](docs/skills/patchnotes/SKILL.md) when writing user-facing SnappyRef patchnotes, release notes, changelog copy, or a release summary from PRs since the last version release.

Use [use-windows](docs/skills/use-windows/SKILL.md) when checking, building, testing, or syncing SnappyRef on a real Windows machine from the Mac development checkout.

## Repo Principles

- Optimize for speed first. Prefer changes that keep startup and core interactions fast.
- Keep interactions shortcut-driven and low-friction.
- Use sensible defaults so repeated flows happen automatically.
- Keep the code and UI simple. If a solution feels complex, look for a smaller one first.

## UI Work

Use the design-system skill before planning or editing any UI, visual semantics, shared components, tokens, styling, layout, motion, or design-system behavior.

There is no standalone design-system document. Treat the code as the canonical visual vocabulary:

- start with `src/ui/theme.rs` for shared tokens, spacing, radius, type, color, and motion constants
- read `src/ui/motion.rs` for shared motion semantics
- read the relevant component/module before changing a pattern
- use inspect scenarios/artifacts as runtime visual evidence
- when visual semantics change, update the owning token/component comment, API name, test, or inspect scenario in the same change

## Verification

Use [inspect](docs/skills/inspect/SKILL.md) for behavior evidence and
[perf-profiling](docs/skills/perf-profiling/SKILL.md#regular-profiling-and-refactors)
for the before/after architecture-refactor procedure and measurement sequencing.
Use [use-windows](docs/skills/use-windows/SKILL.md) for platform-sensitive changes
before GitHub Actions. It owns native verification and sync requirements.

[Release operations](docs/release-operations.md#update-smokes) owns networked
update smoke procedures and their explicit-request-only scope.
[Development checks](README.md#development-checks) owns pre-push setup.
