# Group panel: bottom dock

Chosen design for the contextual group controls, from the three prototypes at
https://claude.ai/artifact/6RBqjaZNJdkbJhwMg53nmj (artboards "A · Bottom dock" and
"A · Member editing"). Reference for implementation. Behavior rules live in
`grouping-target-shape.md`.

## Group selected

One floating row, centered at the bottom of the canvas, 52 px tall, warm panel surface, gold
accent for active states. Slides up when a group is selected, slides down when the selection
leaves the group. Left to right:

1. Name (inline editable) and member count.
2. Flow: segmented Row, Column, Wrap, Free with icons.
3. Gap, row gap (Wrap only), padding as icon plus number fields.
4. Align button showing the current nine-position state. Click opens a popover above the
   dock with the nine-position grid and the Auto spacing choices Off, Between, Evenly,
   Around. That popover is the only second level.
5. W and H fields, each showing Hug or the fixed value, each with a hug toggle.
6. Clip toggle.
7. Ungroup.

In Free flow, gap, row gap, align and auto spacing are dimmed.

## Member editing

Same dock, content swapped when the user double-clicks or Cmd+clicks a member:

1. Breadcrumb: group name, member index.
2. W and H, each Fixed or Fill.
3. Absolute toggle.
4. Earlier and later arrows (reorder).
5. Detach.
6. Hint at the far right: Esc leaves member editing.

Add to group keeps its separate palette (`G`).
