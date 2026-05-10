# Tiered canvas image LOD

SnappyRef uses canvas image levels of detail instead of loading every existing placement at full detail during startup or board switches. Existing image load can make the board usable with **Overview LOD** first, then promote visible images to **Working LOD** or **Detail LOD** when screen size justifies it, while **Backfill** creates missing LOD cache files in the background.

This keeps startup and board switching fast for image-heavy boards without giving up detail when the user zooms in. The trade-off is extra cache bookkeeping and background work, but it protects the core interaction budget better than treating managed originals or full-detail derivatives as the default canvas representation.

## Consequences

- Canvas rendering may temporarily show lower-detail imagery while higher LODs are loading.
- Cache files are disposable derivatives; the **Managed Original** remains the asset source of truth.
- Promotion and backfill should stay separate from board activation so image availability does not block switching boards.
- Runtime CPU pixel snapshots should be warmed only for selected editing operations; canvas image LODs are the durable cache, not an always-resident CPU copy of every placement.
