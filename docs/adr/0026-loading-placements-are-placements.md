# Loading placements are placements

SnappyRef should not model visible in-flight imports or search additions as separate placeholder canvas items. If the user can see an image-shaped object on the canvas and move it, that object is a **Placement**. Loading, loaded, and failed asset resolution are **Placement Content State**, not different canvas object types.

This refines ADR-0020. The preview-first user contract remains: Add-to-Canvas should show content immediately at the intended board positions, support compact initial layout, and preserve placement intent across async work. Internally, Add-to-Canvas is user-facing wording; the architecture seam is **Placement Creation**. Preview visuals are rendered by loading Placements instead of parallel placeholder lists.

## Decision

- A Placement is the canonical board-local object for image-like canvas content, even before its Asset is available.
- Placement Creation creates normal Placements immediately, with a Placement Content State such as loading, loaded, or failed.
- Before Placement Creation, unresolved content is represented as a Source Intent. Do not introduce a domain object such as `PlacementDraft`: before creation it is not a Placement, and after creation it is a Placement in a lifecycle and content state.
- Placement Lifecycle owns provisional-vs-committed behavior: start, commit, cancel, and cleanup for provisional Placement batches.
- Placement Movement owns selection drag, Viewport-safe pan/zoom during drag, Guide Snapping, movement selection borders, and transform mutation for all Placements regardless of content state.
- Content Resolution owns runtime queue id to Placement id correlation, source-specific worker request construction, and completion/failure correlation for unresolved Placement content.
- Import, Search, and clipboard flows act as source adapters: they decide what content should be added and start any async work, but they do not own movement behavior.
- Async import/search completion resolves the content state of existing Placements. It must not replace a separate placeholder object with a new Placement when the user-visible object already exists.
- Loading Placements should participate in Stack Order, selection, deletion, undo/redo, board switch lifecycle, and persistence semantics as Placements.
- If dimensions are known, loading Placements render at the intended size. If dimensions are approximate, Placement Creation may create a Placement with fallback dimensions and later adjust only under an explicit rule that preserves user-authored movement.
- Failed content resolution leaves a Placement in a failed visual state that remains selectable, movable, and removable.

## Consequences

- Selected existing Placements, external imports, and Search additions can share one Placement Movement module.
- Placement Creation is the seam where Source Intents become board-local Placements. Layout planning before insertion is private implementation, not shared domain language.
- Placement Lifecycle is the seam where provisional Search placements are committed or discarded without leaking lifecycle mutation into Search.
- Placeholder-specific canvas state such as import hover placeholders, move preview placeholders, and search drag previews should be treated as migration targets, not long-term architecture.
- Source-specific modules keep their real responsibilities: intake, asset creation or lookup, queueing, loading, and content resolution.
- Document History and persistence become more important for loading Placements because they are real board content before asset resolution completes.
- ADR-0020's placeholder language should be read as a visual requirement, not as a requirement for separate placeholder canvas objects.
