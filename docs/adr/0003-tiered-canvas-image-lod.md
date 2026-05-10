# Tiered canvas image LOD

SnappyRef uses canvas image levels of detail instead of loading every existing placement at full detail during startup or board switches. Existing image load can make the board usable with **Overview LOD** first, then promote visible images to **Working LOD** or **Detail LOD** when screen size justifies it, while **Backfill** creates missing LOD cache files in the background.

This keeps startup and board switching fast for image-heavy boards without giving up detail when the user zooms in. The trade-off is extra cache bookkeeping and background work, but it protects the core interaction budget better than treating managed originals or full-detail derivatives as the default canvas representation.

## Decision

- Keep **Canvas Image Cache** generation off the UI thread.
- Use an **Image Preparation Helper Process** for path import, promotion, and backfill cache generation that needs to decode or encode large raster images.
- The helper returns only the downscaled LOD pixels needed by the main app plus a disposable cache file to install under the asset's cache path.
- The helper is intentionally short-lived: when the import or LOD job completes, the process exits and releases decoder/encoder allocator memory and CPU activity back to the OS.
- Animated image imports may fall back to the in-process path when animation frames are needed for playback.

## Consequences

- Canvas rendering may temporarily show lower-detail imagery while higher LODs are loading.
- Cache files are disposable derivatives; the **Managed Original** remains the asset source of truth.
- Promotion and backfill should stay separate from board activation so image availability does not block switching boards.
- Runtime CPU pixel snapshots should be warmed only for selected editing operations; canvas image LODs are the durable cache, not an always-resident CPU copy of every placement.
- Large decoder and cache-encoder allocations now die with the helper process, keeping **Quiescent Memory** close to the true live app footprint after import/backfill work drains.
- Helper cold-start overhead is acceptable for the current interactive baseline: `import-1` remains about 1.5s time-to-idle and `import-20` remains about 2.9s while quiescent RSS drops by roughly an order of magnitude.
