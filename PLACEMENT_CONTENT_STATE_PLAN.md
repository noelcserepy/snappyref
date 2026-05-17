# Loading Placements Architecture Plan

## Core Idea

If the user can see an image-shaped object on the canvas and move it, that object is a **Placement**.

Asset availability is content state on the Placement, not a separate canvas object. Loaded images, newly dropped files, search additions, and failed loads should all participate in the same selection, movement, Guide Snapping, Stack Order, Document History, deletion, and Viewport behavior.

Target invariant:

> Placement Movement owns movement for all Placements. Placement Lifecycle owns provisional-vs-committed behavior. Content Resolution owns async loading. Source modules only say what content to add.

Async completion must never change Placement geometry. Add-to-Canvas chooses the initial visual frame when it creates the Placement; Content Resolution only changes content state.

## Current Problem

The code currently has several representations for movable image-like canvas content:

- selected loaded Placements, moved by canvas `DragState`
- import hover/import placeholders, stored separately from Placements
- search drag previews and move preview placeholders, stored separately from Placements

That split makes similar user flows behave differently:

- selected Placement drag can zoom, pan, snap, and mutate real Placement transforms
- import and search previews have parallel movement/preview state
- import/search completion replaces or integrates placeholders later
- snapping and movement logic is partly duplicated around placeholder geometry

The architecture should collapse committed canvas content into one Placement movement path.

## Target Model

### Placement

A board-local canvas item with:

- id
- board id
- position
- scale
- rotation
- visual dimensions
- Stack Order
- archive state
- lifecycle state
- content state

The Placement's transform plus visual dimensions are the single geometry source for rendering, hit testing, Guide Snapping, selection borders, Stack Order operations, and Document History. Loading and failed visuals render into that same Placement geometry.

### Placement Lifecycle

Keep provisional placement behavior separate from content state:

```rust
enum PlacementLifecycle {
    Provisional {
        batch_id: PlacementBatchId,
    },
    Committed,
}
```

`Provisional` Placements are real Placements for canvas behavior. They render, select, move, Guide Snap, and stack like committed Placements, but they are not durable document content until accepted.

If placement mode is cancelled, or a search drag never drops onto the canvas, provisional Placements are discarded history-neutral and persistence-neutral. If accepted, they become `Committed` and the creation/movement is recorded as normal document history.

### Placement Content

Keep Placement content about what the Placement can draw, not about async machinery:

```rust
enum PlacementContent {
    Loaded {
        asset_id: i64,
    },
    Loading {
        label: String,
    },
    Failed {
        label: String,
        message: String,
    },
}
```

`Loaded` renders a real Asset.

`Loading` renders a placeholder at the Placement bounds.

`Failed` renders an error placeholder at the Placement bounds.

All states remain normal Placements.

### Content Resolution

Async source work belongs outside Placement content.

Content Resolution persists intent by Placement id and tracks runtime worker correlation separately.

Durable state:

- `placement_id -> resolution intent`
- source metadata
- restart behavior
- retry/failure state

Runtime state:

- `queue_id -> placement_id`
- worker request lifecycle
- completion
- failure
- deletion/cancellation behavior

Suggested shape:

```rust
struct ContentResolutionStore {
    pending: HashMap<i64, ContentResolutionIntent>,
}

struct ContentResolutionIntent {
    placement_id: i64,
    board_id: i64,
    source: PlacementContentSource,
    status: ContentResolutionStatus,
}

struct ContentResolutionRuntime {
    queue_to_placement: HashMap<u64, i64>,
}

enum PlacementContentSource {
    ExternalImport(ExternalImportSource),
    ExistingAsset(i64),
    Clipboard,
}
```

This store is for resolving content, diagnostics, retry, restart recovery, and cleanup. It must not own movement behavior or Placement geometry.

## Architecture

The target architecture has three core responsibilities.

### 1. Placement Model

Files likely involved:

- `src/data/`
- `src/canvas/document.rs`
- `src/canvas/render.rs`
- `src/storage/db.rs`

Owns:

- Placement identity and transform
- Placement content state
- loaded/loading/failed rendering inputs
- persistence shape
- selection eligibility
- Stack Order membership
- Document History membership

Does not own:

- drag logic
- worker queues
- search result interpretation
- external drag normalization

### 2. Placement Movement

Files likely involved:

- `src/canvas/interaction.rs`
- `src/canvas/document.rs`
- `src/canvas/render.rs`
- possibly a new `src/canvas/placement_movement.rs`

Owns:

- selected Placement drag lifecycle
- movement of selected Placements
- Guide Snapping
- Viewport-safe zoom/pan during drag
- movement selection border behavior
- transform mutation

Does not own:

- file import
- search result interpretation
- asset creation
- async loading
- embedding work

Interface shape should stay small:

```rust
begin_selected_placement_drag(pointer_world, now)
update_selected_placement_drag(pointer_world, modifiers, viewport, now)
finish_selected_placement_drag()
cancel_selected_placement_drag()
```

The implementation can keep using `DragState`, but `DragState` should operate over Placement ids regardless of content state.

### 3. Content Resolution

Files likely involved:

- `src/add_to_canvas.rs`
- `src/import_intake.rs`
- import worker/job code under `src/jobs/`
- app integration in `src/app.rs`

Owns:

- creating loading Placements when source content is not ready
- persisting Content Resolution intents by Placement id
- registering runtime queue ids against Placement ids
- starting source-specific async work
- resolving a loading Placement into loaded content
- marking a loading Placement failed
- ignoring completions for deleted Placements

Does not own:

- movement after Placements exist
- Guide Snapping during drag
- placeholder-specific canvas state
- Placement geometry changes after creation

## Source Adapters

### Add-to-Canvas

Add-to-Canvas should split placement creation from async registration:

1. Receive source intents.
2. Build `PlacementDraft`s with initial visual dimensions and compact layout.
3. Create loaded or loading Placements from those drafts, with lifecycle set to committed or provisional according to the source flow.
4. Select those Placements.
5. Emit `ContentResolutionIntent`s for unresolved content.
6. Stop owning movement.

`PlacementDraft` is only the boundary where approximate dimensions are normalized before a real Placement exists. After the draft becomes a Placement, async completion must not resize it.

### Import

Import owns:

- external source normalization
- source labels
- source resolution intents
- worker request payloads
- asset creation

Import does not own:

- canvas movement
- placeholder items
- selection drag behavior

### Search

Search owns:

- overlay state
- result selection
- converting selected Asset Results or Placement Results into Add-to-Canvas source intents
- accepting or cancelling search placement mode

Search does not own:

- drag preview geometry after provisional Placements exist
- search-specific movement state
- placeholder movement

Search should hand selected results to Add-to-Canvas, which creates real provisional Placements. From that point, canvas movement takes over selected Placements. Search only decides whether the provisional batch is accepted or discarded.

## Hover Rule

OS drag hover is not committed document content.

Allowed:

- ephemeral hover preview while the pointer is dragging external files over the window

Not allowed:

- hover preview participating in Document History
- hover preview being persisted
- hover preview pretending to be a Placement before drop

Name this state explicitly as `ExternalHoverPreview` or equivalent. Avoid reusing committed-content placeholder types for hover-only visuals.

Drop is the commit point for external imports:

1. Drop creates real committed loading Placements.
2. Those Placements are selected.
3. Placement Movement owns movement from that point.

Search keyboard/drag add-to-canvas can create real provisional Placements as soon as the user enters placement mode, because that is an explicit app action rather than an OS hover. Those Placements use normal canvas movement immediately. Accepting the drop commits them; cancelling the placement discards them.

## Persistence And History Rules

Loading and failed Placements are real document content.

Persistence/history semantics are part of Phase 1, not a cleanup detail.

Rules:

- provisional Placements are real in-memory Placements but not durable document content
- provisional Placement movement uses normal Placement Movement but stays history-neutral until accepted
- accepting a provisional Placement batch commits it and records the document edit
- cancelling a provisional Placement batch discards it history-neutral and persistence-neutral
- creation of loading Placements is a document edit
- movement of loading Placements is a document edit
- deletion of loading Placements is a document edit
- failed Placements persist until removed
- content resolution updates existing Placement content
- content resolution must not reset Placement geometry
- content resolution should be history-neutral unless a later product decision says otherwise
- pending Content Resolution intents persist by Placement id

Restart behavior:

- loaded Placements restore normally
- failed Placements restore normally
- loading Placements restore deterministically:
  - either resume if source can be resumed
  - or become failed with a recoverable message

## Dimension Rule

Known dimensions are used when Add-to-Canvas creates the Placement.

Approximate dimensions are normalized at the draft boundary:

```text
Add-to-Canvas may choose fallback visual dimensions before creation. Content Resolution never changes Placement geometry after creation.
```

On completion, Content Resolution updates content/rendering only. It must preserve:

- x/y
- visual dimensions
- scale
- rotation
- Stack Order
- selection state unless explicitly intended

## Implementation Plan

### Phase 1: Model Placement Content State And Persistence Semantics

Goal: loaded/loading/failed are Placement content states, provisional/committed is a separate lifecycle state, and loading Placements are real document content when committed.

Steps:

1. Add Placement content state to the in-memory model.
2. Map existing loaded Placements to `Loaded { asset_id }`.
3. Add Placement lifecycle state to the in-memory model.
4. Add stable visual dimensions to Placement geometry if the current model does not already have a content-independent size.
5. Define storage shape for committed loaded/loading/failed Placements.
6. Define durable Content Resolution intent storage by Placement id.
7. Define restart behavior for committed loading Placements.
8. Render loading/failed Placements using existing placeholder visual language inside Placement geometry.
9. Rename OS-hover-only placeholder state to `ExternalHoverPreview` or an equivalent non-document concept.

Tests:

- loaded Placement still renders through existing image path
- loading Placement renders placeholder within Placement visual dimensions
- failed Placement renders failed placeholder and remains selectable
- loaded/loading/failed Placements all appear in hit testing
- provisional Placements are selectable and movable but not persisted
- cancelling provisional Placements removes them history-neutral
- accepting provisional Placements records a document edit
- loading/failed Placements persist and reload deterministically
- content resolution does not reset Placement geometry
- pending Content Resolution intents persist by Placement id

### Phase 2: Make Placement Movement Content-State Agnostic

Goal: canonical selected Placement drag works for loaded, loading, failed, provisional, and committed Placements.

Steps:

1. Ensure selected layout items include loading/failed and provisional Placements.
2. Ensure hit testing can select loading/failed and provisional Placements.
3. Ensure `DragState` mutates Placement transforms without requiring loaded Asset textures.
4. Ensure Guide Snapping candidates include loading/failed and provisional Placement visual bounds.
5. Ensure selection borders and movement border hiding work for all content states.

Tests:

- loaded + loading multi-selection moves as one group
- provisional + committed multi-selection moves as one group
- loading Placement Guide Snapping uses its visual bounds
- provisional Placement Guide Snapping uses its visual bounds
- Viewport zoom works during loading Placement drag
- MMB pan works during loading Placement drag
- deleting selected loading Placement removes it from the board
- moving loading Placement mutates only Placement transform

### Phase 3: Convert Add-to-Canvas Sources To Placements Plus Content Resolution

Goal: import/search create real Placements and register async resolution work.

Steps:

1. Add `PlacementDraft` creation for Add-to-Canvas sources.
2. Add Content Resolution store plus runtime queue correlation.
3. Convert external file drop to create selected loading Placements on drop.
4. Register durable Content Resolution intents by Placement id.
5. Register import worker queue ids as runtime correlation.
6. On import completion, resolve existing Placement content to loaded without changing geometry.
7. On import failure, mark existing Placement failed.
8. Convert Search Asset Results to create loaded or loading Placements.
9. Convert Search Placement Results to duplicate/create loaded Placements.
10. Remove import/search ownership of movement.

Tests:

- dropping multiple files creates selected loading Placements immediately
- dragging dropped loading Placements uses canonical Placement Movement
- import completion preserves Placement geometry
- import completion preserves Stack Order
- import failure leaves selectable failed Placement
- deleting loading Placement before worker completion prevents stale completion from resurrecting it
- switching boards while imports are in flight does not mutate the wrong Active Board
- entering search placement mode creates selected provisional Placements
- dragging search provisional Placements uses canonical Placement Movement
- cancelling search placement mode discards provisional Placements history-neutral
- accepting search Asset Results commits selected Placements
- accepting search Placement Results duplicates selected Placements
- Guide Snapping matches selected on-board Placement drag
- Viewport zoom/pan during search Add-to-Canvas drag matches selected Placement drag

### Phase 4: Delete Parallel Placeholder Movement Paths

Goal: remove duplicate movement representations.

Delete or narrow:

- `move_preview_placeholders`
- `search_drag_preview`
- `SearchResultDragState` movement responsibilities
- placeholder-specific Guide Snapping branches
- search-specific placement preview geometry
- `ImportPlaceholder` as a canvas item concept

Keep only visual rendering helpers if useful:

- loading placeholder painter
- failed placeholder painter
- loading shimmer

`ExternalHoverPreview` may remain only for non-Placement OS drag hover, and must not share movement behavior with real Placements. Search placement mode should use provisional Placements instead of a separate preview representation.

Tests:

- no behavior regressions in import/search placement flows
- no stale placeholder state after cancel/drop/failure
- no special search/import movement code remains for committed content

## Things To Check Carefully

### Transform Preservation

Async completion must not mutate Placement geometry or interaction state. It must preserve:

- x/y
- visual dimensions
- scale
- rotation
- Stack Order
- selection state unless explicitly intended

This rule applies even when the source later reports better dimensions than the initial draft used.

### Stack Order

Loading Placements should enter Stack Order exactly like loaded Placements. Search/import should not have separate z-order handling.

### Undo/Redo

Document History should treat committed loading Placements as document content. Provisional Placements should become history-visible only when accepted.

Key histories:

- create loading Placement
- move loading Placement
- delete loading Placement
- accept provisional Placement batch

Content resolution should be history-neutral unless later product behavior requires undoing loaded-vs-loading visual state.

### Provisional Placement Lifecycle

Provisional Placements are real in-memory Placements, not drafts or placeholder structs.

Rules:

- they use normal Placement geometry, rendering, selection, Guide Snapping, Stack Order, and movement
- they are excluded from persistence and Document History until accepted
- cancellation deletes them and removes or cancels any pending Content Resolution intents
- acceptance flips them to committed and records the resulting document edit
- async completion for a discarded provisional Placement must be ignored

### Cancellation

If the user deletes a loading Placement, async completion should be ignored or cancel the queued work.

The durable Content Resolution intent should be removed or marked cancelled with the Placement deletion so restart cannot resurrect it.

### Selection

Creation should select the new Placements. Completion should not unexpectedly change selection.

### Performance

Movement frames must remain storage-free. Persist loading Placement changes through existing dirty/persistence queue behavior.

### Tests Should Prefer Behavior

Good test names:

- `dropped_files_create_selected_loading_placements`
- `loading_placements_drag_like_loaded_placements`
- `search_placement_mode_uses_provisional_placements`
- `cancelled_provisional_placements_do_not_persist`
- `import_completion_preserves_loading_placement_geometry`
- `search_asset_results_use_canonical_placement_movement`
- `deleting_loading_placement_ignores_late_import_completion`

Avoid tests that assert internal placeholder structs disappear. Assert user-visible behavior and public module effects.

## Migration Strategy

Do not rewrite all paths at once.

Recommended order:

1. Add Placement Content State, Placement Lifecycle, stable visual dimensions, and storage semantics.
2. Make canvas selection/movement support loading and failed Placements.
3. Add durable Content Resolution intents keyed by Placement id.
4. Convert external file drop.
5. Convert Search Asset Results to provisional Placements that commit or discard.
6. Convert Search Placement Results to provisional Placements that commit or discard.
7. Remove committed-content placeholder movement state.
8. Keep only renamed hover-only preview state.
9. Update ADR-0020 language or add a follow-up note pointing to ADR-0026.

## Success Criteria

- Dragging selected loaded Placements, newly imported loading Placements, and search-added Placements uses the same Placement Movement code.
- Search placement previews are real provisional Placements, not a parallel preview geometry system.
- Zoom, pan, Guide Snapping, selection borders, Stack Order, and deletion behave the same across all committed Placement flows.
- Import/search completion resolves content on existing Placements instead of replacing separate placeholder objects.
- Import/search completion never changes Placement geometry.
- Placeholder-specific movement state is gone for committed content.
- Cancelled provisional Placements leave no persisted document content or history entry.
- Tests cover movement parity and async completion preserving Placement geometry.
