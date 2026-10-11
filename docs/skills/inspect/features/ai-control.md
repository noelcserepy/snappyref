# AI control

An AI client on the same computer reads and edits SnappyRef through MCP tools.
The client starts `snappyref --mcp`, which forwards each tool call to the
running app over a loopback socket. The user picks the access level in
Settings. It is Off until they change it.

## Sub-features

- `ai-control-access` is the Off, Ask, Full choice. Ask and Full publish
  `control.json` in the library root and start the listener. Off removes the
  file and stops it. Quitting the app leaves the file in place.
- `ai-control-launch` is a tool call that finds the app closed. `--mcp` starts
  it without taking focus and waits up to 30 seconds for it to answer. The
  window opens behind the user's other windows, or above them when Always on
  top was left set. With access Off there is no `control.json`, so the call is
  refused and nothing starts.
- `ai-control-tools` is the 19 tools in `src/control/tools.rs`. `get_board`
  returns the board as JSON. `edit_board` applies a batch of edits by id as one
  undo step and returns what it touched. A refused batch applies nothing.
- `ai-control-confirm` covers `remove_from_library` and `search_model`. At Ask
  the app shows its own confirmation dialog, prefixed with "An AI client asked
  for this.", and the call waits up to a minute. At Full they run at once.
  `search_model` asks only when it would start a download or remove an
  installed model.
- `ai-control-mcp-config` is the Copy MCP config button, shown at Ask and Full.
  It copies the client config for this install, with `SNAPPYREF_DATA_ROOT` and
  `SNAPPYREF_LIBRARY_ROOT` when they are set.

## How to get to it (user POV)

- Open the sidebar, Settings, General, and scroll to AI control. Choose Ask or
  Full.
- Press Copy MCP config and paste the JSON into the AI client's MCP settings.
- Ask the AI client to do something on the board. With SnappyRef closed, the
  first tool call starts it in the background.

## Driving it with inspect

Preconditions:

- `doctor.sh` exits 0.

- **Access, edits and confirmation.** Run `run-scenario.sh ai-control`. The
  library asset and the open Settings page are seeded in code. The scenario
  clicks Ask and asserts the endpoint file appears. It then sends calls over
  the real socket from a second thread, the way `--mcp` does: `set_settings`,
  one `edit_board` batch that adds a label, two rectangles and an arrow and
  groups three of them through refs, `set_view`, `undo`, `redo`, and
  `remove_from_library`. The removal waits on the dialog, and the scenario
  clicks Remove. The sidebar is reopened in code, then clicking Off is driven.
  The last assertion checks that the endpoint file is gone. Frames are
  `screen-000-access-off`, `screen-001-access-ask`,
  `screen-002-ai-edited-board`, `screen-003-user-confirms`, and
  `screen-004-access-off-again`.
- **Every tool against the real app.** Build with
  `cargo build --locked --bin snappyref`, then run
  `scripts/mcp-drive.py scripts/mcp-all-tools.json`. Full access is seeded by
  editing the session row after a first launch. Driven are the launch from
  `--mcp`, the stdio handshake, the token, and a call to each of the 19 tools
  at Full, with real image preparation and a real `.pur` export and import.
  Each assertion says only that a call answered or was refused. Read the
  replies in `events.ndjson` for what changed. `screenshot` calls are skipped
  unless you add `--front`. The artifact is
  `target/snappyref-inspect/mcp-drive/run-<nanos>/`, with the exported
  `board.pur` beside the report.

## Gotchas

- `screenshot` works only while part of the window shows on screen. The app
  does not draw a minimized or fully covered window, and a window `--mcp`
  started opens behind the others. `mcp-drive.py --front` is the only route
  that captures one. It takes focus for about 6 seconds.
- No scenario launches the app from `--mcp` or speaks stdio. `control::mcp`
  unit tests cover the handshake, the tool list, and the token check, and
  `mcp-drive.py` covers the launch.
- Eleven tools are called by `mcp-drive.py` alone, and only at Full:
  `list_boards`, `get_settings`, `create_board`, `rename_board`, `open_board`,
  `delete_board`, `select`, `screenshot`, `export_board`, `search_library`,
  and `search_model`. The last two are only refused there, because the search
  model is not installed.
- Never driven anywhere: `search_library` with the model installed,
  `search_model` starting an install, `search_model` waiting on its dialog at
  Ask, the Copy MCP config click, two apps on one library root, the app dying
  mid-call, and the Windows launch.
- The scenario's "refuses calls" check sends nothing once `control.json` is
  gone, so it proves the file was removed and not that the listener stopped.
- `add_images` and image adds through `edit_board` need an import worker, which
  the scenario does not start. It uses annotations instead.
  `add_images_answers_from_its_board_after_the_user_opens_another` in
  `src/app/tests/control.rs` sends `add_images` over the socket with a worker.
- A tool reply is sent before the app draws the result, and a board edit
  reaches the library through a write queue the reply does not wait for.
  Neither route proves a tool edit reached the disk. For that, read the
  library back after a flush in a scenario.
- `mcp-all-tools.json` uses literal ids. On a fresh root the first board is 1,
  and each new board, item, or group takes the next number from one shared
  sequence. Asset ids follow import completion order, so they can swap between
  runs.
- A waiting call only advances while the harness steps. Send the call from a
  thread and step until it answers.
