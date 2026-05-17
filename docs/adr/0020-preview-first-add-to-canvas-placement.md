# Preview-first add-to-canvas placement

Status note: ADR-0026 refines this decision. Placeholder language here describes the immediate visual user contract. Committed import/search content is now represented by Placement Content State on normal or provisional Placements, not by separate placeholder canvas items. Add-to-Canvas is user-facing wording; architecture should use Placement Creation and Source Intent.

SnappyRef places new canvas content through a preview-first contract. File imports and **Asset Result** search drops first show image-shaped preview content at the intended board positions. ADR-0026 now defines that committed content as real **Placements** with loading or loaded **Placement Content State**; async completion resolves existing Placements while preserving placement intent.

This supersedes treating imports, library-result placement, and final load integration as separate placement behaviors. Users should see one interaction: the preview follows the cursor, compacts as a group, guide-snaps as a group, and final placements land where the preview promised.

## Decision

- Import hovers and Asset Result search drops build a compact preview layout before dispatching async work.
- The compact preview group guide-snaps as one visual bounds group, not as independent items.
- Local imports use known file dimensions when cheaply available; if preview dimensions are approximate, final batch completion compacts again using real placement sizes.
- Asset Result drops use known asset dimensions and skip dimension fetching, but otherwise follow the same placeholder, reservation, guide, snapping, and completion path as imports.
- Async completion owns final selection and final compaction for the batch.
- Pending batches carry the target board id and intended world anchor. Completion for an inactive board must not mutate the current canvas selection or arrangement.
- Preview layout preserves caller/source order when mapping compact slots back to queued work, even if the packer internally reorders items.
- Guide indicators clear when preview placeholders clear or when there is no active snap.

## Consequences

- Preview and final placement stay aligned even when imports complete out of order.
- Multi-file imports leave the newly placed batch selected after finalization, which makes follow-up movement or compaction natural.
- Asset Result placement creates new board-local **Placements** for existing **Assets**; it does not duplicate assets.
- Future add-to-canvas flows should join this preview-first path unless they have a documented reason to bypass placeholders, batch completion, or guide snapping.
