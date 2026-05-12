# Dense compact layout

SnappyRef's compact arrangement command uses **Dense Compact Layout**: it minimizes visible holes first, then prefers a rough square only when density is close. Squareness is a tie-break, not the primary objective.

The command is for selected **Placements**. It may reorder the selected placements and change their positions, but it preserves authored scale and rotation. Rotated placements are packed by their visual axis-aligned footprint so spacing reflects what the user sees on the board.

Earlier compact layout behavior could prefer a visually square bounding box while leaving obvious internal holes. That looked wrong for reference-board work: users expect `Q` / compact arrange to make a selection tighter, not merely more square. A bounded MaxRects-style search gives better density for mixed image sizes while keeping the command deterministic and responsive.

## Decision

- Compact arrangement optimizes required-spacing occupancy first.
- Candidate squareness only decides when occupancy is roughly equivalent, within about three percentage points.
- Right-edge cleanliness is only a weak tie-break among near-equivalent candidates; clean top and left edges matter more.
- The configured workspace layout gap is always respected.
- Packing uses rotated visual placement footprints, not unrotated asset dimensions.
- Repeated compact arrangement on the same selection should be deterministic and idempotent.
- The search remains bounded: deterministic sort orders, a limited set of candidate widths, and pruned/merged free rectangles keep larger selections responsive.

## Consequences

- Compact arrangement is not a masonry layout and not a perfect-square layout.
- A less square result is correct when it is meaningfully denser.
- Future tuning should compare candidate density before adjusting squareness preferences.
- Import/search preview compaction may reuse the same compact layout objective, but it must preserve caller/source order when mapping preview slots back to async work.
