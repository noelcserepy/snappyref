# Budgeted release testing

Status: Mac-only testing continued on 2026-09-21. Known findings fixed; full catalog still has explicit coverage gaps. Companion to the
[128-case functional catalog](release-functional-test-plan.md).

The catalog defines coverage. This document owns execution and agent handoffs.
Use code for assertions, an agent for selected visual judgments, and native
computer use for integrations that the in-process harness cannot exercise.
Do not run the catalog as 128 separate conversations or 16 independent setups.

## Current run scope

The user authorized macOS only, a small native pilot before the full run,
and a maximum of ten percentage points of weekly Codex usage in total.
Use one code worker and one Astra-low Mac pilot. Do not connect to Windows.
Baseline weekly usage is 16 percent; ceiling is 26 percent. Stop dispatching
at 24 percent to reserve room for in-flight work and the final report. Check
usage between short native chunks. Budget includes setup and coordination.
Run records are under `target/release-testing/2026-09-15-mac/`.
The rename crash, Finder duplicate paste and failed-import startup crash are
fixed. Native paste, restart, search, grouping, duplication and persisted Undo
passed. Native group dragging and Redo remain unverified despite passing
related automated checks. The original fixed candidate passed 641 tests and 20
inspect scenarios. The newest candidate passed affected tests and build gates.
The user authorized five more weekly points starting at 23%; ceiling 28%, stop
new chunks at 27%. Last reading 27%, four additional points used. The run is
stopped with headroom; full catalog coverage remains open. Detailed results
are in the run directory README.
The per-platform sections below remain the reusable plan; Windows is excluded
from this run. Real model download and app updating remain excluded.

## September 16 follow-up, categories 1–5

The user authorized five additional weekly percentage points, starting at 38%,
with a ceiling of 43%. Stop adding work at 42%. macOS only; no updating or
model downloads. Continue with the existing code worker and native pilot.
Results live in `target/release-testing/2026-09-16-mac/categories-1-5/`.
New work stopped at 42%, four points above baseline. All 95 affected tests,
the 16-case orientation test, formatting, compilation and Clippy passed.
Seven rows retain unrun automated combinations; native and visual gaps remain.

This run uses an independent copy of the previous disposable workspace and
includes the subsequent group picker and drag-out fixes. It preserves earlier
passing evidence for unchanged behavior and targets missing assertions.

The Shift-marquee failure was a test-input error: the test released Shift
between pointer events. Holding it throughout the gesture passes without an
app change. A new table test checks all eight JPEG/TIFF orientations through
crop and reopening, including removal of the external source file.

Native checks confirmed the visible sidebar toggle, refusal to delete the last
board, board switching with the sidebar hidden, and GIF playback stopping
when deselected. The tool's scroll events pan rather than deliver discrete
mouse-wheel zoom. Earlier scroll observations do not certify native wheel zoom.
Cross-app stacking, desktop minimization state, held-modifier drags and real
trackpad pinch remain outside the documented native tool capabilities.

This follow-up is not a declaration that all 39 cases passed. See its README
and reports for exact coverage and the remaining manual checks.

## September 16 expansion through category10

The next user-authorized ten-point run started43% and stopped53%.
Results: `target/release-testing/2026-09-16-mac/completion-1-10/README.md`.
All seven prior automated intake/navigation gaps passed. Fixed abandoned-hover
imports, rotated cropping, and selected-group deletion. Categories6–10 received
additional coverage, but endpoint editing, moved-textbox focus, legacy clipboard
IDs, native rename discrepancy and detailed variants remain unresolved or untested.
The full checklist is not certified. Exact results and reproduction artifacts
are in that run directory. No new work was dispatched after the cap.

## September 21 continuation through category 16

The user authorized another ten weekly percentage points. Baseline 7%, ceiling
17%, stop new work at 15%. Mac only, no updater or real model downloads.
Results: [Mac findings and remaining checks](release-test-results.md).
Raw evidence: `target/release-testing/2026-09-21-mac/`.

Fixed independent line/arrow endpoint dragging, copied asset IDs invalidated by
deduplication, stale group membership after Library removal, and managed image
paths after moving or copying a Library. The moved-textbox double-click failure
was test timing. The native group-name check passes with confirmed text focus;
the committed name also survives in the database after quitting.

The final full suite passed 657 library tests and 7 other tests, with 3 ignored.
All 23 standard inspect scenarios produced fresh passing artifacts. Formatting,
compilation, Clippy, scenario-registry tests and the release build passed.
One additional picker-navigation test passed after the full suite. The current
release build completed all 15 fast performance trials; the older candidate
timed out on import-20, so no valid speed comparison exists. Helper and archive
validation passed. New work stopped at 15%, eight weekly points above baseline.
The code report maps all 44 category 11–16 cases to exact assertions and remaining
gaps. These are partial cases unless every required assertion is covered.

Earlier category 1–10 evidence remains applicable to unchanged behavior.
Detailed variants and native integration gaps remain in the current report.
The full 128-case catalog is not certified. The user authorized repairs in the
continuation, overriding the original no-repair worker instruction below.

## Remove the expensive repetition

1. Run the existing suite once per platform. Its non-ignored inspect tests
   already execute and capture screenshots. Do not run all scenario commands
   again afterwards, or run the full suite once per agent.
2. Assert counts, geometry, ordering, history, migration, and error handling in
   code. An agent clicking Undo repeatedly is a costly and weaker substitute.
3. Reuse seeded fixture boards and one workspace per native workflow. Creating
   every prerequisite through the GUI tests setup repeatedly.
4. Combine native actions into four working sessions per OS. One import and
   board edit can contribute evidence to several catalog rows.
5. Inspect a small selection of successful artifact frames and every relevant
   failure. Do not send every successful screenshot or full test log to an LLM.
6. Keep failed-case diagnosis out of this run. No self-expanding repair loop,
   second reviewer for every pass, or automatic broader retest.

These changes reduce model work. Fewer tool calls alone do not establish a
subscription saving. Codex usage also depends on model, context, reasoning,
tools, and caching. Measure a pilot before estimating the full run. See
[official usage guidance](https://learn.chatgpt.com/docs/pricing#what-are-the-usage-limits-for-my-plan).

## Small agent team

One coordinator creates at most three subagents for the planned run. No child
delegation and no separate agent per feature. Preserve Astra at low reasoning
for native pilots. Use the configured model for the deterministic worker;
changing that model is optional and secondary to removing LLM work altogether.

| Owner | Work | Returns |
| --- | --- | --- |
| Coordinator | Pin candidate and scope, check budget, dispatch workers, merge coverage, report gaps. | Final summary and ledger link. It does not reread all passing evidence. |
| Code worker | One shell-driven test batch per OS, artifact validation, assertion mapping, small visual sample. | Counts, exact failures/gaps, report paths, visual observations. |
| macOS pilot | Four native workflows below, retaining useful window/fixture context between them. | Checkpoint results and evidence paths. |
| Windows pilot | Same four workflows on native Windows, emphasizing OS-specific input and packaging. | Checkpoint results and evidence paths. |

Spawn each child with no inherited conversation history. Supply absolute
candidate/data/report paths, relevant instructions, and only its assigned
workflow or coverage excerpt. Repository rules still apply. Do not give every
child the entire catalog, source tree, earlier discussion, and other agents'
screenshots. Reuse a pilot for successive workflows; replace it only if context
growth or repeated confusion makes a small clean handoff preferable.

Only one worker may own a desktop's pointer, keyboard, clipboard, or candidate
workspace at a time. Separate app windows are not isolation. Separate OS hosts
may run concurrently only if their control sessions are independent. The code
worker's native clipboard test also needs this ownership. Parallelism saves
elapsed time; it does not make agent work free. Default to sequential native
runs for clearer usage measurement.

## Stage 1. One deterministic batch

Run required checks in a shell process with full output redirected to files.
Return a compact machine summary, not every successful test name. Wait on the
process instead of asking an LLM to inspect unchanged logs repeatedly.

- Run the required formatting, clippy, and compilation gate with `--locked`
  where applicable and shipping default features.
- Record a start timestamp and run
  `cargo test --locked -- --test-threads=1` once. This includes the library's
  non-ignored inspect scenarios. Do not also run `cargo test --lib` or every
  scenario individually. Retain required release checks from
  [release operations](release-operations.md).
- Validate unique non-networked scenario artifacts using the existing
  `SCENARIOS` catalog and `check_artifact` in
  [inspect_scenarios.py](../scripts/inspect_scenarios.py). Skip the alias and
  real-install scenario. Require fresh reports, passing assertions, and
  existing frame files. Collect validation errors rather than dropping them.
- Run archive validation and a production image-preparation helper check
  against the actual candidate. The reusable `check_imageprep` function in
  [check-refactor-behavior.py](../scripts/check-refactor-behavior.py) accepts a
  binary path. Do not run that script's entire main routine after the full
  suite: it repeats compilation, library tests, and a release build. Its PNG
  helper check does not certify all supported formats.
- On Windows, use the repository sync/check/test wrappers and shipping
  dependencies. Validate reports on the Windows host. SSH shell access does
  not imply access to the interactive Windows desktop.

Never pass `--ignored` to the whole suite. The explicitly named
`native_windows_clipboard_image_metadata_files_and_pureref_bytes` test is
ignored because it writes the system clipboard. Schedule that one test alone
in a disposable Windows desktop when clipboard modification is in scope. It
can cover native format exchange but cannot prove actual PureRef, browser,
or Explorer interoperability by itself.

### Reuse these checks before adding tests

These are source-identified coverage candidates, not a claim that the current
candidate has passed. Before marking a catalog assertion covered, read the
test's actual assertion and attach a fresh passing result. A test name or
matching module is not enough.

| Catalog coverage | Existing evidence to reuse | Native work still needed |
| --- | --- | --- |
| Board lifecycle and history | `board-lifecycle-undo-redo`; `board_delete_undo_restores_document_view_history_and_active_fallback`; board create/rename and numeric-shortcut tests in `src/app/tests/board_lifecycle.rs`. | One real create/rename/switch/delete sequence and modal focus check. |
| Intake lifecycle and duplicates | `pending-drop-batch-undo`; `cancelled_reservation_rejects_its_late_result`; `path_import_reuses_existing_exact_asset_and_removes_duplicate_original`; local URL-server tests in image preparation. | Finder/Explorer drops and positions, browser URL intake, native progress responsiveness. |
| Canvas editing and navigation | `canvas-edit-operations`, `canvas-navigation-boards`, `crop-undo-redo-placement`; resize/focus/selection tests in `src/canvas/tests/selection_transform.rs`. | Actual modifier/letter-held drags, trackpad/mouse input, one rendered result per editing mode. |
| Layout | The three compact scenarios; geometry tests in `src/canvas/tests/selection_transform.rs` and placement modules. | One mixed arrangement and snap interaction. Code owns each direction, gap, and size permutation. |
| Groups | `group-operations`; `group_document_history_survives_transfer_delete_restart_and_board_archive`. | Create, edit a member, quick-add, change flow, and container resize as one workflow. Check remaining picker/style assertions before assigning them to code. |
| Annotations and text | Existing annotation geometry/editor tests, mixed selection tests, `canvas-edit-operations`. | Real text focus, typing, arrow/line creation, one toolbar change and textbox resize. Not every swatch through computer use. |
| Internal clipboard | `cross_board_paste_creates_new_images_and_annotations_on_active_board`; clipboard serializer/read tests and group tests. | One mixed cross-board round trip and one native image round trip per OS, plus file/PureRef producer integrations. |
| Search UI and model state | `search-drag-to-canvas`, `blue-image-search-drag`, `search-palette-footer`; selection/paging, stale query, installer coalescing/cancel/failure tests in `src/search/controller.rs`. | Real inference and idle indexing; one drag placement, keyboard placement, and cancellation. Fake results do not prove relevance. |
| Library removal and dedupe | `archive_assets_removes_their_placements_from_all_boards_and_preserves_files`; `exact_dedupe_relinks_placements_to_first_asset_and_deletes_duplicates`. | One confirmation flow and cross-board visible result. No manual construction of legacy duplicate records. |
| Settings, restart, migration, failed writes | `session-restart-restore`, `persistence-write-retry`; storage migration tests; session/settings tests. | Actual process restart and one representative old-workspace copy; native window state and installed-location checks. |
| Image formats | Existing preparation/decode tests, including crop pixels and animated WebP reload. | Candidate decoder coverage where fixtures/assertions are missing; small visual sample for orientation, transparency, animation, and native formats. |

### Gaps worth coding, only if absent

First remove duplication and confirm existing coverage. Prepare at most three
focused additions before freezing the candidate, if their missing assertions
justify the work:

1. A table-driven production-helper format check. Feed fixed redistributable
   fixtures to the candidate helper, assert success, dimensions and appropriate
   pixel/reference comparisons, and corrupt-input failure. Avoid exact pixel
   equality for lossy or platform color-managed output. This replaces repeated
   GUI imports of each format while retaining a visual sample.
2. One inspect sequence for uncovered toolbar/focus combinations, using actual
   widget actions. Assert stored style/text and that unrelated board content
   stays unchanged. Test only missing cases, not a new test for every swatch.
3. One settings-input boundary test for malformed/clamped values and focus,
   if the current suite lacks it. Do not have a pilot type the same invalid
   strings into every field.

Do not build a generic visual-testing framework or a second scenario registry.
Use a small batch invocation of the existing tools and extend the coverage
ledger with exact assertions. If a one-off native check is cheaper than writing
and debugging a new test, keep it native. New tests are not free during the
first run, even when later execution needs no LLM decisions.

### Visual review without replaying the app

The code worker reviews at most eight representative full-window frames from
fresh passing scenarios: canvas edit, crop, compact layout, group controls,
search placement, installer status, delete modal, and restored session. Select
meaningful frames from report metadata. Read the associated assertions first.

Inspect failure frames as needed within the failure budget. If eight images
cannot establish an asserted visual property, leave that property unverified.
Do not downgrade screenshot resolution until text/handles become unreadable,
and do not label a numeric assertion as visual verification. Full artifacts
remain on disk without being repeatedly loaded by the coordinator.

## Stage 2. Four native workflows per platform

These replace category-by-category GUI replay. The pilot receives numbered
actions and expected checkpoints extracted from the linked catalog. Catalog
IDs below identify related coverage, not blanket passes for every subcase.
Assert detailed permutations in Stage 1 or leave them open.

| Workflow | Sequence using shared setup | Main coverage |
| --- | --- | --- |
| N1. Launch and intake | Start fresh, decline model offer, create/rename a board, toggle sidebar and Always on top, resize window, drag a local batch at an off-center point, try an abandoned hover, paste a URL. Check loaded content, pointer placement, modal/input behavior, and viewport. | WIN-01 through WIN-07; representative BRD-01/02; IN-01 through IN-05. |
| N2. Edit a reference board | Reuse N1's board. Select/move/resize/rotate/crop; arrange and snap; group, reorder a member, quick-add, change flow/width; draw arrow, line, textbox; change one style and test text focus; mixed copy to B; one undo/redo chain. | Native portions of NAV, EDIT, LAY, GRP, ANN; CLIP-04/06; focus boundaries in SET. |
| N3. Native media and search | Use the small fixture/model baseline. Native image copy both directions, file clipboard and PureRef paste, browser drag, wheel/trackpad gestures, animation. Query two known subjects and one image pivot; import a new subject and allow indexing; place by drag and keyboard, cancel one preview. | CLIP-01/02/03/05/07; NAV-01/02/03; IMG-03/04/05; SEARCH-01/02/03/06/09/10/11. |
| N4. Persistence and installed use | Reopen N2's workspace; check content/settings and undo. Exercise board deletion cancellation/confirmation and one Library removal across boards. Open the old-workspace copy. Verify writable-install storage and isolated protected-location fallback; finish with a board overview. | BRD-05/06; LIB-01/02; SET-01/02; SAVE-05/06/07/08; END-01/02/04. |

One screenshot can support several observations when they are visible at the
same checkpoint. Example: the reopened board can establish the saved group,
textbox, crop, and board background without four restarts. It does not prove
the entire undo history or invisible storage invariants.

Preflight unavailable integrations once. Missing Windows desktop access,
PureRef, protected-location account, true trackpad input, or existing-data
fixtures become specific BLOCKED rows. Do not let pilots install tools, invent
remote-control workarounds, or repeatedly retry an unsupported gesture.

Live model installation, controlled network failure, offline verification,
and deeper image-residency stress remain separate catalog checks. Use existing
deterministic evidence where sufficient; otherwise report the residual gap.
Do not sneak them into a cheap-pass completion claim. Real installer execution
still needs the explicit request required by the repo.

### Batch actions, not unobserved assumptions

- A safe action block can focus a known field, select its contents, type, and
  commit before one observation. A known shortcut sequence can also be batched
  when each intermediate state is not itself the test.
- Stop at changed layouts, dialog appearance, async completion, unexpected
  results, and spatial gestures whose outcome controls the next target.
- Use supported computer-use APIs only. Some tools return state automatically;
  do not request a second screenshot of the same state.
- Capture the required evidence once. Save paths in the ledger; avoid copying
  screenshots into every message between agents.
- For loading/indexing, use bounded programmatic waits or existing inspect
  state checks where available. Otherwise use a small number of spaced UI
  observations, not an LLM turn for every progress percentage.

## Budget and stop conditions

Current authorized ceiling: ten percentage points of the weekly allowance,
measured from the run's starting usage, including preparation and coordination.
This is a spending limit, not a prediction that the full matrix will fit.
Stop new dispatches at eight points to leave headroom.

1. Record the relevant weekly and five-hour bucket identifiers, used percentage,
   and reset times before any preparation worker starts. Use the app's usage
   tool when available. These are account-wide, so other tasks contaminate the
   measurement; resets invalidate a simple before/after subtraction.
2. Run the code batch first. Then pilot only N1 on macOS, with a checkpoint
   after at most 12 computer-use decision cycles. A cycle is one model decision
   followed by a tool action block and its resulting observation. Save progress
   even if N1 is unfinished.
3. Read usage again. If the delta is meaningful and the budget has room,
   continue in chunks of at most 12 cycles, checking at each boundary. Use the
   pilot as a rough range, not a linear cost guarantee for harder workflows.
4. Each native pilot stops at 60 cycles total unless the user expands the run.
   At most one clean retry per failed check. A blocked integration gets no
   second agent attempt using the same unavailable tool.
5. Stop new work when either usage window is near its limit, the weekly ceiling
   is reached, or a candidate-level failure invalidates downstream work. Preserve
   reports and list all unrun coverage. Never silently switch models, consume
   reset credits, purchase credits, or continue into a new quota window.
6. If usage is missing, stale, too coarse, or reset during the pilot, do not
   infer zero cost. Stop after that pilot and report the measured limitations
   before scheduling the rest.

Percentage checks and cycle limits are operational controls, not a hard
subscription spending cap. Tools can report usage late and a single chunk can
overshoot. Do not claim an enforceable token limit unless the execution system
actually exposes one. Keep the coordinator quiet between worker completions
and budget checkpoints; do not poll or summarize every action.

## Coordinator handoff

Supply paths and budget before execution. Do not dispatch this prompt with
unresolved placeholders.

```text
Coordinate SnappyRef release testing using docs/release-test-execution.md.
Candidate commit and archives: <paths, hashes, versions>.
Disposable roots, fixture manifest, model assets, old-workspace copy: <paths>.
Reports and coverage ledger: <paths>.
Available native desktops: <hosts and control method>.
Weekly usage ceiling and headroom: <adopted limits>.
Live model install: <authorized or excluded>.

Use one code worker and at most one Astra-low native pilot per OS. No nested
delegation. Give workers only their own instructions, paths, assertion rows,
and current workflow. Keep one owner per desktop/clipboard/data root.

Run the deterministic batch once per OS. Map exact passing assertions to the
catalog, and retain gaps. Do not duplicate the suite through scenario commands.
Then run the bounded N1 pilot and measure usage before dispatching more native
work. Reuse pilots for N2-N4 in bounded chunks. No automatic repair work.

Workers return a compact result: covered assertion IDs and mode, failures,
blocked/unrun checks, evidence paths, cycles used, and exact resume state.
Keep full logs and screenshots on disk. Read detailed evidence only for a
specific uncertainty. Merge evidence without turning partial rows into passes.
Stop at budget boundaries and report what remains. Updating is excluded.
```

## Completion criteria

The final ledger distinguishes automated, visual-artifact, and native evidence.
Report fully covered cases, partially covered cases, failures, blocked cases,
and not-run cases for each OS. A sampled native workflow is not a full pass
for its related catalog IDs. Required assertions without evidence remain open.

This budgeted run may finish with a clear, small gap list. That is preferable
to spending the rest of the week attempting to turn every uncertainty into a
green checkbox. The maintainer can choose a targeted code addition, short
human check, or another bounded agent run for each remaining gap.
