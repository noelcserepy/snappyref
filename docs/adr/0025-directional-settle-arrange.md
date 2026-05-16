# Directional settle arrange

SnappyRef uses **Directional Settle Arrange** for arrow-key arrangement of selected Placements: each command treats one edge of the current selection bounds as a fixed wall and moves selected Placements as far as possible toward that wall, stopping at the wall or at the Workspace Settings image gap from already-settled selected Placements. This deliberately differs from plain edge alignment: sparse rows should collapse all the way to the target edge, while crowded rows stack against earlier settled Placements according to geometric order.

## Decision

- Directional settle applies to selected Placements only.
- The target selection-bounds edge remains fixed; the opposite edge may shrink inward.
- Blocking uses the Workspace Settings image gap on both axes: a Placement blocks another when their cross-axis intervals overlap or are within the configured gap.
- Settlement order is geometric, not Stack Order: right/down settle from the far edge inward; left/up settle from the near edge inward, with deterministic center/id tie-breaks.
