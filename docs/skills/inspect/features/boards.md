# Boards and navigation

A library holds several boards, and one is active at a time. Users create,
rename, switch, and delete boards from the sidebar or with shortcuts, then pan
and zoom the canvas. Board lifecycle edits can be undone. The open board, its
view, and the sidebar state come back after a restart.

## Sub-features

- `board-create` uses the "New board" row or ⌘N / Ctrl+N. It names the new
  board "Board N" with the smallest free N, starting at 2.
  Dropping a `.pur` file also creates a board. See
  [PureRef exchange](./pureref-exchange.md).
- `board-rename` uses a double-click on the name and the "Board name" field.
  Enter or a click away commits, and Escape discards.
- `board-switch` uses a click on a `Board {name}` row or ⌘1…⌘9 and ⌘0.
- `board-delete` uses a hover, then "Delete", then the dialog
  `Delete "{name}"?` with "Cancel" and the red "Delete board". Deleting the
  active board activates a fallback board. Deleting another board leaves the
  active one open.
- `board-history` covers ⌘Z and ⇧⌘Z across create, rename, and delete.
- `nav-pan-zoom` covers the mouse wheel zoom, middle-drag, Alt+drag, and Space
  to focus the selection.
- `sidebar` covers ⌘B and the "Boards" and "Settings" tabs. Settings has the
  "General" and "Keys" pages.
- `session-restore` brings back the board, placements, zoom, pan, and sidebar
  state on relaunch.

## How to get to it (user POV)

- Press ⌘B / Ctrl+B or click the "Toggle sidebar" tab.
- Use the "Boards" tab rows, the "New board" row, and hover → "Delete".
- Press ⌘N, ⌘1…⌘0, and ⌘Z / ⇧⌘Z.
- On the canvas, use the wheel, a middle-drag, an Alt+drag, and Space.
- Quit and relaunch the app.
- An AI client calls `list_boards`, `create_board`, `rename_board`,
  `open_board`, `delete_board`, and `set_view`. See
  [AI control](./ai-control.md).

## Driving it with inspect

Preconditions:

- `doctor.sh` exits 0.

- **Lifecycle with history.** Run `run-scenario.sh board-lifecycle-undo-redo`.
  The open sidebar is seeded in code. It clicks "New board", "Delete board
  Lifecycle Board", and the dialog's "Delete board". It then presses ⌘Z and
  ⇧⌘Z through the whole history. The rename is a direct action and not a
  double-click. "final lifecycle replay is durable" reads the boards and the
  active board back from the library.
- **Navigation and shortcuts.** Run `run-scenario.sh canvas-navigation-boards`.
  It drives the wheel, a middle-drag, an Alt+drag, Space, ⌘B, ⌘N, and ⌘1.
  The selection that Space focuses is set in code. "both Boards are durable"
  checks the stored list.
- **Delete dialog safety.** Run `run-scenario.sh delete-board-modal-flash`.
  Enter must not confirm, and Escape must keep the board. The board list is
  read back from the library.
- **Sidebar pages.** Run `run-scenario.sh sidebar-pages`. It renders the
  "Boards", "General", and "Keys" pages with long board names.
- **Restart.** Run `run-scenario.sh session-restart-restore`. It drives a drag,
  a zoom, and ⌘B on the first board, then creates and edits a second board.
  A second app instance reopens the same library with the second board active.
  Shortcuts switch to both boards and verify their distinct edits and views.
- **Board tools in the real app.** Run
  `docs/skills/inspect/scripts/mcp-drive.py docs/skills/inspect/scripts/mcp-all-tools.json`
  after a debug build. It creates, renames,
  opens, and deletes a board through the tools, and lists the boards before
  and after. It drives no sidebar row, shortcut, or dialog.

## Gotchas

- Rename by double-click has no scenario.
- Switching by clicking a board row has no scenario. `sidebar-pages` only
  hovers a row, and `canvas-navigation-boards` switches with ⌘1.
- `session-restart-restore` saves and flushes explicitly before restarting.
  The process-exit flush itself is covered by
  `real_startup_and_exit_keep_pending_content_session_and_history`.
- The last board cannot be deleted.
- Deleting a board archives it. When it was the active board, the app moves
  to the previous board in the list, or the next one when the first board was
  deleted.
- The `delete_board` tool deletes without the dialog, at Ask as well as Full.
- ⌘N leaves the sidebar as it was. The "New board" row keeps it open.
- Each board has its own Undo history. Undoing a create or delete switches the
  active board.
- In Mouse mode the wheel zooms. In Trackpad mode scrolling pans. Scenarios use
  the default, Mouse mode.
- A change to the view or the sidebar alone saves the session 750 ms later. A
  kill inside that window loses it. A board edit saves the session at once.
