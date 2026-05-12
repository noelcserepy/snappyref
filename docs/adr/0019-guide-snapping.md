# Guide snapping

SnappyRef uses **Guide Snapping** for manual placement alignment: Figma-style visual guides that nudge the dragged placement or preview group to nearby placement edges, centers, corners, and layout-gap adjacency.

Guide Snapping replaces the old manual drag layout-snap behavior. The old solver tried to find good non-overlapping arrangement slots. Manual drag needs a different feel: small magnetic nudges, clear indicators, modifier override, and no surprise jumps to distant layout candidates.

## Decision

- Guide Snapping aligns visual axis-aligned bounds, including the expanded bounds of rotated placements.
- Snap targets are viewport-near placements: the visible viewport expanded by guide-snap padding, not the entire board.
- Single-placement drag, multi-selection drag, import previews, and Asset Result search drops use the same guide-snap calculation.
- Edge and center alignment use normal guide lines. Corner and center intersections add small X markers.
- Layout-gap adjacency is part of Guide Snapping because the workspace layout gap is the user's global spacing contract.
- Candidate competition is distance-based per axis; the smallest nudge wins.
- Acquisition uses a small screen-space threshold, with a slightly larger release threshold for hysteresis.
- Cmd/Ctrl temporarily disables Guide Snapping during drag.
- Guide Snapping does not snap to **Canvas Background** grids or dots.
- Equal-spacing detection is out of scope for this decision.
- Textboxes and other annotations are out of scope until annotations are explicitly treated as layout objects.
- Selection borders hide while placements are moving, then reappear after movement pauses or the drag ends.
- The guide-snap calculation is a pure canvas module: desired rect, candidate rects, gap, and threshold produce a snap result with indicators. Callers own state mutation.

## Consequences

- Snapping feels local and visual rather than like automatic arrangement.
- Very distant placements do not influence drag behavior, which keeps large boards predictable and fast.
- Rotated placements snap by what the user can see, not by their original asset geometry.
- Future equal-spacing guides should extend Guide Snapping without reintroducing the old layout-slot solver.
- Any future annotation snapping needs a separate decision about whether annotations participate as layout targets.
