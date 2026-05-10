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

## Phase Metrics

Runs should emit structured events that allow Time to Idle to be broken down
into:

- import queued
- first placement visible
- all placements visible
- embedding model ready
- first import embedding started
- first import embedding done
- all import embeddings done
- import settled
- embedding backfill done
- settled idle

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
  samples after the first post-idle second

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
