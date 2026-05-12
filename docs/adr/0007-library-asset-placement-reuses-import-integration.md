# Library asset placement reuses import integration

Superseded in part by ADR-0020. This decision still stands for the storage
semantics of placing an existing **Asset**: it creates a new board-local
**Placement**, not a duplicate **Asset**. ADR-0020 records the newer
preview-first placement contract shared by imports and Asset Result drops.

SnappyRef places unplaced **Asset Results** from **Library Scope** search by reusing import reservation, placeholder, sizing, snapping, queue, and load-result integration, while replacing the net-new asset creation step with creation of a new **Placement** for the existing **Asset**. The placement flow creates the board item with `create_board_item_from_asset`, then loads it through `LoadExisting` using the same queue id so normal placeholder cleanup and canvas integration apply.

This is deliberately narrower than saying library placement uses the full import pipeline: it does not decode an external source or create a new **Asset**. The trade-off is a little extra orchestration around existing assets, but it prevents library-result placement from growing separate sizing, placeholder, persistence, and canvas integration semantics.

## Consequences

- Empty-board placement and multi-result placement should use the same import sizing and reservation behavior as normal imports.
- Library-result placement should show and clean up import placeholders through the same queue-id mechanism as normal imports.
- Placing an existing **Asset** should create a new board-local **Placement**, not a duplicate **Asset**.
- A direct placement shortcut needs a new decision if it bypasses reservation, placeholder cleanup, or load-result integration.
