# Inspect Notes

## 2026-05-19 Script Trial

Command loop tried:

```sh
docs/skills/inspect/scripts/run-scenario.sh search-drag-to-canvas
docs/skills/inspect/scripts/latest-artifact.sh search-drag-to-canvas
docs/skills/inspect/scripts/summarize-artifact.py <artifact>
docs/skills/inspect/scripts/check-harness.sh
```

What worked:

- `run-scenario.sh search-drag-to-canvas` is the right main handle. It hides the
  Cargo filter and feature flags and prints `status`, `scenario`, and
  `artifact`.
- `latest-artifact.sh search-drag-to-canvas` is useful after the fact, but the
  normal loop should use the artifact path printed by `run-scenario.sh`.
- `summarize-artifact.py` gives the right high-level evidence: assertions,
  actions, targets, facts, and frame paths.
- `check-harness.sh` works as fixed-scenario sugar over `run-scenario.sh` and
  now writes artifacts under `target/snappyref-inspect/check-harness`.

What was noisy:

- `cargo test` warnings dominate the shell output before the script's
  `key=value` lines. Agents should read the final `status=` and `artifact=`
  lines, not scrape the whole command output.
- Running multiple Cargo commands in parallel causes file-lock waiting. For this
  loop, run scenario commands serially.

Screenshot findings:

- Screenshotting needs to be automatic. The first artifact shape without frames
  was too incomplete for UI work.
- Enabling `egui_kittest`'s renderer and trying to capture the raw full-app
  frame produced an all-transparent PNG, even though the app state and scenario
  assertions passed.
- The current working screenshot is therefore a deterministic board-state render
  written by the scenario from the committed Placement geometry. It is useful
  evidence for "where did the board item land?" but it is not yet a raw egui
  screenshot.
- The summarizer now lists frame paths, so no separate `open-frames.sh` is
  needed yet.

Current frame path example:

```text
target/snappyref-inspect/search-drag-to-canvas/<run>/frames/board-after-release.png
```

Best current workflow:

1. Run `run-scenario.sh <scenario>`.
2. Copy the printed `artifact=` path.
3. Run `summarize-artifact.py <artifact>`.
4. Inspect the listed frame path when visual evidence matters.
5. If the summary lacks a decisive fact, add one structured observation or
   assertion to the same scenario and rerun it.

Open issue:

- Replace the board-state render with a true egui/frame capture once the blank
  `egui_kittest` render path is understood. Until then, skill instructions must
  call the frame a deterministic board-state render, not a raw screenshot.

## 2026-05-19 Search Palette Screenshot Trial

Historical note: this documents the old app screenshot/probe path before the
inspect harness replaced it. Do not use this command shape for new work.

Command tried:

```sh
SNAPPYREF_LIBRARY_ROOT=<tmp-library> cargo run --bin snappyref -- \
  --verify-palette \
  --screenshot /tmp/snappyref-palette.png
```

What worked:

- The existing app-native screenshot path can capture a real rendered app PNG.
- The command must use `cargo run --bin snappyref`; plain `cargo run` fails
  because the repo has multiple binaries.
- A disposable `SNAPPYREF_LIBRARY_ROOT` works for avoiding user Library state.

What did not work cleanly:

- `--verify-palette` on a fresh Library also shows the Semantic Search
  first-launch prompt. The screenshot is real, but it is not a clean palette
  capture.
- A quick attempt to mark the prompt as seen for `--verify-palette` made the
  screenshot command hang waiting for capture, so this path needs proper
  diagnosis rather than a drive-by tweak.

Skill implication:

- Static screenshots still need script support. The agent should not have to
  remember `--bin snappyref`, `SNAPPYREF_LIBRARY_ROOT`, screenshot paths, or
  known probe quirks.
- Palette capture should become a named static scenario/probe with a clean
  artifact path and a nonblank screenshot assertion.

## Direction: Inspect v2 Replaces Old App Inspection

The reason for this work is dissatisfaction with the old `app-inspection`
workflow. The new inspect skill should be treated as a better v2 built from
first principles and more experience, not as a requirement to migrate the old
workflow in its entirety.

The old workflow is input material. Keep only the pieces that still work and are
useful. Do not preserve old probes, flags, or habits merely because they exist.

What the old path does well:

- It can capture real app-rendered PNGs through the app's screenshot flow.
- It already has useful static verification states such as palette, sidebar,
  arrow, textbox, search drag preview/source, import matrix, backgrounds, and
  design-system preview.
- It uses disposable `SNAPPYREF_LIBRARY_ROOT` cleanly when the agent remembers to
  set it.

What the old path does poorly:

- The agent must know exact CLI flags.
- The agent must remember `cargo run --bin snappyref`.
- Output paths are ad hoc.
- The resulting screenshot is not automatically summarized into an artifact.
- Probe quirks are implicit; for example, `--verify-palette` currently opens the
  first-launch semantic-search prompt on a fresh Library.
- It cannot reproduce held drags or multi-frame interactions reliably.

Target use through new primitives/scripts:

- Useful static app states should become named inspect scenarios. The legacy
  `--verify-*` flags have been removed and should not be reintroduced as a
  state catalog.
- `run-scenario.sh <scenario>` should be the only command agents need for both
  interactive and static inspection.
- Every static scenario should write the same artifact shape:
  - `report.json`
  - `events.ndjson`
  - `frames/<name>.png`
- Every static screenshot scenario should assert:
  - screenshot file exists
  - screenshot is nonblank
  - expected UI state is present through at least one structured observation
    where possible
- `summarize-artifact.py` should list frames for both static and interactive
  scenarios.

Minimal primitive expansion needed:

- Add a `StaticScreenshotScenario` primitive that can run an app verification
  state with:
  - scenario name
  - disposable Library root
  - app arguments
  - screenshot output path under the artifact dir
  - expected screenshot path in `report.json`
  - nonblank image assertion
- Add a `semantic_search_install_policy`/seed option for inspection:
  - default for most inspect scenarios: accept or trigger Semantic Search install
    if not already installed
  - explicit override for states that must show missing/install UI
  - static palette capture should suppress the first-launch prompt unless the
    scenario is specifically testing that prompt
- Add static states as normal `run-scenario.sh <scenario>` scenarios rather
  than separate probe wrappers.
- Add screenshot validation to the recorder/summarizer:
  - dimensions
  - unique color count or non-transparent/nonblank bounds
  - frame paths in summary

Semantic Search install preference:

- For most inspection tasks, if Semantic Search is not installed, the inspect
  flow should accept/install it rather than stopping at missing-assets UI.
- Missing-assets UI should be tested only by explicit scenarios such as
  `semantic-search-first-launch-prompt` or `semantic-search-install`.
- This preference should live in deterministic scenario setup, not in agent
  instructions that require manual clicking.

Immediate next useful scenario:

- `palette-open`
  - opens the Search palette
  - uses disposable Library root
  - applies the default "accept/install semantic search if missing" policy
  - captures a real app PNG into `frames/palette-open.png`
  - asserts the screenshot is nonblank
  - records whether the palette, missing-assets card, or first-launch prompt was
    shown

Important caution:

- Do not tell agents to use old probes directly once a named inspect scenario
  exists for the same state. The old probe can remain as an implementation
  detail inside the scenario until fully migrated.
- Do not aim for one-to-one parity with old `app-inspection`. If an old probe is
  noisy, stale, redundant, or does not support the new evidence loop, leave it
  behind.

## 2026-05-19 Semantic Search Install Scenario Trial

Command tried:

```sh
docs/skills/inspect/scripts/run-scenario.sh semantic-search-install
artifact=$(docs/skills/inspect/scripts/latest-artifact.sh semantic-search-install)
docs/skills/inspect/scripts/summarize-artifact.py "$artifact"
```

What worked:

- The new inspect script surface can open the app in the in-process harness and
  click the real first-launch `Enable` button via AccessKit.
- The scenario uses a test installer, so it proves "accept install" behavior
  without downloading the 253 MB semantic-search asset during verification.
- The artifact records:
  - first-launch prompt visible
  - `snap.semantic-search.install.enable` button rect
  - click action at the button center
  - install start count
  - app status `Installing Semantic Search`
  - model status `Downloading { percent: 0 }`
  - first-launch prompt marked seen

Current limitation:

- This validates accepting the install prompt, not completing the real asset
  download. A separate explicit scenario should cover full download/install only
  if needed, because that is networked, large, and much slower than the default
  inspect loop.

Skill implication:

- For normal UI inspection, "accept Semantic Search install if missing" can be a
  deterministic harness setup action. The agent should not manually click this in
  a live app unless the task is specifically about the real installer.
- Use `semantic-search-install` as the harness health check for this behavior.

## 2026-05-19 Sidebar Status Screenshot Trial

Command:

```sh
docs/skills/inspect/scripts/run-scenario.sh semantic-search-sidebar-status
artifact=$(docs/skills/inspect/scripts/latest-artifact.sh semantic-search-sidebar-status)
docs/skills/inspect/scripts/summarize-artifact.py "$artifact"
```

What worked:

- Rendering the sidebar through `egui_kittest` gives a deterministic PNG and
  exits under test control.
- The scenario records the actual local semantic-search model-store status as a
  fact, opens the Settings sidebar, scrolls the Semantic Search section into
  view, captures `frames/sidebar-semantic-search-status.png`, and asserts the
  frame exists.
- This is the right shape for static UI inspection: no manual mouse movement,
  no live app process to close, one named scenario, one artifact.

What did not work:

- Wrapping the old `--verify-sidebar --screenshot` app process in the inspect
  script hung without writing a screenshot. That path is not reliable enough as
  a default inspect primitive.

Skill implication:

- Prefer harness-rendered frames for deterministic app states.
- Keep old screenshot probes as migration input only. Do not make them the
  default inspect path unless a scenario proves they exit cleanly and write a
  nonblank frame.

## 2026-05-19 Full-App Screenshot Primitive

Decision:

- Inspect screenshots are always full-app screenshots.
- Component-only screenshots are not a public inspect concept. They can remain
  unit-test internals, but inspect artifacts should represent what the user
  would see in the app viewport.

Implemented behavior:

- `InspectScenario::capture_screen` renders through the full app harness.
- Saved frames are composited over the app background and forced to opaque PNGs.
- Frame events record width, height, raw transparent pixel count, and a sampled
  unique-color count.
- The sidebar status scenario now asserts:
  - viewport is `1200x800`
  - transparent pixel count is `0`
  - frame is nonblank

Reason:

- The earlier sidebar frame rendered only `show_sidebar(...)`, leaving the rest
  of the PNG transparent. That was technically useful but not reliable user
  evidence. The new invariant is simpler: every inspect screenshot is a full app
  viewport, and transparency is treated as a failure signal.

## 2026-05-19 Real Semantic Search Install Trial

Command:

```sh
docs/skills/inspect/scripts/run-scenario.sh semantic-search-real-install
```

What worked:

- The scenario opened the full app harness, opened Settings sidebar, clicked the
  real `Install` button through AccessKit, ran the real semantic-search asset
  installer, and captured full-app screenshots for every observed status change.
- The download/install completed successfully in about 18 seconds in this run.
- Captured frames included before-click, after-click, progress states from 1% to
  100%, and the final `Installed` state.

Important harness lesson:

- Long-running progress flows must use `harness.step()` in a loop, not
  `harness.run()`. The app intentionally requests repaint during install, so
  `run()` can exceed the harness max step count.

Skill implication:

- For progress workflows, inspect should drive one frame at a time, sample state,
  capture only changed states, and stop on a terminal assertion.

## 2026-05-19 Progress Primitive Refactor

Implemented:

- Added `InspectScenario::drive_progress`.
- Added `InspectProgressConfig`.
- Updated `semantic-search-real-install` to use the primitive instead of an
  ad hoc loop.
- Updated `run-scenario.sh` with `--list` and `--help`, and kept
  `semantic-search-real-install` as the script-level scenario entry point.

Primitive contract:

- Use `harness.step()` for progress workflows.
- Sample state after each step.
- Capture a full-app screenshot only when the sampled status changes.
- Record each status as a fact.
- Stop on terminal state or timeout.
- Record a terminal assertion and timeout assertion.

Reason:

- This behavior is deterministic and easy to misuse. Encoding it as a primitive
  is better than relying on skill instructions to remember not to use
  `harness.run()` during repaint-heavy flows.

## 2026-05-19 Sample Image Import Scenario

Command:

```sh
docs/skills/inspect/scripts/run-scenario.sh sample-image-import
```

What worked:

- Added `sample-image-import` as a script-level scenario.
- Captured full-app screenshots before import, during loading placeholders, and
  after loaded image previews were integrated.
- The scenario uses app import lifecycle primitives directly:
  - `reserve_import` creates committed loading placements
  - `integrate_loaded_import` replaces them with prepared image previews

Reason:

- This avoids flaky OS drag/drop timing while still exercising the app's loading
  and loaded canvas states. It is a good deterministic primitive for visual
  checks around "images are loading in" behavior.

## 2026-05-19 Import Lifecycle Primitive Refactor

Implemented:

- Added `InspectImportSample`.
- Added `InspectScenario::drive_import_lifecycle`.
- Refactored `sample-image-import` so the scenario only defines samples and
  delegates the before/loading/loaded screenshots, facts, and assertions.

Primitive contract:

- Capture `screen-000-before-import`.
- Reserve loading placements through the app import lifecycle.
- Record loading count and pending content-resolution count.
- Capture `screen-001-during-import-loading`.
- Integrate prepared loaded previews.
- Record loaded count and pending content-resolution count.
- Capture `screen-002-after-import-loaded`.
- Assert expected placement count and zero pending imports.

## 2026-05-19 Blue Image Search Drag Scenario

Command:

```sh
docs/skills/inspect/scripts/run-scenario.sh blue-image-search-drag
```

What worked:

- Full-app harness screenshots handled a multi-step workflow cleanly:
  - app open with a blue image
  - click-select the blue image
  - drag-move it to the right
  - open an image-pivot search state
  - show loading/search-started state
  - inject deterministic search results
  - multi-select three results with command-click events
  - drag selected results onto the canvas
  - capture provisional placements before drop
  - drop and verify three additional loaded placements
- Pointer events were reliable enough when expressed as app coordinates and
  backed by state assertions.

What was still manual:

- Search result tile coordinates are still hard-coded (`344/472/600, 206`).
  This is acceptable for a first scenario but should become a small helper that
  derives tile centers from the palette layout.
- The search-results-ready screenshot may capture thumbnail placeholders before
  async preview decoding completes. The later selected/drag frames showed the
  preview colors. A future helper should wait for preview cache readiness when
  visual thumbnail fidelity matters.
- The search itself was deterministic state injection, not a real semantic
  query. That is good for repro stability, but the skill should distinguish
  "semantic search behavior" from "palette/result drag behavior".

Skill implication:

- This scenario is strong evidence for the v2 inspect loop: complex click/drag
  behavior is reproducible without OS mouse automation.
- Next useful primitive: `search_result_tile_center(index)` / palette geometry
  helper, so agents stop guessing tile coordinates.

## 2026-05-19 Search Result Tile Center Primitive

Implemented:

- Added `inspect_search_result_tile_center(index, result_count)`.
- Refactored `blue-image-search-drag` to use it instead of hard-coded result
  tile coordinates.

Primitive contract:

- Derive tile centers from the same palette layout constants the UI uses:
  `PALETTE_WIDTH`, `SEARCH_OVERLAY_TOP_MARGIN`,
  `search_result_grid_layout`, and `search_result_tile_height`.
- Keep scenario code focused on intent: select first/second/third result, not
  pixel constants.

Remaining caution:

- The helper still mirrors some overlay chrome geometry (`body_h_pad`, header
  height). If the palette structure changes significantly, this should move
  closer to the UI code or expose a test-only geometry helper from the palette.
