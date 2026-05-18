# Library asset placement reuses import integration

Superseded in part by ADR-0020 and ADR-0026. This decision still stands for the
storage semantics of placing an existing **Asset**: it creates a new board-local
**Placement**, not a duplicate **Asset**. ADR-0026 records the newer rule that
visible pending board rectangles are Placements, not import placeholders.

SnappyRef places **Asset Results** from **Library-wide Search** by reusing import sizing, snapping, placement creation, and canvas integration, while replacing the net-new asset creation step with creation of a new **Placement** for the existing **Asset**. The placement flow creates board-local Placements directly from existing Asset ids.

This is deliberately narrower than saying library placement uses the full import pipeline: it does not decode an external source or create a new **Asset**. The trade-off is a little extra orchestration around existing assets, but it prevents library-result placement from growing separate sizing, persistence, and canvas integration semantics.

## Consequences

- Empty-board placement and multi-result placement should use the same import sizing and compact placement behavior as normal imports.
- Pending library-result placement should be represented by provisional Placements when it needs a pre-drop board rectangle.
- Placing an existing **Asset** should create a new board-local **Placement**, not a duplicate **Asset**.
- A direct placement shortcut needs a new decision if it bypasses shared placement creation or canvas integration.
