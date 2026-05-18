# One active-board load lifecycle

SnappyRef uses one active-board load lifecycle for startup, board switch, and active-board deletion recovery. `board_switch` is orchestration: it persists the current board, updates the **Active Board** in the **Library**, and then invokes the shared load lifecycle; `active_board_load` owns the runtime steps for clearing board-local state, restoring board-scoped session state, and enqueueing existing images.

This keeps board activation, canvas reset, viewport restore, and existing-image loading from becoming separate near-duplicate paths. The trade-off is that the top-level app still wires the host methods, but lifecycle ordering stays explicit and testable instead of spreading across startup, switch, and deletion code.

## Consequences

- Callers must update the **Library** active-board value before invoking active-board load.
- `active_board_load` should not mutate storage or take ownership of search, import, crop, residency, or canvas modules.
- Startup should converge on the same lifecycle as board switching rather than growing a special load path.
