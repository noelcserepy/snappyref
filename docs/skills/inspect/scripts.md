# Inspect Scripts Plan

## Purpose

Scripts are the deterministic edge of the inspect skill. They should give agents
only the fixed commands needed to run the first loop:

```text
run scenario -> summarize artifact -> decide next step
```

The app primitives own `seed -> actions -> observations/assertions -> artifact`.
The scripts should not duplicate that model. They only hide cargo incantations,
artifact path conventions, and artifact parsing.

The important constraint is restraint. Until the first interactive scenario
works end to end, more scripts mostly create surface area the agent has to
understand.

## Script Design Rules

- Scripts should do one job.
- Scripts should be predictable under automation.
- Scripts should print stable, parseable output.
- Scripts should return non-zero on failure.
- Scripts should avoid destructive repo operations.
- Scripts should not hide workflow decisions.
- Scripts should compose through explicit artifact paths.

Prefer paths and structured output over prose. Human-readable messages are fine,
but the important result should be easy for an agent to extract.

## Initial Directory Shape

```text
docs/skills/inspect/
  scripts/
    run-scenario.sh
    latest-artifact.sh
    summarize-artifact.py
    check-harness.sh
```

Scripts should be skill-local so agents can discover them after reading
`SKILL.md`. They should assume they are run from anywhere and resolve the repo
root internally.

## Script: `run-scenario.sh <scenario>`

Purpose:

- Run one inspection scenario and print where its artifacts live.

What it should do:

- Accept one scenario name.
- Run the matching Rust test or scenario command.
- Own the cargo filter, feature set, env vars, and artifact root convention.
- Ensure artifacts are written under a stable root.
- Always print `scenario=...`.
- Always print `artifact=...` when any run directory exists, including failure.
- Print `status=pass` or `status=fail`.
- Preserve the cargo/test exit code.

Reasoning:

- This is the main deterministic handle for the skill.
- Agents should not need to remember how inspection scenarios map to Cargo.
- The normal workflow should be linear: run the scenario, then summarize the
  artifact path returned by the run.
- Failure artifacts are often more valuable than passing artifacts, so the path
  must be visible even when the command exits non-zero.

Expected output shape:

```text
status=fail
scenario=search-drag-to-canvas
artifact=/absolute/path/to/target/snappyref-inspect/search-drag-to-canvas/latest
```

Design boundary:

- This script may know about Cargo, feature flags, env vars, and artifact root
  layout.
- It should not parse artifact contents.
- It should not decide how to debug a failure.
- It should not run cleanup or kill app processes.

## Script: `summarize-artifact.py <artifact-dir>`

Purpose:

- Convert one artifact directory into a concise diagnostic summary.

What it should do:

- Read `report.json`.
- Read `events.ndjson` when present.
- Print scenario, status, first failure, assertion results, key observation
  facts, and relevant frame paths.
- Return non-zero if the artifact directory is malformed.

Reasoning:

- Agents need to turn structured artifacts into a short explanation without
  reimplementing parsing in every conversation.
- This is the only script that should understand artifact contents.
- Listing frame paths here removes the need for a separate `open-frames` script
  in the first version.

Expected summary content:

- Scenario name.
- Pass/fail.
- First failing assertion or error.
- Assertion list.
- Key observation deltas.
- Before/mid/final frame paths if present.

Design boundary:

- This script should not run scenarios.
- This script should not decide whether to fix code, add instrumentation, or
  create another scenario.
- A future `--json` flag is acceptable if the text summary becomes hard to parse.

## Script: `latest-artifact.sh [scenario]`

Purpose:

- Recover the most recent artifact directory when the caller does not already
  have a path.

What it should do:

- With a scenario: print that scenario's latest artifact.
- Without a scenario: print the latest artifact across all scenarios.
- Return non-zero if no artifact exists.

Reasoning:

- `run-scenario.sh` should print the artifact path during normal workflow.
- This script exists for recovery, manual inspection, and interrupted sessions.
- Keeping lookup separate prevents `summarize-artifact.py` from guessing paths.

Expected output:

```text
/absolute/path/to/target/snappyref-inspect/search-drag-to-canvas/latest
```

Design boundary:

- This script should only locate artifacts.
- It should not parse, summarize, delete, or open artifacts.

## Script: `check-harness.sh`

Purpose:

- Confirm the inspection harness works before debugging a real product behavior.

What it should do:

- Call `run-scenario.sh check-harness`.
- Preserve its output shape and exit code.

Reasoning:

- This is fixed-scenario sugar, not a second execution path.
- When a repro fails, the first question can be whether the inspection tool
  itself still works.
- Routing through `run-scenario.sh` keeps harness health and product scenarios
  on the same command path.

Design boundary:

- It should not duplicate cargo invocation logic.
- It should not inspect artifacts beyond what the `check-harness` scenario
  itself asserts.

## Output Conventions

Shell scripts should print simple `key=value` lines:

```text
status=fail
scenario=search-drag-to-canvas
artifact=/absolute/path
```

Reasoning:

- `key=value` is easy for agents and shell tools to parse.
- It avoids scraping decorative output.
- The script surface stays stable even if the underlying Cargo command changes.

## Deferred Scripts

Do not implement these until the first scenario loop is working and repeated use
shows they are needed.

Possible later scripts:

- `cleanup-artifacts.sh`: remove old verify artifacts only. Do not kill app
  processes or touch user Library data.
- `new-scenario.sh`: create scenario stubs only after the scenario shape is
  stable enough to template.
- `open-frames.sh`: likely unnecessary if `summarize-artifact.py` lists frame
  paths.

Deferred larger ideas:

- A full cargo subcommand.
- Complex argument parsers.
- Scenario generators that edit many files.
- Snapshot baseline management.
- Process cleanup that kills repo-built app instances.
- Platform smoke orchestration.

These may be useful later, but the first goal is a reliable, small
diagnose/fix/verify loop.
