# Groups

Users gather images and textboxes into named groups that lay out their members
as a row, a column, a wrap, or freely. Selecting a group opens a dock card on
the right of the canvas for layout and frame settings. Opening a member shows a
member card for that one item.

## Sub-features

- `group-create` uses ⌘G / Ctrl+G or the context-menu "Group" row. It names
  the group "Group N".
- `group-ungroup` uses ⇧⌘G / Ctrl+Shift+G, the "Ungroup" row, or the dock's
  "Ungroup" button.
- `group-picker` uses a bare G or "Add to group…". The user filters, picks with
  ↑↓ and Enter, or chooses `Create new "{query}"`.
- `group-move-resize` covers dragging the body or the name strip, dragging an
  edge or corner, and ⌘⌥+drag to scale.
- `group-dock` covers the "Group name" field, the flow buttons ("Row",
  "Column", "Wrap", "Free"), the align pad, "Gap" and "Row gap", spread
  ("Off", "Between", "Evenly", "Around"), size with hug, "Padding", and
  "Clip".
- `group-member` uses a double-click or ⌘-click on a member. Its card has
  "Detach", "Fill width", "Fill height", and "Absolute". Arrow keys or a drag
  reorder the member, and dragging it out detaches it.
- `group-history` records group edits as one "Edit board" entry each.

## How to get to it (user POV)

- Select images, then press ⌘G or right-click and choose "Group".
- Press G with a selection to open the picker.
- Click a group to open its dock card. Double-click a member to open the member
  card.
- Escape steps back from the member card to the group card, then clears the
  selection.

## Driving it with inspect

Preconditions:

- `doctor.sh` exits 0.

- **End-to-end group flow.** Run `run-scenario.sh group-operations`. It drives
  a click, a Shift+click, ⌘G, a group drag, "Column", a member double-click, a
  reorder, a drag-out, Undo, Redo, detaching the last member, "Wrap", a resize,
  a scale, the picker's "Cancel", a picker Enter, "Free", and "Clip". It
  checks memory only.
- **Rename and create through the picker.** Run
  `run-scenario.sh group-picker-rename-create`. It drives a click-away rename
  of "Group name", G with "New references" and Escape, then
  `Create new "New references"`, Undo, and Redo. It also checks a narrow
  window. It is the only group scenario that reads groups back from the
  library: "create is one undoable durable edit".
- **Dock controls.** Run `run-scenario.sh group-dock`, `group-dock-narrow`,
  and `member-dock`. These start from a seeded "Faces" group. They drive the
  numeric fields, the align pads, spread "Between", Free dimming, and Wrap
  refusing width hug. On the member card they click "Fill height", "Absolute",
  "H hug contents", and "Detach".
- **Context menu rows.** Run `run-scenario.sh context-menu`. It opens the menu
  on a group selection but clicks no group row.

## Gotchas

- Arrows and lines cannot join groups. Only images and textboxes can.
- The dock card sits on the right, even though one assertion says "bottom
  dock".
- Typing in the picker swallows G, Q, W, E, and ⌘G.
- No scenario drives picker ↑↓ navigation, a name-strip drag, an edge resize,
  "Fill width", or spread "Off", "Evenly", or "Around".
- No scenario drives ⇧⌘G, or reorders a member with the arrow keys.
  `group-operations` reorders by drag.
- No scenario clicks the context-menu "Group", "Ungroup", or "Add to group…"
  rows through to a result.
- Dock-only scenarios seed the group directly, so they say nothing about
  `group-create`.
