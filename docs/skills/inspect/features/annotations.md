# Annotations

Users draw labels, arrows, lines, and rectangles on a board with one key
each. T places a label under the pointer. A, L, and S arm a draw mode that the
next drag uses up. A label is a compact chip that sizes itself to its text, except along an axis
a Group fills. A
colored label is filled with its color and takes dark text. A rectangle is a
bordered region with no fill, a soft tint, or a flat color. Lines and arrows
have round caps, and an end that sits on a label stops at the label's edge.
Stroke and border weights are world units, so they zoom with the images. Each
endpoint of a line or arrow can be dragged on its own.

## Sub-features

- `annot-arrow` and `annot-line` are drawn by pressing A or L, then dragging.
  The press that starts the drag uses the mode up, so a plain click draws
  nothing and disarms. The same key again, or Escape, also disarms. While a
  mode is armed the key row has a "Drag" key chip labelled "Arrow", "Line",
  or "Rectangle", then "Color", "Size", "Fill" for a rectangle or "Head" for
  an arrow or line, and "Cancel". H swaps an armed arrow and line.
- `annot-label` is placed by pressing T. It lands under the pointer, or at the
  canvas center when the pointer is elsewhere. Editing starts at once.
- `annot-rectangle` is drawn by pressing S, then dragging. Only its edge takes
  a press until it is selected, so the image under it stays reachable. A
  solid rectangle takes a press anywhere.
- `annot-text-edit` covers double-click or Enter to edit, typing, ⌘A,
  Backspace, Shift+Enter for a new line, and Enter, Escape, or a click away to
  commit. A label left empty is deleted.
- `annot-style` is keys only. With annotations selected, 1 to 5 set the
  color, Shift+1 to 5 set a label's text size or the width of a stroke or a
  rectangle's border, H switches a stroke's head, and F steps a rectangle's
  fill through none, tint, and solid. With nothing selected, or with a draw
  mode armed, the color, size, and fill keys set the next annotation's style,
  and every change carries into the next one. A
  key row shows bottom-center while annotations are selected. After each key
  a HUD replaces it for about a second with the full scale and the new value
  lit. Both move to top-center when the selection sits under them.
- `annot-move-resize` covers a body drag, a rectangle edge or corner resize,
  and a drag of a selected line's endpoint.

## How to get to it (user POV)

- Press T over the canvas, or press A, L, or S and drag. Settings → Keys lists
  them as "Place label", "Draw arrow", "Draw line", and "Draw rectangle".
- Double-click a label, or select it and press Enter.
- Click a line once to select it, then drag one of its ends.
- An AI client adds and restyles labels, rectangles, arrows, and lines through
  `edit_board`. See [AI control](./ai-control.md).

## Driving it with inspect

Preconditions:

- `doctor.sh` exits 0.

- **Create, style, and edit.** Run `run-scenario.sh annotation-editing`. It
  drives four A-drags, an L-drag, A and L clicks that must draw nothing,
  Shift+digit widths, 1 to 5 colors, H head toggles, midpoint drags, and a T
  press with Unicode text. It then drives Escape, Shift+digit text sizes, Enter, ⌘A,
  Backspace, an S-drag, a corner resize, a body move, and a double-click edit.
  Each live check is recorded in the report. A failing check keeps the
  later observations and frames, then marks the report as failed.
- **Text sessions and newlines.** Run `run-scenario.sh annotation-text-sessions`.
  It edits a seeded label from "before" to "hello" in five typing frames,
  commits with Enter, and drives one Undo and Redo, including after reopening
  the library. It types "first", Shift+Enter, and "second", commits with Enter,
  reads "first\nsecond" back after a flush, and undoes the whole session.
  Finally it exits with an unfinished edit and checks the saved text and
  batched history after reopening. Clearing a label and committing removes
  it; one Undo restores its original text.
- **Board of real images.** Run `run-scenario.sh annotation-board`.
  It seeds six readme-board images, places labels with T, draws arrows
  from labels and rectangles with S, and colors them with number keys. Frames
  are resting, selected arrow, selected rectangle, label editing, and zoomed
  out.
- **Style keys and HUD.** Run `run-scenario.sh annotation-style-keys`.
  Same board. Frames show the key row for a label, an arrow, a rectangle, a
  mixed selection, and a crowded arrow, the typing row, and the HUD for next
  color, text size, arrow color, head off, fill, mixed size, and a selection
  under the strip, which moves the HUD to the top. The last frame is the key row
  with arrow mode armed.
- **Endpoints.** Run `run-scenario.sh annotation-endpoint-drag`. It selects a
  seeded line and a seeded arrow, drags each endpoint, and presses ⌘Z and ⇧⌘Z
  after each drag.
- **Stored label.** Run `run-scenario.sh canvas-edit-operations`. It creates
  a label with a T press and reads it back from the library after a flush.
  `pureref-import` is the only other scenario that reads an annotation back.

## Gotchas

- T places the label where the pointer last was. Hover the point first.
- Holding A, L, or S through the drag still draws, because the press arms the
  mode.
- Size keys match the physical digit key, because Shift+digit is a different
  character on every layout.
- Style keys type text while a label is being edited. Press Escape or Enter
  first. The label stays selected.
- The HUD lasts about a second at 60 steps a second. Capture it within 60
  steps of the key, and wait it out before a frame that should show the key
  row.
- Two clicks within egui's double-click interval of an earlier click read as a
  triple click. Let a few frames pass before a scripted double-click.
- Dragging the end of an unselected line moves the whole line. Select the line
  first.
- Draw keys are ignored while a text field has focus. Typing T in a label types
  a T.
- Drawing needs an open library. Without one, creation does nothing.
