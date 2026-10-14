# Repo guide

## What this is

SnappyRef is a local reference-board desktop app written in Rust on
eframe/egui and shipped for macOS and Windows. One `snappyref` binary plays
several roles. [src/lib.rs](src/lib.rs) dispatches on the launch arguments:

- no flag: the GUI process
- `--imageprep`: a short-lived helper subprocess that decodes and prepares one image
- `--semantic-embed-server`: a subprocess hosting the SigLIP2 model for embedding requests
- `--mcp`: an MCP server on stdio that forwards an AI client's tool calls to the GUI process
- `--windows-self-replace-runner`: Windows only. Swaps `snappyref.exe` after the
  GUI exits, in [windows_self_replace.rs](src/updater/windows_self_replace.rs)

On macOS the update swap is a separate helper binary,
[snappyref-updater](src/bin/snappyref-updater.rs), shipped inside the app
bundle next to `snappyref`.

## Build, check, test

Always use `--locked` and the shipping default features.
Leave `CARGO_TARGET_DIR` unset so each worktree builds into its own `target/`,
which goes away with the worktree. Never point it at or clone another
checkout's target. A shared target dir serves one worktree's binaries to
another and corrupts test results.

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
python3 -m unittest discover -s scripts -p 'test_*.py'
docs/skills/inspect/scripts/run-scenario.sh --list
docs/skills/inspect/scripts/run-scenario.sh <scenario>
```

The [pre-push hook](.githooks/pre-push) runs fmt, clippy, Rust tests, and Python
script tests in that order and stops at the first failure. Rust tests include
every non-ignored inspect scenario and the helper subprocess tests. Ref-deletion
pushes and branches without Cargo.toml skip the checks. Hook setup is in the
[README](README.md#development-checks).

[CI](.github/workflows/ci.yml) runs the same checks on macOS for every pull request in about 2-3 minutes.
Windows CI takes about 10 minutes, so it runs on `main` after the merge and opens an issue when it fails.
Merge without waiting for it. `gh workflow run ci.yml --ref <branch>` runs both jobs on a branch.

When the user wants to test a build by hand, stop the previous instance and run
`SNAPPYREF_DATA_ROOT="$HOME/dev/snappyref-dev-data" cargo run --locked --release --bin snappyref`
in the background with a timeout of at least two hours. The shared root keeps
their imported images and the downloaded model.

## Testing

Test behavior at the boundary users or processes cross, with concrete inputs
and literal expected outputs or effects. Never compute expectations with the
code under test. Pair absence assertions with a positive case. Reject tests
that check only `is_some`, `is_ok`, counts, or restated constants. Keep useful
relations across catalogues. Delete or merge duplicate tests and checks of
incidental implementation details. Give bug fixes a regression that fails
before the fix and passes after it.

## Module map

- [Board editing](src/active_board/editing.rs) owns accepted edits, history,
  reload scheduling, and queued persistence. [Active Board](src/active_board/lifecycle.rs)
  owns activation and session continuity.
- [Placement creation](src/placement/completion.rs) owns reservations, preparation
  completion, acceptance, and batch layout. [Image preparation](src/image_preparation/mod.rs)
  owns source decoding and helper execution.
- [Canvas](src/canvas/mod.rs) owns interaction and rendering.
  [Source adapters](src/sources/mod.rs) normalize external intake.
- [Library storage](src/library/storage/mod.rs) owns durable data.
  [Local data location](src/local_data_location.rs) resolves shared roots.
- [Semantic Search](src/search/semantic/mod.rs) owns model readiness and indexing.
  [Updater](src/updater/mod.rs) owns discovery and staging.

- [AI control](src/control/mod.rs) owns the `--mcp` role, the tool catalogue, and
  the local endpoint. [App control](src/app/control/mod.rs) runs each tool call
  on the UI thread.

Shared visual rules live in [theme](src/ui/theme.rs) and [motion](src/ui/motion.rs).

## Project language

There is no standalone glossary. The codebase is the domain language:

- use domain names in modules, types, functions, tests, and inspect scenarios
- add short local comments only where naming cannot carry the rationale
- keep legacy persistence or compatibility vocabulary inside adapter modules

There is no separate spec. The code is the product contract. The owning
module's types and module comment state it, and its tests and inspect
scenarios prove it. When a boundary or decision is unclear, inspect the repo
first, and otherwise ask one question at a time.

## Repo principles

- Optimize for speed first. Prefer changes that keep startup and core interactions fast.
- Keep interactions shortcut-driven and low-friction.
- Use sensible defaults so repeated flows happen automatically.
- Keep the code and UI simple. If a solution feels complex, look for a smaller one first.

## Don'ts

- Never run `scripts/update-smoke-macos.sh`, `scripts/update-smoke-windows.sh`,
  or the `semantic-search-real-install` scenario unless the user asks. They
  are networked, launch the GUI, and depend on the published release.
- Never launch the GUI by hand when an inspect scenario covers the behavior.
  Add a scenario if none does.
- Never commit `library/`, `models/`, downloaded model files, or perf corpora.
- Keep changes fast-path aware. Startup, import, and canvas interaction never
  wait on decoding, embedding, or persistence. That work belongs in the existing
  helpers and queues. Measure with perf-profiling when touching those paths.

## Skills and verification

- [inspect](docs/skills/inspect/SKILL.md) owns UI/app behavior evidence. Prefer
  `docs/skills/inspect/scripts/run-scenario.sh <scenario>` and its artifacts over
  manual app launches, OS mouse automation, ad hoc screenshots, or debug flags.
- [perf-profiling](docs/skills/perf-profiling/SKILL.md#regular-profiling-and-refactors)
  owns startup, import, embedding, idle CPU, and memory measurement, and the
  before/after sequence for architecture refactors.
- [use-windows](docs/skills/use-windows/SKILL.md) owns native Windows checks,
  builds, tests, and sync from the Mac checkout. Use it for platform-sensitive
  changes before GitHub Actions.
- [patchnotes](docs/skills/patchnotes/SKILL.md) owns user-facing release notes
  from the PRs merged since the last release.
- [model-packaging](docs/skills/model-packaging/SKILL.md) owns building and
  publishing Semantic Search packages and updating the app descriptor.
- [release](docs/skills/release/SKILL.md) owns app release preparation, draft
  inspection, publication and public update-channel verification.
- [Release operations](docs/release-operations.md#update-smokes) owns the
  networked update smokes and their explicit-request-only scope.

## UI work

The code is the visual vocabulary. Reuse it before building new widgets:

- start with `src/ui/theme.rs` for shared tokens, spacing, radius, type, color, and motion constants
- `src/ui/card.rs` owns shared cards and controls
- `src/ui/shortcut.rs` owns key chips and platform shortcut text; `src/app/dialogs.rs` owns the dialog
- read `src/ui/motion.rs` for shared motion semantics
- read the relevant component/module before changing a pattern
- use inspect scenarios/artifacts as runtime visual evidence
- when visual semantics change, update the owning token/component comment, API name, test, or inspect scenario in the same change
