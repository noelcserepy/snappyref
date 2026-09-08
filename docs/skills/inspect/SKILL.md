---
name: inspect
description: |
  Reproduce, inspect, diagnose, and verify SnappyRef app behavior with deterministic in-process scenarios, full-app screenshots, structured artifacts, and harness-driven clicks/drags/keyboard input. Use for UI behavior, visual state, app interaction, search palette workflows, imports, progress states, and screenshot-backed verification.
user-invocable: true
argument-hint: "[behavior-or-scenario]"
---

# Inspect

Reproduce app behavior through this loop:

```text
seed -> act -> wait -> capture -> inspect artifact -> adjust/fix -> rerun
```

## Core Model

Build every inspection scenario from four verbs:

- `seed`: create prerequisite app state directly when reaching it would be
  setup noise.
- `act`: perform the behavior under test with harness clicks, drags, keys, or
  semantic widget actions.
- `wait`: step the harness through async/progress/repaint work until the state
  is terminal or observably changed.
- `capture`: save full-app screenshots and structured facts/assertions.

Invariant: use user-like actions for the behavior being tested; seed everything
else when direct setup makes the scenario clearer and more deterministic.

## Start Here

List available scenarios:

```sh
docs/skills/inspect/scripts/run-scenario.sh --list
```

Run a scenario:

```sh
docs/skills/inspect/scripts/run-scenario.sh <scenario>
```

Summarize the returned artifact:

```sh
docs/skills/inspect/scripts/summarize-artifact.py /absolute/artifact/path
```

Recover the latest artifact:

```sh
docs/skills/inspect/scripts/latest-artifact.sh [scenario]
```

Check the harness if scenario failures look suspicious:

```sh
docs/skills/inspect/scripts/check-harness.sh
```

Use `--list` for the current scenario catalogue. Useful starting examples:

- `sample-image-import`: before, loading, and loaded image states.
- `search-drag-to-canvas`: provisional and committed search-result placement.
- `semantic-search-install`: first-launch installation with a test installer.

`semantic-search-real-install` uses the real networked installer. Use it only
when verifying that integration.

## Evidence Rules

Always prefer artifacts over terminal impressions.

Read evidence in this order:

1. `report.json`
2. `events.ndjson`
3. `frames/*.png`
4. app source code

A useful artifact should contain:

- full-app screenshots
- structured actions
- target/fact observations
- assertions that decide pass/fail
- enough state to explain what happened

If the artifact does not explain the failure, add one narrow observation or
assertion and rerun the same scenario.

## State Rules

A scenario owns its initial world.

Reset camera, selection, settings, search state, library state, and async queues
unless the bug depends on persisted state. Keep inspection-only setup behind
test/debug-only helpers and call it from harness scenarios, not production CLI
flags.

Do not add a primitive for one-off setup. Add a primitive only when a second
scenario would otherwise duplicate timing, coordinate derivation, screenshot, or
state setup logic.

## Screenshot Rule

Inspect screenshots are always full app viewport screenshots.

Do not create component-only screenshots as inspect evidence. If a scenario
needs a detail, capture the full app and optionally add a derived crop later.

Current screenshot primitive:

- renders through the full app `egui_kittest` harness
- saves opaque PNGs under `frames/`
- records width, height, transparent pixel count, and color sample
- treats transparent pixels as a failure signal

## Interaction Rules

Use the in-process harness, not OS mouse automation.

Prefer semantic actions when possible:

```rust
harness.get_by_label("Install").click();
harness.get_by_label("Semantic Search").scroll_to_me();
harness.key_press(egui::Key::Enter);
```

Use pointer events for canvas/spatial work:

```rust
harness.hover_at(source);
harness.drag_at(source);
harness.hover_at(target);
harness.step();
harness.drop_at(target);
harness.step();
```

Use `harness.step()` for held input, animations, downloads, imports, search
progress, and anything that intentionally keeps repainting. Use `harness.run()`
only when the UI should settle.

## Primitive Rules

Use existing inspect primitives before writing scenario-local logic.

Current useful primitives include:

- `app_full_harness`: full app viewport harness.
- `InspectScenario::capture_screen`: full-app screenshot + frame metadata.
- `InspectScenario::drive_progress`: step frames, capture changed statuses as
  facts, stop at terminal state or timeout, and assert success/failure.
- `InspectScenario::drive_import_lifecycle`: capture before/loading/loaded
  states and assert placement count and completed content resolution.
- `inspect_search_result_tile_center`: derives palette tile centers from UI
  layout constants.
- `inspect_click_at`: sends pointer press/release with modifiers.

## Writing A New Scenario

Create a focused Rust test-backed scenario when no existing scenario reproduces
the behavior. Name it after the user action, capture meaningful before/mid/after
frames, and assert intermediate and final behavior. Write facts and artifacts
through `InspectScenario` rather than unstructured terminal output. Use
`harness.step()` instead of sleeps when frames can drive the work.

## Search Palette Work

Use `inspect_search_result_tile_center` for tile coordinates.

Use deterministic result injection when testing palette behavior. Use the real
semantic search service only when the bug is specifically about search ranking,
indexing, model loading, or asset installation.

When Semantic Search is missing and the task is not about missing-assets UI,
prefer accepting/installing it as part of deterministic setup.

Injected search results can exist before their thumbnails finish decoding.
When thumbnail appearance matters, wait for preview readiness before capture;
result count alone does not establish that the thumbnails are visible.

## Reporting Back

Report:

- scenario command
- artifact path
- key assertions
- screenshot paths or inline images when asked
- any limitation in the scenario, especially deterministic injection vs real
  service behavior

Be concise. The artifact is the detailed evidence.

## Before architecture refactors

Run `python3 scripts/check-refactor-behavior.py`. It runs the library suite
sequentially, rejects stale or incomplete artifacts for the main interaction
and persistence scenarios, then checks the actual release image-preparation
subprocess. The report is `target/refactor-behavior.json`.

Keep the existing tests that describe user-visible outcomes. Add a focused
scenario only when the boundary being changed lacks coverage. Search ranking
and live model installation are separate from deterministic search interaction
tests. Native clipboard/external drops and Windows behavior still need platform
checks when those paths change.

Run performance separately afterward with [perf-profiling](../perf-profiling/SKILL.md).
