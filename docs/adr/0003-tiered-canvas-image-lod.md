# Canvas image LOD

SnappyRef uses canvas image levels of detail instead of loading every existing placement at full detail during startup or board switches. Existing image load can make the board usable with **Overview LOD** first, then promote visible images to **Working LOD** or **Detail LOD** when screen size justifies it, while **Backfill** creates missing LOD cache files in the background.

This keeps startup and board switching fast for image-heavy boards without giving up detail when the user zooms in. The trade-off is extra cache bookkeeping and background work, but it protects the core interaction budget better than treating managed originals or full-detail derivatives as the default canvas representation.

## Decision

- Keep **Canvas Image Cache** generation off the UI thread.
- Use an **ImagePrep Helper Process** for path import, promotion, and backfill cache generation that needs to decode or encode large raster images.
- The helper returns only the downscaled LOD pixels needed by the main app plus a disposable cache file to install under the asset's cache path.
- The helper is intentionally short-lived: when the import or LOD job completes, the process exits and releases decoder/encoder allocator memory and CPU activity back to the OS.
- Keep the **ImagePrep Helper Process** per-file instead of burst-scoped for now. A May 2026 burst-scoped prototype reused one helper per **ImagePrep Worker** until the import/LOD queue drained, then exited it. The measured latency win was too small to justify the extra protocol and lifecycle complexity.
- Animated image imports may fall back to the in-process path when animation frames are needed for playback.

## Consequences

- Canvas rendering may temporarily show lower-detail imagery while higher LODs are loading.
- Cache files are disposable derivatives; the **Managed Original** remains the asset source of truth.
- Promotion and backfill should stay separate from board activation so image availability does not block switching boards.
- Runtime CPU pixel snapshots should be warmed only for selected editing operations; canvas image LODs are the durable cache, not an always-resident CPU copy of every placement.
- Large decoder and cache-encoder allocations now die with the helper process, keeping **Quiescent Memory** close to the true live app footprint after import/backfill work drains.
- Helper cold-start overhead is acceptable for the current interactive baseline. In issue #70 measurements, the per-file helper baseline produced `import-1` first/all placement visible at 131 ms, **Time to Idle** at 860 ms, quiescent RSS at 123 MB, and quiescent CPU at 0.0%. The burst-scoped prototype produced 135 ms, 877 ms, 126 MB, and 0.0%.
- For `import-20`, the same baseline produced first placement visible at 33 ms, all placements visible at 559 ms, **Time to Idle** at 2249 ms, quiescent RSS at 163 MB, and quiescent CPU at 0.0%. The burst-scoped prototype produced 27 ms, 548 ms, 2215 ms, 173 MB, and 0.0%. The 6 ms first-placement and 34 ms idle wins do not outweigh the `import-1` regression, higher quiescent RSS, and more complex helper lifetime protocol.
- Issue #70 run directories: baseline `perf-assets/runs/20260511-120118-import-1` and `perf-assets/runs/20260511-120243-import-20`; prototype `perf-assets/runs/20260511-120920-import-1` and `perf-assets/runs/20260511-120958-import-20`.
