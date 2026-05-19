# Inspect Skill Plan

## Goal

Create an `inspect` skill that gives agents a reliable app verification loop for
SnappyRef. The skill should combine deterministic tools with adaptive workflow
instructions so an agent can reproduce a described failing behavior, inspect
evidence, add narrow temporary instrumentation, fix the issue, and verify the
fix without brittle OS mouse automation.

The design has three buckets:

1. App source code primitives needed for inspection.
2. Deterministic scripts the agent can run reliably.
3. Workflow instructions and references that tell the agent how to choose and
   combine the tools.

Keep the first implementation small. Prove the loop with one canonical
interaction scenario before building a broader framework.

## Bucket 1: App Source Code Primitives

App source changes should expose reusable inspection primitives, not encode every
scenario. Scenario-specific hacks should stay out of product code.

Required primitives:

- `VerifyDriver` or equivalent test helper around the existing
  `egui_kittest::Harness`.
- Deterministic app construction for inspection:
  - temp Library root
  - fixed window size
  - fixed theme
  - updater/network disabled where possible
  - deterministic board and Library seeds
- Semantic probe registry for core targets:
  - `snap.canvas`
  - `snap.search.result[n]`
  - `snap.canvas.placement[id]`
  - `snap.canvas.selection`
  - later: sidebar, modal, toolbar targets as scenarios require them
- Artifact writer:
  - `events.ndjson`
  - `report.json`
  - optional `frames/*.png`
- Gesture helpers:
  - `click_target`
  - `drag_target_to_target`
  - `drag_point_to_point`
  - `hold_drag` with one input event per frame
  - `key`
  - `type_text`
  - `paste`
  - `drop_files`
- Reusable seeds:
  - empty board
  - board with placements
  - seeded search results from generated local images
  - import/drop fixture image

Initial canonical scenario:

- Search result drag to canvas.
- Capture before, mid-drag, and after-release evidence.
- Assert both app state and visible/probe state.

## Bucket 2: Deterministic Skill Scripts

Scripts live under `docs/skills/inspect/scripts/`. They are the reliable,
repeatable parts of the skill. Favor stable, parseable stdout over decorative
output.

Proposed scripts:

- `run-scenario.sh <scenario>`
  - Runs one Rust inspection scenario.
  - Writes artifacts.
  - Prints the artifact directory path.
- `latest-artifact.sh [scenario]`
  - Prints the latest artifact directory for a scenario.
- `summarize-artifact.py <artifact-dir>`
  - Reads `report.json` and `events.ndjson`.
  - Prints pass/fail, first failure, key assertions, and relevant frame paths.
- `check-harness.sh`
  - Runs a tiny known-good scenario to confirm the harness is functioning before
    debugging real behavior.

Script rules:

- Scripts should be small and boring.
- Scripts should compose rather than hide workflow decisions.
- Scripts should avoid changing source files unless their name and docs make
  that explicit.
- Cleanup scripts must not delete user work or reset git state.

## Bucket 3: Skill Workflow Instructions

The main `SKILL.md` should be short and decision-oriented. Larger procedures can
move into `references/`.

Proposed structure:

```text
docs/skills/inspect/
  SKILL.md
  plan.md
  scripts/
    run-scenario.sh
    latest-artifact.sh
    summarize-artifact.py
    check-harness.sh
  references/
    writing-scenarios.md
    reading-artifacts.md
    live-manual-testing.md
    temporary-instrumentation.md
```

Main workflow:

1. Classify the inspection.
   - Pure logic: write or run a unit test.
   - egui interaction: use the inspection scenario harness.
   - static visual state: use a seeded inspect scenario.
   - platform integration: use the live/manual platform procedure.
2. Start deterministic.
   - Run `check-harness.sh`.
   - Run the closest existing scenario.
   - Inspect the artifact summary.
   - Add instrumentation or a new scenario only after that.
3. For failing behavior.
   - Create or modify one focused scenario.
   - Capture before, mid-interaction, and final state.
   - Assert app state and visible state.
   - Preserve the failing artifact before fixing.
   - Fix the issue.
   - Rerun and compare the artifact after the fix.
4. Temporary instrumentation.
   - Prefer structured artifact fields over ad hoc logging.
   - Avoid `println!` or noisy terminal output.
   - Remove narrow temporary instrumentation after the fix unless it becomes a
     reusable primitive.
5. Static states.
   - Use for screenshotable seeded UI states.
   - Do not use as final evidence for drag/click behavior when a scenario is
     possible.
6. Manual/live app testing.
   - Use the repo-built binary, not the installed release.
   - Resize near full screen, but do not enter macOS fullscreen.
   - Keep driven coordinates inside the app window.
   - Close the app after testing.
   - Do not treat manual confirmation as final when a harness scenario is
     feasible.

## Initial Implementation Slice

First PR should be intentionally small:

1. Add the minimal `VerifyDriver` around the existing `egui_kittest` harness.
2. Add artifact writing for `report.json` and `events.ndjson`.
3. Add semantic/probe data for canvas, search result, and selection bounds.
4. Add one search-result-drag-to-canvas scenario.
5. Add `run-scenario.sh`, `latest-artifact.sh`, and `summarize-artifact.py`.
6. Update `docs/skills/inspect/SKILL.md` to route agents through this path.

Defer:

- Recreating every old visual probe as an inspect scenario.
- Broad snapshot baseline strategy.
- Proc macros.
- A separate harness crate.
- Full tracing integration.
- Windows platform smoke beyond existing `use-windows` workflows.

## Design Principles

- Deterministic tools first, adaptive workflow second.
- App primitives should be generic and small.
- Scripts should be reliable handles agents can trust.
- Skill markdown should explain when to use each handle.
- OS-level mouse automation is a last resort, not a default.
- Screenshots are evidence, but state/probe assertions decide pass/fail.
- Keep release behavior untouched.
