---
name: inspect
description: |
  Reproduce, inspect, diagnose, and verify SnappyRef app behavior with deterministic in-process scenarios, full-app screenshots, structured artifacts, and harness-driven clicks/drags/keyboard input. Use for UI behavior, visual state, app interaction, search palette workflows, imports, progress states, and screenshot-backed verification.
user-invocable: true
argument-hint: "[behavior-or-scenario]"
---

# Inspect

Inspect is the app inspection path. Use it when you need to see or verify
SnappyRef behavior as the user experiences it, especially clicks, drags, search
palette interactions, import/loading states, and multi-step UI flows.

The loop is:

```text
seed -> act -> wait -> capture -> inspect artifact -> adjust/fix -> rerun
```

This is the default path. Legacy `--verify-*` app flags have been removed; do
not reintroduce them as a state catalog.

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

## Current Scenarios

- `check-harness`: proves the inspect path can run a known drag scenario.
- `search-drag-to-canvas`: drags one search result to the canvas and verifies
  provisional and committed placement state.
- `semantic-search-install`: clicks the first-launch Semantic Search install
  prompt with a test installer.
- `semantic-search-sidebar-status`: opens Settings sidebar and screenshots the
  Semantic Search status.
- `semantic-search-real-install`: clicks Settings install and drives the real
  Semantic Search installer to installed or failed.
- `sample-image-import`: captures before, loading, and loaded image import
  states.
- `pending-drop-batch-undo`: drops two files, undoes the accepted Loading
  batch, then drains controlled late Loaded and Failed results.
- `blue-image-search-drag`: select/move image, open image search, select three
  results, drag provisional placements, drop and verify loaded placements.
- `delete-board-modal-flash`: opens the delete-board confirmation and captures
  the first modal frame to prove the sidebar blocker stays invisible.
- `search-palette-footer`: captures empty and populated search-palette footer
  states with fixed-width result/status metadata.
- `compact-equal-size-images`, `compact-mixed-column-images`, and
  `compact-windows-column-images`: screenshot-backed compact-arrange examples
  for equal, mixed, and window-shaped placement groups.

Use these as examples, not as a closed set.

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
- `InspectScenario::drive_progress`: step/sample/capture status changes until
  terminal state or timeout.
- `InspectScenario::drive_import_lifecycle`: before/loading/loaded image import
  flow.
- `inspect_search_result_tile_center`: derives palette tile centers from UI
  layout constants.
- `inspect_click_at`: sends pointer press/release with modifiers.

These primitives implement the core verbs:

- `seed`: scenario-specific app setup and deterministic result injection.
- `act`: labels, keys, pointer events, `inspect_click_at`, layout-derived
  coordinates.
- `wait`: `drive_progress`, `drive_import_lifecycle`, explicit `harness.step()`.
- `capture`: `capture_screen`, report facts, events, and assertions.

## Writing A New Scenario

Create a focused Rust test-backed scenario when no existing scenario reproduces
the behavior.

A good scenario:

- seeds deterministic app state
- uses `app_full_harness` for screenshots
- performs user-like actions through labels, keys, or pointer events
- captures meaningful before/mid/after frames
- records structured facts for important app state
- asserts both intermediate and final behavior
- writes artifacts through `InspectScenario`

Keep the scenario name user-behavior oriented:

```text
blue-image-search-drag
sample-image-import
semantic-search-real-install
```

Avoid:

- OS-level mouse automation
- raw app screenshot flags as the primary path
- new legacy `--verify-*` probes
- unstructured `println!` debugging
- sleeps where `harness.step()` can drive frames
- pixel coordinates when a layout helper or semantic target exists

## Search Palette Work

For search result interactions, do not guess tile coordinates. Use:

```rust
inspect_search_result_tile_center(index, result_count)
```

Use deterministic result injection when testing palette behavior. Use the real
semantic search service only when the bug is specifically about search ranking,
indexing, model loading, or asset installation.

When Semantic Search is missing and the task is not about missing-assets UI,
prefer accepting/installing it as part of deterministic setup.

## Progress Work

Use `drive_progress` for downloads, installer state, import state, indexing, or
other repaint-heavy workflows.

The primitive should:

- step one frame at a time
- sample app state
- capture only changed statuses
- record each status as a fact
- stop on terminal state or timeout
- assert success/failure explicitly

Do not use `harness.run()` in a progress loop.

## Import/Loading Work

Use `drive_import_lifecycle` for visual checks around images loading in.

It captures:

- before import
- loading placements
- loaded previews

It asserts:

- expected placement count
- no pending content-resolution work

## Old Probes

Old `--verify-*` flags were removed because useful visual states should be
reproduced through inspect scenarios, not stored behind app startup flags.

Do not add new legacy probes. If an old state matters, rebuild it as
`seed -> act -> wait -> capture`. If it does not matter enough to become a
scenario/helper, leave it deleted.

Historical note: at least one old `--verify-* --screenshot` launch hung during
inspection. Prefer harness-rendered scenarios.

## Reporting Back

Report:

- scenario command
- artifact path
- key assertions
- screenshot paths or inline images when asked
- any limitation in the scenario, especially deterministic injection vs real
  service behavior

Be concise. The artifact is the detailed evidence.
