# Perf Testing

SnappyRef perf testing measures real app work on the local machine. The first
baseline starts from an empty Library and empty Active Board, imports either one
asset or the 20-file mixed perf corpus, and reports Time to Idle.

## Baseline Scenarios

- `import-1/default-jpeg`: import one medium JPEG from the perf corpus.
- `import-20/mixed-corpus`: import the full 20-file perf corpus.
- `import-format/<format>`: import all corpus files for one format. Singleton
  formats run as one-image cases.
- `animated`: import only animated corpus assets.
- `animated-control`: import the same number of small static control assets.
- `static`: import all non-animated corpus assets.
- `static-20`: import 20 non-animated assets, cycling through the static corpus
  when needed, to separate count/placement load from animation cost.

## Headline Metric

Time to Idle is the elapsed time from scenario start until Settled Idle.
Settled Idle means no queued or in-flight import work, image residency work,
crop work, demand embedding, embedding backfill, or canvas image backfill.

CPU and memory are outcome metrics, not part of the Settled Idle definition.
The idle CPU target is under 1% after Settled Idle.

Quiescent CPU is the post-tail idle target. It excludes the first few seconds
after Settled Idle and excludes app shutdown. The first few seconds after
Settled Idle are still measured separately as idle tail so late cleanup, CPU
accounting decay, and OS/window events remain visible in the profile.

## Phase Metrics

Runs should emit structured events that allow Time to Idle to be broken down
into:

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

Each asset event should include format, byte size, dimensions, and source role
so mixed-corpus runs can be grouped by image format.

## Corpus

The first representative perf corpus contains 20 files:

- 5 JPEG
- 5 PNG
- 4 WebP
- 1 GIF
- 1 SVG
- 1 TIFF
- 1 BMP
- 1 AVIF
- 1 JPEG 2000 / JP2

Perf corpus files live under `perf-assets/images/`, which is private and
gitignored. `perf-assets/manifest.json` records filename, format, dimensions,
byte size, source URL, license/status, and role.

Prefer stable, permissive, low-risk sources such as Wikimedia Commons,
public-domain or Creative Commons media, and official format sample repositories.
Avoid blind scraping and sketchy downloads.

## Measurement Split

The app should emit app-state timing events because it knows its work queues.
An external wrapper should launch the app and sample process CPU, RSS, and
thread count from macOS while the scenario runs.

Expected run outputs:

- `events.ndjson`: app timing events
- `samples.csv`: external CPU/RSS samples
- `summary.json`: scenario summary and phase breakdown
- `samples-summary.json`: CPU/RSS aggregate summary, including steady idle
  samples split into idle tail and quiescent idle windows

Run examples:

```sh
scripts/run-perf-scenario.sh import-1
scripts/run-perf-scenario.sh import-20
scripts/run-perf-scenario.sh format:png
scripts/run-perf-scenario.sh animated
scripts/run-perf-scenario.sh animated-control
scripts/run-perf-scenario.sh static-20
```

The wrapper keeps the app alive briefly after Settled Idle so the sampler can
measure real idle CPU and memory after app work has drained.

Set `SNAPPYREF_PERF_MODEL_UNLOAD_AFTER_IDLE_MS` to unload the semantic model
during that post-idle hold window. Model load timing events report asset
resolution, tokenizer load, text session load, vision session load, and total
cold-mount time. This lets a run compare cold model mount cost, embedding run
time after the model is ready, memory while the model is warm, and memory after
model unload.

`scripts/run-perf-scenario.sh` writes:

- `idle_tail_0_to_3s`: first three seconds after Settled Idle.
- `quiescent_5_to_15s`: steady post-tail idle window.
- `time_to_quiescent_ms`: time from Settled Idle until CPU stays below 1% for
  a sustained 3-second window.
- `memory`: RSS summaries for all samples, idle samples, idle tail, quiescent
  idle, and post-idle-without-shutdown windows.

`scripts/run-idle-board-size.sh` creates libraries with `0`, `1`, `20`, `100`,
and `1000` placements of the same asset, launches the app without import work,
waits for startup/load to settle, and samples idle CPU. This isolates whether
idle CPU grows with board population.
