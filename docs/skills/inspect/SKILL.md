---
name: inspect
description: |
  Reproduce, inspect, diagnose, and verify SnappyRef app behavior with in-process scenarios, app screenshots, structured artifacts, and harness-driven clicks/drags/keyboard input. Use for UI behavior, visual state, app interaction, search palette workflows, imports, progress states, and screenshot-backed verification. Also drives the real app through its MCP tools with mcp-drive.py. features/ maps each user-facing feature to its entry points, scenarios, and coverage gaps.
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
- `capture`: save screenshots or state renders and structured facts/assertions.

Invariant: use user-like actions for the behavior being tested; seed everything
else when direct setup makes the scenario clearer and more deterministic.

## Start Here

Agents verify SnappyRef through these in-process scenarios, not by launching
the GUI. Most scenarios build the full app inside a `cargo test`, drive it
with `egui_kittest`, and write an artifact. Each scenario's own facts live in
its code, in `run-scenario.sh --list`, and in the [feature maps](features/README.md),
not in this skill. There is no server to start or keep alive. The first run compiles the test binary, which takes a few minutes. Later
runs take seconds. The one exception in this skill is `mcp-drive.py`, under
"Driving the real app through MCP" below.

[features/README.md](features/README.md) maps each user-facing feature to its
entry points, scenarios, and coverage gaps. Read the matching feature file
before verifying a change. If you drive only one entry point of a feature that
lists several, report the verification as partial.

Check the checkout first, and again whenever a result looks wrong. The check is
read-only:

```sh
docs/skills/inspect/scripts/doctor.sh
```

It exits 1 when `CARGO_TARGET_DIR` points outside this checkout. In that case a
different worktree's binary would answer for this one, so unset it. It also
lists test binaries and debug-build apps from this checkout that are still
running, and prints the latest artifact and its status.

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

The intentional `check-harness` alias runs `search-drag-to-canvas` and writes
that scenario's artifact. Use it to exercise harness input and capture:

```sh
docs/skills/inspect/scripts/check-harness.sh
```

Use `--list` for the current scenario catalogue. Useful starting examples:

- `sample-image-import`: before, loading, and loaded image states.
- `search-drag-to-canvas`: provisional and committed search-result placement.
- `semantic-search-install`: first-launch installation with a test installer.

The [AGENTS.md explicit-request rule](../../../AGENTS.md#donts) governs
`semantic-search-real-install` and the update smokes.
[Release operations](../../release-operations.md#update-smokes) owns the smoke procedure.

## Driving the real app through MCP

`mcp-drive.py` starts the debug build of the app the way an AI client does and
replays a list of AI control tool calls through `snappyref --mcp`:

```sh
cargo build --locked --bin snappyref
docs/skills/inspect/scripts/mcp-drive.py docs/skills/inspect/scripts/mcp-all-tools.json
```

Use it when a change touches the `--mcp` role, the launch, a tool, or anything
that only the real window and the real helper subprocesses show. It writes the
same artifact shape as a scenario under
`target/snappyref-inspect/mcp-drive/run-<nanos>/`, so `summarize-artifact.py`
and `latest-artifact.sh mcp-drive` read it. Each call is `[tool, arguments]`,
or `[tool, arguments, "refused"]` for a call the app must refuse. An argument
of `"$3.board.id"` takes that value from the reply to call 3. A call's
assertion checks acceptance or refusal. For `create_board`, `rename_board`,
`delete_board`, and `set_settings`, it also checks selected deterministic reply fields.
Read the other replies and effects in `events.ndjson`.

Each MCP reply and final shutdown wait has a 60-second deadline, adjustable with
`--timeout <seconds>`. Use a positive finite number. Once the artifact directory
is created, call-file parsing, setup, replay, and cleanup errors produce a
failed `report.json` and refresh `latest.txt`. A caught drive error adds
`the drive finishes` with the error; event-log failures cannot prevent the report.
CLI usage errors and a missing executable fail before the artifact directory
is created. Do not treat an older latest artifact as evidence for a failed invocation.

The app opens behind the frontmost window and never takes focus. `screenshot`
answers from there, and each image lands in `frames/`.

No tool sends a click, a key, or a drag. This route records what the tools
answer. Gestures, shortcuts, menus, and dialogs still need a scenario.

## Isolation and cleanup

Each scenario opens its library in a fresh directory under the OS temp dir and
removes it on drop. Scenarios never write the user's library or data root, with
one exception. `semantic-search-real-install` downloads the model into the
resolved data root and leaves it there. Runs from the same checkout queue on cargo's build lock, so start them one after
another.

`mcp-drive.py` sets `SNAPPYREF_DATA_ROOT` to a fresh temp directory and removes
it at the end on a best-effort basis. It points the update check at a dead
local address. It attempts to close the MCP helper and stop its app by PID,
on a failed run too. Cleanup steps run independently; timed-out processes
are force-killed, and reported cleanup errors appear in `report.json`.
Directory-removal errors are ignored, so a passing report alone does not
prove the temporary directory was removed. If the driver is killed first,
processes or temporary data can remain; `doctor.sh` lists any app left running
from this checkout.

After successful cleanup, only the artifact remains. If a run hangs, stop the
`run-scenario.sh` or cargo process you started, using its PID. Never kill by
process name, because the user may have a dev build of `snappyref` open.

Artifacts stay under `target/snappyref-inspect/<scenario>/run-<nanos>/` in this
checkout. Removing the worktree deletes them, so copy any artifact you still
need somewhere else first.

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

Use full app viewport screenshots for new scenarios. If a scenario needs a
detail, capture the full app and optionally add a derived crop later.

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
harness.get_by_label("SEMANTIC SEARCH").scroll_to_me(); // captions render uppercase
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

Register the scenario in `SCENARIOS` in `scripts/inspect_scenarios.py` so
`run-scenario.sh` can run it. `latest-artifact.sh <scenario>` prints the newest
run directory.

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

## Refactor behavior evidence

`cargo test --locked` runs every inspect scenario except the `#[ignore]`d ones,
so it is the behavior gate for refactors. The last field of each entry in
`scripts/inspect_scenarios.py` marks an ignored scenario, such as the networked
`semantic-search-real-install`.

Keep the existing tests that describe user-visible outcomes. Add a focused
scenario only when the boundary being changed lacks coverage. Search ranking
and live model installation are separate from deterministic search interaction
tests. Native clipboard/external drops and Windows behavior still need platform
checks when those paths change.

[Perf-profiling](../perf-profiling/SKILL.md#regular-profiling-and-refactors) owns
the before/after sequence and measurement requirements.
