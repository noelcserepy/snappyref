# Format-specific import optimization candidates

Tracking issue: #71. These are the format-specific slow paths observed in the
new `format:<format>` perf scenarios, surfaced by `events.ndjson` joined per
asset by `scripts/summarize-perf-by-format.py`. None of these were shipped as
part of issue #71. Each is a follow-up candidate; pick whichever has the best
ROI for the next perf bundle.

## How to reproduce

```sh
# Single-format
scripts/run-perf-scenario.sh format:jp2

# All formats + per-format summary
scripts/run-perf-format-suite.sh

# Existing manual sweep
scripts/summarize-perf-by-format.py perf-assets/runs/<run-dir>...
```

Baseline runs cited below: `perf-assets/runs/20260511-1941*-format:*`,
`20260511-1944*-format:*` (clean per-format), `20260511-194449-import-20`
(mixed 20-image), `20260511-194514-animated` /
`20260511-194536-animated-control` (animated GIF vs control BMP).

## Baseline numbers (clean single-format runs, 20260511-194116..194419)

| format | n | import mean (ms) | import max (ms) | embed mean (ms) | slowest file |
| --- | --- | --- | --- | --- | --- |
| avif | 1 | 159.3 | 159.3 | 9.1 | avif/libheif-example.avif (1 asset, ~0.1MB) |
| bmp  | 1 | 13.4  | 13.4  | 16.1 | bmp/rgb24.bmp |
| gif  | 1 | 12.8  | 12.8  | 16.3 | gif/pffffft.gif (animated) |
| jp2  | 1 | 225.0 | 225.0 | 10.1 | jp2/file6.jp2 (~0.4MB) |
| jpg  | 5 | 131.1 | 282.2 | 11.8 | jpg/gathering-a-sample.jpg (31.6MP, 12.0MB) |
| png  | 5 | 120.8 | 385.2 | 15.9 | png/commons-screenshot-cropped.png (43.8MP, 8.7MB) |
| svg  | 1 | 12.9  | 12.9  | n/a | svg/svg-logo.svg |
| tiff | 1 | 110.9 | 110.9 | 11.3 | tiff/palisades-rift.tiff (4.1MP) |
| webp | 4 | 80.9  | 151.7 | 11.3 | webp/knuft.webp (4.6MP) |

Notes:

- `import_mean` here is queue->placement_visible latency stamped on the new
  per-asset events (see `events.ndjson` from these runs).
- `embed_mean` measures queue->done elapsed for the demand embedding; in the
  current env the semantic model is not loaded, so these are floor times
  dominated by queue handoff. They are comparable across formats but should
  not be treated as decode cost.
- AVIF/JP2/SVG show 0x0 dimensions in `import_queued` because placeholders
  are not loaded by the time the event is emitted; the slowest_file value
  is still the right one because there is only one asset.

## Candidate 1 - JP2 / AVIF native fallback fork+exec overhead

**Symptom.** Single-asset format runs show JP2 at 225ms and AVIF at 159ms
import wall time for tiny files (~0.4MB and ~0.1MB respectively). For
comparison, BMP and GIF round-trip in ~13ms and TIFF (4.1MP, 3.2MB) lands
at 111ms. The per-MP cost is off the chart for AVIF/JP2 because the file
is small but two child processes need to fork+exec.

**Root cause.** `src/jobs/image_formats.rs:decode_image_file_with_native_fallback`
(`#[cfg(target_os = "macos")]`) shells out to `sips` (or `qlmanage` for JP2)
to produce a PNG temp file, reads it back, then decodes that PNG via the
`image` crate. On top of that, `native_image_dimensions` runs a *third*
child process (`sips -g pixelWidth/pixelHeight`) to recover the source
dimensions. So a single JP2 import does:

1. `qlmanage -t -s <max> -o <tmp> <path>` (fork/exec, Quick Look generator)
2. `sips -g pixelWidth -g pixelHeight <path>` (fork/exec)
3. read tmp PNG, decode via `image::load_from_memory_with_format(Png)`
4. delete tmp dir

AVIF is the same flow, swapping step 1 for `sips -s format png ...`. Each
macOS fork/exec is ~30-60ms on M-series hardware before any real work
starts, so the floor is dominated by process startup, not decode.

**Fix sketch.** Two stacked wins:

1. Replace `native_image_dimensions`'s third child process with a single
   in-process header parse. For AVIF, read the `ispe` box from the
   container (HEIF). For JP2, read the SIZ marker. Both are < 100 lines
   of pure-Rust parsing. Save ~30-60ms per asset immediately.
2. Replace the entire `sips`/`qlmanage` round-trip with an in-process
   decoder. For AVIF: link `libavif` (or `image-rs`'s `avif` feature with
   `dav1d`). For JP2: link `openjpeg-sys` or use `jpeg2k`. Both are
   already on crates.io. Save the remaining ~100-150ms per asset and
   stop touching the filesystem for every import.

**Upside vs cost.** Step 1 is a small parser, ~half a day, ~30-60ms saved
per AVIF/JP2 import. Step 2 is a real dependency add (libavif/openjpeg or
pure-Rust equivalents), 1-2 days plus a CI/distribution review. Total
potential: drop AVIF/JP2 imports to ~30-50ms (in line with TIFF/PNG of
similar size). The acceptance check for this is whether the per-MP cost
collapses into the same band as the rest of the corpus.

## Candidate 2 - Large PNG decode dominates raw wall time

**Symptom.** `png/commons-screenshot-cropped.png` (43.8MP, 8.7MB) takes
385ms (max) in single-format `format:png` and 530ms (max) when run as
part of `import-20`. PNG's per-MP cost (~10ms/MP) is already in line with
JPEG, so the issue is sheer pixel count, not the codec.

**Root cause.** `decode_with_image_crate` in `image_formats.rs` runs the
default `image` crate PNG decoder single-threaded. On a 43.8MP image
that is 200MB of RGBA8 even before any further processing.

**Fix sketch.** Three options, increasing effort:

1. Cap or downsample PNG decode to canvas-max-side at decode time so we
   never materialize a 200MB RGBA8 buffer for a thumbnail. Today
   `DEFAULT_CANVAS_IMAGE_MAX_SIDE` is applied downstream of decode for
   raster, so the peak allocation is still full-resolution. Hooking
   into `image`'s `Limits` or pre-scaling via the existing thumbnail
   path would shave allocation + copy time.
2. Switch large PNGs to a streaming decoder (`png` crate's `Reader`)
   that decodes one row band at a time and downsamples on the fly.
3. Use `oxipng`/`fpng`/`spng` only if benchmarks show pure decode is
   the bottleneck rather than the alloc/copy chain.

**Upside vs cost.** Option 1 is the highest-leverage and probably 1-2
days. Expected: 200-300ms shaved off `commons-screenshot-cropped.png`
import; cuts peak RAM during PNG import by ~5x for the slowest assets.
Options 2 and 3 are larger and need their own measurement.

## Candidate 3 - Large JPEG decode (single-asset wall time)

**Symptom.** `jpg/gathering-a-sample.jpg` (31.6MP, 12MB) hits 282ms max
in `format:jpg` and 265ms in `import-20`. JPEG already runs through
turbojpeg, so per-MP cost is ~9ms - this is the best of the bunch.

**Root cause.** Sheer pixel count again. turbojpeg is already SIMD; the
extra cost is allocate-and-copy of the 31.6MP RGBA8 buffer.

**Fix sketch.** Use turbojpeg's `tj3DecompressHeader` to read dimensions,
then call `tj3SetScalingFactor` with `2/8` or `4/8` so libjpeg-turbo
downsamples directly during IDCT to whatever the canvas needs. This is a
~1-day change in `decode_jpeg_with_turbojpeg`.

**Upside vs cost.** Expected ~40-50% reduction in import time for any
JPEG larger than `DEFAULT_CANVAS_IMAGE_MAX_SIDE`. Same scaling-on-decode
strategy as Candidate 2 option 1 but cheaper to implement because
turbojpeg exposes it directly. ~1 day; ~100ms saved on the slowest JPEG.

## Candidate 4 - Mixed-corpus worker contention

**Symptom.** Running the corpus in isolation: PNG max 385ms, WebP max
152ms, TIFF 111ms. Running all 20 mixed (`format:` set replaced by
`import-20`): PNG max 530ms (+38%), WebP max 466ms (+207%), TIFF 378ms
(+241%), SVG 274ms (vs 13ms isolated, +2000%). JP2/AVIF are essentially
unchanged because they run on a different decoder.

**Root cause.** Imports share the rayon decoder pool; large PNG/JPEG
imports starve the smaller-format imports behind them. Surfaces in the
event stream as long queue->placement_visible deltas for the assets
that landed late, not as long decode times.

**Fix sketch.** Two options:

1. Prioritise small files (by `bytes` from the new `import_queued`
   metadata) so the user sees thumbnails for 100KB SVG/BMP/GIF before
   the 12MB JPEG.
2. Reserve at least one worker for "fast" decoders (BMP/GIF/SVG/WebP-
   small) so a single huge PNG cannot block the queue entirely.

**Upside vs cost.** Option 1 is small (~half a day; we already know
each file's size at enqueue time after the changes in this bundle).
Expected user-visible win: first-placement-visible drops from
~hundreds of ms to <50ms when a large asset is in the same batch. No
total throughput gain; this is purely a perceived-latency lever.

## Candidate 5 - Embedding queue is single-flight per asset

**Symptom.** Single-format runs show embedding mean ~10-16ms per asset
even though the model is not loaded - this is queue handoff floor. In
`import-20`, a single 47.6ms outlier (BMP) and 345.6ms outlier (PNG)
appear in `embed_max`. Those are not real decode work, they are the
demand-embedding queue back-pressuring on the semantic model startup
that happens around the same time.

**Root cause.** `src/search.rs:SearchService` handles demand embeddings
serially; placement_visible -> import_embedding_queued -> done is one
RPC per asset. When the model is cold, every entry blocks behind model
load. With model loaded, this floor disappears.

**Fix sketch.** Batch demand-embedding requests when several are queued
within a short window. `pending_demand_embeddings` already holds a map;
turning that into a "drain up to N" loop and calling the batched
embed_image path (already present for indexing) cuts per-asset RPC
overhead.

**Upside vs cost.** Hard to measure without a hot model in CI. Expected
~30-50% reduction in `embed_mean` once batching is in place; bigger win
in `import-20` style scenarios. ~1 day, fits naturally with the model
demand-batch work already in `search.rs`.

## Candidate 6 - SVG rasterization (idle vs busy)

**Symptom.** `format:svg` isolated: 12.9ms (under the noise floor).
`import-20`: 273.9ms. Acceptance criteria asked for SVG coverage which
this scenario provides.

**Root cause.** Same contention as Candidate 4. SVG itself parses and
rasterises instantly when alone.

**Fix sketch.** Subsumed by Candidate 4 - prioritising small/fast
formats fixes the SVG case automatically. No SVG-specific change
warranted today.

**Upside vs cost.** Free if Candidate 4 ships. Otherwise nothing to do.

## Animated vs static control (acceptance check)

Verified: `format:gif` 12.8ms vs `animated-control` (`bmp/rgb24.bmp`)
13.0ms - animation cost on import is negligible. The animation-frame
work happens lazily when the placement first renders, not during
import. No animated-specific optimization candidate.

## What was actually shipped in #71

- Per-format perf scenarios (`format:<format>`) already existed in the
  perf harness; this bundle enriched them.
- `import_queued`, `placement_visible`, `import_embedding_queued/done`
  events now carry `format`, `file`, `bytes`, `queue_id` and `asset_id`
  so the join in `scripts/summarize-perf-by-format.py` is a 1-line
  basename match instead of guesswork.
- `SemanticPerfEvent::ImportEmbeddingFinished` so per-asset embed-done
  events fire with the right asset_id instead of a count loop.
- `scripts/summarize-perf-by-format.py` builds per-format mean/max/min
  tables across one or many runs; ships with a built-in `--self-test`
  that the agent ran before publishing.
- `scripts/run-perf-format-suite.sh` runs the full suite and summarises
  in one shot.

No production decode paths were changed. All five candidates above are
follow-up work.
