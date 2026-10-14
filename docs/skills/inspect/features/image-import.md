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
- **Browser URL drop on macOS.** Run `run-scenario.sh browser-url-drop`.
  It negotiates a real Cocoa pasteboard containing only `public.url`, extracts
  the native payload, and feeds the accepted URL into the app's drop intake.
  A loopback server serves an extensionless WebP URL with a query string and
  `application/octet-stream`. The frames show the empty board and one loaded
  80x50 image. This covers pasteboard negotiation and app intake, not a live
  browser drag session or the window's native callback delivery.
- **Pasted URL, mixed batch, inactive board.** Run `run-scenario.sh slow-url-inactive-mixed-batch`.
  It serves a slow PNG from a local HTTP server, pastes its URL, then drops a
  valid file, a corrupt file, and a failing URL. ⌘N switches boards while the
  batch loads and ⌘1 switches back. Each live check is recorded in the report,
  so a failed URL, geometry, or inactive-board check keeps its artifact. Frames are `screen-000-url-loading`,
  `screen-001-url-loaded`, and `screen-002-return-to-completed-batch`.
- **Loading appearance.** Run `run-scenario.sh sample-image-import` to see the
  before, loading, and loaded frames. Everything in it is seeded. It shows how
  placeholders look and drives no import path.
- **Literal pixels and fitting.** Run `run-scenario.sh image-format-gallery`.
  It uses generated PNG, BMP, TIFF, WebP and GIF plus checked-in farbfeld,
  HDR, ICO, JPEG XL and EXR. macOS also includes PSD and JPEG 2000.
  Two generated TIFFs prove rotated and mirrored orientation. Each import
  checks literal dimensions and RGBA bytes, then rendered colors within
  texture-filtering tolerance. Space starts from an offscreen viewport and
  must fit the selection without changing its identities or geometry.
- **External contact sheet.** Run `run-scenario.sh external-image-format-gallery`
  with `SNAPPYREF_MEDIA_FIXTURES`. This optional, ignored scenario retains the
  20 external files across 13 types. Its non-SVG pixels and orientation still
  need visual review. Space fitting is asserted from an offscreen viewport.
- **Real preparation.** Run
  `docs/skills/inspect/scripts/mcp-drive.py docs/skills/inspect/scripts/mcp-all-tools.json`
  after a debug build. Its `add_images` call imports two JPEGs by path in the
  real app. A path import goes to the `--imageprep` subprocess first and falls
  back to decoding in the app without a message, so a pass does not prove the
  helper ran. It drives the tool's intake and not drop or paste.

## Gotchas

- Clipboard image paste, clipboard file paths, the context-menu "Paste" row,
  live browser drag delivery, and PureRef have no scenario. A kittest key press of V reads
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
