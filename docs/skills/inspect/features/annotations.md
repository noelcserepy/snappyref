# Annotations

Users draw arrows, lines, and textboxes on a board by holding a key and
dragging or clicking. Textboxes take multiline Unicode text with size, color,
and background options. Lines and arrows take width and color. Each endpoint of
a line or arrow can be dragged on its own.

## Sub-features

- `annot-arrow` and `annot-line` are drawn by holding A or L and dragging. A
  plain click draws nothing.
- `annot-textbox` is drawn by holding T and clicking or dragging. Editing
  starts at once.
- `annot-text-edit` covers double-click or Enter to edit, typing, ⌘A,
  Backspace, and Escape or a click away to commit. A textbox left empty is
  deleted.
- `annot-text-style` covers "S", "M", "L", and "XL", five swatches, and "BG".
  The toolbar shows only while editing.
- `annot-segment-style` covers "S", "M", and "L" widths and the swatches. The
  toolbar shows only when exactly one line or arrow is selected.
- `annot-move-resize` covers a body drag, a textbox edge or corner resize, and
  a drag of a selected line's endpoint.
- `annot-default-fill` is the Settings → General "Fill new textboxes" switch.

## How to get to it (user POV)

- Hold A, L, or T over the canvas and drag or click. Settings → Keys lists them
  as "Draw arrow", "Draw line", and "Draw textbox".
- Double-click a textbox, or select it and press Enter.
- Click a line once to select it, then drag one of its ends.

## Driving it with inspect

Preconditions:

- `doctor.sh` exits 0.

- **Create, style, and edit.** Run `run-scenario.sh annotation-editing`. It
  drives four A-drags, an L-drag, A and L clicks that must draw nothing, the
  "S", "M", and "L" widths, midpoint drags, and T+click with Unicode text. It
  then drives "S", "M", "L", "XL", "BG", Escape, Enter, ⌘A, Backspace, a
  T-drag, a corner resize, a body move, and a double-click edit. Colors are
  applied in code, and no swatch is clicked. The "Fill new textboxes" setting
  is switched on in code before the T-drag. Only the first assertion is a
  live check. The rest are plain asserts. The run checks memory only.
- **Endpoints.** Run `run-scenario.sh annotation-endpoint-drag`. It selects a
  seeded line and a seeded arrow, drags each endpoint, and presses ⌘Z and ⇧⌘Z
  after each drag.
- **Stored textbox.** Run `run-scenario.sh canvas-edit-operations`. It is the
  only scenario that reads an annotation back from the library. It creates the
  textbox with T+click and checks it after a flush.

## Gotchas

- "S", "M", and "L" exist on both toolbars, and L is also the line key. Only
  one toolbar shows at a time.
- The color swatches show no visible names. Their accessibility labels are
  "default", "coral", "gold", "mint", and "sky".
- A selected textbox has no style toolbar until editing starts.
- Dragging the end of an unselected line moves the whole line. Select the line
  first.
- Draw keys are ignored while a text field has focus. Typing T in a textbox
  types a T.
- No scenario flips the "Fill new textboxes" switch in Settings.
- Drawing needs an open library. Without one, creation does nothing.
