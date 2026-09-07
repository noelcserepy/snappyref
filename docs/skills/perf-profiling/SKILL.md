---
name: perf-profiling
description: |
  Measure SnappyRef startup, import, embedding, idle CPU, and memory using the optimized release app. Run repeated baselines and compare them before and after refactors.
user-invocable: true
argument-hint: "[scenario-or-regression]"
---

# Performance evaluation

Use the real release app for performance. Use [inspect](../inspect/SKILL.md)
for behavior and screenshots. Inspect timings are not app performance evidence.

## Before a refactor

Run behavior checks first, then performance alone. Concurrent builds, tests,
or other heavy work invalidate timing comparisons.

```sh
python3 scripts/check-refactor-behavior.py
python3 scripts/_perf_sampling.py --self-test
python3 -m unittest discover -s scripts -p test_perf_eval.py
python3 scripts/perf_eval.py run --suite full --output perf-assets/runs/before
# Make the change, repeat behavior checks, then:
python3 scripts/perf_eval.py run --suite full --output perf-assets/runs/after
python3 scripts/perf_eval.py compare \
  perf-assets/runs/before/evaluation.json \
  perf-assets/runs/after/evaluation.json \
  --output perf-assets/runs/comparison.json
```

The runner builds with `cargo build --locked --release` and shipping default
features. `--binary /absolute/path` skips the build for an already-built release
binary. Never pass a debug or release-fast binary. `--models` supplies an
installed models directory, default `target/release/models`. `--assets`
supplies the corpus directory, default `perf-assets/images`; its parent must
contain `manifest.json`.

Each suite runs three repetitions per case, sequentially. `--repeats 1` is for
smoke checks only. Comparisons require at least three repetitions. Existing
output directories are refused, so a run cannot overwrite a baseline.

The runner currently supports macOS. Use [use-windows](../use-windows/SKILL.md)
for separate Windows behavior/build verification. Do not compare different
operating systems or machines as a refactor performance result.

## Scenarios

| Suite | Cases | What changes |
| --- | --- | --- |
| `core` | `import-1`, `import-20`, `trickle-import`, `format:png`, `animated`, `animated-control`, `static-20` | Fresh library import and embedding |
| `boards` | `board-0`, `board-1`, `board-20`, `board-100`, `board-1000` | Placements of one indexed asset |
| `libraries` | `library-100`, `library-1000` | Indexed library assets, empty active board |
| `libraries` | `library-100-board-100`, `library-1000-board-1000` | Indexed assets and placements together |
| `libraries` | `library-20-backfill` | Empty board, 19 missing embeddings |
| `full` | All cases above | Complete baseline |

Run a subset with explicit names:

```sh
scripts/run-perf-scenario.sh import-1 trickle-import
scripts/run-idle-board-size.sh
scripts/run-existing-library-baseline.sh
scripts/run-perf-format-suite.sh jpg png webp
```

`import-1` uses the medium JPEG. `import-20` uses the mixed 20-file corpus.
`trickle-import` queues the same file three times, two seconds apart, to check
cached embedding completion and return to idle. Deduplication means the later
placements reuse an asset; this case does not prove repeated model inference. `static-20` cycles nonanimated corpus files.
`animated-control` uses the manifest's static controls. Animation follows
shipping interaction rules; a quiet, unselected animated image does not have
to advance. Inspect `last_canvas_frame.active_animated_images` in the raw
summary when investigating animation CPU.

Imports measure scripted queue-to-paint and embedding work. They do not time
OS drag delivery or the entire user drop gesture. Inspect covers batch-drop
behavior separately.

Scaling fixtures use the schema created by a real one-image import. They
clear its undo history, use current placement geometry/content fields, and
store originals inside each trial's library. Distinct trailing JPEG bytes
produce distinct content hashes while retaining identical pixels. Indexed
fixtures reuse the seed vector. This measures library and board cardinality,
not search relevance. Backfill is a separate workload.

## Measurement contract

- Fresh process and isolated library for every trial. The suite copies model
  assets to its own data directory before timing. No downloads or personal
  app data. Update discovery uses a closed loopback port.
- Shipping helper grace and model retention policies. No forced unload or
  helper-disable flags. Inherited `SNAPPYREF_*` overrides are cleared.
- Nonactivating window. Avoid interacting with measured windows or changing
  display/power settings during a suite.
- OS filesystem cache is uncontrolled, usually warm. Do not call this a
  cold-disk measurement. There is one untimed fixture-seed run for scaling.
- Sampling starts at process launch, including app construction. An app clock
  event aligns phase timestamps with the external monotonic timeline.
- A first-frame marker means the app submitted a canvas paint pass, not that
  a monitor displayed the frame. First/all-visible markers require paintable
  textures. Loading placeholders do not count.
- Settled idle means queues drained and expected placements loaded. It does
  not mean CPU is low. Resumed work clears the idle marker and summary.
- Quiescent measurements cover 5 to 15 seconds after settled idle. CPU uses
  process CPU-time delta, not `ps %cpu`. Target under 1%. A reported zero can
  mean below the roughly 0.1 percentage point CPU-time resolution.
- Quiescent RSS is the median main-process RSS during that window. Whole-tree
  peak sums parent and helper RSS at each sampling tick. Sampling around
  200 ms can miss short-lived helpers/peaks; RSS sums can double-count shared
  pages. Always report both retained main RSS and sampled tree peak.
- The sampler ends after the full window, before shutdown. It terminates only
  the trial's private process group. macOS may defer an unfocused window's
  scheduled close, so successful measurement does not require that callback.

A run fails on timeout, app failure, incomplete loading/painting, unfinished
eligible embeddings, changed fixture counts, missing markers, or an incomplete
idle window. SVG assets are explicitly excluded from semantic indexing by the
app. Their exclusion is counted, not reported as failed embedding work.

## Artifacts and comparisons

`evaluation.json` records source commit/dirty state, binary hash, machine,
compiler, corpus/model/evaluator fingerprints, settings, every trial, and
per-case median/min/max. Raw trial directories contain:

- `launch.json`, `app.log`: exact command/environment and app output.
- `events.ndjson`, `summary.json`: app phases and completion counts.
- `samples.csv`, `samples-tree.csv`: CPU time and main/helper RSS timelines.
- `result.json`: validity, metrics, final database counts, phases, format
  timings, helper lifetimes. Invalid runs have an error instead of metrics.
- `fixture.json`: prelaunch cardinality and embedding readiness for scaling.

Format timing joins use import queue IDs. Repeated filenames remain separate
imports. Per-format loading latency is not per-format paint latency. Worker
phase events reach the UI in batches, so receipt-to-receipt inference timing
can be near zero. Use the model's own load timing in `summary.json` for model
mount cost, and queue-to-completion for embedding drain.

Comparisons refuse invalid runs, missing cases/metrics, insufficient repeats,
or different measurement contracts. Exit codes are 0 for pass, 1 for detected
regression, and 2 for invalid/incompatible evaluation. Source/binary hashes
may differ; model/corpus/evaluator/machine/settings must match.

Initial regression budgets require both a relative and an absolute increase:

| Metric | Relative allowance | Absolute allowance |
| --- | --- | --- |
| First frame/visible, all-visible | 20% | 100 ms |
| Time to idle | 15% | 200 ms |
| Quiescent CPU | 50% | 0.3 percentage points |
| Quiescent RSS | 10% | 16 MiB |
| Tree peak RSS | 10% | 32 MiB |

These are review thresholds, not statistical significance claims. Three trials
support a median and range, not a useful p95. Inspect spread and rerun suspicious
cases. Never update the reference baseline merely to make a regression pass.

The corpus is private and gitignored. `perf-assets/manifest.json` records source
and licensing metadata. Keep large assets/raw runs local; commit only compact
baseline summaries. Record measured results and limitations in the PR.
