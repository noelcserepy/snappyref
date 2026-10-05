# Canvas editing

Users select images on the canvas and edit them in place. They can move,
scale, rotate, crop, duplicate, restack, arrange, delete, and undo. Moves
debounce into one "Edit board" history entry, and every edit persists to the
library.

## Sub-features

- `edit-select` covers click, Shift+click, marquee, ⌘A, and click-and-hold.
- `edit-move` covers dragging one image or the whole selection.
- `edit-scale` covers corner handles and ⌘⌥+drag / Ctrl+Alt+drag.
- `edit-rotate` covers holding R and dragging the handle, with 15° snapping
  that Alt bypasses.
- `edit-crop` covers holding C and dragging. It creates a derived asset.
- `edit-duplicate` and `edit-stack` cover ⌘D, ⌘. (bring to front), and ⌘,
  (send to back).
- `edit-arrange` covers Q (compact), W (row), E (column), arrow-key settles,
  ⌘L, and ⇧⌘L.
- `edit-delete` covers ⌫ on macOS and Del on Windows.
- `edit-history` covers ⌘Z and ⇧⌘Z (Ctrl+Z and Ctrl+Shift+Z). Ctrl+Y does
  nothing.
- `edit-context-menu` covers the right-click rows "Copy", "Paste",
  "Duplicate", "Delete", "Group", "Ungroup", "Add to group…", "Bring to front",
  "Send to back", "Search using image", and "Remove from library…".
- `edit-persist` keeps the edit when a library write fails and retries it.

## How to get to it (user POV)

- Click or drag images on the canvas. Drag on empty canvas for a marquee.
- Use the shortcuts above. Settings → Keys lists them.
- Right-click an image or empty canvas for the context menu.

## Driving it with inspect

Preconditions:

- `doctor.sh` exits 0.

- **Most edits in one run.** Run `run-scenario.sh canvas-edit-operations`. It
  drives a click, a drag, ⌘D, ⌘Z, ⌘A, W, ⌘, and ⌘., an R+drag, a ⌘⌥ drag, a
  T+click textbox, and Delete. Its last assertion reads the library after a
  flush: "flush persists the edited Board: one rotated orange, no blue, one
  textbox". Frames run from `screen-000-seeded` to `screen-007-deleted`.
- **Multi-selection transforms and layouts.** Run
  `run-scenario.sh multi-transform-layout-contracts`. It covers multi-drag,
  multi-scale, rotation snapping with the Alt bypass, a scale released outside
  the canvas, W and E at gaps 0 and 24, the four arrow settles, ⌘L, and ⇧⌘L.
  The real checks are plain asserts that run before the summary assertions.
- **Delete, Undo, Redo.** Run `run-scenario.sh delete-undo-redo-placement`.
  "explicit flush makes final Redo durable" proves the stored state.
- **Crop.** Run `run-scenario.sh crop-undo-redo-placement`. A real C+drag
  creates a derived asset through the real prep worker. It then checks one
  Undo, a second Undo that must do nothing, and Redo. Look for "pointer crop
  creates one derived Asset" and "explicit flush preserves the cropped Asset
  relation".
- **Compact (Q).** Run `compact-equal-size-images`,
  `compact-mixed-column-images`, and `compact-windows-column-images`. Each
  seeds a selection and drives Q, then compares `screen-000-before-compact`
  with `screen-001-after-q-compact`.
- **Click-and-hold selection.** Run `run-scenario.sh placement-click-hold-flash`.
  It checks that the selection border stays visible through 8 held frames.
- **Context menu.** Run `run-scenario.sh context-menu`. It opens the menu,
  checks that it flips placement near the corner, and clicks only "Remove from
  library…", which must ask for confirmation.
- **Write failure.** Run `run-scenario.sh persistence-write-retry`. It makes
  the library refuse writes, drags an image, waits for "durable write failed",
  then recovers.

## Gotchas

- Marquee selection and scaling an image by its corner handles have no
  scenario. `canvas_selection_gestures_preserve_content_geometry` in
  `src/app/tests/inspect_operations.rs` checks the marquee without an artifact.
- Copy, Paste, Duplicate, Delete, Group, and front/back have no scenario that
  runs them from the context menu. Only "Remove from library…" is clicked.
- Copy fires only on `Event::Copy`. A kittest ⌘C key press does not copy.
- C, R, A, L, and T are hold keys. Hold them with `key_down` before the
  pointer press.
- A drag needs held steps. The app waits 90 ms and a small distance before a
  move starts.
- Q, W, E, the arrow keys, and ⇧⌘L need 2 or more images selected.
- Selection shortcuts do nothing while a text field, modal, search overlay, or
  group picker has focus. Undo, Redo, and paste check only for a modal.
- Crop runs in the background. Step the harness until it finishes.
- `inspect_world_to_screen` is only correct at the default pan and zoom.
