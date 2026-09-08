# SnappyRef

SnappyRef is a local-first reference-board app for collecting, arranging,
annotating, and searching image references across boards.

## Run and inspect

Use shipping default features for builds and checks. On macOS, launch with
an isolated data directory:

```sh
SNAPPYREF_DATA_ROOT="$(mktemp -d)" cargo run --locked --bin snappyref
```

Keep that directory to revisit the same Library and settings. A Library-only
override uses `SNAPPYREF_LIBRARY_ROOT`; model data still follows the data root.
See [data locations and releases](docs/release-operations.md).

[The entrypoint](src/lib.rs) dispatches the GUI, image-preparation subprocess,
semantic embedding subprocess/server, and Windows update replacement mode.
[The macOS updater](src/bin/snappyref-updater.rs) is a separate binary.

```sh
cargo check --locked
python3 scripts/check-refactor-behavior.py
docs/skills/inspect/scripts/run-scenario.sh --list
docs/skills/inspect/scripts/run-scenario.sh sample-image-import
```

Use [inspect](docs/skills/inspect/SKILL.md) for app behavior and rendered
artifacts. Use [Windows checks](docs/skills/use-windows/SKILL.md) after syncing
Mac changes. Configure the local pre-push gate once with
`git config core.hooksPath .githooks`.

Run performance alone, after behavior checks. Fast is the default and needs the local
corpus and model assets described in [perf-profiling](docs/skills/perf-profiling/SKILL.md):

```sh
python3 scripts/perf_eval.py run --output perf-assets/runs/my-change
```

Use `--suite full` only when full profiling is explicitly requested.

## Follow an operation

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

The [.specs tree](.specs/) records product contracts. Shared visual rules live
in [theme](src/ui/theme.rs) and [motion](src/ui/motion.rs).
