# On-demand canvas pixel snapshots

SnappyRef does not keep CPU-side RGBA pixels for every loaded placement. Canvas rendering uses textures and persisted **Canvas Image Cache** LODs, while **Canvas Pixel Snapshots** are warmed only when a selected placement needs CPU pixels for crop preview or crop undo.

This avoids multi-gigabyte passive memory use on image-heavy boards. The trade-off is that the first crop after selecting a placement may pay an on-demand decode or resize cost, but that cost is preferable to retaining large idle CPU buffers for placements that may never be edited.

## Consequences

- Existing image load should not eagerly populate CPU pixel caches.
- Selection changes should prune warmed pixel snapshots for deselected placements.
- Crop may synchronously warm the needed snapshot after the target placement is known.
- Persisted canvas image LOD files remain the preferred source for warming snapshots, with the **Managed Original** as fallback.
