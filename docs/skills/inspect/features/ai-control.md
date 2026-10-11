# AI control

An AI client on the same computer reads and edits SnappyRef through MCP tools.
The client starts `snappyref --mcp`, which forwards each tool call to the
running app over a loopback socket. The user picks the access level in
Settings. It is Off until they change it.

## Sub-features

- `ai-control-access` is the Off, Ask, Full choice. Ask and Full publish
  `control.json` in the library root and start the listener. Off removes the
  file and stops it.
- `ai-control-tools` is the 19 tools in `src/control/tools.rs`. `get_board`
  returns the board as JSON. `edit_board` applies a batch of edits by id as one
  undo step and returns what it touched.
- `ai-control-confirm` covers `remove_from_library` and `search_model`. At Ask
  the app shows its own confirmation dialog, prefixed with "An AI client asked
  for this.", and the call waits up to a minute. At Full they run at once.
- `ai-control-mcp-config` is the Copy MCP config button. It copies the client
  config for this install, with the data root override when one is set.

## How to get to it (user POV)

- Open the sidebar, Settings, General, and scroll to AI control. Choose Ask or
  Full access.
- Press Copy MCP config and paste the JSON into the AI client's MCP settings.
- Ask the AI client to do something on the board. With SnappyRef closed, the
  first tool call starts it.

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
  The last assertion checks that the endpoint file is gone and a new call
  fails. Frames are `access-off`, `access-ask`, `ai-edited-board`,
  `user-confirms`, and `access-off-again`.

## Gotchas

- The scenario skips the stdio half. `control::mcp` unit tests cover the MCP
  handshake, the tool list, and the token check. No scenario launches the app
  from `--mcp`.
- `screenshot` has no scenario. The harness does not answer
  `ViewportCommand::Screenshot`.
- `add_images` and image adds through `edit_board` need an import worker, which
  this scenario does not start. It uses annotations instead.
- `search_library`, `search_model`, `export_board`, `create_board` with
  `pureref`, and Copy MCP config are not driven.
- A waiting call only advances while the harness steps. Send the call from a
  thread and step until it answers.
