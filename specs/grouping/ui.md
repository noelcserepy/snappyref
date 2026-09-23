# Grouping UI

Behavior lives in four Quint modules, one per question, sharing the guards in [rules.qnt](rules.qnt):

| Module | Question | Check |
| --- | --- | --- |
| [sizing.qnt](sizing.qnt) | Which flow, gap, Fixed/Hug and Fill modes may coexist, and how edits and membership changes move between them | every reachable state, TLC |
| [board.qnt](board.qnt) | Who is in which group in what order, and what the dock shows after each operation | every reachable state, TLC |
| [layout.qnt](layout.qnt) | Where Wrap breaks rows, how Fill shares leftover, which member an arrow lands on | every input up to width 1000, Apalache |
| [enablement.qnt](enablement.qnt) | Is each control enabled exactly when its submission would be accepted | every valid view, Apalache |

This document owns appearance, exact geometry, stored values, text and platform details, and the operations the modules leave out. The bottom dock replaces its contents when member editing starts. No second inspector or extra settings menu.

## Appearance sources

| Source | Owns | Verification during implementation |
| --- | --- | --- |
| [references/dock.html](references/dock.html) | Group dock order, dimensions, spacing, icons, colors, alignment popover | Inspect screenshot beside the PNG; compare values against the HTML |
| [references/dock.png](references/dock.png) | Selected-group appearance with alignment open | Human visual comparison at 1280 by 800 |
| [references/member-dock.html](references/member-dock.html) | Member dock order, dimensions, icons, breadcrumb and hint | Inspect screenshot beside the PNG; compare values against the HTML |
| [references/member-dock.png](references/member-dock.png) | Member editing appearance | Human visual comparison at 1280 by 800 |
| [src/ui/theme.rs](../../src/ui/theme.rs) | Existing matching surface, text, accent and DM Sans tokens | Use matching tokens; keep artboard-specific dimensions local |
| [src/ui/motion.rs](../../src/ui/motion.rs) | Existing overlay entrance and settle timing | Inspect entry, exit and content replacement |

The local HTML and PNG files are the appearance references. Their preview-runtime script and font URLs are export boilerplate, not required assets or behavior dependencies. The app already embeds DM Sans. The HTML is static; its selected values demonstrate a state, not defaults or permission rules. In particular, the member artboard's H Fill requires a Fixed group height. It is a separate state from the group artboard's H Hug.

Keep one horizontal row at its natural width, centered within the canvas, 24 px above its bottom. Height 52 px, radius 14 px, 6 px control gaps, horizontal dock padding 6 px left and 10 px right. Surface `#161514`, shadow `0 12px 40px rgba(0,0,0,.7)`. Controls use 30 px height, 8 px radius, inset segmented beds `#1B1A19` and fields `#22211F`. Separators are 1 by 20 px, `#2A2826`, with 4 px side margins. These values intentionally differ from the general 28 px control height and 12 px panel radius.

Active choices use `#E4CEB0` with dark ink. Primary text `#F0F0F0`, secondary `#8C8C8C`, metadata `#666666`. DM Sans, weights 400/500/600; control text 13 px, group name 14 px, count 12 px, member hint 11 px. Match SVG silhouettes and stroke widths in the HTML. Earlier/later are icon buttons, not text labels. Ungroup and Detach use muted text, not destructive red. Muted text alone does not mean disabled.

The alignment trigger previews the saved nine-position setting. Its only popover sits above the trigger with an 8 px gap, warm panel background and 12 px radius. Match the HTML's padding, nine 28 px cells, labels and four Auto segments. Do not turn these into separate popovers. Primary-axis cells dim under Auto according to `controlEnabled`.

The selected group has a fine gold container outline and name above it. Member editing softens the container outline, gives the selected member a 2 px gold outline and shows its ordinal beneath it. Match the two PNGs. Insertion feedback belongs at the slot a drag would take.

Entry slides up with the existing short settle; exit slides down. Member editing swaps the same dock's contents. Use existing motion timing; animation does not delay an operation. Hover/focus/disabled treatments must remain distinguishable. The artboards do not define every such treatment; reuse existing component semantics.

## Modes and control inventory

`dock.panel` is `Hidden`, `GroupDock` or `MemberDock` in board.qnt. Its identity is the selected group or member. `GroupDock.alignOpen` owns the only second level. A selection change closes it. Names, counts, member ordinals, Hug labels, Fixed values, Fill and active toggles derive from board state.

`controlShown(c)` and `controlEnabled(c)` in enablement.qnt specify visibility and enablement, and `controlReason(c)` is the admission the operation applies. A visible disabled control stays in place and dims. Fields accept only valid submitted values: a trimmed nonblank name, any integer gap or row gap, a nonnegative padding, a positive finite size. An invalid draft never becomes a board edit; the field stays enabled and the submission is refused.

### Selected group

Read left to right as in the group artboard. Each row names the operation the control dispatches, with the selected group as its target. Other configuration fields remain unchanged.

| Control | Kind / text | Fires | Enabled when | Appearance or value |
| --- | --- | --- | --- | --- |
| Name | Inline text field | rename | `controlEnabled(Name)` | Stored string; blank refused at submission. Not modeled |
| Flow | Row / Column / Wrap / Free segments with icons | `setFlow` (sizing, board) | `controlEnabled(Flow(flow))` | Active segment follows saved flow |
| Gap | Horizontal gap icon and number | `setGap(g, "Number")` plus the stored number | `controlEnabled(Gap)` | Signed number; dims in Free and while Auto is on. The number is kept under Auto and restored by Off |
| Row gap | Vertical gap icon and number | stored value | `controlEnabled(RowGap)` | Only shown in Wrap, signed number; hide outside Wrap even in Free. Default: the shared placement gap |
| Padding | Nested-box icon and number | stored value | `controlEnabled(Padding)` | One nonnegative value for all sides; active in Free. Default: the shared placement gap |
| Alignment | Nine-dot preview and chevron | `toggleAlign` (board) | `alignEnabled` | Dimmed in Free; expanded state follows `alignShown` |
| W / H numeric value | Axis label and current size or Hug | `setAxisMode(g, axis, true)` (sizing) plus the stored size | `controlEnabled(Dimension(axis))` | Numeric entry makes the axis Fixed; positive finite outer size converted to usable extent |
| W / H Hug | Small icon toggle in each size field | `setAxisMode(g, axis, not fixed)` (sizing) | `controlEnabled(Hug(axis))` | Gold when Hug; W Hug dimmed in Wrap; active Hug clicked again freezes current size |
| Clip | Icon toggle, "Clip" | stored flag | `controlEnabled(Clip)` | Active follows saved `clip`; off by default |
| Ungroup | Icon button, "Ungroup" | `ungroup` (sizing, board) | `controlEnabled(Ungroup)` | Keeps member geometry and hides group dock |

Rename, row gap, padding, clip and alignment cells store a value and change no mode or order, so no module carries them; unit tests cover their storage. Member count is passive text from `dockCount`, formatted as "1 item" or "N items". Empty groups show "0 items" and retain their dock, name, size and settings.

### Alignment popover

| Control | Kind / text | Fires | Enabled when | Notes |
| --- | --- | --- | --- | --- |
| Nine-position grid | Start / Center / End on each axis | stored alignment | `controlEnabled(Align((x,y)))` | Under Auto, only the three cells along the counter axis at the saved primary coordinate remain active |
| Auto spacing | Off / Between / Evenly / Around segments | `setGap(g, "Number")` or `setGap(g, "Auto")` plus the stored variant | `controlEnabled(Auto(mode))` | Off maps to Number and restores the last numeric gap. Between, Evenly and Around are one Auto mode with a stored variant; selecting any of them fixes the primary axis at current size |
| Outside press | Nonmodal dismissal | `dismissAlign`, then the clicked target's operation | Always | Outside both popover and trigger; close without swallowing the click |
| Esc | Dismiss one level | `escape` | Always | Close popover before clearing group selection |

Section labels are "ALIGN" and "AUTO SPACING". Use descriptive accessible grid names such as "Align left, top", not coordinate numbers. The alignment trigger only toggles; it does not also run outside dismissal. Alignment and spacing choices keep the popover open. Switching flow to Free closes it. An open popover never belongs to member editing or another group.

### Member editing

| Control | Kind / text | Fires | Enabled when | Appearance or value |
| --- | --- | --- | --- | --- |
| W / H Fixed | Segmented button per axis | `setMemberMode(g, m, axis, false)` (sizing) | `controlEnabled(MemberMode((axis,false)))` | Freezes rendered size; dim in Free or Absolute |
| W / H Fill | Segmented button per axis | `setMemberMode(g, m, axis, true)` (sizing) | `controlEnabled(MemberMode((axis,true)))` | Requires parent axis Fixed; also dim in Free or Absolute; retain saved active state while dimmed |
| Absolute | Icon toggle, "Absolute" | `toggleAbsolute(g, m, slot)` (board) | `controlEnabled(Absolute)` | On preserves position; off uses the nearest insertion slot supplied by geometry |
| Earlier / later | Left/right chevrons in a shared bed | `arrow` along the primary axis (board) | `controlEnabled(Earlier)` / `controlEnabled(Later)` | Earlier/later in flowing order, including Column; dim at an endpoint and for Free/Absolute |
| Detach | X icon and "Detach" | `detach` (sizing, board) | `controlEnabled(Detach)` | Keep the loose item selected and hide dock |

Passive breadcrumb: group name, slash, "Member N" from the name and `dockMemberNumber`. Not a back button. Canvas caption: "MEMBER N OF M", including Absolute members in the count and ordinal. Far-right hint: "esc leaves member editing". Esc returns to the group dock.

## Keyboard, pointer and other entry points

`pressKey` in board.qnt dispatches the keys the dock owns. Primary means Cmd on macOS and Ctrl on Windows. `dock.textFocus` prevents board shortcuts while typing in a name, number, textbox or other text input. Enter/blur submits the field then releases focus through `focusText(false)`; Esc discards its unsubmitted draft and releases focus before closing a panel.

| Entry point | Fires | Enabled when | Context / arguments |
| --- | --- | --- | --- |
| Single group/member click | `selectGroup(group)` | `hasGroup(board,group)` | Normal member click selects its group |
| Double-click / Primary+click member | `editMember(member)` | `owners(board,member).size() > 0` | Directly shows member dock |
| Empty canvas / loose item / mixed or multiple selection | `clearSelection` | Always | Hide dock for mixed/multiple selections |
| Enter / field blur | the control's operation, then `focusText(false)` | `controlEnabled(control)` and valid submitted value | Commits the field; Esc discards the draft through `escape` |
| Acquire/release text focus | `focusText(active)` | Always | Text owner controls caret and drafts |
| Esc | `pressKey("Escape",...)` → `escape` | Always | Text draft, popover, member editing, group selection, in that order; palette consumes its own Esc |
| Primary+G / context Group / palette Create new | `pressKey("Primary+G",...)` → `create` | `not(dock.textFocus)` for shortcut; `not(hasGroup(board,identity))` | Empty eligible selection is a no-op; images and textboxes group, lines and arrows stay loose. The kind filter is the canvas selection's, not modeled |
| Primary+Shift+G / context Ungroup | `pressKey("Primary+Shift+G",...)` → `useDock(Ungroup)` | `not(dock.textFocus)` and `controlEnabled(Ungroup)` | Whole-group command; member dock does not expose it |
| G / context Add to group | existing palette | `not(dock.textFocus)` for shortcut | Falls through `pressKey` unhandled. Query, last-destination highlight and Create new stay owned by the palette |
| Choose destination in palette | `add(group,member)` for each selected member | `board.live.contains(member)` and `hasGroup(board,group)` | Append, transfer, repeat-safe re-add; transfer keeps editing the member in its destination; palette typing never triggers canvas keys |
| Primary+V image | `pressKey("Primary+V",...)` → `paste(identity)` | `not(dock.textFocus)` and `not(board.live.contains(identity))`; decoded image | Member: insert after it. Group: append. Hidden: board. No remembered group captures intake |
| Primary+D member | `pressKey("Primary+D",...)` → `duplicate` | `not(dock.textFocus)` and `memberDockShown` and `not(board.live.contains(identity))` | Insert after edited member; select the new member, also after member paste |
| Arrow keys | `pressKey(direction,...)` → `arrow` | `arrowEnabled(direction)` | Row: Left/Right. Column: Up/Down. Wrap: Left/Right, Up/Down to same column in adjacent row if it exists (`arrowTarget` in layout.qnt) |
| Delete / Backspace member | `pressKey("Delete",...)` → `deleteMember` | `not(dock.textFocus)` and `memberDockShown` | Return to retained group dock, including empty group |
| Primary+C, Primary+D, Delete on whole selection | existing canvas owner | `not(dock.textFocus)` | Fall through `pressKey` unhandled; structure-preserving whole-group copy, duplicate and delete |
| Drag loose item into group | `dragIn(group,member,slot)` | `board.live.contains(member)` and `hasGroup(board,group)` | Nearest slot from drop center; explicit intake |
| Drag edited member | `dropMember(group,member,slot,false)` → `reorder` | `memberReason(board,group,member) == ""` | Flowing member reorders; Free/Absolute moves manually without order change |
| Drop edited member center outside container | `dropMember(group,member,slot,true)` → `detach` or `reorder` | `memberReason(board,group,member) == ""` | Keeps final dropped geometry; Absolute stays attached outside, see geometry note |
| Drag group edge/corner / size field | `setAxisMode(group,axis,true)` plus the stored size | `axisReason(flow,axis,true) == ""` | Each dragged axis becomes Fixed; existing Fixed members do not scale |
| Double-click edge handle | `setAxisMode(dockGroup,axis,false)` | `groupDockShown` and `axisReason(flow,axis,false) == ""` | Wrap width refuses; same conversion as the Hug toggle |
| Double-click corner handle | `setAxisMode` Hug on each legal axis | `groupDockShown` | Hug both legal axes; keep Wrap width Fixed |
| Primary+Alt drag group | existing transform owner | `groupDockShown` | Scales container, members, absolute offsets, gap, row gap and padding |

Ordinary group drag remains with the canvas move owner, outside this model. It moves the container and all members together.

No bindings for Shift+A, Alt+Shift+A or Shift+arrow gap nudges. Unknown keys fall through to their existing owner. Imports and search results remain loose unless explicitly added. Clipboard decoding and fresh identities belong to the clipboard adapter. Persistent encoding, undo history and whole-group clipboard internals are outside this dock model and remain with their existing owners.

## Geometry obligations retained from the earlier design

layout.qnt checks Wrap row breaks, Fill shares, Hug width and same-column arrows for integer widths up to 1000 with one gap; the Rust layout test runs the same properties over the float code. Nothing proves fractional positions, aspect ratios, rotated bounds, clipping paint or Free-flow envelopes. Implement numeric examples as focused geometry tests, then use inspect scenarios for rendered evidence.

- Group defaults: Wrap, Fixed width from selection bounds plus padding, Hug height, numeric gap and row gap from shared placement gap, one padding value, top-left alignment, Clip off. New members start Fixed on both axes and non-absolute. Row/Column presets hug both axes; switching an existing group preserves explicit sizing choices.
- Container size fields show outer size. Layout capacity on axis `a` is `max(0, outerSize[a] - 2 * padding)`. The model receives capacity, not the displayed outer size. Empty groups keep their last container size even when Hug.
- A member's layout footprint is its rotated visual bounds. Row/Column place members in order; Wrap forms rows in order, with no packing. Wrap width is Fixed. Fill members count as zero minimum width when deciding row breaks. Each row then divides its leftover width among its Fill members.
- Primary-axis Fill share is `max(0, capacity - sum(fixed footprints) - numericGap * max(0, memberCount - 1)) / fillCount`. Auto contributes zero numeric gap. Example: capacity 300, fixed widths 100 and 80, two Fill members, numeric gap 10. Four members create three gaps, leaving 90, so each Fill member receives 45. At capacity 180, Fill receives zero; Fixed members may overflow.
- Fill consumes leftover before Auto distributes the remainder. For `n` members and remainder `L >= 0`, Between has `L/(n-1)` between members for `n > 1`, no outer space; Evenly has `L/(n+1)` at each end and between; Around has `L/n` between and half that at each end. With one member, Between starts at the origin; Evenly/Around center it. No division for an empty row. In Wrap this runs separately per row. Cross-axis placement and the row stack use saved counter-axis alignment.
- Numeric gap may be negative. Numeric Start-aligned append preserves existing positions; Auto or non-Start alignment may move existing members on add. Later ordered members paint above earlier ones. Absolute members paint above flowing members, retaining order within each set.
- Alignment offset for leftover room is zero, half or all for Start, Center or End. Primary alignment acts only on a Fixed axis with room left and numeric spacing. Auto owns primary distribution; preserve saved primary alignment for when Auto turns off.
- Images keep aspect ratio. One Fill axis determines the other dimension from the image ratio. Both Fill axes aspect-fit within the cell using the smaller scale factor; use alignment to position the result. Textboxes stretch each Fill axis independently and wrap text inside their user-drawn rect. No text auto-size mode.
- Hug includes non-absolute members and padding. Free ignores flow layout, gap, Auto, alignment and Fill, preserves manual member positions, and hugs their bounds plus padding on Hug axes. Fixed axes preserve user size. Entering Free preserves current rendered positions; leaving Free lays members out in saved order.
- Absolute stores a top-left-relative offset, does not contribute to Hug or flow, ignores Fill and alignment, and may extend outside the container. Toggling on freezes rendered geometry; toggling off inserts at the slot nearest its center. Absolute dragging moves its offset; it does not auto-detach merely for crossing the container. The outside-drop detach gesture applies to flowing members. Explicit Detach remains available for Absolute.
- Clip only crops paint to the container, including overflowing Fixed and Absolute members; never changes layout. The group's hit area stays its container. Members outside it do not enlarge group hit testing.
- Incoming fit affects only a newly arriving, non-absolute, all-Fixed member entering Row/Column/Wrap with Fixed width. Scale an oversized incoming image down proportionally to usable width, never up; do not resize existing members. Free, Hug width, Absolute and Fill intake skip this fit. Transfer retains Absolute and compatible Fill intent (sizing.qnt); incompatible Fill axes freeze at source rendered size. Hugging an axis freezes its Fill members at their rendered size; leaving a group freezes the member's rendered size.
- Drag insertion uses the drop center against existing ordered member geometry, with the dragged member removed first. Show the resulting insertion indicator. Turning Absolute off uses the same slot calculation. Detach and Ungroup preserve rendered geometry. No separate paint-order controls, nesting, Grid, min/max sizing, constraints for Absolute, hidden members or per-side padding.

Numeric examples for verification: 300 px capacity with the four members above gives 45 px Fill. A 400 by 200 image in a 100 by 100 cell renders 100 by 50; Center leaves 25 px above and below. Three fixed 40 px members in 300 px capacity with Auto have 180 px leftover: Between gaps 90 with zero ends; Evenly gaps/ends 45; Around gaps 60 and ends 30. A 100 px outer width with 16 px padding has 68 px capacity.

## Implementation evidence

Use the repo's [inspect skill](../../docs/skills/inspect/SKILL.md). Add or extend scenarios for group dock with open alignment, member dock, Free dimming, Hug/Fill refusal, Auto alignment, empty group, Esc/focus, selection changes. Compare the first two screenshots with the supplied PNGs. Check labels against this inventory and numeric examples against the geometry adapter. This planning change does not claim that the current `src/canvas/groups.rs` matches these artboards.
