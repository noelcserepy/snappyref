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

## Regular profiling and refactors

Fast is the default, including architecture refactors. Full runs require explicit
user demand. Do not automatically run full for releases, omitted workloads, or
inconclusive fast results. A fast pass covers only fast workloads and latency
metrics. This adds no push or merge gate.

Run [refactor behavior checks](../inspect/SKILL.md#refactor-behavior-evidence)
first, then performance alone. Concurrent builds, tests,
or other heavy work invalidate timing comparisons.

```sh
python3 scripts/perf_eval.py run --output perf-assets/runs/before
# Make the change, repeat behavior checks, then:
python3 scripts/perf_eval.py run --output perf-assets/runs/after
python3 scripts/perf_eval.py compare \
  perf-assets/runs/before/evaluation.json \
  perf-assets/runs/after/evaluation.json \
  --output perf-assets/runs/comparison.json
```

When changing the evaluator or sampler, run its tests before measurement:

```sh
python3 -m unittest discover -s scripts -p test_perf_eval.py
```

The runner builds with `cargo build --locked --release` and shipping default
features. `--binary /absolute/path` skips the build for an already-built release
binary. Never pass a debug or release-fast binary. `--models` supplies an
installed models directory, default `target/release/models`. `--assets`
supplies the corpus directory, default `perf-assets/images`; its parent must
contain `manifest.json`.

Each suite runs three repetitions per case, sequentially in recorded case order. `--repeats 1` is for
smoke checks only. Comparisons require at least three repetitions. Existing
output directories are refused, so a run cannot overwrite a baseline.

The runner currently supports macOS. Use [use-windows](../use-windows/SKILL.md)
for separate Windows behavior/build verification. Do not compare different
operating systems or machines as a refactor performance result.

## Scenarios

| Suite | Cases | What changes |
| --- | --- | --- |
| `fast`, default | `import-1`, `import-20`, `board-0`, `board-1000`, `library-1000` | Latency only, 15 fresh trials |
| `core` | `import-1`, `import-20`, `trickle-import`, `format:png`, `animated`, `animated-control`, `static-20` | Fresh library import and embedding |
| `boards` | `board-0`, `board-1`, `board-20`, `board-100`, `board-1000` | Placements of one indexed asset |
| `libraries` | `library-100`, `library-1000` | Indexed library assets, empty active board |
| `libraries` | `library-100-board-100`, `library-1000-board-1000` | Indexed assets and placements together |
| `libraries` | `library-20-backfill` | Empty board, 19 missing embeddings |
| `full`, explicit demand only | All 17 cases in `core`, `boards`, and `libraries` | Latency and resources |

When the user requests full, select `--suite full` and compare with a matching
full reference. Existing explicit workload commands remain available:

```sh
python3 scripts/perf_eval.py run import-1 trickle-import
python3 scripts/perf_eval.py run --suite boards
python3 scripts/perf_eval.py run --suite libraries
python3 scripts/perf_eval.py run format:jpg format:png format:webp
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

Fast omits trickle imports, isolated format/animation controls, intermediate
sizes, combined library/board sizes, and missing-embedding backfill. Trickle
previously caught a cached-completion hang; behavior checks do not replace that
workload. `board-0` contains one indexed library asset. `board-1000` reuses one
asset; `library-1000` has an empty active board. These fixtures measure
cardinality, not representative photo collections or search relevance.

Neither suite proves pan/zoom/drag smoothness, input or search latency, network
installation/update speed, cold-disk startup, or Windows performance.

## Measurement contract

- Fresh process and isolated library for every trial. The suite copies model
  assets to its own data directory before trial timing. Setup counts in command
  runtime. No downloads or personal
  app data. Update discovery uses a closed loopback port.
- Shipping helper grace and model retention policies. No forced unload or
  helper-disable flags. Inherited `SNAPPYREF_*` overrides are cleared.
- Nonactivating window. Avoid interacting with measured windows or changing
  display/power settings during a suite.
- OS filesystem cache is uncontrolled, usually warm. Do not call this a
  cold-disk measurement. There is one fixture-seed run for scaling, excluded from latency metrics but
  included in command runtime. It uses the selected profile completion rule.
- Sampling starts at process launch, including app construction. An app clock
  event aligns phase timestamps with the external monotonic timeline.
- A first-frame marker means the app submitted a canvas paint pass, not that
  a monitor displayed the frame. First/all-visible markers require paintable
  textures. Loading placeholders do not count.
- Settled idle means queues drained and expected placements loaded. It does
  not mean CPU is low. Resumed work clears the idle marker and summary.
- Full quiescent measurements cover 5 to 15 seconds after settled idle. CPU uses
  process CPU-time delta, not `ps %cpu`. Target under 1%. A reported zero can
  mean below the roughly 0.1 percentage point CPU-time resolution.
- Full quiescent RSS is the median main-process RSS during that window. Whole-tree
  peak sums parent and helper RSS at each sampling tick. Sampling around
  200 ms can miss short-lived helpers/peaks; RSS sums can double-count shared
  pages. Full reports both retained main RSS and sampled tree peak.
- Full sampling ends after the full window, before shutdown. It terminates only
  the trial's private process group. macOS may defer an unfocused window's
  scheduled close, so successful measurement does not require that callback.

Fast stops 500 ms after the current settled-idle marker, with a coherent summary
and no later work-resumed event. Resumed work restarts qualification. The app
already qualifies idle for 500 ms before emitting its marker. This extra guard
does not measure idle CPU/RSS or guarantee that work never resumes later. Keep
the app's 20-second hold setting and shipping retention/grace policies; terminate
the private process group externally and finish cleanup before the next trial.
Fast omits CPU, retained RSS and sampled tree peak from comparison. Early raw
resource samples are diagnostic only. The sampling cadence remains 200 ms.

Startup first-visible means at least one paintable placement, not all 1,000.
Fast retains required clocks, loading/paint counts, worker/embedding completion,
fixture cardinality and final database checks. Fast requires coherent
summary/events and replaces the full resource observation window.

A run fails on timeout, app failure, incomplete loading/painting, unfinished
eligible embeddings, changed fixture counts, missing markers, or an incomplete
profile observation window. SVG assets are explicitly excluded from semantic indexing by the
app. Their exclusion is counted, not reported as failed embedding work.

## Command runtime

The target is below 60 seconds for a prepared fast command on the baseline Mac.
The target is not a hard abort deadline or a runtime guarantee. Recorded timings,
acquisition conditions, and calibration limits live with the
[baseline evidence](../../../perf-assets/baselines/README.md).

Include fingerprinting, model copies, fixture seed/construction, every trial,
cleanup and reporting. Exclude compilation and model download/installation;
report cold setup separately. Report overruns and retained failures. Never cut
cases, repeats or completion checks to meet the target.

Before claiming runtime, measure at least 10 sequential complete prepared commands
across two idle sessions on the baseline-class Mac. Report every total, setup,
trial and cleanup duration, plus min/median/max, failures and overruns. Compare
same-binary controls against a new fast reference and investigate every false
alarm or inconclusive result. Synthetic comparison tests do not prove end-to-end
sensitivity; that needs a removable controlled delay in a measured app path and
a complete fast run. No p95 claim or new gate follows from these samples.

## Artifacts and comparisons

`evaluation.json` records source commit/dirty state, binary hash, machine,
compiler, corpus/model/evaluator fingerprints, settings, every trial, and
per-case median/min/max. Raw trial directories contain:

- `launch.json`, `app.log`: exact command/environment and app output.
- `events.ndjson`, `summary.json`: app phases and completion counts.
- `samples.csv`, `samples-tree.csv`: CPU time and main/helper RSS timelines.
- `result.json`: validity, metrics, final database counts, phases, format
  timings, helper lifetimes. Invalid runs have an error instead of metrics.
- `timing.json`: trial and process cleanup wall time. Evaluation timing also
  includes setup, seed, fixture construction, build and complete command time.
- `fixture.json`: prelaunch cardinality and embedding readiness for scaling.

Format timing joins use import queue IDs. Repeated filenames remain separate
imports. Per-format loading latency is not per-format paint latency. Worker
phase events reach the UI in batches, so receipt-to-receipt inference timing
can be near zero. Use the model's own load timing in `summary.json` for model
mount cost, and queue-to-completion for embedding drain.

Comparisons refuse invalid runs, missing cases/metrics, insufficient repeats,
or different measurement contracts. Fast requires a separately acquired matching
fast reference. Never slice the published full reference into fast data or compare
fast against full. Profile, stop rule, metric/case sets, repetitions and fixture
version belong to the recorded contract. Exit codes are 0 for pass, 1 for a budget
crossing with nonoverlapping observed ranges, and 2 for inconclusive or invalid
results. A median above budget with overlapping ranges is inconclusive, never
a green result. Investigate it or acquire another complete repeated fast run; preserve the
original result. It does not authorize an automatic full run.

Source/binary/evaluator hashes are provenance and may differ. Model, corpus,
machine, compiler, settings, and `measurement_version` must match. Bump that
version when changing timing/sampling or metric definitions. Comparison-policy
changes can reuse recorded measurements without pretending they were rerun.

Initial regression budgets require both a relative and an absolute increase:

| Metric | Relative allowance | Absolute allowance |
| --- | --- | --- |
| First frame/visible, all-visible | 20% | 100 ms |
| Time to idle | 15% | 200 ms |
| Full quiescent CPU | 50% | 0.3 percentage points |
| Full quiescent RSS | 10% | 16 MiB |
| Full tree peak RSS | 10% | 32 MiB |

These are review thresholds, not statistical significance claims. Observed
ranges are a conservative noise check, not confidence intervals. Three trials
support a median and range, not a useful p95. Inspect spread and use a separate complete repeated acquisition for noisy
results. Do not selectively retry trials or drop outliers. A pass cannot exclude
smaller regressions or rare stalls. Never update the reference baseline merely to make a regression pass.

`scripts/inventory-perf-assets.sh` lists local corpus files, formats, byte sizes, and
hashes. It is an inventory; the evaluator fingerprints the measured corpus.

The corpus is private and gitignored. `perf-assets/manifest.json` records source
and licensing metadata. Keep large assets/raw runs local; commit only compact
baseline summaries. Record measured results and limitations in the PR.
