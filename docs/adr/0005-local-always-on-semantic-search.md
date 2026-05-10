# Local always-on semantic search

SnappyRef semantic search should run locally with a fast on-device embedding model, store embeddings in the managed **Library**, and index quietly in the background so text and image-pivot search are available without the user configuring external services. For normal users, semantic search is a product capability rather than a preference; the build feature remains available for lightweight dev, test, or packaging variants.

This keeps the app local-first and avoids network dependence while still making search useful across **This Board** and **Library Scope**. The trade-off is that the app must manage model packaging, background indexing, failures, and cache invalidation without letting model load or embedding work block startup, import, board switching, or canvas interaction.

## Consequences

- Release builds should include or otherwise provide the local model needed for semantic search.
- Indexing should happen silently in the background and tolerate missing or failed embeddings.
- Search behavior should preserve the same ranking semantics across **This Board** and **Library Scope** while changing only result eligibility.
- The `semantic-search` build feature may exist for dev/test builds, but user-facing product behavior should assume semantic search is on.
