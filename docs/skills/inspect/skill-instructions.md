# Inspect Skill Instructions Plan

## Purpose

The inspect skill is the adaptive part of the system. It should route agents
through one reliable loop:

```text
classify -> run deterministic command -> summarize artifact -> decide next step
```

The app primitives provide `seed -> actions -> observations/assertions ->
artifact`. The scripts provide stable commands. The skill instructions tell the
agent when to use those commands, how to read the evidence, and when to
instrument, fix, or fall back.

The skill should not try to make every debugging decision deterministic. Its job
is to keep the repetitive and risky parts fixed so the agent can spend judgment
on the bug.

## Main Skill Shape

`docs/skills/inspect/SKILL.md` should be short, operational, and true to the
tools that exist.

Initial layout:

```text
docs/skills/inspect/
  SKILL.md
  plan.md
  primitives.md
  scripts.md
  skill-instructions.md
  scripts/
```

References can be added later when a procedure becomes too large for `SKILL.md`.
Do not create reference files before repeated use proves the extra split is
useful.

Reasoning:

- The main skill must fit in working memory during a task.
- A skill that names unavailable tools trains agents into dead ends.
- Planning docs can describe the target shape; `SKILL.md` should describe the
  current usable program.

## Trigger Scope

Use inspect when the agent needs to:

- Reproduce or verify UI behavior.
- Verify drag, click, keyboard, paste, hover, or drop behavior.
- Inspect app UI state with artifacts or screenshots.
- Add temporary UI instrumentation.
- Compare before/after UI behavior after a fix.

Do not use inspect as the default for:

- Pure domain logic tests.
- Storage-only changes.
- Release packaging checks.
- Performance profiling, unless the issue is specifically visual or
  interaction-related.
- Windows-native checks that require a real Windows machine; route those through
  `use-windows`.

Reasoning:

- Inspect should reduce ambiguity, not claim every task.
- Perf and Windows already have local skills; inspect should cooperate with them
  rather than replace them.

## Route Selection

Start by choosing one route.

1. Interactive scenario.
   - Use for drag, click, keyboard, paste, hover, drop, and multi-frame egui
     behavior.
   - This is the default route for interaction bugs.
2. Static visual check.
   - Use for seeded UI states that do not require held input or multi-frame
     behavior.
   - Reproduce the state through an inspect scenario rather than a legacy app
     flag.
3. Manual or platform fallback.
   - Use only when the harness cannot represent the behavior yet or platform
     fidelity is the subject of the test.
   - Examples: real OS file drag/drop, installed app launch, updater behavior,
     Windows-native behavior.

If the task is pure logic, skip inspect and write a normal unit/integration test.

Reasoning:

- Choosing the wrong route caused much of the prior session's noise.
- Static screenshots cannot prove a held drag.
- OS automation should not be the default when egui can be driven in-process.

## Interactive Scenario Loop

Use this loop for interactive UI bugs:

1. Run `scripts/check-harness.sh`.
2. Run `scripts/run-scenario.sh <scenario>`.
3. Run `scripts/summarize-artifact.py <artifact-dir>` using the printed
   artifact path.
4. Confirm the behavior reproduced before fixing.
5. If the artifact is insufficient, add the narrowest observation or assertion
   and rerun the same scenario.
6. Add the narrowest product fix.
7. Rerun the same scenario.
8. Compare failing and passing artifact summaries.
9. Report the evidence.

If no scenario exists, create one focused scenario before fixing. The scenario
should use the smallest user-like interaction that reproduces the behavior.

Reasoning:

- Reproduce before fix is non-negotiable for hard UI bugs.
- The same scenario should fail before the fix and pass after the fix.
- Adding observations to the artifact is better than adding terminal log spam.
- The fix should be validated against the same behavior, not a nearby manual
  impression.

## Diagnosing From Artifacts

Read artifacts before reading product code when the failure shape is not yet
clear.

Inspect in this order:

1. `report.json`
2. `events.ndjson`
3. relevant `frames/*.png`
4. product source code

When a scenario fails, identify:

- First failing assertion.
- Last successful action.
- Relevant target rects.
- Relevant observation facts near the failure frame.
- Before/mid/final frame paths.

If this evidence is insufficient:

- Add or extend one structured observation.
- Add semantic target metadata if the scenario lacks a necessary app fact.
- Add or tighten one assertion.
- Rerun the same scenario.

Avoid:

- `println!` or terminal log spam.
- Long-lived one-off flags.
- Feature-specific temp files unless the recorder cannot capture the fact yet.
- Instrumentation spread across unrelated modules.

Reasoning:

- The artifact should be the shared evidence between user and agent.
- Structured observations let agents and scripts summarize the issue
  consistently.
- Instrumentation should answer a specific diagnosis or assertion question.

## Scenario Writing Rules

Scenarios should be small and behavior-focused.

A good scenario:

- Starts from deterministic seed state.
- Performs the smallest user-like interaction that reproduces the behavior.
- Captures evidence at meaningful frames.
- Asserts on app state and observations.
- Has a name that describes user behavior, not an implementation detail.

A bad scenario:

- Encodes a large end-to-end workflow when a smaller interaction would fail.
- Depends on physical pixels or OS window placement.
- Sleeps instead of stepping egui frames.
- Asserts only final state when the bug is mid-interaction.
- Requires manual setup.

Reasoning:

- Small scenarios are easier to trust and maintain.
- Mid-interaction bugs need mid-interaction evidence.
- User-behavior names keep the skill tied to product outcomes, not internal
  speculation.

## Static Visual Checks

Static visual checks are allowed for seeded UI states that do not require
multi-frame interaction.

Reproduce static states with inspect scenarios. Seed prerequisite state directly,
capture a full-app screenshot, and assert the relevant app facts. When a state
requires held input, drag, hover timing, or keyboard sequencing, use the same
scenario harness with explicit `act` and `wait` steps.

Reasoning:

- Static states and interaction states should use one artifact shape.
- Startup flags are a weak place to store visual state.
- The harness should make state reproduction cheap enough that probes are not
  needed.

## Manual Or Platform Fallback

Manual checks are allowed only when the harness cannot represent the behavior
yet or platform fidelity is the subject of the test.

Rules:

- Launch the repo-built binary, not the installed release.
- Use a disposable Library root where possible.
- Resize near full screen, but do not enter macOS fullscreen.
- Keep driven coordinates inside the app window.
- Close the app after testing.
- Treat manual evidence as supplementary if a harness scenario is feasible.

Reasoning:

- The previous failure involved accidentally interacting with the wrong app
  instance and clicking outside the window.
- Fullscreen Spaces introduced unreliable event delivery.
- Manual testing is sometimes necessary, but it should not become the default
  proof.

## Final Reporting Style

After an inspection task, report:

- Scenario or route used.
- Whether the behavior reproduced.
- Artifact path or key screenshot paths.
- What changed, if code was edited.
- Which verification passed.
- Any remaining gap, especially if manual/platform testing was required.

Keep the report concise. The user needs decision-relevant evidence, not the
entire artifact log.

## First Version Of `SKILL.md`

The first `SKILL.md` should route to only the first usable capabilities:

- Choose route.
- Check harness.
- Run one existing scenario.
- Summarize the returned artifact path.
- Add the narrowest observation/assertion if the artifact is insufficient.
- Use inspect scenarios for static states.
- Use manual/platform testing only as fallback.

Do not describe future scripts, broad target vocabularies, generated scenario
templates, or probe wrappers as if they exist.

Reasoning:

- Skills must be true.
- The first version should support the smallest reliable diagnose/fix/verify
  loop.
- Add instructions as each deterministic tool lands.
