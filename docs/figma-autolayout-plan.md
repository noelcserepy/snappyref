# Figma Auto layout observation plan

## Overview

Auto layout is a property of a frame that positions its participating children.
The frame is the container. Its children can be shapes, text, or other frames.
`Horizontal` places a sequence across the frame; `Vertical` places it down the frame.
This document calls that direction the primary axis and the other direction the counter axis.
`Wrap` adds rows to horizontal flow. `Grid` has a separate track and cell model.
`Hug contents`, fixed dimensions, and `Fill container` determine who supplies each dimension.
Gap, padding, alignment, visibility, rotation, and inside strokes can change the resulting layout.
`Ignore auto layout` removes a child from normal flow and exposes position constraints.

This is an observation document with 124 stories for editing before a SnappyRef specification. It does not decide which behavior SnappyRef should copy. No implementation code was changed.

### Evidence and limits

Observed on this Mac in the running Figma desktop app on 2026-09-22, using Computer Use. All edits were confined to the newly created [autolayout-probe](https://www.figma.com/design/Wz8GihsoQrmCmAnRFVFpUG/autolayout-probe). The initial project destination showed a file-limit upgrade dialog. That dialog was canceled, and the file was created in Drafts. Onboarding popovers were dismissed. No existing document, account setting, preference, or plugin was changed.

The tested layout version was `Updated`. Figma also offered `Legacy`. The app build number was not inspected. Measurements below are Figma document units read from the right panel, not screenshot pixels. Figma sometimes displayed rounded values, such as 159.1 instead of 159.099.

Every story below describes an observed result in the stated situation. Menu availability is distinguished from a tested behavioral effect. Examples are successive states of the probe, not a promise that every historical fixture remains intact. The final probe contains frames named `Frame 1` through `Frame 4`, text named `Alpha`, and rectangle layers.

This document covers every requested area, but is not a complete behavioral specification. The final section lists the unverified cases. In particular, failed drag attempts establish no rule about Figma's intended drag behavior.

## Defaults table

There is no single verified universal creation default. The starting selection changed the result. These are observed initial values for specific creation or conversion operations. Values retained from an earlier configuration are identified as retained, not defaults. Short labels such as `Hug`, `Fill`, `W`, and `H` appear in fields; their menus use longer labels.

| Control or property | Observed initial value | Context and qualification |
| --- | --- | --- |
| `Toggle auto layout` | Off on a newly drawn frame, on after `Shift+A` | Empty frame test |
| `Flow` / `Horizontal` | Selected | Two loose, horizontally aligned rectangles; their group; one text layer; empty frame conversion |
| `Flow` / `Vertical` | Selected | Wrapping an existing auto layout frame in another frame with `Shift+A`; initial single-rectangle wrapper |
| `Flow` / `Freeform` | Selected before conversion and after removal | Ordinary frame |
| `Flow` / `Grid` | Not selected | Newly created horizontal or vertical layouts |
| `Wrap` | Off | Horizontal layouts tested; control absent in vertical flow |
| Width / `Hug contents` | Hug, 346 | Two loose 168-wide rectangles with a 10 gap |
| Height / `Hug contents` | Hug, 57 | Same loose-rectangle selection |
| Width and height after empty-frame conversion | Hug, 148 × 86 | Kept the empty frame's existing dimensions |
| Width and height for one text wrapper | Hug, 85 × 49 | Text measured 65 × 29, with 10 padding on all sides |
| Outer width when wrapping an existing frame | Fixed, 320 | Inner frame was 300 wide and became `Fill`; outer side padding was 10 |
| Outer height when wrapping an existing frame | Hug, 77 | Inner frame was 57 high, outer top and bottom padding were 10 |
| `Horizontal gap between objects` | 10 | All horizontal creation examples tested; the loose rectangles already had a 10 gap |
| `Vertical gap between objects` | 10 | Vertical wrappers tested |
| Gap mode | Numeric, not `Auto` | Newly created linear layouts tested |
| `Vertical gap between rows` | Empty field with placeholder `10` | Enabling wrap when the item gap was 10; effective row gap was 10 |
| `Horizontal padding`, `Vertical padding` | 0, 0 | Wrapper around two loose rectangles; conversion of their group |
| `Horizontal padding`, `Vertical padding` | 10, 10 | Single text wrapper, nested-frame wrapper, empty-frame conversion |
| `Individual padding` | Collapsed into two paired inputs | Layouts tested |
| `Left padding`, `Right padding`, `Top padding`, `Bottom padding` | Existing paired values | Expanding individual mode exposed the existing values, initially 10 each in that test |
| `Alignment` | `Align left` | Two equal-height loose rectangles and their group |
| `Alignment` | `Align center` | Single text wrapper |
| `Alignment` | `Align top left` | Empty frame and vertical nested-frame wrapper |
| `Inside stroke` | `Included` | Fresh advanced-settings inspections in `Updated` layouts |
| `Canvas stacking` | `Last on top` | Fresh advanced-settings inspections of linear layouts |
| `Align text baseline` | `Disabled` | Horizontal text layout; the control was disabled in the vertical rectangle test |
| `Auto spacing` | `Between` | Advanced settings; control disabled until gap was `Auto` |
| `Layout` | `Updated` | All fresh settings inspections |
| `Clip content` | Off | Wrappers created around loose objects, text, and an existing frame |
| `Clip content` | On | Newly drawn ordinary frame; remained on after converting that frame |
| `Lock aspect ratio` | Off | Inspected parent and child dimension controls |
| `Add min width…`, `Add max width…` | No limits set | Fresh sizing menus offered the add actions |
| `Add min height…`, `Add max height…` | No limits set | Fresh height menus offered the add actions |
| `Ignore auto layout` | Off | Participating rectangles, text, and nested frames |
| `Horizontal constraints` | `Left` | First enabling `Ignore auto layout` in the absolute-position test |
| `Vertical constraints` | `Top` | Same test |
| Rectangle width / height sizing | Context dependent | Initial single-rectangle wrapper made width `Fill` and kept fixed height; loose-pair conversion kept their sizes |
| Text `Auto width` | Selected | Newly created `Alpha` text |
| Text `Auto height`, `Fixed size` | Not selected | Newly created `Alpha` text |
| Text width / height inside its initial wrapper | Hug / Hug | `Alpha` at 24 px measured 65 × 29 |
| Text typography | `Inter`, `Regular`, 12 px, line height `Auto`, letter spacing `0%` | Newly created text, before the probe changed font size |
| `Toggle automatic positioning` | On | Converting a linear layout to `Grid` |
| Grid dimensions | `1 × Auto` | Conversion from the tested vertical or wrapped single-column arrangement; not established for every source arrangement |
| `Gap between columns`, `Gap between rows` | Retained gaps | One conversion retained 10 and 20; another retained 10 and 10 |
| Grid `Track sizing` | `Fill`, `1fr` | First inspected column and rows after conversion |
| Grid child `Column span`, `Row span` | 1, 1 | Inspected text child |
| Grid child alignment | `Align left`, `Align top` | Inspected text child |
| Container fill | None on object wrappers; white on drawn frame | Observed creation contexts, not an auto layout requirement |
| Stroke / effects | None on tested new wrappers | Added explicitly for appearance tests |
| Added stroke | `Inside`, weight 1, black | Parent and child stroke creation tests |
| Added `Drop shadow` | X 0, Y 4, blur 4, black at 25%; `Show behind transparent areas` off | The effect was explicitly chosen from `Add effect`, not automatically present |

## 1. Creating and removing auto layout

`Shift+A` worked on loose objects, a group, an ordinary frame, text, and an existing auto layout frame. The results depended on the starting object. A loose selection gained a frame. `Group 1` became `Frame 1`, with the right panel changing from `Group` to `Frame`. An existing ordinary frame kept its identity when auto layout was added or removed. Applying `Shift+A` to an already automatic frame created an outer automatic frame.

Two loose rectangles measured 168 × 57 each, with a 10 gap and the same Y position. Their new wrapper measured 346 × 57, at the selection's top-left position, with zero padding. Converting their group produced the same dimensions. These tests do not establish direction inference for diagonal, overlapping, or mixed selections.

The right-panel `Toggle auto layout` removed automatic layout. `Option+Shift+A` also removed it. A tested 370 × 81 frame stayed 370 × 81 after removal, and `Freeform` replaced the automatic direction. Reapplying `Shift+A` produced horizontal flow, the existing 10 gap and 12 padding, with both axes hugging.

- C01. As a user, when I press `Shift+A` with two loose 168 × 57 rectangles arranged horizontally with a 10 gap, then Figma creates a 346 × 57 horizontal frame with both dimensions set to `Hug contents` and zero padding.
- C02. As a user, when I press `Shift+A` with the group containing that same rectangle arrangement, then Figma converts the group to a frame and preserves the measured 346 × 57 bounds.
- C03. As a user, when I press `Shift+A` with the tested 370 × 81 freeform frame, then Figma enables horizontal auto layout on that frame and shows both dimensions as `Hug`.
- C04. As a user, when I turn off `Toggle auto layout` with that populated frame, then Figma keeps the frame at 370 × 81 and changes its flow to `Freeform`.
- C05. As a user, when I press `Option+Shift+A` with that automatic frame, then Figma removes auto layout and keeps its 370 × 81 dimensions.
- C06. As a user, when I press `Shift+A` with an existing auto layout frame measuring 300 × 57, then Figma creates a vertical outer frame measuring 320 × 77 with 10 padding on each side.
- C07. As a user, when I press `Shift+A` with `Alpha` text measuring 65 × 29, then Figma creates a horizontal 85 × 49 hugging frame with 10 padding and centered alignment.
- C08. As a user, when I press `Shift+A` with an empty 148 × 86 ordinary frame, then Figma retains 148 × 86, marks both axes `Hug`, selects horizontal flow, and uses a 10 gap and 10 padding.
- C09. As a user, when I convert that empty frame with `Clip content` already on, then `Clip content` remains on.

## 2. Direction, wrap, and Grid

`Flow` exposed `Freeform`, `Vertical`, `Horizontal`, and `Grid`. `Wrap` was a separate checkbox available with `Horizontal`, not a fifth flow button. Changing a vertical frame with two fill-width rectangles to horizontal placed them beside one another, shared its width between them, and reduced its hugging height from 178 to 94.

Wrap introduced `Vertical gap between rows` in addition to `Horizontal gap between objects`. A controlled case had fixed child widths 79 and 80, heights 74 and 40, a 10 item gap, and 12 padding on every side. Width 193 fit both children in one row and height hugged to 98. Width 192 forced two rows. With row gap 20, the height was 158. At width 180 with the inherited row gap 10, height was 148.

`Grid` exposed `Toggle automatic positioning`, a dimensions picker, separate column and row gaps, and `Open grid settings`. The picker offered numeric `Number of columns`, `Number of rows`, an `Auto` row option, and a matrix of dimension presets. The track editor had `Columns`, `Rows`, `Add column`, `Add row`, per-track sizing and values, and removal buttons. Column sizing offered `Fixed width`, `Hug contents`, and `Fill container`. Initial inspected tracks used `Fill` and `1fr`.

Grid is only partly mapped. Conversion, track controls, and a few changes were observed; a general placement or track-sizing algorithm was not established. A preset labeled `2 × 2` resulted in a displayed `2 columns and 3 rows` in a previously modified manual-positioning fixture. That discrepancy remains unresolved and must not become a proposed rule.

- D01. As a user, when I choose `Horizontal` with the tested two-child vertical frame, then its children appear side by side and its hugging height changes from 178 to 94.
- D02. As a user, when I choose `Horizontal` with an auto layout frame selected, then Figma exposes a separate `Wrap` checkbox.
- D03. As a user, when I enable `Wrap` with horizontal flow selected, then Figma exposes `Vertical gap between rows` alongside the horizontal item gap.
- D04. As a user, when I set width to 193 with fixed child widths 79 and 80, gap 10, and side padding 12, then both children fit on one row and the parent hugs to height 98.
- D05. As a user, when I reduce width to 192 with the same children, gap, and padding, then the second child moves to another row.
- D06. As a user, when I set row gap to 20 with those children on separate rows, then the parent hugs to height 158.
- D07. As a user, when I choose `Grid` with the tested wrapped single-column arrangement, then Figma shows `1 × Auto`, column gap 10, row gap 20, and automatic positioning on.
- D08. As a user, when I open the dimensions picker with the tested Grid selected, then I can see column and row inputs, dimension presets, and `Open grid settings`.
- D09. As a user, when I open `Open grid settings` with the initial Grid conversion selected, then the inspected column and two rows show `Fill` with values of `1fr`.
- D10. As a user, when I choose `Hug contents` with that Grid column selected, then Figma changes its label to `Hug` and reports that both rectangle widths were set to fixed.
- D11. As a user, when I use `Add column` with the existing column set to Hug, then the new column also shows `Hug`, and the two-item Grid changes from two rows to one row.
- D12. As a user, when I open `Auto layout settings` with the tested Grid selected, then I see `Inside stroke` and `Layout`, without the linear-layout stacking, baseline, and Auto-spacing controls.
- D13. As a user, when I select a text child with the tested Grid as its parent, then Figma exposes `Column span`, `Row span`, and separate horizontal and vertical alignment controls.
- D14. As a user, when I turn off `Toggle automatic positioning` with the tested Grid selected, then the checkbox becomes off while the Grid dimensions control remains available. The resulting placement algorithm was not verified.

## 3. Item gap and Auto spacing

The gap field accepts numeric values, negative values, and `Auto`. Its menu showed the current numeric value, `Auto`, and `Apply variable…`. The inspected UI used `Auto` plus `Auto spacing: Between`, rather than a control labeled "space between".

With fixed children in a hugging horizontal frame, changing gap from -20 to 10 increased width from 168 to 198. With two 168-wide children and gap -84, the second child started at x=84 and overlapped the first. Painting order was independently configurable.

Choosing `Auto` on a hugging horizontal parent changed its width to fixed and displayed a conversion message. The nine-position picker became three counter-axis choices, `Align top`, `Align center`, and `Align bottom`. `Auto spacing` in advanced settings offered `Between`, `Evenly`, and `Around`.

In a 400-wide frame with 10 side padding and three children 30, 129, and 30 wide, first-child X was 10 for `Between`, 57.75 for `Evenly`, and 41.83 for `Around`. These measurements support the expected different edge allocations; every interior interval was not separately read.

- G01. As a user, when I replace gap -20 with 10 in the tested hugging frame, then its width increases by 30, from 168 to 198.
- G02. As a user, when I set gap to -84 with two visible 168-wide rectangles in horizontal flow, then the second rectangle starts 84 units after the first and overlaps it.
- G03. As a user, when I choose gap `Auto` with the tested horizontal parent hugging its width, then Figma sets the parent width to fixed and reports that conversion.
- G04. As a user, when I use gap `Auto` with horizontal flow selected, then the alignment picker offers top, center, and bottom counter-axis alignment instead of nine positions.
- G05. As a user, when I open `Auto spacing` with an Auto gap, then Figma offers `Between`, `Evenly`, and `Around`.
- G06. As a user, when I choose `Between` with the 400-wide three-child fixture, then its first child starts at x=10.
- G07. As a user, when I choose `Evenly` with that fixture, then its first child starts at x=57.75.
- G08. As a user, when I choose `Around` with that fixture, then its first child starts at x=41.83 as displayed by Figma.
- G09. As a user, when I set the first child in that fixture to `Fill container` with an Auto gap, then its width becomes 221, its X becomes 10, and the available space goes to the child rather than visible automatic gaps.
- G10. As a user, when I replace that Auto gap with 10, then the fill child's width becomes 201, leaving two 10-unit gaps.

## 4. Padding and input modes

The collapsed controls were `Horizontal padding` and `Vertical padding`. `Individual padding` expanded them into `Left padding`, `Right padding`, `Top padding`, and `Bottom padding`. Collapsing unequal values preserved them and displayed pairs such as `20, 10` and `30, 10`.

Typing one number into a paired input replaced both sides. Setting both paired inputs to 12 produced uniform padding. A separate four-side uniform input was not found or verified. Modifier-key and on-canvas padding editing were not tested.

- P01. As a user, when I open `Individual padding` with horizontal and vertical padding 10, then Figma shows four side inputs each set to 10.
- P02. As a user, when I change only `Left padding` to 20 with all four sides initially 10, then `Right padding` remains 10.
- P03. As a user, when I change top padding from 10 to 30 with a 74-high child and bottom padding 10, then the parent's hugging height becomes 114.
- P04. As a user, when I collapse individual padding with left/right 20/10 and top/bottom 30/10, then the paired fields display `20, 10` and `30, 10` without equalizing them.
- P05. As a user, when I enter 12 into both paired padding inputs with that fixture selected, then all four sides become 12 and the parent measures 192 × 98 with its existing children and gap.

## 5. Alignment and text baselines

The nine labels were `Align top left`, `Align top center`, `Align top right`, `Align left`, `Align center`, `Align right`, `Align bottom left`, `Align bottom center`, and `Align bottom right`. These are the layout picker labels, distinct from ordinary selection-alignment buttons in `Position` and text alignment in `Typography`.

All nine positions were tested in both directions. The fixture was 400 × 200, with 10 padding and gap 10. Its first text child measured 30 × 87 and its second 129 × 58. The table records the first child's coordinates. Horizontal primary alignment moved the 169-wide sequence; vertical primary alignment moved the 155-high sequence. Counter-axis alignment used the selected child's own size.

| Picker choice | Horizontal X, Y | Vertical X, Y |
| --- | --- | --- |
| `Align top left` | 10, 10 | 10, 10 |
| `Align top center` | 115.5, 10 | 185, 10 |
| `Align top right` | 221, 10 | 360, 10 |
| `Align left` | 10, 56.5 | 10, 22.5 |
| `Align center` | 115.5, 56.5 | 185, 22.5 |
| `Align right` | 221, 56.5 | 360, 22.5 |
| `Align bottom left` | 10, 103 | 10, 35 |
| `Align bottom center` | 115.5, 103 | 185, 35 |
| `Align bottom right` | 221, 103 | 360, 35 |

With wrap enabled, width 150, height 300, padding 10, and the same children on separate rows, first-child coordinates were 10,10 for top-left, 60,72.5 for center, and 110,135 for bottom-right. This shows horizontal alignment within rows and vertical alignment of the row collection in this case. Other wrap picker positions were not measured.

`Align text baseline` in `Auto layout settings` had `Disabled` and `Enabled` choices. It was enabled for a horizontal pair of `Alpha` layers at 24 and 48 px. The smaller text's Y became 33 with 10 top padding, and the rendered text baselines lined up. Baseline alignment was also available in the inspected wrapped horizontal layout. Its effect across multiple wrapped rows was not tested.

- A01. As a user, when I choose `Align top left` with the horizontal alignment fixture, then the first child is at 10,10.
- A02. As a user, when I choose `Align top center` with that horizontal fixture, then the first child is at 115.5,10.
- A03. As a user, when I choose `Align top right` with that horizontal fixture, then the first child is at 221,10.
- A04. As a user, when I choose `Align left` with that horizontal fixture, then the first child is at 10,56.5.
- A05. As a user, when I choose `Align center` with that horizontal fixture, then the first child is at 115.5,56.5.
- A06. As a user, when I choose `Align right` with that horizontal fixture, then the first child is at 221,56.5.
- A07. As a user, when I choose `Align bottom left` with that horizontal fixture, then the first child is at 10,103.
- A08. As a user, when I choose `Align bottom center` with that horizontal fixture, then the first child is at 115.5,103.
- A09. As a user, when I choose `Align bottom right` with that horizontal fixture, then the first child is at 221,103.
- A10. As a user, when I choose `Align top left` with the vertical alignment fixture, then the first child is at 10,10.
- A11. As a user, when I choose `Align top center` with that vertical fixture, then the first child is at 185,10.
- A12. As a user, when I choose `Align top right` with that vertical fixture, then the first child is at 360,10.
- A13. As a user, when I choose `Align left` with that vertical fixture, then the first child is at 10,22.5.
- A14. As a user, when I choose `Align center` with that vertical fixture, then the first child is at 185,22.5.
- A15. As a user, when I choose `Align right` with that vertical fixture, then the first child is at 360,22.5.
- A16. As a user, when I choose `Align bottom left` with that vertical fixture, then the first child is at 10,35.
- A17. As a user, when I choose `Align bottom center` with that vertical fixture, then the first child is at 185,35.
- A18. As a user, when I choose `Align bottom right` with that vertical fixture, then the first child is at 360,35.
- A19. As a user, when I choose `Align top left` with the 150 × 300 wrapped fixture, then the first child is at 10,10.
- A20. As a user, when I choose `Align center` with that wrapped fixture, then the first child is at 60,72.5.
- A21. As a user, when I choose `Align bottom right` with that wrapped fixture, then the first child is at 110,135.
- A22. As a user, when I enable `Align text baseline` with the tested 24- and 48-pixel horizontal text pair, then their rendered baselines align and the smaller text has Y=33.
- A23. As a user, when I inspect baseline alignment with the tested vertical rectangle frame selected, then its `Disabled` and `Enabled` controls are unavailable.
- A24. As a user, when I inspect baseline alignment with wrapped horizontal flow selected, then Figma offers both `Disabled` and `Enabled`.

## 6. Container sizing and limits

Width and height menus offered a fixed dimension, `Hug contents`, `Add min width…` or `Add min height…`, `Add max width…` or `Add max height…`, and `Apply variable…`. A nested frame additionally offered `Fill container`. A fixed mode appeared as `Fixed width (346)` or `Fixed height (29)` with the current value in the label.

Hug was measured against gaps, padding, child sizes, rotated bounds, and participating visible children. Empty layouts retained their previous bounds in the tested creation, hiding, and deletion cases. Do not assume that an empty Hug frame is zero-sized.

Minimum width 400 expanded the tested 346-wide Hug frame to 400. In a later fixed-width state with minimum 400, setting maximum to 300 changed the actual width and both limits to 300. The menu then offered `Remove min and max`; choosing it removed those limits. Minimum height 100 expanded the empty 44-high Hug frame to 100.

Dragging the right edge of a 366-wide hugging outer frame made width fixed at 490. Its hugging height stayed 77. The nested fill-width child became 470 wide, accounting for the parent's 10 padding on both sides.

- S01. As a user, when I open the width menu with a top-level auto layout frame selected, then Figma offers fixed width and `Hug contents`, plus minimum, maximum, and variable actions.
- S02. As a user, when I open the width menu with a nested automatic frame selected, then `Fill container` is also available.
- S03. As a user, when I add minimum width 400 with the tested 346-wide Hug frame selected, then its measured width becomes 400.
- S04. As a user, when I set maximum width 300 with fixed width 400 and minimum width 400, then Figma changes width, minimum width, and maximum width to 300.
- S05. As a user, when I choose `Remove min and max` with that constrained frame selected, then Figma removes the two width constraints.
- S06. As a user, when I add minimum height 100 with the empty 44-high Hug frame selected, then its measured height becomes 100.
- S07. As a user, when I drag the right edge with the tested outer frame initially Hug at 366 × 77, then its width becomes fixed at the resulting 490 while height remains Hug at 77.
- S08. As a user, when I resize that outer frame to width 490 with 10 side padding, then its nested Fill child becomes 470 wide.
- S09. As a user, when I open the height sizing menu with the tested automatic frame selected, then Figma offers `Fixed height`, `Hug contents`, `Add min height…`, and `Add max height…`.

## 7. Child sizing and text

Sizing is per axis. Plain rectangles offered fixed dimensions and `Fill container` in their parent context. Text and nested automatic frames also exposed content-based sizing. The existence and behavior of Hug for every other layer type were not tested.

In a 400-wide horizontal frame with 10 side padding, a 129-wide middle child, two Fill siblings, and two 10 gaps, each Fill sibling measured 115.5. A single Fill sibling with the other two fixed at 129 and 30 measured 201. With Auto spacing it measured 221 instead.

Text exposed `Auto width`, `Auto height`, and `Fixed size`. Fresh text selected `Auto width`. Switching the 24-pixel `Alpha` layer to `Auto height` and width 30 produced three displayed lines, `Al`, `ph`, and `a`, with height 87. Making its width Fill allowed it to fit on one line and reduced height to 29. Making its height Fill selected `Fixed size` and filled the 180-high padded interior of its 200-high horizontal parent.

- Z01. As a user, when I create `Alpha` with the text tool in the probe file, then its resizing mode starts at `Auto width`.
- Z02. As a user, when I press `Shift+A` with the 24-pixel `Alpha` text selected, then its child width and height show `Hug` at 65 × 29.
- Z03. As a user, when I choose `Auto height` and width 30 with that text selected, then it wraps onto three lines and height becomes 87.
- Z04. As a user, when I set that text's width to Fill with enough available width, then its text becomes one line and its hugging height becomes 29.
- Z05. As a user, when I make only the first child Fill with the 400-wide numeric-gap fixture, then it measures 201 wide.
- Z06. As a user, when I also make the final child Fill with that fixture, then both Fill children measure 115.5 wide.
- Z07. As a user, when I set text height to `Fill container` in the 200-high horizontal parent with 10 top and bottom padding, then its height becomes 180 and text resizing changes to `Fixed size`.
- Z08. As a user, when I switch the parent to `Vertical` with the same children and sizing modes, then the same child fills width to 380 and receives height 73 from the remaining vertical space in that fixture.
- Z09. As a user, when I enter width 80 and height 40 with the tested rectangle child selected, then its dimension fields show 80 × 40 and it remains in its parent's flow.

## 8. Layout order and canvas stacking

In the tested horizontal frame, `Right` moved the selected child later in the layout sequence; `Left` moved it back. The layer list changed with the layout order. This was reordering, not a one-unit position change. Numeric X and Y fields for participating children were disabled.

`Canvas stacking` was independent of position. Its menu offered `First on top` and `Last on top`. With negative gap and a later blue rectangle overlapping an earlier gray rectangle, `Last on top` painted blue over the shared area. `First on top` painted gray over it. The second rectangle remained at x=84.

Canvas and layer-list drag attempts did not produce a verified reorder. No insertion indicator was captured. Up/down behavior, boundary key presses, and modifier-key variants remain unverified.

- O01. As a user, when I press `Right` with the first child selected in the tested horizontal frame, then it moves after its sibling and its displayed X becomes 235.67 in the 400-wide Evenly-spaced fixture.
- O02. As a user, when I press `Left` with that moved child, then it returns to the earlier position at x=84.33 in that fixture.
- O03. As a user, when I select a rectangle with normal participation in auto layout, then its layout-owned X and Y fields are disabled.
- O04. As a user, when I use `Last on top` with the later blue rectangle overlapping the earlier gray rectangle, then blue paints over the overlap.
- O05. As a user, when I choose `First on top` with that overlapping rectangle fixture, then gray paints over the overlap while the later rectangle's X remains 84.

## 9. Ignore auto layout and constraints

The observed control label was `Ignore auto layout`, not "Absolute position". Enabling it kept the child's current coordinates, made X and Y editable, enabled ordinary alignment controls, and exposed constraints. Initial constraint values were `Left` and `Top`.

The horizontal constraint menu offered `Left`, `Right`, `Left + Right`, `Center`, and `Scale`. The vertical menu offered `Top`, `Bottom`, `Top + Bottom`, `Center`, and `Scale`. Only the horizontal `Right` behavior was measured through a parent resize.

An ignored child at x=84 moved to x=184 when its parent grew by 100 with `Right` selected. Moving that child to y=200 did not increase the parent's 57-high Hug height. Turning `Ignore auto layout` off returned it to flow, disabled position fields, and assigned a layout position. Its reinsertion index was not stable enough across these probes to define a general rule.

- I01. As a user, when I enable `Ignore auto layout` with the tested flowing child selected, then its current position remains and its X and Y become editable.
- I02. As a user, when I enable that control with the initially unconstrained child in the constraint fixture, then Figma shows `Left` horizontal constraints and `Top` vertical constraints.
- I03. As a user, when I open horizontal constraints with the ignored child selected, then I see `Left`, `Right`, `Left + Right`, `Center`, and `Scale`.
- I04. As a user, when I open vertical constraints with the ignored child selected, then I see `Top`, `Bottom`, `Top + Bottom`, `Center`, and `Scale`.
- I05. As a user, when I grow the tested ignored child's parent by 100 with `Right` constraints, then the child's X changes from 84 to 184.
- I06. As a user, when I set y=200 with that ignored child selected, then its parent's hugging height remains 57.
- I07. As a user, when I turn `Ignore auto layout` off with that child selected, then it rejoins flow, its position fields become disabled, and the measured position becomes 0,0 in that fixture.

## 10. Adding, deleting, hiding, resizing, and rotating children

Creating a rectangle inside an empty Hug frame caused the parent to fit the new child. The child measured 37 × 24 at 10,10; the parent became 57 × 44. Duplication, deletion, sizing changes, and visibility changes also recomputed layout.

The appearance control changed from `Hide` to `Show` when a layer was hidden. With the taller rotated child hidden and the 57-high child visible, the parent's Hug height became 57 and the remaining child moved to 0,0. With both children hidden, the parent retained the previous 159.1 height in that test. Deleting the final child of another frame retained its last 57 × 44 bounds.

A 168 × 57 rectangle rotated to 45° retained those local width and height fields. Its layout extent was about 159.1 × 159.1. The parent hugged to height 159.1, and the next child started at x=169.1 for a 10 gap. With center counter-axis alignment, that unrotated 57-high neighbor had y=51.05.

- M01. As a user, when I draw a 37 × 24 rectangle inside the tested empty Hug frame with padding 10, then the rectangle sits at 10,10 and the frame becomes 57 × 44.
- M02. As a user, when I duplicate that child with `Cmd+D` in horizontal flow with gap 10, then its copy starts at x=57 and y=10.
- M03. As a user, when I delete the copy and then the final child with that fixture, then the empty Hug frame retains 57 × 44.
- M04. As a user, when I choose `Hide` with a visible child selected, then its appearance action changes to `Show` and it disappears from the canvas.
- M05. As a user, when I show the 57-high unrotated child with the taller rotated child still hidden, then it sits at 0,0 and the parent hugs to height 57.
- M06. As a user, when I hide every child with the tested Hug frame currently 159.1 high, then the empty visible layout retains height 159.1.
- M07. As a user, when I set rotation to 45° with the 168 × 57 rectangle selected, then its local dimensions remain 168 × 57 and the parent hugs to height 159.1.
- M08. As a user, when I rotate the preceding rectangle with an unrotated child after it and gap 10, then the next child's X becomes 169.1 in this 45° fixture.
- M09. As a user, when I use center counter-axis alignment with the 57-high neighbor beside that rotated rectangle, then its Y becomes 51.05.

## 11. Nesting and circular sizing

Nested auto layout frames kept their internal direction, gap, and child layout while receiving dimensions from an outer frame. The tested outer frame used vertical flow with 10 padding and an inner horizontal frame.

There is no single observed rule that "Fill inside Hug is invalid." Two cases differed. Changing the width of a horizontal parent with two Fill rectangle children to `Hug contents` converted both rectangle widths to fixed and displayed a message. Changing a vertical outer frame's width to Hug while its nested automatic frame had Fill width preserved the child's Fill mode. The inner frame became 346 wide from its contents and the outer frame hugged to 366.

The evidence does not isolate whether the difference depends solely on axis, on the child's ability to size from contents, or on both. A SnappyRef specification must preserve the distinction as an open decision until more cases are tested.

- N01. As a user, when I wrap the tested existing automatic frame with `Shift+A`, then its internal horizontal layout remains inside a new vertical automatic parent.
- N02. As a user, when I choose `Hug contents` for width in the tested horizontal parent with two Fill rectangles, then Figma sets both rectangle widths to fixed and names them in a conversion message.
- N03. As a user, when I choose outer Hug width with the vertical parent containing the Fill-width automatic inner frame, then the inner frame stays Fill at width 346 and the outer frame becomes Hug at width 366.
- N04. As a user, when I set outer width to a fixed 490 with that nested fixture, then the inner Fill frame becomes 470 wide while keeping its own horizontal flow.
- N05. As a user, when I set outer width to 590 with the same nested fixture, then the inner Fill frame becomes 570 wide.

## 12. Strokes, clipping, and effects

The advanced control was `Inside stroke`, with `Included` and `Excluded`. In the tested `Updated` layout, it accounted for the container's inside stroke. The zero-padding parent hugged to height 57 before a stroke, 59 with a 1-unit inside stroke, and 77 with a 10-unit inside stroke. Setting `Excluded` restored height 57.

A 10-unit outside stroke on a child did not enlarge the parent's measured Hug height. This does not establish every combination of parent and child stroke alignment. `Clip content` cropped the tested child's painted overflow without changing layout dimensions.

`Add effect` offered `Inner shadow`, `Drop shadow`, `Layer blur`, `Background blur`, `Noise`, `Texture`, `Glass`, and `Shader`, with `Shader` marked `Beta`. A selected `Drop shadow` left the parent's measured 470 × 57 dimensions unchanged. Other effects were not applied.

- E01. As a user, when I add a 1-unit inside stroke to the zero-padding 57-high Hug parent with `Inside stroke: Included`, then its height becomes 59.
- E02. As a user, when I increase inside stroke to 10 with that parent selected, then the parent's height becomes 77.
- E03. As a user, when I select `Excluded` with that 10-unit inside stroke present, then the parent's height returns to 57.
- E04. As a user, when I add a 10-unit outside stroke with the tested 57-high child selected, then its parent's Hug height remains 57.
- E05. As a user, when I enable `Clip content` with the tested stroked frame selected, then the overflowing child paint is cropped and the parent remains 470 × 57.
- E06. As a user, when I add `Drop shadow` with that frame selected, then its measured layout dimensions remain 470 × 57.
- E07. As a user, when I open `Add effect` with that frame selected, then Figma offers the eight effect types listed above, including `Shader` marked `Beta`.

## 13. Canvas interaction, duplication, and paste

`Cmd+D` duplicated a selected child inside its existing automatic parent and placed the copy after it in the tested horizontal and vertical layouts. Copying with `Cmd+C` and pasting with `Cmd+V` also depended on selection. With a child selected, the pasted text appeared after it. With the parent selected, the pasted text appeared at the end.

The vertical duplicate test used a 74-high child at y=10, gap 10, and padding 10. The copy appeared at y=94 and the parent's Hug height changed from 94 to 178. The horizontal text paste test used children 30 and 129 wide, gap 10, and padding 10. Pasting after the first child placed the copy at x=50. Pasting into the selected parent placed it at x=189.

Several canvas drags of selected text and a rectangle out of their frames left their hierarchy and coordinates unchanged. A layer-list drag also produced no verified reorder. The tool supports an atomic drag, so an in-progress drop indicator was not captured. Dragging in, out, between frames, and to insertion positions remains unverified, rather than classified as unsupported.

- V01. As a user, when I press `Cmd+D` with the selected 74-high child in the tested vertical frame, then its copy stays in the parent at y=94 and parent Hug height becomes 178.
- V02. As a user, when I press `Cmd+D` with the selected 37-wide child in the horizontal fixture, then its copy stays in the parent at x=57.
- V03. As a user, when I copy the first 30-wide text child and paste with that child selected, then the copy appears immediately after it at x=50 in the tested layout.
- V04. As a user, when I paste that copied text with the parent selected instead, then the copy appears after both existing children at x=189.
- V05. As a user, when I press `Backspace` with the pasted child selected, then that child disappears from the parent and its layer list.

## 14. Other controls and submenu inventory

Every submenu in the linear `Auto layout settings` popover was opened: `Inside stroke`, `Canvas stacking`, `Auto spacing`, and `Layout`. The baseline control is a two-choice control, not a submenu. `More info` beside `Layout` was also opened. It described Updated as more consistent with CSS and Legacy as temporarily available during transition. This is Figma's explanatory UI text, not a verified claim of CSS equivalence.

`Layout` offered `Updated` and `Legacy`. No Legacy algorithm behavior was tested. The Grid advanced popover offered the reduced set described above. Other menus inspected were the width and height sizing menus, the gap menu, individual padding, both constraint menus, the Grid dimensions picker, its row-mode menu, and the Grid track sizing menu.

`Apply variable…` entries were observed in sizing and gap menus but not opened. Grid row-value popovers, all track numeric-value submenus, and other conditional submenus were not exhaustively traversed. Thus the broader request to open every possible submenu is only complete for the inspected linear advanced-settings popover, not every conditional right-panel state.

### Verified shortcuts

| Shortcut | Observed action |
| --- | --- |
| `Shift+A` | Add auto layout or wrap an already automatic frame |
| `Option+Shift+A` | Remove auto layout from the selected automatic frame |
| `Cmd+D` | Duplicate selected child in its parent |
| `Cmd+C`, `Cmd+V` | Copy and paste the probe child |
| `Left`, `Right` | Reorder the selected child in the horizontal test |
| `Backspace` | Delete a selected probe child |
| `Cmd+G` | Group the loose probe rectangles before the group-conversion test |
| `Cmd+Shift+G` | Remove the probe frame wrapper and leave its objects selected |

No dedicated shortcut for direction, wrap, Grid, alignment, gap, padding, sizing modes, constraints, or advanced settings was verified. `Return` on a selected frame opened a frame-targeted agent prompt in this app state; that prompt was dismissed. Do not assume it enters the frame's children in this build.

- X01. As a user, when I open `Layout` with advanced settings displayed, then Figma offers `Updated` and `Legacy`, with `Updated` selected in the tested layouts.
- X02. As a user, when I open `More info` beside `Layout` with advanced settings displayed, then Figma shows an explanation of Updated and Legacy plus a `Learn more` link.
- X03. As a user, when I open advanced settings with numeric gap, then `Auto spacing` is present but disabled.
- X04. As a user, when I reopen advanced settings with gap set to Auto, then `Auto spacing` is enabled.
- X05. As a user, when I open a dimension or gap menu with the tested automatic frame selected, then `Apply variable…` is offered.
- X06. As a user, when I press `Return` with the probe frame selected in this app state, then Figma opens an agent prompt targeted at that frame.

## Interactions between areas

- Creation uses the starting selection. Zero-padding multi-object wrappers, padded single-object wrappers, and empty-frame conversion had different outcomes.
- Layout operates on participating children. `Ignore auto layout` removed a child from the measured Hug height; hidden children also stopped contributing when another child remained visible.
- Empty Hug is stateful in the observed cases. Creating an empty Hug frame, hiding all children, and deleting the last child retained previous bounds rather than producing zero.
- Numeric gap and padding contributed to the measured Hug size. A negative gap reduced it and created overlap.
- Canvas stacking changed which overlapping child painted on top without changing its layout coordinates.
- Auto spacing needed a fixed primary extent in the tested Hug-to-Auto operation. Figma changed the parent width to fixed.
- A primary-axis Fill child consumed remaining space before any visible Auto spacing in the measured example. Two Fill siblings shared remaining width equally when a numeric gap was used.
- Fill on the counter axis used the padded interior. Changing direction changed which dimension was shared among siblings.
- Text width affected line count and Hug height. Fill height switched the tested text layer to `Fixed size`.
- Baseline alignment changed text Y positions independently of the ordinary nine-position picker.
- Wrap used available width after padding and item gaps. The exact-fit boundary in the measured case was 193; 192 wrapped.
- Wrap alignment positioned children horizontally within their rows and positioned the row collection vertically in the three measured picker states.
- Rotation used the rotated extent for the measured gap and Hug calculations while preserving local width and height fields.
- Included container inside strokes added to the measured Hug extent. Child outside strokes and the tested drop shadow did not add to it.
- Clipping changed painted overflow without changing the measured dimensions in the test.
- Hug parent plus Fill child is not one universal conversion rule. The rectangle primary-axis case converted to fixed; the nested-frame counter-axis case preserved Fill and resolved from contents.
- Dragging a Hug edge changed that axis to fixed and propagated the new available size into its nested Fill child.
- Conflicting width limits were reconciled when the new maximum was committed. Both limits became the new smaller maximum in the tested order of edits.
- A right-constrained ignored child followed a change in its parent's width while remaining outside the flow calculation.
- Paste insertion depended on whether a child or the parent was selected.
- Grid conversion introduced track sizing and cell controls. Its behavior must not be inferred from the linear frame's alignment and distribution rules.

## Not verified

These are gaps in evidence, not negative claims about feature support. They must remain open in any formal specification derived from this document.

| Area | Not verified | Why |
| --- | --- | --- |
| Creation | Direction inference and sorting for diagonal, overlapping, unevenly spaced, reverse-selected, mixed-type, and multi-parent selections | Only controlled simple arrangements were converted |
| Creation | Exact preservation or movement of every child on each group/frame conversion; retained custom group names | Bounds and resulting controls were read, but every child's before/after coordinates and custom naming were not recorded |
| Defaults | Factory defaults independent of selection and prior edits; persistence of last-used settings | The session was already logged in, and no preferences were reset |
| Direction | Direction-change shortcut and all sizing-mode conversions during every direction change | Only the listed operations and fixtures were exercised |
| Wrap | Oversized children, zero-width parents, heterogeneous min/max limits, wrapped Fill distribution, Auto row gaps, and all line-breaking ties | The measured wrap boundary used two fixed-size children |
| Grid | General row/column placement algorithm, auto versus manual placement, empty cells, track fractions, overflow, fixed/Hug mixtures, track limits, deleting tracks, and dragging cells | Controls were inspected and a few transitions tried, but no complete track model was established |
| Grid | Span changes beyond available rows/columns, and exact dimension-preset behavior | Attempted span edits remained at 1; a `2 × 2` preset in a modified fixture displayed two columns and three rows, without a resolved cause |
| Grid | All numeric track-value and row-value submenus | Only the documented dimensions, row-mode, and track-sizing menus were opened |
| Gap | Minimum accepted negative gap, overlap beyond child width, fractional and expression input rules, and one-child Auto spacing | Negative values were tested only at -20 and -84 |
| Auto spacing | Every interior interval for Between/Evenly/Around; overflow and vertical variants | First-child offsets and the Fill interaction were measured, not the full distribution matrix |
| Padding | Negative padding, expressions, variable binding, on-canvas handles, modifier-key editing, and a separate four-side uniform input | Paired and individual numeric inputs were tested; no separate uniform input was established |
| Alignment | Remaining six wrap picker positions, per-row mixed heights, baseline across wrapped rows, mixed fonts, multiline baseline rules, and non-text baseline fallback | Nine positions were measured in linear flow, three with wrap, and one unequal-size text baseline case |
| Container sizing | Maximum-height enforcement, raising minimum above maximum, Hug plus both limits, fractional rounding, aspect-ratio lock, and resizing from every edge/corner | Minimum height and the stated width-limit conflict were measured; maximum-height entry was not successfully completed |
| Child sizing | Fill with min/max saturation, unequal Fill weights, overflow, every layer type, and content-based sizing of ordinary groups | The tests used rectangles, text, and nested automatic frames |
| Circular sizing | A general rule across axes and child types; choosing Fill after an already hugging parent; multilevel cycles | Two distinct outcomes were observed, but the deciding condition was not isolated |
| Reordering | Canvas drag, layer-list drag, insertion indicator, Up/Down, key behavior at either end, and modifier variants | Drag attempts left hierarchy and coordinates unchanged; only Left/Right in horizontal flow was verified |
| Absolute children | Center, Scale, dual-edge constraints, all vertical constraint behaviors, and reinsertion-index rules | Menus were opened, but only Right was measured through a parent resize; return-to-flow order varied across probes |
| Visibility | Empty-layout behavior with nonzero padding/strokes/limits in every combination, hidden-child export behavior, and visibility shortcuts | The listed empty and nonempty cases were measured only |
| Rotation | Negative angles, 90° and 180°, rotated containers, nested rotations, strokes plus rotation, and min/max interaction | One 45° rectangle case established the measured rotated extent |
| Strokes | Every parent/child combination of Inside, Center, Outside, individual sides, zero width, and rotation | Container inside-stroke inclusion and child outside-stroke non-growth were tested |
| Clipping | Effects crossing a clipped edge, corner-radius clipping, ignored-child clipping, and nested clipping | One visible overflow case was inspected |
| Effects | Layout and clipping behavior of Inner shadow, Layer blur, Background blur, Noise, Texture, Glass, Shader, and large shadow spread | Only Drop shadow was applied; other choices were inspected in the menu |
| Canvas | Dragging children into/out of/between frames, modifier-assisted reparenting, hover delay, and the live drop/insertion indicator | Repeated atomic drags did not yield a verified move; no in-progress pointer state was captured |
| Clipboard | Paste from another file/app, multiple-child paste, paste in place, cut-and-paste, and mixed-parent selections | Only same-file single-child copy, paste, and duplication were tested |
| Advanced | Actual `Legacy` algorithm behavior and equivalence claims about CSS | Legacy was listed but not exercised; the CSS statement was only read from Figma's information popover |
| Variables | `Apply variable…` submenus and bound-value behavior | Entries were observed but not opened or bound |
| Shortcuts | Any shortcut not listed in the verified table | No shortcut inventory or exhaustive keyboard test was completed |
| Numerical limits | Extreme sizes/counts, invalid input, rounding policy, and performance | This was an interaction probe, not a stress test |

Before formalization, retain the concrete examples as acceptance cases, resolve the missing behaviors that SnappyRef intends to support, and explicitly omit the rest. Do not turn an unverified entry into a Figma rule.
