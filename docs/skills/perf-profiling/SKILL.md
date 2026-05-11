---
name: perf-profiling
description: |
  Profile SnappyRef startup, import, embedding, idle CPU, and memory performance with the real app on the local machine. Use when asked to measure perf, improve idle CPU/RSS, diagnose startup/import/embedding speed, compare image formats, or update perf baselines.
user-invocable: true
argument-hint: "[scenario-or-regression]"
---

# Perf Profiling

Use this skill when improving SnappyRef performance. Treat perf work as a measurement loop: define the state, run the real app, isolate the phase, change one thing, and update docs when the result changes how future agents should reason.

## Principles

- Measure the real app on the local machine before optimizing.
- Keep work state separate from outcome metrics.
- Use **Settled Idle** for queue drain state, not for CPU success.
- Use **Quiescent Idle** and **Quiescent Memory** for steady-state targets.
- Optimize idle CPU and quiescent RSS before optimizing **Time to Idle**.
- Preserve phase breakdowns so a faster headline number does not hide a worse import, embedding, backfill, or memory profile.
- Prefer helper-process isolation when native libraries or large allocators do not return memory to the OS after work drains.
- Perf scenario launches are non-activating by default so agent-run measurements do not steal focus. This keeps the app visible and running normally; avoid hiding/minimizing the app during measurements unless that is the condition being tested.

## Baseline Scenarios

Run the established scenarios before and after substantial changes:

```sh
scripts/run-perf-scenario.sh import-1
scripts/run-perf-scenario.sh import-20
scripts/run-perf-scenario.sh trickle-import
scripts/run-perf-scenario.sh format:png
scripts/run-perf-scenario.sh animated
scripts/run-perf-scenario.sh animated-control
scripts/run-perf-scenario.sh static-20
scripts/run-idle-board-size.sh
scripts/run-existing-library-baseline.sh
```

Scenario meanings:

- `import-1`: import one medium JPEG from the perf corpus.
- `import-20`: import the full mixed 20-file perf corpus.
- `trickle-import`: import the same medium JPEG three times with a 2s gap between each. Reproduces **Embedding Helper Process** churn — pre-fix every import paid a fresh model cold-mount; post-fix the **Embedding Helper Grace Period** keeps one helper alive across all three.
- `format:<format>`: import all corpus files for one format; singleton formats become one-image cases.
- `animated`: import animated corpus assets.
- `animated-control`: import the same number of small static control assets.
- `static`: import all non-animated corpus assets.
- `static-20`: import 20 non-animated assets, cycling through the static corpus to separate count/placement load from animation cost.
- `run-idle-board-size.sh`: compare idle CPU with `0`, `1`, `20`, `100`, and `1000` placements of the **same library asset**. Isolates **Board Size** scaling.
- `run-existing-library-baseline.sh`: launch the app cold against libraries that already contain `100`/`1000` library **Assets** (and either `0` or matching `100`/`1000` **Placements** on the active board). Isolates startup, **Active Board Load**, **Quiescent Memory**, and queued **Embedding Backfill** cost from empty-library import cost.

### Distinguishing the scaling axes

- `run-idle-board-size.sh` keeps **Library Size** fixed (1 asset) and varies **Board Size**. Idle CPU should stay flat across sizes.
- `run-existing-library-baseline.sh` cases `library-100` and `library-1000` keep **Board Size** at 0 placements and vary **Library Size**. Differences here come from library load, search index residency, and pending **Embedding Backfill**, not from the canvas.
- Cases `library-100-board-100` and `library-1000-board-1000` move both axes together. Compare them with the two isolated runs above to attribute regressions to library, board, or their interaction.

## Headline Metrics

Report these first:

- **Time to Idle**: scenario start until **Settled Idle**.
- **Quiescent CPU**: process CPU-time delta during the post-tail quiescent window. Target: under 1%.
- **Quiescent Memory**: RSS during the same quiescent window.
- **Time to Quiescent**: elapsed time from **Settled Idle** until CPU stays below the idle target for the sustained window.

Do not call the app "idle" because CPU is low. The state is idle when app work queues have drained; low CPU is the success criterion measured afterward.

## Phase Metrics

Use app events to break **Time to Idle** into phases:

- import queued
- first placement visible
- all placements visible
- embedding model load started
- embedding model load timing
- embedding model ready
- first import embedding started
- first import embedding done
- all import embeddings done
- embedding model unload requested
- embedding model unloaded
- import settled
- embedding backfill done
- settled idle
- idle tail
- quiescent idle

Each asset event should preserve format, byte size, dimensions, source role, and asset id where available so mixed-corpus runs can be grouped by image format.

## Output Files

Perf runs write to `perf-assets/runs/<timestamp>-<scenario>/`.

Use:

- `events.ndjson`: app timing events.
- `samples.csv`: external CPU/RSS samples for the **main process only**.
- `samples-tree.csv`: external CPU/RSS samples for the main process **and**
  every descendant process (the **Embedding Helper Process**, the future
  **Image Preparation Helper Process**, and anything else under the main
  pid). One row per pid per sampling tick.
- `summary.json`: scenario summary and phase counts.
- `samples-summary.json`: CPU/RSS aggregates split into all samples, idle
  samples, idle tail, quiescent idle, and post-idle-without-shutdown
  windows. Includes a `process_tree` section with whole-tree peak RSS and
  per-helper-role peaks (see "Child-process peak vs quiescent main RSS"
  below).
- `phase-summary.json`: before/after-friendly headline summary derived
  from `events.ndjson`. Contains phase markers (`scenario_started`,
  `first_placement_visible`, `semantic_model_load_started`,
  `semantic_model_ready`, `first_import_embedding_started`,
  `first_import_embedding_done`, `all_import_embeddings_done`,
  `semantic_model_unload_requested`, `semantic_model_unloaded`,
  `settled_idle`) plus precomputed durations (queue→first-placement,
  model-load, first-embedding, embedding-drain, model-unload,
  all-placements→settled-idle). Also includes per-format groups so a
  single artifact carries the per-format breakdown previously only
  available via `summarize-perf-by-format.py`.

`run-existing-library-baseline.sh` adds per-case files under
`perf-assets/runs/<timestamp>-existing-library-baseline/case-<name>/`:

- `idle-samples.csv`: ps cpu/rss timeline for the case.
- `summary.json`: case summary including library/board sizes,
  `rss_settle_elapsed_ms` (coarse settling marker), `time_to_quiescent_ms`,
  per-window CPU/RSS aggregates, and `background_state` (queued
  **Embedding Backfill** signals).
- `samples-summary.json`: per-window CPU/RSS aggregates only.
- `background-state.json`: queued background work snapshot taken after
  the sample window. The presence of pending embeddings is exposed as
  `queued_background_work_present` in the case summary.
- `existing-library-overall.json` at the run root: all cases rolled up
  for cross-case comparison.

Important windows:

- `idle_tail_0_to_3s`: first three seconds after **Settled Idle**.
- `quiescent_5_to_15s`: steady post-tail idle window.
- `post_idle_0_to_15s_shutdown_excluded`: post-idle window excluding shutdown samples.

### Child-process peak vs quiescent main RSS

Treat the main-process **Quiescent Memory** and the whole-process-tree
peak as two distinct numbers, not as substitutes for each other:

- `samples-summary.json` → `memory.quiescent_5_to_15s.max_rss_bytes`
  is **Quiescent Memory** for the main app process. This is the steady
  retained-memory target. Helpers have exited by this window, so this
  number measures what the main app is still holding onto after import,
  embedding, and backfill have drained.
- `samples-summary.json` → `process_tree.active_work.peak_tree_rss_bytes`
  is the whole-process-tree peak during active work. This is the
  closest thing to "how much RAM did the import flow actually use",
  because it sums the main app and every helper at the worst tick. A
  faster main-process pipeline that simply shifts allocations into a
  helper would make **Quiescent Memory** look better while making this
  number worse.
- `samples-summary.json` → `process_tree.per_helper_active` and
  `per_helper_overall` break the peak down by helper role
  (`main`, `embedding_helper`, `other_helper`). Use these to attribute
  a tree-peak regression to a specific helper.

When reporting perf changes, quote both **Quiescent Memory** and the
process-tree peak (active-work window) so a helper-side regression
cannot hide behind a main-process improvement.

## Corpus

Perf corpus files live under `perf-assets/images/`, which is private and gitignored. `perf-assets/manifest.json` records filename, format, source URL, license/status, and role.

The representative 20-file corpus is:

- 5 JPEG
- 5 PNG
- 4 WebP
- 1 GIF
- 1 SVG
- 1 TIFF
- 1 BMP
- 1 AVIF
- 1 JPEG 2000 / JP2

Use stable, permissive, low-risk sources such as Wikimedia Commons, public-domain or Creative Commons media, and official format sample repositories. Avoid blind scraping and sketchy downloads.

## Diagnosis Loop

1. Reproduce the perf issue with the closest baseline scenario.
2. Identify which window regressed: startup, import, embedding, idle tail, quiescent idle, or quiescent memory.
3. Form ranked hypotheses with predictions.
4. Add the narrowest instrumentation that distinguishes those hypotheses.
5. Change one variable.
6. Re-run the same scenario and compare the same windows.
7. Remove throwaway probes.
8. Update `CONTEXT.md`, `docs/adr/`, or this skill when the result changes durable project language or perf process.

## Known Patterns

- If quiescent RSS stays high but live Rust allocations look small, inspect native allocator retention with `vmmap`.
- If RSS is retained by semantic model/native runtime memory, prefer the **Embedding Helper Process** path so the model dies with the helper.
- If RSS is retained by image decode/cache generation, prefer the **Image Preparation Helper Process** path so full-resolution decoder/encoder allocations die with the helper.
- If CPU is high only in the first few seconds after **Settled Idle**, classify it as **Idle Tail** before optimizing steady idle.
- If idle CPU grows with board population, use `scripts/run-idle-board-size.sh` before changing render or residency logic.
- If idle CPU or **Quiescent Memory** grows with library size on an unchanged active board, use `scripts/run-existing-library-baseline.sh` to confirm whether the regression is library-size-driven (search/backfill/asset load) rather than board-size-driven.
- If animated images raise idle CPU, compare `animated` with `animated-control` before blaming placement count.

## Embedding Profiling

For embedding changes, measure:

- model cold mount timing: assets resolved, tokenizer loaded, text session loaded, vision session loaded, total.
- first embedding latency after model ready.
- total import embedding drain time.
- memory while model is warm.
- memory after model unload.

`SNAPPYREF_PERF_MODEL_UNLOAD_AFTER_IDLE_MS=0` keeps the current baseline focused on immediate unload after embedding queues drain.

### Embedding Helper Grace Period

The embedding worker keeps the **Embedding Helper Process** alive for a
short grace window after its queue drains so trickled imports reuse a
single model load instead of cold-mounting per asset.

- Default grace: **3000ms** (3s).
- Override: `SNAPPYREF_EMBEDDING_HELPER_GRACE_MS=<ms>`. Setting `0`
  reproduces the previous behavior (helper exits as soon as the queue
  empties), which is occasionally useful when isolating
  **Quiescent Memory** measurements from the helper-alive window.
- The `trickle-import` scenario is the canonical reproduction: pre-fix
  it shows three separate `semantic_model_load_timing` events in
  `events.ndjson`; post-fix it shows one.
- The helper still exits after the grace window, so existing
  `import-1`, `import-20`, and library-baseline **Quiescent Memory**
  numbers must remain stable.

## Reporting

Keep reports concise and numerical:

- command and run directory
- before/after **Time to Idle**
- before/after main-process **Quiescent Memory** (RSS)
- before/after process-tree peak RSS during active work
  (`samples-summary.json` → `process_tree.active_work.peak_tree_rss_bytes`)
- before/after quiescent CPU
- largest phase movement (use `phase-summary.json` →
  `phase_summary.durations_ms`)
- per-format movement when format matters (use `phase-summary.json` →
  `per_format`)
- known caveats, especially whether child-process peak memory was sampled

## What Not To Do

- Do not optimize from anecdotal Activity Monitor impressions alone.
- Do not average shutdown into idle CPU or memory.
- Do not redefine **Settled Idle** to mean low CPU.
- Do not commit large perf assets; `perf-assets/` is private and gitignored.
- Do not treat one mixed-corpus result as a format-specific conclusion; run `format:<format>` when format matters.
