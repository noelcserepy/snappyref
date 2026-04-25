# Image Format Support Plan

This plan keeps the current import architecture intact for raster formats:

1. Decode the original into RGBA pixels.
2. Downscale for the canvas when downscale mode is active.
3. Store the original unchanged.
4. Store the canvas cache as JPEG when fully opaque, or lossless WebP when alpha is present.

The shared post-decode work is not part of the decoder comparison. The current release baseline for decode-only performance is the 4.75 MB progressive JPEG fixture at `7360x4912`, decoded through TurboJPEG in about 190-205 ms. Use this as the first practical budget for similarly large static images: around 250 ms decode time is acceptable, over 400 ms needs a native or preview-first strategy.

## Decoder Matrix

| Format | First implementation | Faster fallback | Target behavior | Perf stance |
|---|---|---|---|---|
| JPEG | Existing TurboJPEG path | Rust `image` fallback | Static RGBA | Baseline: about 190-205 ms decode at 36 MP |
| PNG | Existing `image` path | None initially | Static RGBA, preserve alpha | Measure only |
| BMP | Existing `image` path | None initially | Static RGBA | Should be trivial |
| ICO | Existing `image` path | None initially | Static RGBA from selected icon image | Measure, usually small |
| PBM / PGM / PPM / PNM | Existing `image` path | None initially | Static RGBA | Watch very large ASCII files |
| QOI | Existing `image` path | None initially | Static RGBA | Should be very fast |
| TGA | Existing `image` path | None initially | Static RGBA | Measure |
| TIFF | Existing `image` path | Native libtiff only if measurements justify it | Static RGBA | Watch large LZW-compressed files |
| EXR | Existing `image` path | macOS Image I/O or OpenEXR only if needed | Static RGBA | Watch unsupported compression variants |
| DDS | Existing `image` path first | `image_dds` if BC4-BC7 support is needed | Static RGBA | Add compressed DDS fixtures before deciding |
| AVIF | Existing `image` path first | Done: macOS Image I/O fallback if crate support is unavailable | Static RGBA initially | Escalate only if over budget |
| GIF | Existing `image` animation path | None initially | Animated playback | Already supported |
| WebP | Existing `image` static path plus new animation path | Native libwebp only if slow | Static + animated playback | Add animated WebP fixture/test first |
| SVG | Vector asset path, not raster decode | Raster fallback only for thumbnails/search if needed | Preserve vector scaling on canvas | Needs a separate asset/render path |
| HEIC / HEIF | macOS Image I/O first; Windows libheif later | libheif fallback where needed | Static RGBA first | Native justified because current stack has no decoder |
| JP2 | macOS Image I/O first | Reduced-resolution OpenJPEG preview later if needed | Static RGBA or fast preview | Full OpenJPEG RGBA decode was slower than `sips` on the 36 MP benchmark |
| JXL | Done: `jxl-oxide` with `moxcms` | macOS Image I/O or libjxl if slow | Static RGBA | Near budget on 36 MP image; keep measuring |
| PSD | macOS Image I/O first; Rust PSD on other platforms | Native/Image I/O for flattened composite | Flattened composite RGBA | Do not import layers as separate board items |
| RAW | Embedded preview first | macOS Image I/O / LibRaw full decode later | Fast preview RGBA | Full demosaic should be background-only |
| XBM | Small crate or parser | None initially | Static RGBA | Low risk |
| XPM | Small custom parser | None initially | Static RGBA | Scope to common XPM3 |

## SVG Direction

SVG should not go through the normal "decode to RGBA and cache as JPEG/WebP" path as the primary representation. The useful behavior is to keep it as a vector asset so it stays sharp when scaled on the canvas.

That implies a separate asset kind:

- Raster image assets: decoded to RGBA and cached as JPEG/WebP for display.
- Vector image assets: original SVG retained and rendered vectorially by egui/resvg where possible.

We may still need rasterized derivatives for thumbnails, search previews, or semantic embedding. Those should be explicitly named preview artifacts, not the source of truth for the canvas item.

Current implementation status:

- Local `.svg` imports are parsed with `usvg` to get intrinsic dimensions.
- The original SVG bytes are stored unchanged with `image/svg+xml`.
- SVG imports and existing SVG loads skip the app's JPEG/WebP canvas cache.
- URL SVG imports use the same vector path when the content type or URL extension identifies SVG.
- Canvas rendering uses egui's SVG loader from the original bytes, so display-size rasterization is owned by the renderer cache rather than persisted as the asset representation.
- Semantic-search indexing and cache-tier backfills are skipped for SVG until explicit raster derivatives are added.
- Cropping SVG is explicitly rejected for now, rather than silently rasterizing or failing through the raster decoder.
- Debug verification is available with `cargo run -- --verify-svg --screenshot /tmp/reference-svg.png`.

## Benchmark Shape

Add a decode-only CLI probe beside the existing end-to-end import probe:

```bash
target/release/reference --perf-decode /path/to/image
```

Expected output:

```text
decode source=... format=... source_mb=... dims=... megapixels=... decode_ms=... ms_per_mp=...
```

Rules:

- Run in release mode only.
- Time decode-to-RGBA for raster formats.
- For animated GIF/WebP, report first-frame decode and full-frame decode separately.
- For RAW, report embedded-preview decode separately from full demosaic.
- For SVG, report vector parse/load time and any optional preview rasterization time separately.
- Keep `--perf-import` for end-to-end import-to-visible measurements.

## Initial Large-Image Matrix

Measured with:

```bash
cargo build --release
REFERENCE_DECODE_BENCH_DIR=/tmp/reference-decode-bench scripts/benchmark-decoders.sh /Users/noelcserepy/Downloads/bailey-zindel-NRQV-hBF10M-unsplash.jpg
```

The source image is a 4.75 MB progressive JPEG at `7360x4912` (`36.15 MP`). Generated benchmark variants use the same visual content where tooling supports it.

| Format | Generated size | Decode time | ms/MP | Current read |
|---|---:|---:|---:|---|
| JPEG | 4.75 MB | 204.09 ms | 5.65 | Baseline is acceptable with TurboJPEG |
| PNG | 54.71 MB | 199.47 ms | 5.52 | Rust `image` path is acceptable |
| BMP | 103.43 MB | 105.01 ms | 2.90 | Rust `image` path is acceptable |
| TIFF RGBA | 139.00 MB | 56.66 ms | 1.57 | Rust `image` path is acceptable for this simple variant |
| TGA | 78.59 MB | 115.53 ms | 3.20 | Rust `image` path is acceptable after extension fallback |
| WebP | 2.13 MB | 381.04 ms | 10.54 | Slower than budget; keep Rust initially, but watch real-world imports |
| QOI | 44.53 MB | 114.01 ms | 3.15 | Rust `image` path is acceptable |
| PBM | 4.31 MB | 51.22 ms | 1.42 | Rust `image` path is acceptable for generated binary PBM |
| PGM | 34.48 MB | 36.39 ms | 1.01 | Rust `image` path is acceptable |
| PPM | 103.43 MB | 28.63 ms | 0.79 | Rust `image` path is acceptable |
| EXR | 551.72 MB | 5183.79 ms | 143.39 | macOS native fallback works after Rust memory-limit failure, but is far over budget |
| JP2 | 13.07 MB | 3782.87 ms | 104.64 | macOS native fallback works but is far over budget |
| JXL | 3.96 MB | 432.42 ms | 11.96 | Dedicated `jxl-oxide` path is much faster than `sips`, still slightly over the large-image budget |

Notes:

- TGA required a filename-extension fallback because it is often ambiguous from magic bytes alone.
- ffmpeg's default TIFF output produced a color type that `image` rejects; the benchmark script now generates RGBA TIFF to measure a supported baseline. TIFF still needs more fixtures for compression/color variants.
- WebP is much smaller on disk but roughly 2x slower than JPEG decode-to-RGBA for this test image. That does not automatically justify native libwebp, but it is the first candidate to re-evaluate if users feel WebP import latency.
- AVIF generation failed on this 36 MP source due encoder memory pressure. It still needs a smaller generated benchmark and real fixtures.
- The first generated EXR exceeded the Rust decoder memory limit; macOS `sips` can convert that file, but it is far too slow for large EXR imports.
- JP2 native fallback through macOS `sips` is functional but much too slow for large images. It needs an OpenJPEG dependency decision before claiming acceptable performance.
- JXL now uses `jxl-oxide`, reducing the 36 MP decode from about 1.9 s through `sips` to about 0.43 s. It is close enough to keep pure Rust for now, but still worth testing on a real external corpus.
- The external JXL corpus includes a CMYK fixture, so `jxl-oxide` is built with the pure-Rust `moxcms` feature.
- A full OpenJPEG experiment using the Rust `jpeg2k` crate decoded the small `768x512` external JP2 fixture in `9.56 ms`, but decoded the 36 MP generated JP2 in `5122.10 ms`, worse than the `sips` fallback. Do not add it as a default full-resolution import path.

## Fixture Strategy

Use a hybrid fixture strategy. Generated fixtures are good for deterministic smoke tests, but real decoder support needs real-world files because many failures come from metadata, compression variants, color profiles, animation chunks, camera-specific RAW containers, or malformed-but-common files.

### Checked-In Smoke Fixtures

Create tiny fixtures under `tests/fixtures/images/smoke/`.

Use generated files where the format is deterministic and simple:

- PNG, JPEG, BMP, TGA, PNM family, QOI.
- Simple TIFF variants: uncompressed, Deflate, LZW if tooling is available.
- GIF and animated WebP with two frames and known delays.
- XBM and XPM hand-authored minimal files.
- SVG vector fixture with simple shapes and a known viewBox.

These should stay small enough to keep in git.

### External Corpus Fixtures

Use `scripts/fetch-image-fixtures.sh` to download larger or specialty fixtures into the ignored directory `tests/fixtures/external/files/`. Use `scripts/check-external-image-fixtures.sh` after a release build to decode them and validate dimensions.

Do not commit large corpora or unclear-license images. `tests/fixtures/external/manifest.json` stores:

- URL.
- License.
- Expected hash.
- Expected dimensions.
- Expected decoder path.
- Known caveats.

Useful sources to evaluate:

- `image-rs/image` examples and test images for formats covered by the Rust `image` crate.
- ImageMagick example images for broad small-format coverage.
- JPEG XL project/libjxl testdata and public JPEG XL test page samples for JXL variants.
- libheif examples/test files for HEIC/HEIF/AVIF coverage.
- Camera RAW samples from explicitly redistributable sources, or local user-provided files kept out of git.

Current external fixture coverage:

| Source | Fixture | Purpose |
|---|---|---|
| libheif | `example.heic` | HEIC through macOS fallback |
| libheif | `example.avif` | AVIF through `image` or macOS fallback |
| jpeg2k | `file6.jp2` | JP2 through macOS fallback |
| jxl-oxide | `cmyk_layers.jxl` | JXL + CMYK color-management coverage |
| image-rs/image | small EXR | EXR positive decode coverage |
| image-rs/image | BMP, APNG PNG | Real corpus coverage for common static decoders |
| image-rs/image | malformed WebP fuzz input | Expected negative robustness fixture |

### Generated Benchmark Set

For speed comparisons, generate same-content fixtures at a few target sizes:

- Small: about 1 MP.
- Baseline large: about 36 MP, matching the JPEG reference image.
- Stress: about 50 MP for slow-path investigation.

Prefer generating from one canonical high-resolution PNG/TIFF source so visual content is consistent across codecs. Use external tools only in scripts, not in Rust unit tests:

- ImageMagick or GraphicsMagick for common raster formats.
- `cwebp` / `gif2webp` for WebP and animated WebP if needed.
- `cjxl` / `djxl` for JPEG XL.
- `heif-enc` for HEIC/HEIF/AVIF experiments where available.
- Camera RAW should use real files; synthetic RAW is not representative.

Generated files are useful for performance baselines, but passing them is not enough to claim broad format support.

## Implementation Order

1. Done: add `--perf-decode` and decode-only report struct.
2. Done for current scope: add fixture manifest structure, tiny checked-in smoke fixtures, generated common-format smoke tests, and an external fixture manifest/checker.
3. Done: generalize animation decoding from GIF-only to GIF + WebP.
4. Done: add generated same-content benchmark script for common large static formats.
5. Done: add extension fallback for ambiguous file formats such as TGA.
6. Done for current scope: add SVG as a vector asset path. Local path import, URL import, existing load, crop rejection, and visual probe coverage are implemented. Future raster derivatives may still be needed for thumbnails/search.
7. Done: add XBM/XPM as small raster decoders into the normal RGBA import pipeline.
8. Started: macOS native fallback via `sips` for AVIF, HEIC/HEIF, JP2, JXL failure fallback, PSD, EXR fallback, and common RAW extensions when Rust decoding fails.
9. Done for current scope: external fixture downloader infrastructure at `scripts/fetch-image-fixtures.sh`, plus decode/dimension checker at `scripts/check-external-image-fixtures.sh`.
10. Done for JXL: add a dedicated `jxl-oxide` decode path plus smoke coverage that exercises it when `cjxl` is available.
11. Done for full-resolution JP2/OpenJPEG experiment: rejected as a default path because it was slower than macOS Image I/O on the large benchmark.
12. Use measurements to decide whether JP2 needs a reduced-resolution preview decode path, and whether HEIC/HEIF, PSD, RAW, or EXR need optional native dependencies beyond macOS Image I/O fallback.

## References

- `image` crate default format list and decoder API: https://github.com/image-rs/image
- libheif HEIC/HEIF/AVIF coverage and codec backends: https://github.com/strukturag/libheif
- libjxl reference implementation and tools: https://github.com/libjxl/libjxl
- JPEG XL public test page: https://jpegxl.info/resources/jpeg-xl-test-page.html
- ImageMagick example image directory: https://usage.imagemagick.org/images/
