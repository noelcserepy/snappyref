# Grouping target shape

The agreed shape of SnappyRef groups. It copies the parts of Figma auto layout that make it
feel direct, keeps the parts of SnappyRef grouping that a reference board needs and Figma
lacks, and drops nesting, Grid and design-only content rules. Story ids such as `S07` refer
to `figma-autolayout-plan.md`. Guarantee ids such as `append-stable` refer to
`.specs/spatial-layout/spatial-layout.yml` and the other spec modules.

Decisions taken on 2026-09-22 are listed at the end. Everything else is the argument and the
resulting rules. Edit freely; the next step is plan-with-quint.

## What SnappyRef has today

- A Group is a named, flat, board-local, ordered list of image Placements with a container
  rect, one flow (`Wrap`, `Row`, `Column`), one spacing, one padding. No nesting. No
  annotations as members. A Placement belongs to at most one Group.
- Wrap is the default. Width is always fixed. Height is always content-driven.
- `Cmd+G` groups the selection. `G` opens a searchable picker; Enter appends the selection to
  the highlighted group, the last destination is preselected, typing a new name creates a
  group. Context menu has Group, Ungroup, Add to group.
- Adding appends at the end and never moves existing members (`append-stable`). An incoming
  image wider than the usable width scales down to fit (`incoming-fit`). Nothing else ever
  resizes a member.
- Container resize is width only, from the bottom-right handle or the Width field. Members
  reflow but keep their size and may overflow. `Cmd+Alt` drag scales container, members,
  spacing and padding together.
- Click on a member selects the group. Double-click enters member editing. Dragging a member
  reorders it by drop center. Dropping its center outside the container detaches it. Escape
  returns to group selection.
- Empty groups keep their name, config and size and stay valid destinations. Ungroup keeps
  member geometry. Duplicate and copy/paste keep structure with new ids.
- Search results, imports and pastes are never captured by a group (`group-intake-explicit`).
- Controls: a floating panel with Name, Row/Column/Wrap, Spacing, Padding, Width.
- Textboxes are fixed rects drawn by the user; text wraps inside the rect.

## Why Figma feels good

The feel comes from a small number of rules, not from the size of the feature.

1. The container fits its content unless you tell it otherwise, and you tell it by
   grabbing an edge. Dragging a Hug edge makes that axis fixed (`S07`). No mode switch first.
2. Every change reflows immediately. Add, delete, resize, rotate: the layout is always the
   function of current members and config (`M01`, `M07`, `V01`).
3. Order is a first-class thing you can see and nudge. Arrow keys move a child one slot
   (`O01`, `O02`). Position fields are disabled for flowing children (`O03`) so there is only
   one way to think about where a child is.
4. New things land where you are looking. Duplicate and paste insert after the selected child,
   or at the end when the parent is selected (`V01` to `V04`).
5. Alignment is one nine-position picker that covers both axes.
6. Fill and Auto spacing are the two ways leftover space gets used, and they compose in one
   fixed order: Fill members eat it first, Auto spacing distributes what is left (`G09`).
7. Gap, padding and alignment are the only numbers a user has to touch.

## Why SnappyRef should not be Figma

A reference board is a flat collection of images that grows over time. That changes the
priorities.

- Growth by addition, not by drawing. In Figma you drag a layer into a frame. That gesture is
  fiddly enough that a computer use agent could not complete it (see "Not verified", Canvas).
  On a board with 300 images and 12 groups, "select, `G`, type three letters, Enter" is faster
  and works from any zoom. The picker is the primary add path. Drag-in is the secondary one.
- Names matter. Figma frames have names but you never search them. SnappyRef groups are
  destinations, so naming is part of creation, not an afterthought.
- Incoming fit is the right default for images. Figma lets a 4000 pixel paste blow up the
  frame. A reference group should absorb whatever you throw at it (`incoming-fit`), and never
  touch what is already there (`incoming-fit-isolated`).
- Flat beats nested. Every hard Figma rule in the plan comes from nesting: Fill inside Hug
  converts or does not depending on axis (`N02`, `N03`), min and max clamp each other
  (`S04`), a hugging outer frame propagates width to a Fill inner frame (`S08`). None of it
  buys anything for a mood board. Keep `group-flat-membership`. Without nesting, Fill has one
  clean rule: it only exists on a Fixed container axis.
- Explicit intake. A remembered destination must not swallow the next paste. Keep
  `group-intake-explicit`. Paste-after-member below is still explicit: selecting a member
  inside a group is the signal.
- Uniform scale as a gesture. Figma scales with a separate tool. `Cmd+Alt` drag on a group
  is better for images and already exists. Keep it.

## Proposed shape

### Data model

```
Group {
  id, name,
  members: ordered [Member],
  container: rect,
  flow: Row | Column | Wrap | Free,
  width: Hug | Fixed(f32), height: Hug | Fixed(f32),
  gap: Number(f32) | Auto(Between | Evenly | Around),
  row_gap: f32,                        // Wrap only
  padding: f32,
  align: (Start | Center | End) x (Start | Center | End),
  clip: bool                           // crop paint outside the container
}

Member {
  item: PlacementId | AnnotationId(textbox),
  width: Fixed | Fill, height: Fixed | Fill,
  absolute: bool                       // out of flow, positioned inside the container
}
```

Constraints on the model, enforced at edit time so layout never sees an invalid group:

- Wrap forces `width: Fixed`. Free ignores every layout field except padding for the
  container bounds.
- `gap: Auto` forces the primary axis `Fixed` (`G03`). Switching that axis back to Hug turns
  gap into a number (the last numeric value).
- A `Fill` member axis requires the matching container axis to be `Fixed`. Setting the
  container axis to Hug converts every Fill member on that axis to Fixed at its current size
  (`N02`). This is the one Figma conversion rule kept, because it is the only one without
  nesting.
- Images keep their aspect ratio. Fill on one axis of an image sets the other axis from the
  ratio. Fill on both axes of an image means fit inside the available cell: the largest size
  that fits both dimensions, placed by the alignment picker. Textboxes have no ratio and
  stretch each axis independently.
- An absolute member ignores flow, Fill, gap and alignment. It has a position relative to
  the container's top-left, may sit partly or fully outside the container, moves with the
  group, scales with uniform scale, and does not contribute to Hug (`I06`).
- `clip` crops paint outside the container without changing any layout result (`E05`). It
  applies to overflowing fixed members and to absolute members alike. Off by default. The
  group's hit area is always the container.

Defaults: Row and Column hug both axes. Wrap is `width: Fixed(creation width),
height: Hug`. Members start `Fixed`, not absolute. Gap and row gap start at the shared
placement gap. Alignment starts at top-left. Clip is off.

### Creation

- `Cmd+G` and the picker's "Create new" stay as they are. A new group hugs the selection
  bounds plus padding, in Wrap. Selected textboxes become members; lines and arrows stay
  loose. (`C01` with SnappyRef defaults.)
- Wrap is the default flow. Figma's Horizontal default (`C08`) is for UI rows.

### Adding and removing

- Picker, context menu, `G`, last-destination highlight, transfer between groups, re-add
  no-op: unchanged.
- Drag a loose image or textbox over a group and drop it. Insert at the slot nearest the
  drop center, same rule as reorder. Picker stays the primary path.
- With a member selected in member editing, `Cmd+D` duplicates after it inside the group
  (`V01`, `V02`) and `Cmd+V` of an image pastes after it (`V03`). With the group selected,
  paste appends (`V04`). With nothing group-related selected, paste lands on the board.
- Remove closes the gap. Empty groups persist. Unchanged.

### Layout

- Row, Column, Wrap keep `ordered-not-packed` and `append-location`. `append-stable` holds
  only with numeric gap and Start alignment; Auto spacing or other alignment recomputes
  positions on every change, including add.
- Gap may be negative. Later members paint over earlier ones (`G02`, `O04`).
- Wrap has its own row gap (`D03`, `D06`).
- Auto spacing: `Between`, `Evenly`, `Around` on a Fixed primary axis (`G05` to `G08`). With
  Auto gap the alignment picker reduces to the counter axis (`G04`). Fill members consume
  leftover space before Auto spacing distributes any (`G09`). In Wrap, Auto spacing applies
  within each row.
- Fill: on a Fixed container axis, leftover space after fixed members, gaps and padding is
  split equally among Fill members (`Z05`, `Z06`). Fill never enlarges a container.
- Rotated members occupy their rotated bounds (`M07`, `M08`). Already true through
  `visual_bounds`.
- Alignment: nine positions. Counter-axis alignment is what matters for images of different
  heights in a Row. Primary-axis alignment only has an effect when that axis is Fixed and
  content is smaller. In Wrap, alignment applies inside each row and to the row stack.
- Free flow: no layout. Members keep manual positions inside the container. Moving the group
  moves them. Uniform scale scales them. Each axis is Hug or Fixed like every other flow: Hug
  fits the members' bounds plus padding and follows members as they move, Fixed keeps the
  user size. This is Figma's `Toggle auto layout` off (`C04`) as a fourth flow, not a second
  kind of group. Switching Free to another flow lays members out in their current member
  order; switching to Free keeps the positions the layout produced.
- Absolute members: the equivalent of `Ignore auto layout` (`I01`, `I06`, `I07`). Toggle
  per member in member editing. Turning it on keeps the member where it is. Turning it off
  reinserts the member at the slot nearest its center, same rule as drag reorder, which
  resolves the ambiguity Figma left open. No Figma constraints in v1: an absolute member is
  anchored to the container's top-left. Typical use is a caption sitting over or beside a
  specific image.
- Textbox members: fixed rect like today, text wraps inside. Fill on width stretches the
  rect. No auto width or auto height modes (`Z01` to `Z04` not copied); the user still draws
  the box.

### Sizing

- Each container axis is Hug or Fixed. Dragging any edge or corner sets that axis Fixed at
  the dragged size (`S07`). A Hug button per axis in the controls, and double-click on the
  handle, returns it to Hug. This replaces the width-only handle and the Width field.
- Fixed smaller than content: fixed members overflow, never shrink
  (`fixed-member-overflow`). Fill members shrink to zero leftover but not below.
- Incoming fit applies to a non-absolute member with both axes Fixed entering Row, Column
  or Wrap with Fixed width. Hug width grows instead. Free and absolute never fit. A member
  added as Fill takes its size from the layout.
- No min and max. No nesting.

### Order and editing

- Member editing by double-click, Escape to leave, drag to reorder, drop outside to detach:
  unchanged. Dragging an absolute member moves it freely inside the container instead of
  reordering.
- `Cmd+click` on a member selects it directly without the double-click (Figma deep select).
- Arrow keys in member editing move the selected member one slot earlier or later (`O01`,
  `O02`). Left/Right in Row and Wrap, Up/Down in Column. Wrap also accepts Up/Down as "same
  column, previous or next row" when such a slot exists.
- An insertion indicator while dragging a member, drawn at the slot the drop would take.
- Paint order inside a group follows member order. Later paints over earlier. Absolute
  members paint above flowing members.

### Whole-group actions

- Move, `Cmd+Alt` uniform scale, duplicate, copy/paste, ungroup, delete: unchanged. Uniform
  scale also scales absolute member offsets.
- Ungroup keeps geometry for every member, flowing or absolute.

### Controls

Group panel: Name, flow (Row, Column, Wrap, Free), gap with an Auto menu (Between, Evenly,
Around), row gap in Wrap, padding, the nine-position alignment picker, W and H each with a
Hug toggle, a Clip toggle. Drop the Width field.

Member panel in member editing: W and H each Fixed or Fill, an Absolute toggle. The picker
for adding stays a separate palette.

### Shortcuts

| Action | Keys | Status |
| --- | --- | --- |
| Group selection | `Cmd+G` | existing |
| Ungroup | `Cmd+Shift+G` | existing |
| Add to group | `G` | existing |
| Uniform scale | `Cmd+Alt` drag | existing |
| Enter member editing | double-click member | existing |
| Deep select member | `Cmd+click` | new |
| Move member earlier / later | arrows in member editing | new |
| Duplicate after member | `Cmd+D` in member editing | new |
| Paste after member | `Cmd+V` in member editing | new |
| Hug an axis | double-click handle, or button | new |
| Toggle absolute | button in member panel | new |

Figma's `Shift+A`, `Option+Shift+A` and `Shift+arrow` gap nudges are not needed. SnappyRef
has one concept where Figma has group, frame and auto layout frame.

## Not copied, and why

| Figma | Reason |
| --- | --- |
| Nesting (`N` stories) | Flat groups. Every hard sizing rule disappears with it. |
| Min and max (`S03` to `S06`) | No use on a board. Fixed or Hug is enough. |
| Constraints on absolute members (`I02` to `I05`) | Anchored top-left in v1. Add if resizing groups with captions turns out to matter. |
| Hidden children (`M04` to `M06`) | SnappyRef has no hide. Revisit if it gets one. |
| Text auto width and auto height (`Z01` to `Z04`) | Textboxes are user-drawn rects. |
| Strokes and effects (`E01` to `E04`, `E06`, `E07`) | Not a design tool. |
| Grid (`D07` to `D14`) | Unverified in Figma and Wrap covers the board case. |
| Per-side padding (`P` stories) | One value. |
| Separate canvas stacking (`O04`, `O05`) | Member order owns paint order. |
| Frame conversion (`C02` to `C06`) | One concept, not three. |

## Decisions taken

1. Default flow: Wrap.
2. Alignment: nine positions.
3. Negative gap: allowed.
4. Padding: one value.
5. Member sizing: per member Fixed or Fill per axis, Figma model.
6. Drag a loose item into a group: in v1.
7. Paste and duplicate after the selected member: yes.
8. `Cmd+click` deep select: yes.
9. Paint order: owned by member order.
10. Free flow: yes, as a fourth flow.
11. Textboxes as members: yes, plus a per-member absolute toggle so a member can sit outside
    the flow but inside the group.
12. Auto spacing: Between, Evenly, Around on a Fixed primary axis.
13. Fill on both axes of an image: fit inside the cell, ratio kept.
14. Absolute members may sit outside the container. A per-group Clip toggle crops paint
    outside the container, off by default.
15. Free flow has Hug or Fixed per axis like every other flow.
16. Append stability yields to layout: Auto spacing and non-Start alignment may move
    existing members on add.
17. Wrap row breaks treat Fill members as zero width; Fixed members decide the breaks, then
    each row's leftover is split among its Fill members.
18. Transfer keeps member intent: absolute stays; Fill on an axis the destination hugs
    becomes Fixed at the source size.
19. Incoming fit only for all-Fixed, non-absolute members entering a Fixed-width flow.
20. Forbidden edits (Hug width on Wrap, Fill on a hugging axis) are refused, never
    auto-fixed by changing another setting.

## Figma stories this keeps

`C01`, `C04` (as Free flow), `C09`, `D03` to `D06`, `E05`, `G01` to `G10`, `P03`, `P05`, `A` stories,
`S07`, `S09` (Hug and Fixed only), `Z05` to `Z08` (images keep ratio), `N02` (as the only
conversion rule), `O01` to `O03`, `I01`, `I06`, `I07`, `M01` to `M03`, `M07` to `M09`,
`V01` to `V05`.
