# Document history

SnappyRef uses an active-board **Document History** for undo and redo of board document edits. Undo and redo apply changes to the in-memory active canvas in place instead of reloading the whole board.

This replaces the earlier crop-only undo behavior. Reference-board work involves repeated small adjustments: imports, paste and duplicate, deletion, transforms, arrange, stack order, crop/relink, annotations, and board lifecycle edits. Treating those as document edits gives the user one predictable undo model while keeping the **Library** and **User Session** concepts separate.

## Decision

- Keep undo/redo scoped to the **Active Board** document.
- Record reversible **History Entries** for board-local document edits, including Placement and Annotation changes, Stack Order changes, crop/relink operations, and board lifecycle edits that affect the active document surface.
- Apply undo/redo in place to the active canvas so the board does not flash or fully reload for ordinary history navigation.
- Keep **Document History** separate from **User Session** changes such as sidebar state, viewport state, and Workspace Settings.
- Persistence still follows ADR-0023: history application mutates in-memory document state, marks **Document Dirty State**, and relies on the **Persistence Queue** plus **Flush Seams** for durability.

## Consequences

- Commands that mutate the active board should either create a meaningful **History Entry** or explicitly document why they are not undoable.
- History capture must preserve user-authored geometry, selection-relevant document state, and relative stack order well enough for repeated undo/redo to feel stable.
- Undo/redo should not depend on reloading the active board from storage.
- Future cross-board or library-wide undo would need a separate decision because this history is intentionally active-board document history, not global app history.
