# Release functional test plan

Status: planned, not executed. Prepared on 2026-09-15 against commit
`6be5d4b4ede7151852e27191e0e250923bba61fc`.

Exercise the shipping app on macOS and Windows before the next release.
Cover ordinary user interactions, their cancellation and history behavior,
and recovery after restart. App updating is excluded, including update
discovery, downloads, installation, rollback, and update smokes.

Use the [budgeted execution plan](release-test-execution.md) to run this
catalog. It assigns deterministic checks to code and combines native checks
into four workflows per platform, coordinated through a small set of subagents.
The 128 cases below are coverage requirements, not 128 separate LLM tasks.

## Execution agreement

- This document plans the run. It does not launch the app, publish a release,
  or authorize the real networked model-install scenario. App updating remains
  excluded.
- Use existing tests and inspect assertions for deterministic behavior. Use
  native computer use for OS integration and representative complete workflows,
  following the user's explicit request over the usual harness preference in
  [inspect](skills/inspect/SKILL.md).
- Record code evidence and native evidence separately. A matching test name is
  not proof of every expected result in a catalog row. Uncovered assertions
  remain gaps until tested.
- Run the deterministic suite on macOS and Windows. Run native checks on each
  platform where tools or a human operator are available. Use
  [use-windows](skills/use-windows/SKILL.md) for Windows setup. A macOS result
  does not establish native Windows behavior.
- Pin each candidate's commit, archive name/hash, displayed version, OS,
  resolution, display scaling, and input devices. Use shipping default features
  and the release profile. Do not fix code during a verification run.
- Use disposable data. Close the app before copying its entire data root,
  including SQLite sidecars if present. Never alter the user's working Library.
- Seed prerequisites directly or restore a closed-app fixture copy when setup
  is not under test. Perform the tested action through its assigned interface.
  Do not replace a failed native interaction with a database edit and call it
  a native pass.

## Prepare the run

### Candidate and data roots

The maintainer prepares absolute paths for the following before handing a
workflow to the operator. Keep fixtures and reports outside tracked source.

| Name | Contents and use |
| --- | --- |
| `candidate` | Extracted release archive, including the macOS updater helper even though updating is out of scope. |
| `fresh` | Empty disposable data root for first launch and missing-model behavior. |
| `working` | Disposable root for native workflows, seeded from a known fixture when board creation is not under test. |
| `baseline` | Closed-app copy of a known test workspace, restored before independent or destructive cases. |
| `existing` | Copy of a representative v0.2.0 workspace, with recorded board content, settings, and history. Open the candidate directly on this copy, without invoking the updater. |
| `reports` | One subdirectory per candidate and OS, with report and full-window screenshots. |

Use `SNAPPYREF_DATA_ROOT` to isolate ordinary functional runs. Confirm the
launch environment actually reaches the app process. Run the installed-location
checks separately because an override would hide portable-root and fallback
behavior. Read [local data location](../.specs/local-data-location/local-data-location.yml)
before that setup.

For a true first launch from a downloaded archive, also exercise the documented
macOS Open Anyway or Windows SmartScreen flow. Record any manual intervention.
A locally built binary does not provide this evidence.

### Fixture pack

Prepare these once. Record filenames, dimensions, expected appearance, and
provenance in a fixture manifest. Use self-created or redistributable content.
Do not commit personal images, model downloads, or performance corpora.

| Set | Required contents |
| --- | --- |
| Geometry | Six visibly distinct numbered images, covering square, portrait, landscape, very wide, very tall, and transparency. Put asymmetric corner labels on one image for rotation and crop checks. Include an exact byte copy under another filename. |
| Formats | JPEG, PNG, WebP, GIF, TIFF, BMP, SVG, OpenEXR, JPEG XL. Add AVIF, HEIC, PSD, and JPEG 2000 for macOS system-decoder coverage. Include upright reference renders for EXIF-oriented JPEG and TIFF, including a mirrored orientation. |
| Animation | A small animated GIF and animated WebP with visibly different numbered frames. Record their expected animation. |
| Intake failures | A corrupt file with a supported extension, unsupported text file, empty file, and a valid image with spaces and non-ASCII characters in its path. |
| Slow intake | A batch large enough to observe loading, plus a controllable slow image URL and a failing image URL. The maintainer supplies a reachable fixture server; do not depend on arbitrary live websites. |
| Search | About 20 distinct images spanning recognizable subjects, with several related images per subject and written query expectations. The current palette loads five results per page, so this exercises More without a large corpus. Do not demand an exact ranking from the model. |
| Clipboard | Native image editor, file manager, browser page exposing a direct image URL, and PureRef with a small board made from the fixture pack. Missing PureRef means that case is blocked. |
| Existing data | Closed-app copy from the last release containing multiple boards, overlapping items, annotations, nondefault settings, and undoable edits. Include groups in the new-candidate baseline because the old release may not support them. |

Preinstall a copy of working model assets into the ordinary search test root
if a verified local copy is available. Test missing-model behavior in another
root. A live model download, cancellation, and retry pass needs explicit
execution authorization. A fake installer only establishes UI behavior.

### Operator preflight

1. Read the computer-use tool documentation and select the candidate window.
2. Prove screenshots, clicks, double-clicks, right-clicks, typing, scrolling,
   and dragging work. Prove a drag can hold a letter key and modifier keys,
   then release all keys and buttons. Do not assume a shortcut tool can hold
   `C`, `R`, `A`, `L`, or `T` throughout a drag.
3. Check cross-app drag support, middle-button input, wheel-notch events,
   high-resolution scrolling, and real pinch gestures separately. Mark
   unavailable actions for a native human or another capable operator.
4. Use fresh screenshots to locate canvas targets. Read accessible labels
   where available, but expect some egui canvas content to need visual targeting.
5. Dismiss onboarding with Not Now for the ordinary run. Open the sidebar,
   inspect Settings and the displayed shortcuts, and confirm the isolated root.

## How to run each case

`Mod` means Command on macOS and Control on Windows. `Alt` means Option on
macOS. Delete means Backspace on macOS and Delete on Windows. Redo is
`Mod+Shift+Z`, not an assumed platform shortcut.

For every assigned check, establish the precondition, execute, and compare
against its expected result. Use the execution plan's code batches and native
checkpoints. Reuse one observation for all assertions it actually establishes;
do not repeat the action for each catalog ID.

Record PASS, FAIL, BLOCKED, NOT RUN, or platform-specific N/A, plus evidence
mode and the exact assertion covered. A row is fully passed only when all its
applicable assertions have evidence. Partial coverage remains explicit.

Code should assert Undo/Redo permutations, geometry, counts, identities, and
persistence. Native workflows include a representative Undo/Redo sequence and
restart instead of repeating them after every edit. A native-only failure gets
at most one clean retry. Save its state, then continue independent checks.

Use bounded waits for asynchronous work. Initial operator limits are 10 seconds
for UI settling, 120 seconds for fixture import, and five minutes for initial
model readiness. These are execution limits, not performance promises. Prefer
process waits and scripted state checks where supported to repeated LLM polling.
A timeout is not a pass.

## 1. Launch, window, and sidebar

Start with `fresh`, then use `working`. No installed model required.

| ID | Actions | Expected result |
| --- | --- | --- |
| WIN-01 | Launch the candidate with an empty root. Choose Not Now in Enable Semantic Search. | Usable empty board; prompt closes; core editing works. Fresh Pointer mode is Mouse and Placement gap is 0. |
| WIN-02 | Quit normally and reopen the same root. | Not Now persists; no repeated onboarding prompt. |
| WIN-03 | Toggle sidebar with `Mod+B` and its visible toggle. Switch Boards and Settings tabs and scroll their contents. | Both routes work; content and controls remain reachable; canvas input does not leak through the sidebar. |
| WIN-04 | Resize to a small usable window and then a large window. Move, minimize, and restore it. | Canvas redraws; sidebar, dialogs, and toolbars remain usable; no lost input or blank viewport. |
| WIN-05 | Turn Always on top on in Settings. Focus another normal app. Toggle off with `Mod+Shift+T`, then repeat through Settings. | On keeps SnappyRef above ordinary windows; off restores ordinary stacking; both controls agree. |
| WIN-06 | Set a distinctive window size/position and sidebar tab/visibility. Quit and reopen. | Usable window bounds and saved sidebar state return. |
| WIN-07 | Read app version and shortcut list. Hover/click ordinary controls. | Version matches candidate; shortcuts use the running platform; no clipped text, flashing selection, or moving click targets. Do not activate update controls. |

## 2. Board lifecycle

Use two clearly different populated boards, A and B, and a disposable board C.

| ID | Actions | Expected result |
| --- | --- | --- |
| BRD-01 | Create a board with the sidebar create control, then another with `Mod+N`. Undo/redo creation. | New boards activate with empty content; names/list remain coherent; history restores the correct board. |
| BRD-02 | Double-click a board title, rename it, and commit with Enter. Repeat with click-away. Try a blank name. | Valid names commit without losing content; blank input cannot leave a nameless unusable board. Undo/redo restores the name. |
| BRD-03 | Switch using rows and `Mod+1` / `Mod+2`. Create enough temporary boards to exercise `Mod+9` and `Mod+0`. | Shortcut selects the corresponding list position; active indicator and content agree. |
| BRD-04 | Give A and B different pan, zoom, and backgrounds. Switch repeatedly, including with the sidebar hidden. | Each board restores its own view and background; content does not move between boards. |
| BRD-05 | Hover a board row and request deletion. Cancel. Reopen and confirm for inactive C, then for the active disposable board. | Dialog identifies the target; cancel changes nothing; confirm removes the correct board and leaves a usable active board. |
| BRD-06 | Delete a board containing images, annotations, and a group. Undo, then redo, then undo again. | Content, group order/configuration, view, and history return with the board. Library images remain available. |
| BRD-07 | In a disposable root, delete the last board. | App leaves a usable board state; no crash, dead canvas, or stale deleted-board selection. Record the actual replacement-board behavior. |

## 3. File and URL intake

Start each subsection on an empty board. Imports must not pan or zoom the
view. There is no current file-picker control in the app UI; do not invent
an import menu because internal contracts mention file-picker intake.

| ID | Actions | Expected result |
| --- | --- | --- |
| IN-01 | Drag one PNG from Finder/Explorer onto an off-center canvas location. | One placement appears near the drop, resolves visibly, and keeps the viewport unchanged. |
| IN-02 | Drag several different-size files together. | One coherent batch, sensible initial sizing and layout, no duplicate native/framework deliveries. One Undo removes the batch and Redo restores it. |
| IN-03 | Hover a dragged batch over the canvas, move it, then leave without dropping. Repeat and drop. | Preview follows the pointer; abandoned preview disappears without history/content; accepted drop creates one batch. |
| IN-04 | Drop near different corners with sidebar open/closed and a moved/resized window. Repeat under Windows display scaling. | Drop coordinates follow the actual cursor; no fixed offset or center-only placement. |
| IN-05 | Paste a direct image URL copied from the browser. Drag that browser image onto the canvas. | Supported URL intake produces one placement, remains responsive, and preserves the viewport. Record native browser-drag availability separately on Windows. |
| IN-06 | Drop an exact copy under another filename, then import it onto another board. | Each action creates a placement; the shared Library reuses the exact image. Confirm one asset result in search or supplemental storage evidence. |
| IN-07 | Import the path containing spaces and non-ASCII characters. | File resolves with correct pixels and no path parsing error. |
| IN-08 | Import the corrupt image, empty file, unsupported file, and failing URL individually, then mix a corrupt image with valid files. | Invalid intake is ignored/rejected or shown as a failed placement as applicable; valid files still resolve. Accepted failures are visible, do not spin forever, and can be removed. |
| IN-09 | Start a slow batch, move a loading placement, and navigate while it resolves. | UI remains usable; completion updates existing placements without duplicating them. Record any final batch layout adjustment. |
| IN-10 | Start a slow batch on A, immediately switch to B, then return after completion. | Batch belongs only to A; final content/layout is complete even though A was inactive. |
| IN-11 | Start a slow batch, Undo before completion, wait, then Redo. Repeat by deleting a pending item before it completes. | Late results do not resurrect undone/deleted content. Redo resolves the intended batch with one history step. |
| IN-12 | Import a local file, wait until loaded, close the app, move only the original fixture out of its source directory, then reopen. | The Library's managed copy still renders on all boards. Restore the source fixture afterwards. |

Windows browser URL drag has a documented limitation when the framework owns
the drop target. Record that limitation, and still require Explorer file drops
to work. It is not permission to accept lost file drops.

## 4. Decoding and image display

Use the fixture manifest's reference appearance. Run each common format on
both platforms rather than treating one successful PNG as format coverage.

| ID | Actions | Expected result |
| --- | --- | --- |
| IMG-01 | Import JPEG, PNG, WebP, GIF, TIFF, BMP, SVG, OpenEXR, and JPEG XL individually. Zoom in/out. | Every supported format renders recognizable, correctly proportioned content. No blank/failed tile for a valid fixture. Record each format separately. |
| IMG-02 | On macOS, repeat with AVIF, HEIC, PSD, and JPEG 2000. | System-decoder formats render against the references. Windows is N/A for this case. |
| IMG-03 | Import rotated and mirrored EXIF JPEG/TIFF fixtures. Duplicate, crop, and reopen them. | Visible orientation matches the upright references throughout. |
| IMG-04 | Inspect transparency, SVG edges, tiny images, and a large high-resolution image at multiple zooms. | Transparency and proportions are correct; higher-detail content resolves without identity/position changes. No persistent blank image after navigating away and back. |
| IMG-05 | Hover animated GIF/WebP, select it, then move the pointer away and deselect. | Animation advances while hovered or selected and holds its current frame while unengaged. Capture multiple timestamped frames. |
| IMG-06 | Pan through a large board, zoom out and back in, switch boards and return. | Visible images reload when needed; no persistent missing content, stale thumbnails, or unexplained geometry changes. |

## 5. Navigation and selection

Use six images and several annotations, including overlapping items.

| ID | Actions | Expected result |
| --- | --- | --- |
| NAV-01 | In Mouse mode, wheel over a recognizable image corner to zoom in/out. | Pointer-centered zoom preserves the corner under the pointer; content geometry stays unchanged. Require actual wheel events. |
| NAV-02 | Pan with middle-button drag and with Alt+primary drag. | Both move the view without moving board items; release ends the gesture. |
| NAV-03 | Test high-resolution two-finger scroll in Mouse mode. Switch to Trackpad and test translation and pinch. | High-resolution scroll pans; Trackpad translation pans and pinch zooms around the pointer. If the tool cannot generate these inputs, leave the case blocked for native verification. |
| NAV-04 | Select one image and press Space. Repeat with several images, a textbox, and mixed content. Double-click an ungrouped image. | Focus fits the intended selection in usable canvas space; double-click focuses the image. |
| NAV-05 | Click an item, Shift-click to add/remove, drag a marquee, Shift-marquee, click empty canvas, then `Mod+A`. | Selection matches the gestures; select-all includes board content; no unintended movement or new item. |
| NAV-06 | Select overlapping items after changing stack order. Click and hold without dragging. | The visually front item receives the hit; selected image remains visible without a blank flash. |
| NAV-07 | Scroll over sidebar/search, then over canvas. Dismiss an overlay and immediately drag a canvas item. | Input goes to the intended region; no accidental canvas pan/zoom under controls or stuck capture afterwards. |

## 6. Image transforms and crop

Use the asymmetric corner fixture and a differently sized pair. Keep an
uncropped copy on another board for comparison.

| ID | Actions | Expected result |
| --- | --- | --- |
| EDIT-01 | Drag one image, then a multi-selection. | Selected items move together; relative geometry stays fixed; unselected items stay put. |
| EDIT-02 | Resize a single image using corner and edge handles, including after rotation. | Resize follows the handle, preserves image proportions, and leaves a usable nonzero image. |
| EDIT-03 | Hold Mod+Alt and drag on a selected image horizontally, first alone then in a multi-selection. | Uniform scaling changes selected image sizes and their spacing together around the selection; it does not pan. |
| EDIT-04 | Hold R, locate the rotation handle, and drag it for one image and then a multi-selection. Set Rotation snap to 15 degrees and repeat; hold Alt during an active rotation for unsnapped rotation. | Rotation uses the selection center and configured angle increments; precision bypass works. Start rotation before adding Alt so Alt-pan does not take precedence. |
| EDIT-05 | Hold C and drag a crop rectangle inside the asymmetric image; release and wait. Undo/redo. | Crop replaces that placement's displayed content; original source file and other placements of the original asset stay unchanged. |
| EDIT-06 | Crop a rotated image and an image already cropped once. Try a tiny crop and a crop crossing the image boundary. | Valid crop matches the intended visible region; invalid/tiny crop does not corrupt the item or create endless loading. Record the accepted boundary behavior. |
| EDIT-07 | Use `Mod+D`, Delete, `Mod+.` and `Mod+,`. Repeat through Duplicate, Delete (from board), Move to Front, and Move to Back in the context menu. | Both routes agree; duplicate is separately editable; delete affects this board; stacking preserves relative order within a multi-selection. |
| EDIT-08 | Begin a transform, release outside the canvas, and return. Then start a normal drag. | Gesture ends cleanly; no stuck rotation/resize mode or lingering held modifier. |

## 7. Arrangement and snapping

Use ungrouped images so group auto-layout does not obscure these commands.

| ID | Actions | Expected result |
| --- | --- | --- |
| LAY-01 | Select mixed portrait/landscape images and press Q. Repeat with equal sizes and a tall-image-heavy selection. | Compact layout avoids overlap and needless single-column output; it keeps a reasonably rectangular footprint and image proportions. |
| LAY-02 | Apply W, then Undo; apply E. | Row and column layouts follow the command and preserve image proportions. |
| LAY-03 | Arrange with Placement gap 0, then 24. Repeat duplicate and a fresh multi-image import. | Neighbor spacing follows the configured gap; changing the setting alone does not rearrange existing items. |
| LAY-04 | Scatter a selection and press each arrow key, undoing between directions. | Selected items settle in the requested direction with configured spacing and without overlap. Other items stay put. |
| LAY-05 | Drag near another item's edge, center, and gap alignment. Repeat at a different zoom, then while holding Mod without Alt. | Relevant guides and snapping appear; Mod suppresses guide snapping; thresholds remain usable at both zooms. |
| LAY-06 | Resize near an alignment and uniformly scale a multi-selection near neighboring geometry. | Guides match the accepted geometry; scale remains uniform and resize preserves its anchor. |
| LAY-07 | On differently sized images, apply `Mod+L`, undo, then `Mod+Shift+L`. | Default average area uses the built-in target area; selection average area equalizes to the selection's mean area. Aspect ratios remain intact. |

## 8. Groups and quick add

Prepare three ungrouped numbered images, several spare images, and two
destination groups named Faces and Shapes. Groups contain images, not
annotations or nested groups.

| ID | Actions | Expected result |
| --- | --- | --- |
| GRP-01 | Select three images and `Mod+G`. Repeat through Group in the context menu. | Named container appears with ordered members; new group defaults to Wrap. |
| GRP-02 | Click a member and drag. Double-click that member, drag it within the group, then press Escape and drag again. | Ordinary click selects/moves the group; double-click enters member editing; member drag reorders the flow; Escape restores group selection. Dragging a member with its center outside the group detaches it at its dropped position, closes the gap, and supports Undo/Redo. |
| GRP-03 | Edit Name and choose Row, Column, then Wrap. Change Spacing, Padding, and Width. | Name commits on leaving the field; flow preserves member order; spacing/padding visibly apply; Wrap height follows its contents. |
| GRP-04 | Narrow and widen the group with its container resize interaction and Width control, including below an existing member's width. Separately apply explicit uniform scaling. | Container resize reflows members; an existing member too wide for the new container retains its size and overflows. Uniform scale changes members, container, spacing, and padding together. |
| GRP-05 | Select spare images, press G, type Faces, choose with arrows and Enter. Repeat through Add to group in the context menu. | Picker lists matching groups on this board; both routes append selected images to the chosen group's ordered flow. |
| GRP-06 | Add another image with G then Enter. Choose Shapes on the next attempt. Cancel another attempt with Escape and with Cancel. | Last-used valid destination starts highlighted; choice can change; cancellation changes nothing. |
| GRP-07 | Transfer a member from Faces to Shapes. Undo/redo. Add an image already in its destination again. | Membership belongs to one group; both layouts update as one edit; re-adding does not duplicate/reorder the member. |
| GRP-08 | Append a large image to a small group. Compare prior members before/after. | New member fits under the group sizing policy; existing members keep sizes and positions when configuration is unchanged. Append fills the end of the flow. |
| GRP-09 | Enter member editing and delete the first, middle, and finally all members. Add a spare image to the empty named group using G. | Gaps close; empty group retains its name/configuration and remains a destination. |
| GRP-10 | Duplicate a group, edit the copy, delete the selected copy, and undo. Then ungroup with `Mod+Shift+G`. Repeat Ungroup through the menu. | Duplicate has independent members/configuration; group deletion removes the intended group and members and Undo restores them. Ungroup removes the container and retains member geometry. |
| GRP-11 | Copy a group to B, then inspect its name, flow, order, spacing, and padding. Edit it. | Structure survives paste; edits do not affect A or its original members. |
| GRP-12 | After using a destination, import, paste, and place search results. Switch to a different board and open G. | New intake is not automatically captured by the remembered group; picker lists only current-board groups. |
| GRP-13 | Type `g`, `q`, `w`, and `e` into group Name and picker search fields. Try grouping an annotation-only selection. | Typing does not trigger canvas commands; annotations do not become group members. |

## 9. Arrows, lines, and textboxes

| ID | Actions | Expected result |
| --- | --- | --- |
| ANN-01 | Hold A and drag an arrow in each direction. Hold L and draw a line. Try a click without a meaningful drag. | Arrowheads point to the intended endpoint; line has no arrowhead; tiny gestures do not leave stray segments. |
| ANN-02 | Select a line/arrow, move it, and drag each endpoint. Change S/M/L stroke widths and each color swatch. | Geometry and selected style change without affecting other annotations. |
| ANN-03 | Hold T and click to create a textbox; repeat with T+drag. Type multiline text and non-ASCII characters. | Both creation routes work; text is editable, wraps within bounds, and renders correctly. |
| ANN-04 | Finish text editing by clicking outside. Re-enter by double-click and by selecting then Enter; exit with Escape. | Text persists; caret/focus is predictable; Escape ends editing. |
| ANN-05 | Choose S/M/L/XL, each text color, and BG. Resize from an edge/corner and move using its body/border. | Toolbar updates the intended textbox; text wraps; resizing and moving do not silently edit or erase text. |
| ANN-06 | Toggle Textbox background default in Settings and create another textbox. | New textbox follows the default; existing textbox keeps its own background choice. |
| ANN-07 | While editing text, type shortcut letters and use text selection, copy/paste, Backspace, and undo. | Normal text operations work without deleting board items, drawing shapes, arranging images, or grouping content. |
| ANN-08 | Select images and annotations together. Move, duplicate, copy/paste, reorder front/back, delete, and undo/redo. | Supported mixed edits preserve relative geometry, text/style, and shared stack order. |

## 10. Clipboard exchange

Run on each OS with its native clipboard. Use a scratch document in the
external editor. Do not infer native success from internal clipboard tests.

| ID | Actions | Expected result |
| --- | --- | --- |
| CLIP-01 | Copy a raster image in an external editor and paste with `Mod+V`; repeat using the canvas Paste menu. | One new image per paste, correct pixels, usable placement, unchanged viewport. |
| CLIP-02 | Copy one and several image files in Finder/Explorer, then paste. | File clipboard paths import the intended batch once. |
| CLIP-03 | Copy a direct image URL and paste. Copy plain unsupported text and paste on an unfocused canvas. | Image URL imports; unsupported text does not produce a bogus image or mutate existing content. |
| CLIP-04 | Copy one transformed/cropped image inside SnappyRef and paste on the same board and on B. | Internal geometry/content survives; paste creates independent placements without unnecessary overlap with current accepted content. |
| CLIP-05 | Copy one ordinary loaded image from SnappyRef and paste into the external image editor. | Native image pixels are available and recognizable. |
| CLIP-06 | Copy a multi-image selection, annotation-only selection, and mixed selection between boards. | SnappyRef round-trip preserves supported structure and relative stack order. Multi/mixed selections are not promised as flattened native images. |
| CLIP-07 | Copy from PureRef using fixtures, then paste into SnappyRef. | Referenced images import once and form a usable batch. Record PureRef version and payload route if known. |
| CLIP-08 | Paste twice, then undo each paste. Move items and paste again before background saves finish. | Each paste is one independent edit and uses the current board geometry for placement. |
| CLIP-09 | Copy with nothing selected, use an empty clipboard, and replace a SnappyRef clipboard payload by copying a fresh external image. | Empty actions change nothing; the new external clipboard replaces old internal content. |

## 11. Semantic Search setup and availability

Keep missing-model and installed-model roots separate. Cases requiring live
downloads remain blocked until that networked pass is explicitly authorized.

| ID | Actions | Expected result |
| --- | --- | --- |
| MODEL-01 | In a fresh missing-model root, choose Not Now, open `Mod+F`, then Settings. | Prompt dismissal persists; search and Settings expose installation; ordinary board work remains usable. |
| MODEL-02 | In separate fresh runs, start installation from onboarding Enable, Settings Install, and the search palette's install action. | All entry points show the same installation state; switching views does not start duplicate downloads. |
| MODEL-03 | During an authorized live download, inspect progress in Settings/search and continue editing a board. Cancel, then start again. | Progress is visible; cancel returns to missing; retry can complete; edits stay usable and intact. |
| MODEL-04 | With a maintainer-controlled failure setup, attempt installation, inspect failure, restore service, and use Retry. | Error is visible and actionable; retry reaches Installed; no permanent spinner. Use inspect for deterministic failure states that cannot be produced safely in the native run. |
| MODEL-05 | On installed assets, click Remove and cancel its confirmation. Repeat and confirm Remove Semantic Search. | Cancel retains search; confirm removes model availability while retaining boards, images, and saved indexing. |
| MODEL-06 | Reopen after removal, then restore/install assets and search again. | Onboarding does not reappear; manual setup remains available; preserved library content is searchable again. |

## 12. Search queries and placement

Use installed real model assets and the search fixture set distributed across
A and B. Leave several assets unplaced by deleting only their board placements.
Record pending indexing before judging missing results.

| ID | Actions | Expected result |
| --- | --- | --- |
| SEARCH-01 | With nothing selected, `Mod+F`, type a concrete subject query, wait for results, then Escape. | Search opens with focused input; recognizable relevant results appear; Escape closes without adding content. |
| SEARCH-02 | Query subjects present only on B and only among unplaced assets. | Search reaches the whole Library, not just the active board. |
| SEARCH-03 | Import a new subject, allow idle indexing, and search for it. While other images are still indexing, search an already indexed subject. | New content becomes discoverable; existing queries remain usable during backfill. |
| SEARCH-04 | Change query text rapidly, clear it, and enter a different subject. | Results/status converge on the latest query; old responses do not replace it. Blank query shows its intended empty/recent state. |
| SEARCH-05 | Reopen search and activate recent searches by click and arrows+Enter. Quit/reopen and repeat. | Recent queries are usable and retained according to the saved workspace state. |
| SEARCH-06 | Select exactly one image, press `Mod+F`, and repeat through Search using image. Clear the pivot using its control and Backspace with empty query. | Image search uses the selected image; relevant similar images appear; clearing returns to text search. Context action is disabled for unsupported selections. |
| SEARCH-07 | Click a result, Mod-click to toggle others, Shift-click a range, then use arrows and Shift+arrows. | Selection/highlight matches the action and remains visible as the list scrolls. |
| SEARCH-08 | Navigate down from the final visible result row when More is shown. | Another result page loads; keyboard selection advances and remains visible. No duplicated results or lost query. |
| SEARCH-09 | Drag one result onto the canvas. Repeat with multiple selected results. | Provisional images follow the pointer; palette changes for placement; drop commits exactly one batch with correct thumbnails/content. |
| SEARCH-10 | Press Enter to position selected results, move the pointer, and click to commit. Repeat with `Mod+Enter` for Place All. | Correct result set enters positioning; click accepts once. Record how many results were offered and placed. |
| SEARCH-11 | Start positioning and press Escape. Drag out and back into the palette, then abandon the drag. | Cancellation removes provisional content without a document history entry; no later image appears from cancelled work. |
| SEARCH-12 | Position a result batch larger than the viewport, then a small batch. | Large preview can zoom out once, stays anchored at the pointer, and does not refit repeatedly; small preview does not auto-zoom in. |
| SEARCH-13 | Commit results, undo/redo, switch boards, and restart. | Result placements survive and replay like other board additions; shared Library assets remain reusable. |
| SEARCH-14 | Inspect empty-library search in a separate installed-model root, and an unavailable/error state through controlled setup. | Empty/error states are readable, dismissible, and do not block other app work. Do not require a nonsense query to return zero results from similarity search. |

## 13. Library operations

Restore `baseline` before this category. Finish other categories before
Library-wide removal because it changes every board using the asset.

| ID | Actions | Expected result |
| --- | --- | --- |
| LIB-01 | Delete a placement from A while the same image exists on B, then search for it. | B and the Library asset survive; the image remains discoverable. |
| LIB-02 | Right-click a selected image, choose Remove from Library..., and cancel. Repeat and confirm. | Confirmation distinguishes Library removal; cancel changes nothing; confirmation removes that asset's active placements across boards and removes it from search. |
| LIB-03 | Repeat removal with several selected assets, including one that belongs to groups on two boards. | Only intended assets disappear; group membership/layout remains coherent; unrelated images remain. Reopen to verify persistence. |
| LIB-04 | Import exact duplicates and press P for Dedupe library. Search, switch boards, and restart. | References remain valid and search does not show exact duplicate assets. A current-format import may already dedupe, so this can be a valid no-op. |
| LIB-05 | On a maintainer-prepared legacy duplicate fixture, run P, then inspect boards/search and existing undo history. | Historical duplicate records reconcile without breaking references or restored content. Without this fixture, duplicate repair remains untested despite LIB-04 passing. |

## 14. Settings and focus boundaries

| ID | Actions | Expected result |
| --- | --- | --- |
| SET-01 | Change Pointer mode, Always on top, Rotation snap, Textbox background, and Placement gap; switch boards and restart. | Workspace settings persist and affect subsequent relevant interactions on each board. |
| SET-02 | Change Canvas background through None, Grid, Dots on A; set a different background on B; switch/restart. | Background belongs to each board and remains independent. |
| SET-03 | Set Rotation snap to 0 and 180, and Placement gap to 0 and 128. Try values outside the range, blank text, and nonnumeric text, then leave the field. | Supported values apply; invalid input cannot leave unusable geometry. Current UI clamps numeric values to range and restores the last value for invalid text. Record any conflict with spec validation wording. |
| SET-04 | Type shortcut letters and numbers in each text field: board name, group name, picker, search, textbox, settings numeric input. | Text entry cannot create/switch boards, arrange content, trigger dedupe, or start drawing. |
| SET-05 | Open context menu with empty, image-only, annotation-only, mixed, and group selections. Dismiss by Escape and outside click. | Action availability matches selection; disabled actions do nothing; dismissal does not trigger a hidden item beneath the menu. |

## 15. History, persistence, and existing data

This category verifies combinations beyond the per-case Undo/Redo checks.

| ID | Actions | Expected result |
| --- | --- | --- |
| SAVE-01 | On A: import, move, rotate, crop, annotate, group, edit group, and reorder. Capture each accepted state. Undo to the initial state, then redo forward. | Each user edit replays in order with correct pixels, geometry, text, groups, and stack. Worker completion does not create extra per-image history entries. |
| SAVE-02 | Edit A, edit B, return to A and Undo; return to B and Undo. | Each board consumes its own history. |
| SAVE-03 | Undo an edit, make a different edit, then Redo. | Abandoned redo branch is unavailable; no stale content returns. |
| SAVE-04 | Make an edit, pan/zoom, change background/sidebar/settings, then Undo. | Undo reverses the document edit, not view/workspace adjustments. |
| SAVE-05 | Quit after accepted edits and reopen. Undo/redo after reopening, including board rename/delete restoration and group transfers. | Complete board state and committed history survive restart. |
| SAVE-06 | Make several rapid edits and switch boards immediately. Return, quit normally, and reopen. | Latest accepted state wins; no stale content, lost edit, wrong-board completion, or duplicated history. |
| SAVE-07 | Open the candidate on the closed-app copy of the v0.2.0 workspace. Compare all recorded boards/settings, edit, quit/reopen, and replay known history. | Supported old data opens and remains usable; migration preserves recorded content and history. Keep the original untouched and do not reopen the migrated copy in the old app. |
| SAVE-08 | With the app closed, clone the candidate's populated test root. Reopen that clone at another absolute location with its data-root override. | Managed images, model availability, boards, and session work without relying on fixture source paths. |
| SAVE-09 | Maintainer runs deterministic persistence failure/retry evidence and checks its report. | Accepted in-memory content survives failed writes; retries do not duplicate content and successful persistence restores on reopen. Native UI alone does not certify this. |

Do not force-kill during unflushed writes and assume every last edit must
survive. Crash timing and atomicity need a controlled persistence test with
known durable boundaries, not a guessed screenshot-based expectation.

## 16. Installed locations and a complete working session

| ID | Actions | Expected result |
| --- | --- | --- |
| END-01 | Launch an extracted archive from a writable disposable installation folder without data-root overrides. Import, quit, and reopen. | Portable data is created beside the install and survives restart. |
| END-02 | Maintainer prepares an isolated OS account and a protected install location without overrides. Repeat END-01. | Fallback app-data root works; app does not silently lose its Library. Use an isolated account to avoid the user's existing fallback data. |
| END-03 | With model assets installed and fixtures local, go offline in an isolated test environment. Edit, search, switch boards, and reopen. | Local board work and semantic queries remain usable. Network URL intake failure is contained. Do not grade incidental updater activity. |
| END-04 | Create a new project board, import a mixed batch, arrange/group it, add notes/arrows, find related references, place them, copy a group to a second board, revise with undo, and reopen. | The entire workflow completes with both boards intact and no unexplained errors or stuck modes. Save a final overview of each board. |
| END-05 | Spend 10 minutes revisiting the most awkward interactions observed in the run, using only disposable data. | Record additional concrete failures or "none observed". This exploratory pass supplements the case ledger and cannot substitute for missing cases. |

## Existing automated coverage

Run these against the same source commit as part of the single automated
batch described in the execution plan. Keep results separate from native
computer-use evidence. A passed inspect scenario does not turn a blocked
native drag or clipboard case into a pass.

| Coverage | Existing inspect scenarios |
| --- | --- |
| Import and pending work | `sample-image-import`, `pending-drop-batch-undo` |
| Board lifecycle and view | `canvas-navigation-boards`, `board-lifecycle-undo-redo`, `delete-board-modal-flash` |
| Editing, crop, deletion | `canvas-edit-operations`, `crop-undo-redo-placement`, `delete-undo-redo-placement`, `placement-click-hold-flash` |
| Layout | `compact-equal-size-images`, `compact-mixed-column-images`, `compact-windows-column-images` |
| Groups | `group-operations` |
| Search interaction | `search-drag-to-canvas`, `blue-image-search-drag`, `search-palette-footer` |
| Model setup UI with test installer | `semantic-search-install`, `semantic-search-sidebar-status` |
| Restart and failed writes | `session-restart-restore`, `persistence-write-retry` |

List the current catalog with
`docs/skills/inspect/scripts/run-scenario.sh --list`. The full test suite already
runs non-ignored scenario tests; validate those artifacts instead of running
each scenario again. Use the scenario script for targeted failure reruns.
These scenarios cover portions of the categories, not every row above.

The release maintainer also runs the required formatting, clippy, compilation,
ordinary tests, and archive validation from [release operations](release-operations.md).
Those checks are separate from this functional pass. Use
[perf-profiling](skills/perf-profiling/SKILL.md) for startup, import, idle, and
memory measurements; computer-use impressions are not performance numbers.
Updater checks remain outside this plan. Do not run either update-smoke script
or `semantic-search-real-install` without the explicit request required by the
repo rules.

## Report and release decision

Maintain one coverage ledger, grouped by OS. Every case and required subcase
gets a row. Format variants, toolbar choices, and alternate input routes should have
their own result when only part of a case could be exercised.

| Case / assertion / OS | Mode | Result | Evidence | Gap or follow-up |
| --- | --- | --- | --- | --- |
| IN-01 / native file drop / macOS | native | NOT RUN | | |

For a failure, include the candidate, OS, fixture, exact reproduction steps,
expected versus actual behavior, reproducibility, and screenshot paths. Mark
severity by user impact: crash/data loss; blocked core workflow; incorrect
behavior with a workaround; cosmetic defect. Keep findings local unless asked
to file issues.

The final summary gives passed, failed, blocked, not-run, and N/A totals for
each OS, unresolved defects, and explicit gaps requiring native hardware or
maintainer evidence. Do not say "the entire app passed" while cases remain
blocked or untested.

The candidate is ready for a release decision when every applicable case has
evidence, there are no unexplained failures affecting data or core workflows,
and the maintainer has accepted any remaining limitations. After a fix, rerun
the failed case, its category's affected workflows, and persistence checks
when stored state changed. Repeat the whole matrix only when the change is
broad enough to invalidate earlier evidence.

## Sources for maintaining this plan

- [Shortcut definitions](../src/app/shortcuts.rs), [app command routing](../src/app/mod.rs), and [context menu](../src/app/context_menu.rs).
- [Sidebar settings](../src/ui/sidebar/settings.rs) and [board controls](../src/ui/sidebar/boards.rs).
- [Canvas interaction](../.specs/canvas-interaction/canvas-interaction.yml), [spatial layout](../.specs/spatial-layout/spatial-layout.yml), and [group controls](../src/canvas/groups.rs).
- [Annotation controls](../src/canvas/render/textbox_editor.rs) and [clipboard contract](../.specs/clipboard-exchange/clipboard-exchange.yml).
- [Search contract](../.specs/semantic-search/semantic-search.yml), [search input](../src/search/overlay.rs), and [placement creation](../.specs/placement-creation/placement-creation.yml).
- [Board history/persistence](../.specs/board-document/board-document.yml), [Library](../.specs/local-library/local-library.yml), and [workspace continuity](../.specs/workspace-continuity/workspace-continuity.yml).

If the candidate differs from the reviewed commit, reconcile changed controls
and contracts before the run. Record source/spec disagreements as findings;
do not quietly redefine success to match whatever the app does.
