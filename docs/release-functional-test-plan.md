# Release feature list

Every user-facing feature on current main, one row each, for a release QA pass
on macOS and Windows. A computer-use agent can walk it top to bottom, except
the rows under [Human only](#human-only). App updating and the networked
smokes are out of scope.

`Mod` is Command on macOS and Control on Windows. `Alt` is Option on macOS.
Delete is Backspace on macOS. Redo is `Mod+Shift+Z`.

The Covered column names the inspect scenario (`inspect::name`, run with
`docs/skills/inspect/scripts/run-scenario.sh name`) or harness test that checks
every action and result in the row. Those run in `cargo test --locked`, so a
manual pass skips them. A test that checks only part of a row does not count,
and that row stays blank. Rows that depend on the OS, such as Finder or
Explorer drops, the system clipboard, real wheel or trackpad input, and window
stacking, always need a native run.

## Setup

- On macOS, build the QA app with `scripts/build-qa-app-macos.sh`. It prints
  the path of `SnappyRef QA.app`, whose bundle id `dev.snappyref.SnappyRef.QA`
  keeps the pass off an installed SnappyRef. Launch its
  `Contents/MacOS/snappyref` directly so it inherits `SNAPPYREF_DATA_ROOT`.
- Run exactly one instance, with `SNAPPYREF_DATA_ROOT` pointing at a fresh empty
  folder. Never use a real Library.
- Keep the window unoccluded, since eframe pauses an occluded window. Drive all
  input through the computer-use tool.
- Images: PNG, JPEG, GIF (animated), WebP, a large image (4000 px or more), a
  very wide and a very tall image, a transparent PNG, a JPEG with a rotated
  EXIF orientation and one with a mirrored orientation, a corrupt `.png`, and a
  file name with spaces and non-ASCII characters.
- Semantic Search rows need the model already in `<data root>/models/`. Do not
  install it from the network during a QA pass.

## Window and sidebar

| ID | Do | Expect | Covered |
| --- | --- | --- | --- |
| WIN-1 | First launch on an empty root, choose Not Now in "Enable Semantic Search?" | Prompt closes, empty board usable, prompt does not return after restart | |
| WIN-2 | `Mod+B` twice; open it again and click the canvas | `Mod+B` opens and closes the sidebar; a canvas click closes it | |
| WIN-3 | Close the sidebar and look at the left edge | A small pull tab showing the `Mod+B` label sits level with the sidebar top; clicking it opens the sidebar | |
| WIN-4 | Switch the sidebar between Boards and Settings tabs, scroll each | Both tabs reachable, scroll does not pan or zoom the canvas | |
| WIN-5 | Resize the window small, then large; minimize and restore | Canvas redraws, sidebar and cards stay usable, no blank viewport | |
| WIN-6 | Settings > Always on top on, focus another app; toggle off with `Mod+Shift+T` | On keeps SnappyRef above other windows, off restores normal stacking, setting and shortcut agree | |
| WIN-7 | Read Settings > Shortcuts and the version line | Shortcuts use the platform's keys, version matches the build. Do not click update controls | |

## Boards

| ID | Do | Expect | Covered |
| --- | --- | --- | --- |
| BRD-1 | Click "+ New board", then press `Mod+N` | Each creates and activates an empty board with a generated name | |
| BRD-2 | Double-click a board name, type a new name, press Enter; repeat committing by clicking away; try a blank name; type a name and press Escape | Name commits, content unchanged, blank name is not accepted, Escape keeps the old name | |
| BRD-3 | Switch boards by clicking rows and with `Mod+1`..`Mod+9`, `Mod+0` | The matching board activates, row highlight and canvas agree | |
| BRD-4 | Give two boards different pan, zoom and background, switch back and forth | Each board restores its own view and background | `switching_boards_saves_previous_viewport_and_loads_target_viewport` |
| BRD-5 | Hover a board row, click Delete, cancel; repeat and confirm | Dialog names the board, cancel changes nothing, confirm removes it | |
| BRD-6 | Delete a board with images, a group and annotations, then Undo and Redo | Undo brings back the board with content and view, Redo removes it again | `inspect::board-lifecycle-undo-redo`, `board_delete_undo_restores_document_view_history_and_active_fallback` |
| BRD-7 | Try to delete the only board | Deletion refused, board stays usable | `deleting_last_board_is_rejected_and_cancel_preserves_content` |

## Import sources

| ID | Do | Expect | Covered |
| --- | --- | --- | --- |
| IMP-1 | Drag one image from Finder or Explorer onto an off-center spot | One image appears at the drop point and loads; view does not pan or zoom | |
| IMP-2 | Drag several images of different sizes at once | One batch, compacted near the drop, selected; one Undo removes all, Redo restores | |
| IMP-3 | Drag files over the canvas and leave without dropping | Hover preview follows the pointer and disappears; nothing added | |
| IMP-4 | Hover and drop near each window corner with the sidebar open and closed. On Windows, repeat at 100% and a scaled display (150%) | Preview and images land under the cursor at every scale, no fixed offset | |
| IMP-5 | Copy an image in an image editor or browser, `Mod+V` on the canvas | One image with correct pixels | |
| IMP-6 | Copy one or several image files in Finder or Explorer, `Mod+V` | Each file imports once | |
| IMP-7 | Copy a direct `https://` image URL, `Mod+V`; drag a browser image onto the canvas | One image per action | |
| IMP-8 | Copy plain non-URL text, `Mod+V` on the canvas | Nothing added, existing content unchanged | |
| IMP-9 | Import each format: PNG, JPEG, GIF, WebP, TIFF, BMP, SVG, EXR, JPEG XL, AVIF, HEIC, PSD, JPEG 2000 | Each renders correctly proportioned, no failed tile. On Windows, AVIF, HEIC, PSD and JPEG 2000 show as failed, as the README says | |
| IMP-10 | Import JPEGs with rotated and mirrored EXIF orientation | Displayed upright, matching the photo's intended orientation | |
| IMP-11 | Import a corrupt `.png` together with valid images | Valid images load; the corrupt one shows as failed, does not spin forever, can be deleted | |
| IMP-12 | Import a file whose path has spaces and non-ASCII characters | Loads normally | |
| IMP-13 | Import the same file twice, on two boards | Two placements, one Library asset | |
| IMP-14 | Hover an animated GIF, then move the pointer away | Animation plays while hovered or selected and holds its frame otherwise | |
| IMP-15 | Start a large batch, switch to another board right away, come back later | Batch lands only on the first board, complete | |
| IMP-16 | After IMP-13, press `P` (Dedupe library) with no text field focused, then search and switch boards | Status reports the duplicates removed or none found; every board keeps its images, search lists each image once | |

## Canvas navigation

| ID | Do | Expect | Covered |
| --- | --- | --- | --- |
| NAV-1 | Mouse mode: scroll the wheel over an image corner | Zoom centers on the pointer, the corner stays under it | |
| NAV-2 | Middle-button drag, then Alt+drag on empty canvas | Both pan without moving items | |
| NAV-3 | Trackpad mode: two-finger swipe, then pinch | Swipe pans, pinch zooms around the pointer | |
| NAV-4 | Select one image, press Space; repeat with several items and a textbox | View fits the selection | |
| NAV-5 | Double-click an ungrouped image | View focuses that image | |
| NAV-6 | Settings > Canvas background None, Grid, Dots | Background changes on the active board only | |

## Selection and transform

| ID | Do | Expect | Covered |
| --- | --- | --- | --- |
| SEL-1 | Click an item, Shift-click others, marquee drag, Shift-marquee, click empty canvas, `Mod+A` | Selection follows each gesture, nothing moves | `canvas_selection_gestures_preserve_content_geometry`, `command_a_selects_all_active_board_images_and_annotations` |
| SEL-2 | Click and hold on an overlapping image without dragging | Front item gets the hit, no blank flash | |
| SEL-3 | Drag one image, then a multi-selection | Selection moves together, others stay | |
| SEL-4 | Drag an image near another's edge or center; repeat holding `Mod` | Guides appear and the image snaps; `Mod` turns snapping off | |
| SEL-5 | Resize an image by a corner and by an edge handle | Image resizes from the handle and keeps its aspect ratio | |
| SEL-6 | `Mod+Alt`+drag horizontally on a multi-selection | Items and their spacing scale together around the selection, no pan | `inspect_multi_transform_and_layout_contracts_writes_artifact` |
| SEL-7 | Q, W, E on a mixed-size selection, Undo between | Compact, row and column layouts without overlap, aspect ratios kept | |
| SEL-8 | Arrow keys on a scattered selection | Items settle toward that direction with the placement gap | |
| SEL-9 | `Mod+L`, Undo, `Mod+Shift+L` on different-size images | Default average area, then the selection's mean area; aspect ratios kept | |
| SEL-10 | Settings > Placement gap 24, then press Q | Arranged items have a 24-unit gap | |

## Crop and rotate

There is no flip command. Mirrored images only come from EXIF (IMP-10).

| ID | Do | Expect | Covered |
| --- | --- | --- | --- |
| ROT-1 | Hold R on a selected image | Rotation handle appears; hovering it shows a sharp rotate-arrow cursor, not a grab hand or a blurry one on Retina | |
| ROT-2 | Hold R and drag the handle, for one image and a multi-selection | Rotates around the selection center | |
| ROT-3 | Set Rotation snap to 15, rotate; hold Alt mid-rotation | Snaps to 15 degree steps; Alt rotates freely | |
| ROT-4 | Resize a rotated image by a corner | Resize follows the rotated handle | |
| CROP-1 | Hold C and drag a rectangle inside an image, release | Image shows only that region; other placements of the same file unchanged | |
| CROP-2 | Crop an already cropped image, then a rotated one; try a tiny crop and one past the edge | Valid crops match the drawn region; invalid ones leave the image intact, no endless loading | |
| CROP-3 | Undo and Redo a crop | Original returns, then the crop | `crop_can_be_undone_once_and_redone_with_its_asset_intact` |

## Annotations

| ID | Do | Expect | Covered |
| --- | --- | --- | --- |
| ANN-1 | Hold A and drag; hold L and drag; A or L with a click and no drag | Arrow with head at the end point, line without head, no stray segment from the click | `inspect::annotation-editing` |
| ANN-2 | Select an arrow, drag its body, then each endpoint | Body moves the whole arrow; an endpoint moves alone, the other stays | |
| ANN-3 | Change an arrow's S/M/L width and color | Only the selected arrow changes | |
| ANN-4 | T+click, then T+drag, type multiline and non-ASCII text | Both create a textbox, text wraps inside it | |
| ANN-5 | Click away, double-click to edit again, then select and press Enter, then Escape | Text kept, editing resumes both ways, Escape ends editing | |
| ANN-6 | Textbox S/M/L/XL, color swatches, BG; resize and move the box | Toolbar changes the textbox, text rewraps, moving does not edit text | |
| ANN-7 | Settings > Textbox background toggle, create a new textbox | New textbox follows the default, old ones keep theirs | |
| ANN-8 | While editing text, type Q, W, E, G, P, C, R, A, L, T, Delete and `Mod+Z` | Text edits only, no board command fires | |

## Grouping and Group card

| ID | Do | Expect | Covered |
| --- | --- | --- | --- |
| GRP-1 | Select three images, `Mod+G`; repeat with context menu > Group | Group with a fine gold outline and name; Group card appears at the canvas's right edge, vertically centered, Wrap selected | |
| GRP-2 | Group a selection with a textbox in it | Textbox becomes a member; arrows and lines do not | |
| GRP-3 | Click a member, drag | The whole group selects and moves | |
| GRP-4 | Edit the name in the card header, press Enter; edit again and press Escape | Enter updates the name on canvas, blank name rejected, Escape keeps the old name | |
| GRP-5 | Card Row, Column, Wrap, Free buttons | Members reflow in order; Free dims pad, gap and spread | |
| GRP-6 | Click alignment pad cells; pick Between, Evenly, Around, then Off | Members align to the cell; spread modes distribute them, pad shows bands | |
| GRP-7 | Type a Gap and a Padding value, Enter | Spacing changes; invalid text does not apply | |
| GRP-8 | Drag the Gap or Padding icon left and right, release, then `Mod+Z` | Value changes per pixel with a resize cursor, group follows live; one Undo reverts the whole drag | |
| GRP-9 | Type W and H; click a Hug chip | Size fixes the axis; Hug shrinks the frame to its contents | |
| GRP-10 | Clip on, make a member overflow | Overflow hidden | |
| GRP-11 | Resize the group container by its handles; double-click a corner handle | Members reflow; corner double-click hugs | |
| GRP-12 | Make the window short | Card scrolls, all controls reachable | |
| GRP-13 | Double-click a member | Card swaps to the member card: "< Group" back link, "Member N of M", Detach, Fill width, Fill height, Absolute; 2 px gold member outline | |
| GRP-14 | Drag a member inside the group; use arrow keys | Member moves to the insertion slot shown | |
| GRP-15 | Member card Fill width, Fill height, Absolute | Member fills the axis; Absolute takes it out of the flow and dims Fill | |
| GRP-16 | Member card back link, then Escape twice | Back link and first Escape return to the Group card, second Escape clears selection | |
| GRP-17 | Member card Detach; separately drag a member far outside the group | Member leaves at its position, gap closes, Undo restores | |
| GRP-18 | Card Ungroup; then `Mod+Shift+G` and context menu Ungroup on another group | Container removed, members stay in place | |
| GRP-19 | Select loose images, press G, type part of a name, arrows + Enter; repeat via context menu > Add to group… | Picker lists this board's groups; images append to the chosen group | |
| GRP-20 | G, type a new name, Enter; G then Escape; G then Cancel | New group created with the image; cancels change nothing | |
| GRP-21 | Move a member from one group to another with G | Leaves the first, appends to the second, one Undo step | |
| GRP-22 | Delete every member of a group | Empty named group stays and accepts new members | |
| GRP-23 | Duplicate a group, copy it to another board, edit the copy | Copy independent, structure kept | `group_copy_duplicate_delete_ungroup_and_paste_history_preserve_structure` |

## Stack order, copy, paste, duplicate, delete

| ID | Do | Expect | Covered |
| --- | --- | --- | --- |
| ORD-1 | `Mod+.` and `Mod+,` on overlapping items; then context menu Bring to front, Send to back | Selection moves above or below others, relative order kept | |
| ORD-2 | Context menu with nothing selected | Stack order items disabled | `stack_order_context_actions_are_disabled_without_canvas_item_selection` |
| CPY-1 | `Mod+D` and context menu Duplicate on a mixed selection | Offset copies, selected, editable on their own | |
| CPY-2 | `Mod+C` a cropped, rotated image, `Mod+V` on the same and another board; repeat with context menu Copy and Paste | Copies keep crop and rotation | |
| CPY-3 | Copy one SnappyRef image, paste into an external image editor | Pixels arrive | |
| CPY-4 | Copy from PureRef, paste into SnappyRef | Referenced images import once | |
| CPY-5 | Delete, then context menu Delete | Items leave this board only, Library keeps the image | |
| CPY-6 | Context menu > Remove from library…, cancel, then confirm | Cancel keeps all; confirm removes the image from every board and from search | |
| CPY-7 | Open the context menu, dismiss with Escape and with an outside click | Menu closes, nothing underneath fires | |
| CPY-8 | Copy a mixed selection (images, a textbox, an arrow, a group), quit, reopen, `Mod+V` | Every item pastes once with its geometry and the group intact | |

## Undo and redo

| ID | Do | Expect | Covered |
| --- | --- | --- | --- |
| UND-1 | Import, move, rotate, crop, annotate, group; Undo all, Redo all | Each step replays in order | |
| UND-2 | Edit board A, edit board B, Undo on each | Each board undoes its own edits | |
| UND-3 | Undo, make a new edit, Redo | Redo does nothing | |
| UND-4 | Edit, then pan, zoom and change a setting, then Undo | Undo reverts the edit, not the view or setting | |

## Search

Semantic rows need the model installed in the data root.

| ID | Do | Expect | Covered |
| --- | --- | --- | --- |
| SRC-1 | Model missing: `Mod+F` | Palette opens on the install card; Enter does nothing, Esc closes it; ordinary editing still works | |
| SRC-2 | Model installed: `Mod+F`, type a subject, Escape | Relevant results, Escape closes without adding anything | |
| SRC-3 | Search for an image that is only on another board or deleted from all boards | Found; search covers the whole Library | |
| SRC-4 | Select one image, `Mod+F` or context menu Search using image; clear the pivot | Similar images; clearing returns to text search | |
| SRC-5 | Click, `Mod`-click, Shift-click results; arrows and Shift+arrows; arrow past the last row | Highlight follows; more results load | |
| SRC-6 | Drag one result, then several, onto the canvas | Preview follows the pointer, drop adds one batch | |
| SRC-7 | Enter on selected results then click to place; `Mod+Enter` for all | Results follow the cursor, click places once | |
| SRC-8 | Start placing results, press Escape | Nothing added, no Undo entry | |
| SRC-9 | Reopen search, pick a recent search by click and by arrows + Enter | Query restored and run | |
| SRC-10 | Import a new image, wait, search for it | It appears in results | |

## Settings

| ID | Do | Expect | Covered |
| --- | --- | --- | --- |
| SET-1 | Pointer mode Mouse and Trackpad | Hint text updates, navigation follows NAV-1 to NAV-3 | |
| SET-2 | Rotation snap and Placement gap: type 0, max, -4, blank, `abc`, click away | Clamps to range, invalid text restores the previous value | |
| SET-3 | Semantic Search: Remove, cancel, Remove and confirm | Cancel keeps it; confirm removes model availability, boards and images stay | |
| SET-4 | Type shortcut letters and digits into every text field: board name, group name, picker, search, textbox, settings numbers | Text only, no board command | |

## Dialogs

The four dialogs are "Enable Semantic Search?", "Delete "<board name>"?", "Remove asset(s) from Library?" and "Remove Semantic Search?".

| ID | Do | Expect | Covered |
| --- | --- | --- | --- |
| DLG-1 | Open each dialog, drag inside its body | Dialog stays centered, canvas behind does not pan or select | |
| DLG-2 | With a dialog open, click the sidebar and canvas | Clicks blocked until the dialog closes | |
| DLG-3 | Cancel and confirm each | Cancel changes nothing, confirm acts once | |

## Persistence and restart

| ID | Do | Expect | Covered |
| --- | --- | --- | --- |
| PER-1 | Quit after edits on two boards, reopen | Boards, content, groups, view, active board, sidebar state and settings return | |
| PER-2 | After reopening, Undo and Redo | History from before quitting still works | `group_document_history_survives_transfer_delete_restart_and_board_archive` |
| PER-3 | Move the original image file away, reopen | Image still renders from the Library copy | |
| PER-4 | Edit quickly, switch boards, quit right away, reopen | Latest edits kept on the right boards | |
| PER-5 | Set window size and position, quit, reopen | Window returns at the same bounds | |

## Human only

Rows NAV-1, NAV-3 (wheel and trackpad feel), NAV-2, SEL-4, SEL-6 and ROT-3
(held-modifier drags) and CPY-4 and IMP-7 (PureRef and browser drops) need a
person at the machine. So do these:

| ID | Do | Expect | Covered |
| --- | --- | --- | --- |
| HUM-1 | Disconnect the network, launch | App opens, boards usable, no blocking error | |
| HUM-2 | Without `SNAPPYREF_DATA_ROOT`, launch from a folder you cannot write (`Program Files`, a root-owned folder on macOS), import an image; repeat from a writable folder | Protected install keeps data in the platform user-data directory; writable install keeps it beside the app | |
| HUM-3 | On explicit request only: install Semantic Search from the first-launch prompt, the search palette and Settings; cancel one install; Retry after a failed one | Install finishes and search works; cancel leaves it Not installed; Retry recovers | |

See [release operations](release-operations.md) for the update smokes.
