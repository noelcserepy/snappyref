# Image import

Users bring images onto the active board by dropping files, pasting, or
dragging from a browser. Each import shows a placeholder at once. The
placeholder fills in when background preparation finishes, and Undo removes the
whole accepted batch.

## Sub-features

- `import-drop-hover` shows provisional placements while files hover over the
  canvas.
- `import-drop` accepts dropped files as one loading batch at the pointer.
- `import-paste` imports from the clipboard. The app checks for board-selection
  JSON first, then an http(s) URL, then file paths, then PureRef data (Windows
  only), then raster image data.
- `import-url` imports an http(s) URL from a paste or a browser drag.
- `import-loading` fills placeholders with images, and marks broken inputs as
  failed instead of leaving them loading forever.
- `import-undo` removes a batch with one Undo, even while it is still loading.
- `import-inactive-board` finishes a batch on its own board after the user has
  switched to another board.

## How to get to it (user POV)

- Drag files from Finder or Explorer over the canvas and release.
- Press ⌘V / Ctrl+V. On Windows, Shift+Insert also pastes.
- Right-click the canvas and choose "Paste". It pastes where the menu opened.
- Drag an image or link from a browser onto the canvas. This works on macOS and
  Windows only.
- Paste a PureRef selection. This works on Windows only.
- An AI client calls `add_images` with file paths or URLs. See
  [AI control](./ai-control.md).

## Driving it with inspect

Preconditions:

- `doctor.sh` exits 0.

- **Drop, hover, and undo while loading.** Run `run-scenario.sh pending-drop-batch-undo`.
  It drives a file hover, a drop, and ⌘Z. Late prep results are injected. The
  assertions to check are "hover creates two provisional Placements", "drop
  accepts one two-Placement Loading batch", "one Undo removes the whole
  accepted batch while work remains pending", "late results drain without
  restoring the undone batch", and "the undone batch stays absent from durable
  Board state". The frames run from
  `screen-000-before-drop` to `screen-004-late-results-discarded`.
- **Pasted URL, mixed batch, inactive board.** Run `run-scenario.sh slow-url-inactive-mixed-batch`.
  It serves a slow PNG from a local HTTP server, pastes its URL, then drops a
  valid file, a corrupt file, and a failing URL. ⌘N switches boards while the
  batch loads and ⌘1 switches back. The real checks are plain asserts that run
  before the summary assertions, so the summary assertions read as passed
  whenever the run passes. Frames are `screen-000-url-loading`,
  `screen-001-url-loaded`, and `screen-002-return-to-completed-batch`.
- **Loading appearance.** Run `run-scenario.sh sample-image-import` to see the
  before, loading, and loaded frames. Everything in it is seeded. It shows how
  placeholders look and drives no import path.
- **Decoded formats.** Run `run-scenario.sh image-format-gallery`. It is
  ignored by default and needs `SNAPPYREF_MEDIA_FIXTURES`. It covers decoding
  and fitting 20 fixture files across 13 file types, not intake.
- **Real preparation.** Run
  `docs/skills/inspect/scripts/mcp-drive.py docs/skills/inspect/scripts/mcp-all-tools.json`
  after a debug build. Its `add_images` call imports two JPEGs by path in the
  real app. A path import goes to the `--imageprep` subprocess first and falls
  back to decoding in the app without a message, so a pass does not prove the
  helper ran. It drives the tool's intake and not drop or paste.

## Gotchas

- Clipboard image paste, clipboard file paths, the context-menu "Paste" row,
  browser drag, and PureRef have no scenario. A kittest key press of V reads
  the real system clipboard, so it is not hermetic. Drive paste with
  `harness.event(Event::Paste(..))`. Only board-selection JSON and an http(s)
  URL are taken from the event text. File paths, PureRef data, and raster
  images always come from the real clipboard.
- Drop and paste do nothing unless the test app has an import worker.
- The app applies at most 4 import results per frame. Step the harness until
  `pending_len()` reaches 0 before asserting loaded state.
- Status text such as "Queued import: …" and "Undid Import images" expires.
  Read it soon after the action.
- Undo of a single-file import shows "Undid Import image". A batch shows
  "Undid Import images".
- Dropped `.pur` files open as a new board instead of placements. See
  [PureRef exchange](./pureref-exchange.md).
