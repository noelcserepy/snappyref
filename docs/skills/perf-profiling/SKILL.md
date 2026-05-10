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

## Baseline Scenarios

Run the established scenarios before and after substantial changes:

```sh
scripts/run-perf-scenario.sh import-1
scripts/run-perf-scenario.sh import-20
scripts/run-perf-scenario.sh format:png
scripts/run-perf-scenario.sh animated
scripts/run-perf-scenario.sh animated-control
scripts/run-perf-scenario.sh static-20
scripts/run-idle-board-size.sh
```

Scenario meanings:

- `import-1`: import one medium JPEG from the perf corpus.
- `import-20`: import the full mixed 20-file perf corpus.
- `format:<format>`: import all corpus files for one format; singleton formats become one-image cases.
- `animated`: import animated corpus assets.
- `animated-control`: import the same number of small static control assets.
- `static`: import all non-animated corpus assets.
- `static-20`: import 20 non-animated assets, cycling through the static corpus to separate count/placement load from animation cost.
- `run-idle-board-size.sh`: compare idle CPU with `0`, `1`, `20`, `100`, and `1000` placements of the same asset.

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
- `samples.csv`: external CPU/RSS samples.
- `summary.json`: scenario summary and phase counts.
- `samples-summary.json`: CPU/RSS aggregates split into all samples, idle samples, idle tail, quiescent idle, and post-idle-without-shutdown windows.

Important windows:

- `idle_tail_0_to_3s`: first three seconds after **Settled Idle**.
- `quiescent_5_to_15s`: steady post-tail idle window.
- `post_idle_0_to_15s_shutdown_excluded`: post-idle window excluding shutdown samples.

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
- If animated images raise idle CPU, compare `animated` with `animated-control` before blaming placement count.

## Embedding Profiling

For embedding changes, measure:

- model cold mount timing: assets resolved, tokenizer loaded, text session loaded, vision session loaded, total.
- first embedding latency after model ready.
- total import embedding drain time.
- memory while model is warm.
- memory after model unload.

`SNAPPYREF_PERF_MODEL_UNLOAD_AFTER_IDLE_MS=0` keeps the current baseline focused on immediate unload after embedding queues drain.

## Reporting

Keep reports concise and numerical:

- command and run directory
- before/after **Time to Idle**
- before/after quiescent RSS
- before/after quiescent CPU
- largest phase movement
- known caveats, especially whether child-process peak memory was sampled

## What Not To Do

- Do not optimize from anecdotal Activity Monitor impressions alone.
- Do not average shutdown into idle CPU or memory.
- Do not redefine **Settled Idle** to mean low CPU.
- Do not commit large perf assets; `perf-assets/` is private and gitignored.
- Do not treat one mixed-corpus result as a format-specific conclusion; run `format:<format>` when format matters.
