# Jev grouping pilot

This manual experiment asks whether Jev can complete one UI goal from changing
screen observations. It is not a release gate or a general app controller.

The goal is to import two supplied PNGs, group them, name the group `Reference`,
choose Column layout, and leave text editing. Every trial starts with an empty
disposable library. The only setup variations are zoom and pan. Jev creates the
board content through file-drop input, not seeded placements or group fixtures.

## Run

```sh
# Three scripted comparison runs; no API key or network calls.
docs/skills/inspect/scripts/run-scenario.sh jev-grouping-pilot

# Six live runs, two at each zoom/pan setting.
TYPESAFE_API_KEY_FILE=/private/path/to/key JEV_PILOT_LIVE=1 \
  docs/skills/inspect/scripts/run-scenario.sh jev-grouping-pilot

# Replay one recorded trial with exactly its inputs and camera, without Jev.
JEV_PILOT_REPLAY=/absolute/artifact/path/trial-00.json \
  docs/skills/inspect/scripts/run-scenario.sh jev-grouping-pilot
```

`TYPESAFE_API_KEY` is also accepted. Keys never enter requests recorded on disk.
Use `CARGO_TARGET_DIR` to share existing build dependencies. On Macs with an
unconfigured Xcode installation, an already installed Command Line Tools SDK can
be selected for these commands with `DEVELOPER_DIR=/Library/Developer/CommandLineTools`.

The scenario is ignored by normal `cargo test`. Live calls require explicit
`JEV_PILOT_LIVE=1`. It pins `jev-1.13.0`, permits at most 24 decisions per trial,
has a 15-second HTTP timeout and stops making calls after 500,000 reported input
tokens. One final response may cross that token threshold. There are no retries
or reasoning-model fallbacks. The test exits unsuccessfully if any trial fails,
after writing all trial results.

## What Jev gets

Each call includes the goal, reusable sample identities, the current observation,
the six most recent inputs, remaining decisions and a Choice of available inputs.
There is no observe action, prescribed sequence, or oracle feedback.

- Accessibility controls provide actual current labels, draft values, focus,
  selection, enabled state and screen rectangles. The selected Row/Column/Wrap
  button needs supplemental state because egui omits that accessibility property.
- Supplemental canvas records provide visible placements and groups, selection,
  membership, source identity, world geometry and screen rectangles. Placement
  clicks are offered only when the existing canvas hit test reaches that image
  and no foreground egui layer or modal blocks its center.
- The viewport includes pan, zoom and its rectangle in egui logical points.
- Inputs include file drops at two canvas locations, pointer clicks and shift
  clicks, keyboard shortcuts, supplied text entry, replace focused field text,
  wait, done and blocked. Replace uses Select All followed by text input.

The executor only sends harness inputs. It does not directly create groups,
change names, set flow or select images. The scripted controller uses the same
executor. The independent final oracle checks distinct loaded images, exact
membership, the committed name, Column setting, column geometry and editor focus.

## Evidence and limitations

The existing inspect directory contains `report.json`, `events.ndjson`, full-app
screenshots before each decision and after each trial, `summary.json`, and one
`trial-NN.json` with complete API requests, responses and replayable inputs.
Inspect screenshots against observations before attributing a failure to the app.
Replaying removes model variability; it does not establish that the harness is
correct.

The controller and observer live in a test-only app module. A small test-only
function exposes the existing canvas hit test. No runtime hooks, dependencies,
CLI flags or production UI behavior change. Removing the experiment means deleting
that module, its declaration, the test-only function and its inspect catalogue entry.

This intentionally limited observer uses document geometry, not a new rendering
observer. It assumes axis-aligned sample images and a closed sidebar. It does not
validate textures, occlusion, cropping, native file dragging, OS shortcuts, helper
subprocesses or persistence across restart. Source filenames are supplied identity
metadata, not painted labels. The normal test image-preparation worker decodes in
process. The existing full-app harness uses its Nix keyboard configuration, with
abstract Command/Ctrl modifiers. These results do not certify native macOS input.

A useful signal would be successful runs across variants, understandable failure
records, or an unplanned interaction sequence exposing a reproducible app problem.
Six runs cannot establish a general reliability rate. A successful scripted
comparison plus unreliable Jev runs would argue against expanding this approach.

## Results, 2026-09-21

Tested against app commit `6be5d4b4ede7151852e27191e0e250923bba61fc`. Uncommitted
work in the main checkout was excluded. All API responses reported `jev-1.13.0`.
No reasoning model intervened during a trial.

| Controller / observer | Trials completed correctly | False completion claims | Decisions | Input tokens |
| --- | ---: | ---: | ---: | ---: |
| Initial scripted comparison | 3 / 3 | 0 | 33 | 0 |
| Initial Jev observer | 0 / 6 | 0 | 144 | 248,368 |
| Jev with pointer reachability | 0 / 6 | 0 | 144 | 434,903 |
| Final Jev, selected layout and field replacement included | 0 / 6 | 3 | 126 | 380,363 |
| Final scripted comparison, deliberately overlapping imports | 3 / 3 | 0 | 33 | 0 |
| Replay of final Jev trial 1 | 0 / 1 | 1 | 14 | 0 |

The final six trials used zoom/pan settings `(1, 0, 0)`, `(0.7, 100, -60)` and
`(1.3, -90, 50)`, repeated twice. Three exhausted the 24-decision limit; three
declared done with an incorrect board. None hit the API token limit or a service
error. This is a small development sample, not a held-out model benchmark.

Median final API latency was 312 ms per decision. Median final trial wall time was
16.25 seconds including screenshots and harness stepping, compared with 5.12
seconds for the script. The final six trials cost approximately $0.016; all 18
live development trials together used 1,063,634 input tokens, approximately
$0.045 at [$0.042 per million input tokens](https://docs.typesafe.ai/models).
These are token-based estimates, not a billing reconciliation.

### What the experiment found

The first failure was in the observer. Jev dropped both samples at the same point,
but the observer offered a click on the covered image. Jev repeated that click.
The final implementation uses the existing canvas hit test and egui layer lookup
to exclude unreachable placement centers. This does not implement general visual
occlusion.

The second observation gap was missing selected state on the layout buttons.
The accessibility tree returned neither `selected` nor `toggled` for the visibly
selected Column button. The final observer supplements those three rendered
buttons using the selected group's current flow. A generic field replacement
input was also added after Jev appended Reference to Group 1.

After those changes, Jev still selected only one image, created a one-member
group, and tried reimporting the other sample instead of selecting the existing
copy. It repeatedly imported and undid imports. The final scripted comparison
imports both images at that same overlapping position, then successfully selects
both with Command/Ctrl+A. The required interaction is available through the same
executor.

In final trial 1, Jev declared done after 14 decisions. The last observation
explicitly reported group 4 with members `[3]`, placement 2 with no group, and
placement 3 in group 4. The group name and Column setting were correct, but
membership and column geometry failed the independent oracle. Replaying all 14
recorded inputs without Jev produced exactly the same oracle results. The done
choice in this trial had only 0.15 confidence; this pilot executes the top choice
without confidence gating. Abstaining could avoid accepting that decision, but
would not turn it into a completed goal. Full-app
screenshots confirmed the overlapping image and one-member group. No new app bug
was established.

The useful result is a limit on this approach: cheap, fast decisions did not
produce reliable completion, and screen observations needed app-specific work
even for this small case. Do not expand this controller across the app yet. Keep
existing deterministic inspect scenarios as the release checks. This experiment
can support another explicitly scoped investigation, but should not become an
ongoing prompt-tuning project by default.

### Evidence identifiers

Under `target/snappyref-inspect/jev-grouping-pilot`, or the selected
`CARGO_TARGET_DIR`:

- Initial script: `run-1789988221135524000`
- Initial Jev: `run-1789988335190549000`
- Pointer reachability correction: `run-1789988508123897000`
- Final Jev: `run-1789988615899680000`
- Final script: `run-1789988696000644000`
- Replay of final trial 1: `run-1789988715971455000`

Artifacts stay local and are not committed. The table and representative failure
above preserve the findings for PR readers. Run commands above regenerate fresh
evidence; API decisions are not guaranteed to repeat.

Validation: `cargo fmt --check`, `cargo clippy --locked --all-targets -- -D warnings`,
`cargo check --locked`, and `cargo test --locked`. The normal suite passed with
633 library tests, 3 updater-binary tests and 4 helper integration tests. The
manual live scenario and failing replay intentionally exit with failure when the
controller claims success incorrectly or runs out of decisions.
