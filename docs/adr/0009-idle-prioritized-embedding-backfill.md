# Idle-prioritized embedding backfill

SnappyRef keeps **Semantic Search** available as an always-on product capability, but bulk **Embedding Backfill** should be idle-prioritized rather than started immediately during app startup.

Startup, **Active Board Load**, board switching, import, crop, and canvas interaction are more important than completing all missing embeddings as fast as possible. The trade-off is that some **Library-wide Search** results may appear later on a cold or newly expanded library, but the app stays responsive and avoids passive CPU contention.

## Decision

- **Embedding Backfill** starts only after the active board is usable and import/crop/search work has settled.
- **Demand Embedding** may bypass idle gates when triggered by explicit user action.
- **Active Board Embedding Backfill** runs before broader library backfill.
- **Embedding Source** prefers the **Working LOD** when it exists, with the original asset as fallback.
- **Embedding Retry** work runs only after never-tried embedding work has drained.
- Backfill remains off the main thread.
- An open search overlay is not itself active search; active search means a query, prefetch, or typing debounce window is still in flight.
- **Embedding Backfill** checks whether a backfill job exists before initializing the semantic model.
- Image **Demand Embedding** runs through a short-lived **Embedding Helper Process** for queued import/backfill embedding work so the main app process does not retain ONNX Runtime/native allocator RSS after the queue drains.

## Consequences

- Semantic search readiness becomes progressive: active-board coverage should improve before library-wide coverage.
- Search remains available, but missing embeddings may produce incomplete results until background work catches up.
- The scheduler must distinguish opportunistic backfill from user-demanded embedding work.
- Working LOD cache availability now matters for semantic indexing performance as well as canvas rendering.
- Empty or fully indexed libraries must not load the semantic model just to discover there is no background embedding work.
- The helper process amortizes model startup across currently queued work, then exits when that queue burst drains. This preserves batch throughput while returning semantic model memory to the OS.
