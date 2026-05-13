# Keep interaction frames storage-free

SnappyRef interaction should stay responsive while the user pans the Viewport, zooms, or drags Placements and Annotations. These operations update in-memory canvas state every frame, but durability still matters: the Active Board, Viewport, Placement geometry, Annotation geometry/text, Stack Order, and User Session must survive board switches and app restarts.

Before this decision, the main egui frame loop mixed canvas rendering with SQLite reads and writes. Board Summaries were read from the Library every frame for the Boards Tab, and persistence decisions lived in the same app loop that handles pointer input and canvas paint command construction. On Windows, even small storage operations can become visible stutters when they coincide with frequent pan/zoom/drag frames.

## Decision

- Treat pan, zoom, and drag frames as storage-free interaction frames.
- Keep the in-memory canvas and User Session as the immediate source of truth during interaction.
- Cache Board Summaries in app state and refresh them only after board or Placement-count changes.
- Move User Session and Document Dirty State writes behind a Persistence Queue that can write to the Library off the main UI thread.
- Preserve explicit Flush Seams for lifecycle correctness: board switch, app exit, update restart, and destructive board/library actions must wait until queued persistence has completed before continuing.
- Keep Active Board load orchestration aligned with ADR-0002: callers still activate the board in the Library before loading it, but they first flush queued writes for the previous Active Board.

## Consequences

- The UI thread should not perform SQLite reads or writes merely to render the next canvas frame.
- Persistence becomes eventually durable during normal interaction and immediately durable at Flush Seams.
- Persistence errors are reported back to the app from the queue instead of being returned synchronously during ordinary frames.
- Tests should cover observable behavior: Board Summaries stay fresh after board/Placement-count changes, interaction-frame persistence can be queued without immediate storage writes, and Flush Seams make queued changes visible before lifecycle transitions.
- The queue adds lifecycle complexity, but that complexity is real: local-first state must be durable at transitions without sacrificing interaction responsiveness.
