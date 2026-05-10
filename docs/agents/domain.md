# Domain Docs

SnappyRef is a single-context repo.

## Read first

Before normal planning or issue drafting, read:

- `CONTEXT.md`
- relevant ADRs in `docs/adr/`
- `docs/skills/app-inspection/SKILL.md` when the work involves app inspection, UI state verification, screenshots, or temporary debug probes
- `docs/skills/capture-ui-state/SKILL.md` when a screenshot needs a temporary debug-only probe or deterministic in-app UI state setup
- `docs/skills/perf-profiling/SKILL.md` when the work involves startup, import, embedding, idle CPU, or memory performance
- `DESIGN_SYSTEM.md` when the work involves shared UI semantics

If a file does not exist, proceed silently. Do not create new domain docs upfront. `grill-with-docs` creates or updates domain docs lazily when terms or durable decisions are resolved.

## Use project language

Use terms from `CONTEXT.md` in issue titles, PRDs, agent briefs, test names, and implementation plans.

Do not drift to avoided synonyms. Examples:

- Use **Library**, not "all photos view".
- Use **Asset**, not "photo" or "source image".
- Use **Board**, not "project" or "scene".
- Use **Placement**, not "image copy" or "board item".
- Use **Annotation** or **Textbox**, not "note".
- Use **Semantic Search**, not "AI search".

## ADR conflicts

If a proposed issue or PRD contradicts an ADR, surface the conflict explicitly before publishing the issue.

## Documentation updates

If a task introduces or changes project language, update `CONTEXT.md` in the same change.

Create an ADR only when the decision is hard to reverse, surprising without context, and the result of a real trade-off.
