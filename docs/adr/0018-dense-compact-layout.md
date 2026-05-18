# Dense compact layout

SnappyRef's compact arrangement command uses **Dense Compact Layout**: it minimizes visible holes first, then prefers a rough square only when density is close. Squareness is a tie-break, not the primary objective.

The command is for selected **Placements**. It may reorder the selected placements and change their positions. User-invoked compact may also make small aspect-preserving scale changes through **Compact Size Normalization**; import/search preview compaction preserves expected preview sizes. Rotated placements are packed by their visual axis-aligned footprint so spacing reflects what the user sees on the board.

Earlier compact layout behavior could prefer a visually square bounding box while leaving obvious internal holes. That looked wrong for reference-board work: users expect `Q` / compact arrange to make a selection tighter, not merely more square. A bounded MaxRects-style search gives better density for mixed image sizes while keeping the command deterministic and responsive.

## Decision

- Compact arrangement optimizes required-spacing occupancy first.
- User-invoked compact arrangement may run **Compact Size Normalization** before packing: a bounded candidate-generation pass that proposes small aspect-preserving scale changes for selected placements whose visual width or height is near an existing size bucket, then scores those variants by final compact quality. A dimension is near a bucket when it is within about one world unit or about five percent of that bucket.
- Compact Size Normalization must earn its resize: a normalized candidate should beat the no-resize candidate only when post-pack occupancy improves meaningfully, roughly two to three percentage points. Near-equivalent candidates prefer fewer changed placements, smaller total scale deltas, and downscales over upscales.
- Candidate squareness only decides when occupancy is roughly equivalent, within about three percentage points.
- Right-edge cleanliness is only a weak tie-break among near-equivalent candidates; clean top and left edges matter more.
- The configured Placement Gap is always respected.
- Packing uses rotated visual placement footprints, not unrotated asset dimensions.
- Import and search preview compaction do not run Compact Size Normalization; previews keep their expected imported sizes stable.
- Repeated compact arrangement on the same selection should be deterministic and idempotent once no better normalization candidate exists.
- The search remains bounded: deterministic sort orders, a limited set of candidate widths, and pruned/merged free rectangles keep larger selections responsive.

## Consequences

- Compact arrangement is not a masonry layout and not a perfect-square layout.
- A less square result is correct when it is meaningfully denser.
- Future tuning should compare candidate density before adjusting squareness preferences.
- Import/search preview compaction may reuse the same compact layout objective, but it must preserve caller/source order when mapping preview slots back to async work.
