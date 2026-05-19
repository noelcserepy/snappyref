# Inspect App Primitives Plan

## Purpose

The app source should provide the smallest dependable substrate for inspection:
given a seed and a scripted interaction, SnappyRef should produce reproducible
evidence of what happened.

The primitives are not the skill and should not encode debugging workflows. They
exist so the skill can run a simple loop:

```text
seed -> actions -> observations/assertions -> artifact
```

This shape keeps the app-side contract small. The skill can adapt its process,
scripts can provide deterministic entry points, and the app only has to expose
stable ways to construct state, drive input, observe meaning, and record
evidence.

## Non-Goals

- Do not build a standalone inspection framework before one scenario proves the
  loop.
- Do not preserve legacy `--verify-*` probes as a state catalog.
- Do not create a separate scenario language before Rust test helpers are
  insufficient.
- Do not depend on OS mouse automation for the main diagnose/fix/verify loop.
- Do not change release behavior.
- Do not create a broad target taxonomy before repeated scenarios need it.

## Primitive: Scenario Spec

The scenario spec is the smallest app-facing representation of an inspection
run. It should name the seed, the actions to perform, the observations to record,
and the assertions that decide pass/fail.

What this primitive should provide:

- Scenario name.
- Seed selection.
- Action sequence.
- Required observations.
- Assertions.
- Artifact destination.

Reasoning:

- The scenario spec is the contract that keeps scripts, app code, and skill
  instructions aligned.
- It prevents inspection from spreading into implicit setup, ambient user state,
  terminal logs, and manual screenshots.
- It gives agents one thing to rerun while they instrument, diagnose, fix, and
  verify.

Design boundary:

- The spec should describe what to run, not how the app internally implements
  the behavior.
- The first version can be ordinary Rust test/helper code. A scenario language is
  only justified if Rust helpers become noisy.
- Scenario specs should stay narrow. Shared behavior belongs in seeds, actions,
  observations, or assertions.

## Primitive: Deterministic App Construction

Inspection needs a way to instantiate SnappyRef with known state. The app must
start from a predictable Library, board, settings, viewport, theme, and window
size. Without this, every failing screenshot or event log can be contaminated by
prior user state.

What this primitive should provide:

- A temp Library root by default.
- Fixed egui viewport size and pixels-per-point.
- Fixed theme matching the app design system.
- Default settings that avoid persisted-user surprises.
- Disabled updater/network behavior for inspection runs where relevant.
- Seed application before the first interaction frame.

Initial seed vocabulary:

- Empty board.
- Board with one loaded placement.
- Board with multiple loaded placements.
- Search results from generated local images.
- Import/drop fixture image.

Reasoning:

- Determinism is the first requirement for agent reproduction.
- App state must be easier to seed than to manually create through UI.
- Seeds are the deterministic equivalent of a user preparing the app.
- Generated images avoid external fixture and licensing churn.
- Repro artifacts should be explainable from the scenario seed, not from
  whatever the local app remembered.

Design boundary:

- Construction should take a seed; it should not know about every scenario.
- Seeds should describe domain state, not UI steps.
- If a seed requires complex setup, hide that setup behind the seed helper rather
  than inside a scenario body.

## Primitive: Harness

The harness owns app lifetime and frame stepping. It should wrap the existing
`egui_kittest` harness and stay close to the current test harness until a
separate crate is clearly earned.

What this primitive should provide:

- Start an app from deterministic construction.
- Step egui one input event at a time.
- Run until repaint settles where appropriate.
- Expose the current frame number or step number.
- Expose access to observations and screenshots after each step.

Reasoning:

- The session failure came from OS-level event delivery, coordinate translation,
  and uncertainty about the active process. In-process egui driving removes that
  class of failure.
- Held drags need frame-by-frame stepping. The harness should make frame timing
  explicit instead of relying on batched input.
- Keeping app lifetime separate from gesture policy makes interaction helpers
  easier to change without touching setup or artifacts.

Design boundary:

- The harness should not decide which gestures are meaningful.
- The harness should not own artifact schema.
- The harness should provide mechanics; actions and assertions provide intent.

## Primitive: Actions

Actions are the inspection vocabulary for user behavior. They should describe
what a user does in app terms while still being explicit enough to drive egui
frame by frame.

First action vocabulary:

- Resolve source search result.
- Resolve canvas drop target.
- Press pointer at source.
- Move pointer through drag steps.
- Hold drag for an intermediate frame.
- Release pointer on canvas.

Later action vocabulary can add clicks, text input, paste, keyboard shortcuts,
and file drops when scenarios need them.

Reasoning:

- The first failure to solve is search-result drag to canvas. A small action set
  proves whether the loop can reproduce the behavior that OS automation failed
  to drive reliably.
- Actions should make the correct path boring without hiding the frame-sensitive
  details of drag behavior.
- Delaying the full gesture vocabulary avoids a shallow wrapper around
  `egui_kittest`.

Design boundary:

- Actions should resolve semantic targets to points at execution time.
- Point-based actions may exist, but target-based actions should be the default.
- Every action should be recordable as structured evidence.

## Primitive: Observations

Observations are app-owned facts captured during a scenario. They replace the
separate ideas of a semantic target registry and probe log with one model:
targets and facts.

Observation kinds:

- `target`: a stable app name, target kind, egui rect, metadata, and frame.
- `fact`: a named structured value emitted for diagnosis or assertion.

Initial target observations:

- `snap.canvas`
- `snap.search.result[n]`
- `snap.canvas.placement[id]`
- `snap.canvas.selection`

Initial fact observations for search-result drag:

- Pointer position per drag step.
- Search result source rect.
- Canvas rect.
- Provisional or selected placement bounds.
- Group center vs pointer delta.
- Drop/commit result.

Reasoning:

- Agents need semantic targets, not guessed pixels.
- Custom canvas content may not naturally appear as ordinary accessible widgets.
  App-owned observations provide project-specific truth without scraping pixels.
- Search result drag bugs are not only about pixels. The useful evidence is what
  the app thought was happening, what geometry it used, and whether committed
  state matches the intended interaction.
- One observation model prevents agents from deciding between a registry, debug
  flag, terminal log, screenshot note, or ad hoc `println!`.

Design boundary:

- Observations should be compile-gated or debug/verify-only where possible.
- Observation emission should not change interaction behavior.
- Temporary observations should be removed unless they become generally useful.
- Broadly useful observations should use stable names and documented fields.
- Do not add future target namespaces until a scenario needs them.

## Primitive: Assertions

Assertions decide whether the scenario reproduced the behavior and whether a fix
worked. They should use app state and observations as the source of truth, with
screenshots as supporting evidence.

Initial assertions for search-result drag:

- The search result is available before drag starts.
- The source and canvas targets resolve to valid rects.
- Mid-drag provisional or selected geometry exists.
- Mid-drag group center tracks the intended pointer within a small tolerance.
- Release commits a new board placement.
- The committed placement is inside the intended canvas region.

Reasoning:

- Screenshots help humans review failures, but assertions should decide pass/fail
  from structured state.
- Assertions keep instrumentation honest: if an observation does not support a
  diagnosis or assertion, it is probably noise.
- A failed assertion gives the agent a stable repro target while it makes
  temporary changes and reruns the same scenario.

Design boundary:

- Assertions should be scenario-owned, not global app behavior.
- Keep tolerances explicit and local to the geometry they validate.
- Avoid snapshot baselines until structured assertions stop being enough.

## Primitive: Recorder

The recorder writes durable evidence from a scenario run. It is the bridge
between deterministic app execution and agent reasoning.

Initial artifact files:

- `report.json`
- `events.ndjson`
- optional `frames/*.png`

`report.json` should answer:

- What scenario ran?
- Did it pass or fail?
- Where are the artifacts?
- What was the first failing assertion?
- Which frames are relevant?

`events.ndjson` should answer:

- What seed was used?
- What actions were sent?
- Which targets were resolved?
- What facts were observed?
- Which assertions passed or failed?
- Which screenshots were captured?

Reasoning:

- The user does not see raw tool output.
- Agents need to summarize evidence, not rerun repeatedly because they forgot
  what happened.
- A standard artifact shape lets scripts summarize failures reliably.
- Keeping recording separate from actions and harness mechanics prevents the
  inspection layer from becoming a debug logging surface.

Design boundary:

- Start with a small schema. Expand only when a scenario needs a new fact.
- Prefer structured fields over free-form logs.
- Screenshots support human review; assertions and observations decide pass/fail.
- Avoid terminal log spam as the primary evidence path.

## First Scenario To Prove The Layer

The first scenario should be search result drag to canvas.

Why this scenario:

- It directly matches the session failure.
- It exercises deterministic seeding, search overlay, canvas target resolution,
  frame-sensitive pointer drag, provisional placement behavior, final commit,
  observations, assertions, and artifact writing.
- It proves the harness handles mid-drag state, not just final app state.

Required evidence:

- Before state summary showing search result available.
- Source target and canvas target observations.
- Mid-drag evidence showing provisional placement visible/aligned.
- After-release evidence showing committed placement.
- Assertion that the board contains the new placement.
- Assertion that mid-drag selected/provisional group center tracks the intended
  pointer within a small tolerance.

## Defer Until The First Scenario Works

- Full snapshot baseline strategy.
- Recreating every old visual probe as an inspect scenario.
- Proc macros.
- Separate harness crate.
- Full tracing integration.
- Broad gesture vocabulary.
- Broad semantic target namespace.
- Windows smoke changes beyond existing workflows.

These may be useful later, but they are not needed to prove the inspection loop.
