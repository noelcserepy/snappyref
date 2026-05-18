# Prioritize small assets first in mixed-batch imports

SnappyRef imports a mixed batch of asset files through a shared decoder pool.
Workers previously drained the **Interactive** queue in FIFO order, so a single
12 MB JPEG could block several 50–200 KB SVG/BMP/GIF assets queued behind it
and delay their **Placement Visible** times 3–20× past their isolated baseline.

## Decision

The `RequestQueue` orders **Interactive** imports by source byte size
**ascending** at enqueue time, breaking ties by arrival order. Smaller asset
files reach **Placement Visible** first inside a mixed batch.

- `ImagePreparationRequest::priority_bytes()` is the sort key:
  - `ImportPath` reads `std::fs::metadata(source_path)` for the source size.
  - `ImportClipboard` uses the in-memory RGBA byte count.
  - `ImportUrl`, `Crop`, `LoadExisting`, and `BackfillLod` report `None`,
    which sorts to the back. URL imports do not know their payload size
    until the download completes, and the other variants do not participate
    in mixed-batch import ordering.
- Ordering is stable: equal byte sizes preserve FIFO arrival order so canvas
  placement reservations laid out at enqueue time are not reshuffled by the
  sort.
- No reserved-worker mechanism is introduced; one large asset can still
  occupy a worker while smaller assets drain. The byte-size sort is a
  smaller, lower-risk change than reserving a worker for small assets.

## Consequences

- **First-placement-visible** in mixed-batch imports drops by ~5–10× because
  small, fast-decoding assets are no longer blocked behind one large decode.
- The largest asset in a batch finishes at roughly the same wall-clock time
  it would have without the sort (worker count and decoder cost are
  unchanged), so **Time to Idle** stays within the same window.
- Canvas placement positions remain stable: positions are assigned by
  `reserve_import` at the call site, before enqueue, so they follow the
  user's drop iteration order regardless of decode order.
- URL imports keep FIFO ordering relative to one another because their
  size is unknown until the download finishes; they sort behind any
  size-known interactive request.
