# Guide snapping

SnappyRef uses **Guide Snapping** for manual placement alignment: contact-local visual guides that nudge the moving, resizing, or preview placement bounds to nearby placement edges, centers, corners, and gap spacing targets.

Guide Snapping replaces the old manual drag layout-snap behavior. The old solver tried to find good non-overlapping arrangement slots. Manual drag needs a different feel: small magnetic nudges, clear indicators, modifier override, and no surprise jumps to distant layout candidates.

## Decision

- Guide Snapping aligns visual axis-aligned dragged bounds to target edges, centers, or gap spacing targets, including rotated placements' visual bounds.
- Resize Guide Snapping preserves the resize anchor and aspect ratio. Snap targets propose possible scales, and the nearest valid scale wins.
- Scale Drag Guide Snapping preserves the selected placement group center and applies one uniform scale factor to every selected placement. Snap targets propose possible group scale factors; the nearest valid factor wins.
- Gap spacing targets align opposite edges at the Placement Gap. They do not pad every target edge, so a side-by-side gap snap can still combine with real top, center, or bottom alignment on the other axis.
- Snap targets are contact-local placements inside the viewport-near candidate set. A placement becomes contact-local on an axis when an opposite-edge gap spacing target is within the active snap threshold and its interval on the other axis overlaps or nearly overlaps the dragged placement.
- Contact-locality gates target generation: X contact can expose that placement's Y edge/center alignment targets, and Y contact can expose its X edge/center alignment targets. Gap spacing targets remain axis-specific, and X/Y may resolve against different placements.
- Single-placement drag, single-placement resize, Scale Drag, multi-selection drag, import previews, and Asset Result search drops use the same guide-snap target model.
- Edge and center alignment use normal guide lines. Corner and center intersections add small X markers.
- Gap spacing targets are part of Guide Snapping because the Placement Gap is the user's global spacing contract for Placement spacing.
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
