// SnappyRef grouping and contextual bottom dock
// Flat, ordered groups contain images and textboxes; edits preserve valid membership and sizing.
// The dock shows one selected group or edited member, with state-dependent controls, focus,
// keyboard routing and one alignment popover. Empty groups remain destinations.
// Two group identities and three item identities exercise transfer, insertion and row breaks.
// Integer usable extents 0..3 saturate at 3 and floor fractional Fill; they are not pixel geometry.
// Exact positions, aspect ratios, rotated/Free bounds, alignment distribution and clipping paint
// belong to geometry. Appearance, text and local artboard citations live in specs/grouping/ui.md.
// Full canvas selection, text drafts/parsing, picker internals, storage, history and clipboard
// encoding stay outside this model. Whole-selection copy/delete/duplicate and scale are requests
// to their existing owners. No time or animation is modeled.
module grouping {
  // ---- parameters ----
  pure val GROUPS = Set(0, 1)
  pure val ITEMS = Set(0, 1, 2)
  pure val AXES = Set(0, 1)
  pure val SIZES = Set(1, 2, 3)
  pure val FLOWS = Set("Row", "Column", "Wrap", "Free")
  pure val GAPS = Set("Number", "Between", "Evenly", "Around")
  pure val SELECTIONS = Set([], [0], [2], [0, 2], [2, 0], [0, 1, 2], [3])
  /// Three stands for a large extent class, not a product maximum.
  pure val MAX_EXTENT = 3

  // ---- types ----
  type Outcome = Ok | Rejected(str)
  type Member = { kind: str, size: int -> int, fill: Set[int], absolute: bool }
  type Group = { order: List[int], flow: str, fixed: Set[int], size: int -> int,
                 gap: str, number: int, rowGap: int, padding: int,
                 align: int -> int, clip: bool, name: int }
  type Panel = Hidden | GroupDock({ group: int, alignOpen: bool }) | MemberDock({ group: int, member: int })
  type Dock = { panel: Panel, textFocus: bool }
  type Control = Name(int) | Flow(str) | Gap(int) | RowGap(int) | Padding(int)
    | Align((int, int)) | Auto(str) | Dimension((int, int)) | Hug(int) | Clip | Ungroup
    | MemberMode((int, bool)) | Absolute(int) | Earlier | Later | Detach
  type Board = { groups: int -> Group, live: Set[int], members: int -> Member }

  // ---- state ----
  /// The board owns membership lists; member records never duplicate their owner.
  var board: Board
  /// One contextual dock, with the popover owned by its group mode. Text focus captures shortcuts.
  var dock: Dock
  /// Every attempted operation succeeds or rejects without changing the board.
  var outcome: Outcome
  /// Last operation name is observation metadata for operation coverage.
  var event: str
  /// Transition evidence compares accepted results with the preceding board without storing history.
  var effects: { refusal: bool, incoming: bool, empty: bool, resize: bool, append: bool }

  // ---- derived ----
  /// Input domains are the finite wrappers below: known board identity tokens, valid axes,
  /// gap/flow/arrow enums, and finite integer observations. Identity exhaustion is sample-only.
  /// Missing group/item/member, reused identities, non-positive edge size, Wrap Hug width,
  /// and Fill on Hug have explicit refusal outcomes. Operations report refusals even when invoked through a dimmed control.
  /// Product parsing of non-finite floats, invalid enum tokens and cross-board ids is outside scope.
  /// Stored dimensions are observations, not a second source of real layout geometry.
  /// Image aspect-ratio fitting, rotated bounds, Free positions, absolute offsets and Hug envelopes
  /// must come from the implementation's geometry adapter; the model checks the mode/order decisions.

  pure def max(a: int, b: int): int = if (a > b) a else b
  pure def min(a: int, b: int): int = if (a < b) a else b
  pure def extent(n: int): int = min(MAX_EXTENT, max(0, n))
  pure def primary(g: Group): int = if (g.flow == "Column") 1 else 0
  pure def numeric(g: Group): int = if (g.gap == "Number") g.number else 0
  pure def hasItem(xs: List[int], m: int): bool = xs.indices().exists(i => xs[i] == m)
  pure def index(xs: List[int], m: int): int = xs.indices().fold(0, (n, i) => if (xs[i] == m) i else n)
  pure def insert(xs: List[int], m: int, slot: int): List[int] = {
    val k = min(xs.length(), max(0, slot))
    xs.slice(0, k).concat([m]).concat(xs.slice(k, xs.length()))
  }
  pure def owners(b: Board, m: int): Set[int] = b.groups.keys().filter(g => hasItem(b.groups.get(g).order, m))
  pure def hasGroup(b: Board, g: int): bool = b.groups.keys().contains(g)
  pure def hasMember(b: Board, g: int, m: int): bool =
    if (hasGroup(b, g)) hasItem(b.groups.get(g).order, m) else false
  pure def flowing(b: Board, g: int): List[int] =
    b.groups.get(g).order.select(m => not(b.members.get(m).absolute))
  pure def base(b: Board, m: int, a: int): int =
    if (b.members.get(m).fill.contains(a)) 0 else b.members.get(m).size.get(a)
  /// Wrap forms rows using zero minimum width for Fill members,
  /// then divides each row's leftover space.
  pure def rows(b: Board, g: int): List[List[int]] = {
    val group = b.groups.get(g)
    val xs = flowing(b, g)
    if (xs.length() == 0) []
    else if (group.flow != "Wrap") [xs]
    else {
      val partition = xs.foldl({ done: [], row: [], used: 0 }, (s, m) => {
      val w = base(b, m, 0)
      val result = if (s.row.length() == 0) w else s.used + numeric(group) + w
      if (s.row.length() > 0 and result > group.size.get(0))
        { done: s.done.append(s.row), row: [m], used: w }
      else { done: s.done, row: s.row.append(m), used: result }
    })
      partition.done.append(partition.row)
    }
  }
  /// Fixed sizes, gaps and padding-resolved capacity determine the exact rational Fill share.
  /// Auto contributes zero numeric gap: Fill consumes leftover before visual Auto distribution.
  pure def allocation(b: Board, g: int, xs: List[int], a: int): { space: int, count: int } = {
    val group = b.groups.get(g)
    val fixedSum = xs.foldl(0, (n, m) => n + base(b, m, a))
    val count = xs.select(m => b.members.get(m).fill.contains(a)).length()
    { space: max(0, group.size.get(a) - fixedSum - numeric(group) * max(0, xs.length() - 1)), count: count }
  }
  /// The exact share is space/count. Integer size observations floor fractions and saturate.
  /// Counter-axis cells and Wrap row-stack heights are geometric abstractions, not a pixel solver.
  pure def rendered(b: Board, g: int, m: int, a: int): int = {
    val group = b.groups.get(g)
    val member = b.members.get(m)
    if (member.absolute or group.flow == "Free" or not(member.fill.contains(a))) member.size.get(a)
    else if (a != primary(group)) group.size.get(a)
    else {
      val xs = if (group.flow == "Wrap") rows(b, g).foldl([], (r, x) => if (hasItem(x, m)) x else r)
               else flowing(b, g)
      val share = allocation(b, g, xs, a)
      if (share.count == 0) 0 else extent(share.space / share.count)
    }
  }
  /// An observed Hug size uses ordered extents, excludes absolute members, and keeps empty size.
  /// Free manual bounds and Wrap counter-axis bounds are supplied by geometry in production;
  /// here max(member extent) is their representative observation, with no positions modeled.
  pure def measured(b: Board, g: int, a: int): int = {
    val group = b.groups.get(g)
    val xs = flowing(b, g)
    if (group.fixed.contains(a) or xs.length() == 0) group.size.get(a)
    else if (group.flow != "Free" and a == primary(group))
      max(1, extent(xs.foldl(0, (n, m) => n + rendered(b, g, m, a)) + numeric(group) * max(0, xs.length() - 1)))
    else max(1, xs.foldl(0, (n, m) => max(n, rendered(b, g, m, a))))
  }
  /// Cache the abstract current sizes so losing the last member preserves container size.
  pure def settled(b: Board): Board = b.with("groups", b.groups.keys().mapBy(g => {
    val group = b.groups.get(g)
    group.with("size", AXES.mapBy(a => measured(b, g, a)))
  }))
  pure def defaultMember(kind: str, dimension: int): Member =
    { kind: kind, size: Map(0 -> dimension, 1 -> dimension), fill: Set(), absolute: false }
  /// Creation width is an abstract selection-bounds observation; Wrap keeps it fixed.
  pure def defaultGroup(g: int, width: int): Group =
    { order: [], flow: "Wrap", fixed: Set(0), size: Map(0 -> width, 1 -> 1),
      gap: "Number", number: 1, rowGap: 1, padding: 1,
      align: Map(0 -> 0, 1 -> 0), clip: false, name: g }
  pure def without(b: Board, m: int): Board = b.with("groups", b.groups.keys().mapBy(g =>
    b.groups.get(g).with("order", b.groups.get(g).order.select(x => x != m))))
  /// Sole element of a singleton set. TLC lacks getOnlyElement.
  pure def only(s: Set[int]): int = s.fold(-1, (_, x) => x)
  pure def freeze(b: Board, g: int, m: int): Member =
    b.members.get(m).with("size", AXES.mapBy(a => rendered(b, g, m, a)))
  /// transfer preserves absolute and sizing intent; incompatible Fill axes
  /// become Fixed at their source size when the destination hugs. Loose intake starts Fixed.
  /// Incoming fit affects only a newly arriving non-absolute member with both axes Fixed.
  /// fit only to a Fixed width, since a Hug width should absorb its content;
  /// a mixed Fill/Fixed image gets its size from layout, not a second fit pass. Free never fits intake.
  pure def attached(b: Board, g: int, m: int, slot: int): Board = {
    if (hasMember(b, g, m)) b
    else {
      val oldOwners = owners(b, m)
      val member = if (oldOwners.size() == 0) b.members.get(m)
                   else freeze(b, oldOwners.only(), m)
      val group = b.groups.get(g)
      val modes = member.fill.intersect(group.fixed)
      val fit = modes.size() == 0 and not(member.absolute) and group.fixed.contains(0) and group.flow != "Free"
      val fitted = if (fit) min(member.size.get(0), group.size.get(0)) else member.size.get(0)
      // Ratios and textbox scaling are abstracted by one width observation; no height fitting here.
      val incoming = { ...member, fill: modes, size: member.size.set(0, fitted) }
      val removed = without(b, m)
      { ...removed, members: removed.members.set(m, incoming),
        groups: removed.groups.set(g, group.with("order", insert(group.order, m, slot))) }
    }
  }
  /// Changing a Hug axis freezes Fill observations and restores the last numeric primary gap.
  pure def axisChanged(b: Board, g: int, a: int, fixed: bool, dimension: int): Board = {
    val group = b.groups.get(g)
    val nextGroup = { ...group,
      fixed: if (fixed) group.fixed.union(Set(a)) else group.fixed.exclude(Set(a)),
      size: group.size.set(a, dimension),
      gap: if (not(fixed) and group.flow != "Free" and a == primary(group)) "Number" else group.gap }
    { ...b, groups: b.groups.set(g, nextGroup), members: ITEMS.mapBy(m => {
      val member = b.members.get(m)
      if (not(fixed) and hasMember(b, g, m) and member.fill.contains(a))
        { ...member, fill: member.fill.exclude(Set(a)), size: member.size.set(a, rendered(b, g, m, a)) }
      else member
    }) }
  }
  /// Wrap width cannot become Hug; refuse that edit and Fill on a Hug axis,
  /// rather than silently change flow or container mode. Free keeps sizing validity but suspends Auto.
  pure def axisReason(b: Board, g: int, a: int, fixed: bool, dimension: int): str =
    if (not(hasGroup(b, g))) "group_missing"
    else if (not(AXES.contains(a)) or dimension < 1) "invalid_value"
    else if (not(fixed) and a == 0 and b.groups.get(g).flow == "Wrap") "wrap_width_fixed"
    else ""
  pure def memberReason(b: Board, g: int, m: int): str =
    if (not(hasGroup(b, g))) "group_missing"
    else if (not(hasMember(b, g, m))) "member_missing" else ""
  /// Resolve the directional slot once. Minus one means no neighboring flowing member.
  pure def arrowTarget(b: Board, g: int, m: int, direction: str): int = {
      val group = b.groups.get(g)
      val xs = flowing(b, g)
      val i = index(xs, m)
      val horizontal = direction == "Left" or direction == "Right"
      val delta = if (direction == "Left" or direction == "Up") -1 else 1
      val rs = rows(b, g)
      val r = rs.indices().fold(0, (n, j) => if (hasItem(rs[j], m)) j else n)
      val adjacent = r + delta
      val target = if (group.flow == "Wrap" and not(horizontal) and rs.length() > 0) {
        val column = index(rs[r], m)
        if (adjacent >= 0 and adjacent < rs.length()) {
          if (column < rs[adjacent].length()) index(group.order, rs[adjacent][column]) else -1
        } else -1
      } else if ((group.flow == "Column") != horizontal and i + delta >= 0 and i + delta < xs.length())
        index(group.order, xs[i + delta]) else -1
      if (b.members.get(m).absolute or group.flow == "Free") -1 else target
  }
  // ---- dock observations and controls ----
  /// The artboards show exactly one selected group or one edited member.
  /// Mixed/multiple selections use Hidden, like no selection; bulk editing is not implied.
  val groupDockShown = match dock.panel { | GroupDock(_) => true | _ => false }
  val memberDockShown = match dock.panel { | MemberDock(_) => true | _ => false }
  val dockGroup = match dock.panel { | GroupDock(p) => p.group | MemberDock(p) => p.group | _ => -1 }
  val dockMember = match dock.panel { | MemberDock(p) => p.member | _ => -1 }
  val alignShown = match dock.panel { | GroupDock(p) => p.alignOpen | _ => false }
  /// Display values are derived, never a second copy of membership or name state.
  val dockCount = if (dockGroup == -1) 0 else board.groups.get(dockGroup).order.length()
  val dockName = if (dockGroup == -1) -1 else board.groups.get(dockGroup).name
  val dockMemberNumber = if (memberDockShown) index(board.groups.get(dockGroup).order, dockMember) + 1 else 0
  /// Display Hug versus the observed current size; actual pixel values come from geometry.
  def axisIsHug(a: int): bool = dockGroup != -1 and not(board.groups.get(dockGroup).fixed.contains(a))
  def memberIsFill(a: int): bool = memberDockShown and board.members.get(dockMember).fill.contains(a)
  /// Row gap is present only in Wrap. Align options exist only while the popover is open.
  def controlShown(c: Control): bool = match c {
    | MemberMode(_) => memberDockShown
    | Absolute(_) => memberDockShown
    | Earlier => memberDockShown
    | Later => memberDockShown
    | Detach => memberDockShown
    | Align(_) => alignShown
    | Auto(_) => alignShown
    | RowGap(_) => groupDockShown and board.groups.get(dockGroup).flow == "Wrap"
    | _ => groupDockShown
  }
  /// Free/Absolute members retain their settings but cannot edit sizing/order.
  val memberLayoutEnabled = if (memberDockShown)
    board.groups.get(dockGroup).flow != "Free" and not(board.members.get(dockMember).absolute) else false
  /// The alignment button is dim in Free; changing to Free closes the popover.
  val alignEnabled = groupDockShown and board.groups.get(dockGroup).flow != "Free"
  /// Design enablement is stated independently from operation rejection below.
  /// Numeric gap dims in Auto; W/H numeric entry sets Fixed; Hug is a toggle.
  /// Auto exposes only the counter-axis cells on the saved primary coordinate.
  def controlEnabled(c: Control): bool = if (not(controlShown(c))) false else match c {
    | Gap(_) => board.groups.get(dockGroup).flow != "Free" and board.groups.get(dockGroup).gap == "Number"
    | Align(xy) => board.groups.get(dockGroup).flow != "Free" and
        (board.groups.get(dockGroup).gap == "Number" or xy._1 == board.groups.get(dockGroup).align.get(0) and board.groups.get(dockGroup).flow != "Column" or
          xy._2 == board.groups.get(dockGroup).align.get(1) and board.groups.get(dockGroup).flow == "Column")
    | Auto(_) => board.groups.get(dockGroup).flow != "Free"
    | Hug(a) => not(a == 0 and board.groups.get(dockGroup).flow == "Wrap")
    | MemberMode(af) => memberLayoutEnabled and (not(af._2) or board.groups.get(dockGroup).fixed.contains(af._1))
    | Earlier => memberLayoutEnabled and index(flowing(board, dockGroup), dockMember) > 0
    | Later => memberLayoutEnabled and index(flowing(board, dockGroup), dockMember) + 1 < flowing(board, dockGroup).length()
    | _ => true
  }
  /// Actual control admission, including payload validity. Valid enum/axis domains come from controls.
  /// Dimmed/hidden controls reject; they never silently change a different setting to enable themselves.
  /// Existing Auto selection intentionally fixes its primary axis, as already decided for setGap.
  def controlReason(c: Control): str = {
    val memberControl = match c {
      | MemberMode(_) => true | Absolute(_) => true | Earlier => true | Later => true | Detach => true | _ => false
    }
    if ((memberControl and not(memberDockShown)) or (not(memberControl) and not(groupDockShown))) "dock_unavailable"
    else {
      val g = board.groups.get(dockGroup)
      match c {
        | Name(n) => if (n < 0) "invalid_value" else ""
        | Gap(_) => if (g.flow == "Free" or g.gap != "Number") "control_inactive" else ""
        | RowGap(_) => if (g.flow != "Wrap") "control_inactive" else ""
        | Padding(n) => if (n < 0) "invalid_value" else ""
        | Align(xy) => if (not(alignShown)) "popover_closed" else if (g.flow == "Free") "control_inactive"
            else if (g.gap != "Number" and (if (primary(g) == 0) xy._1 else xy._2) != g.align.get(primary(g))) "primary_alignment_inactive" else ""
        | Auto(_) => if (not(alignShown)) "popover_closed" else if (g.flow == "Free") "control_inactive" else ""
        | Dimension(an) => axisReason(board, dockGroup, an._1, true, an._2)
        | Hug(a) => axisReason(board, dockGroup, a, not(g.fixed.contains(a)), 1)
        | MemberMode(af) => if (g.flow == "Free" or board.members.get(dockMember).absolute) "control_inactive"
            else if (af._2 and not(g.fixed.contains(af._1))) "fixed_axis_required" else ""
        | Earlier => if (arrowTarget(board, dockGroup, dockMember, if (g.flow == "Column") "Up" else "Left") < 0) "no_neighbor" else ""
        | Later => if (arrowTarget(board, dockGroup, dockMember, if (g.flow == "Column") "Down" else "Right") < 0) "no_neighbor" else ""
        | _ => ""
      }
    }
  }
  /// All control families, all discrete options. Small numbers stand for valid submitted values.
  pure val CONTROLS = Set(Name(0), Name(1), Gap(-1), Gap(1), RowGap(-1), RowGap(1), Padding(0), Padding(1),
      Clip, Ungroup, Earlier, Later, Detach, Absolute(0), Absolute(2))
    .union(FLOWS.map(f => Flow(f))).union(GAPS.map(g => Auto(g)))
    .union(AXES.map(a => Hug(a)))
    .union(AXES.map(a => Set(Dimension((a, 1)), Dimension((a, 3)), MemberMode((a, false)), MemberMode((a, true)))).flatten())
    .union(0.to(2).map(x => 0.to(2).map(y => Align((x, y)))).flatten())

  /// Compare the actual committed board to the previous board. These are transition properties,
  /// not independent stored domain state. A production replay adapter computes the same evidence.
  action apply(result: Board, reason: str, op: str, nextDock: Dock): bool = {
    val applied = if (reason == "") settled(result) else board
    val appliedDock = if (reason == "") normalizeDock(applied, nextDock) else dock
    val intake = Set("add", "dragIn", "paste").contains(op)
    all {
      board' = applied,
      dock' = appliedDock,
      outcome' = if (reason == "") Ok else Rejected(reason),
      event' = op,
      effects' = {
        refusal: reason == "" or (applied == board and appliedDock == dock),
        incoming: not(intake) or board.live.forall(m =>
          owners(board, m) == owners(applied, m) implies board.members.get(m) == applied.members.get(m)),
        empty: not(Set("add", "dragIn", "detach", "deleteMember").contains(op)) or
          board.groups.keys().forall(g => hasGroup(applied, g) and
            (if (applied.groups.get(g).order.length() == 0)
              applied.groups.get(g) == board.groups.get(g).with("order", []) else true)),
        resize: op != "resizeEdge" or applied.members == board.members,
        append: op != "add" or board.groups.keys().forall(g => {
          val before = board.groups.get(g).order
          val after = applied.groups.get(g).order
          after.length() <= before.length() or after.slice(0, before.length()) == before
        }),
      },
    }
  }

  /// After Detach, the canvas keeps the loose item selected and the dock hides. After Delete,
  /// the dock returns to the retained group. Transfer keeps editing the member in its destination.
  pure def normalizeDock(b: Board, d: Dock): Dock = match d.panel {
    | Hidden => d
    | GroupDock(p) => if (hasGroup(b, p.group))
        d.with("panel", GroupDock({ group: p.group, alignOpen: p.alignOpen and b.groups.get(p.group).flow != "Free" }))
      else { panel: Hidden, textFocus: false }
    | MemberDock(p) => if (hasMember(b, p.group, p.member)) d
      else if (owners(b, p.member).size() > 0)
        { panel: MemberDock({ group: owners(b, p.member).only(), member: p.member }), textFocus: false }
      else if (not(b.live.contains(p.member)) and hasGroup(b, p.group))
        { panel: GroupDock({ group: p.group, alignOpen: false }), textFocus: false }
      else { panel: Hidden, textFocus: false }
  }
  /// All board operations maintain a valid dock. Create selects its new group;
  /// member paste/duplicate selects the new member; group paste keeps the group selected.
  action commit(result: Board, reason: str, op: str): bool = {
    val addedGroups = result.groups.keys().exclude(board.groups.keys())
    val addedItems = result.live.exclude(board.live)
    val updatedDock = if (op == "create" and addedGroups.size() == 1)
      { panel: GroupDock({ group: addedGroups.only(), alignOpen: false }), textFocus: false }
      else match dock.panel {
        | MemberDock(p) => if (Set("paste", "duplicate").contains(op) and addedItems.size() == 1 and hasMember(result, p.group, addedItems.only()))
            { panel: MemberDock({ group: p.group, member: addedItems.only() }), textFocus: false } else dock
        | _ => dock
      }
    apply(result, reason, op, if (reason == "") updatedDock else dock)
  }

  // ---- operations ----
  /// Create from ordered selection. Lines/arrows, represented by id 3, stay loose.
  /// Empty eligible selection is a successful no-op as today; used group identity rejects.
  /// Selection bounds cannot be narrower than a selected member. No creation-time downscaling occurs.
  action create(g: int, selection: List[int], width: int): bool = {
    val xs = selection.select(m => board.live.contains(m))
    val reason = if (hasGroup(board, g)) "group_exists" else if (width < 1) "invalid_value" else ""
    val selectionWidth = xs.foldl(extent(width), (w, m) => {
      val own = owners(board, m)
      max(w, if (own.size() == 0) board.members.get(m).size.get(0) else rendered(board, own.only(), m, 0))
    })
    val fresh = board.with("groups", board.groups.put(g, defaultGroup(g, selectionWidth)))
    val result = xs.foldl(fresh, (b, m) => {
      val own = owners(b, m)
      val current = if (own.size() == 0) b.members.get(m) else freeze(b, own.only(), m)
      val reset = b.with("members", b.members.set(m, { ...current, fill: Set(), absolute: false }))
      attached(reset, g, m, b.groups.get(g).order.length())
    })
    commit(if (xs.length() == 0) board else result, reason, "create")
  }
  /// Picker/context addition appends. It also implements transfer and repeat-safe re-add.
  /// The finite identity must be a live board item and the destination must exist.
  action add(g: int, m: int): bool = {
    val reason = if (not(hasGroup(board, g))) "group_missing" else if (not(board.live.contains(m))) "item_missing" else ""
    val result = if (reason == "") attached(board, g, m, board.groups.get(g).order.length()) else board
    commit(result, reason, "add")
  }
  /// Drag-in uses the nearest insertion slot supplied by hit geometry. Absolute/Free positions
  /// are not represented. Slot bounds clamp to the list ends; an existing destination member is a no-op.
  action dragIn(g: int, m: int, slot: int): bool = {
    val reason = if (not(hasGroup(board, g))) "group_missing" else if (not(board.live.contains(m))) "item_missing" else ""
    commit(if (reason == "") attached(board, g, m, slot) else board, reason, "dragIn")
  }
  /// Paste receives explicit selection context: g=-1 means board, anchor=-1 means group end.
  /// No remembered destination exists here; invalid explicit context rejects rather than reroutes.
  action paste(g: int, anchor: int, m: int, width: int): bool = {
    val reason = if (board.live.contains(m)) "item_exists"
      else if (g != -1 and not(hasGroup(board, g))) "group_missing"
      else if (g != -1 and anchor != -1 and not(hasMember(board, g, anchor))) "member_missing" else ""
    val fresh = { ...board, live: board.live.union(Set(m)), members: board.members.set(m, defaultMember("Image", width)) }
    val result = if (reason != "" or g == -1) fresh else attached(fresh, g, m,
      if (anchor == -1) board.groups.get(g).order.length() else index(board.groups.get(g).order, anchor) + 1)
    commit(result, reason, "paste")
  }
  /// Duplicate uses a fresh item identity, keeps member settings and inserts after source with incoming fit.
  /// A loose source duplicates on the board. Exhausted sample identities use item_exists, not a product limit.
  action duplicate(source: int, m: int): bool = {
    val reason = if (not(board.live.contains(source))) "item_missing"
      else if (board.live.contains(m)) "item_exists" else ""
    val own = owners(board, source)
    val copy = if (own.size() == 0) board.members.get(source) else freeze(board, own.only(), source)
    val fresh = { ...board, live: board.live.union(Set(m)), members: board.members.set(m, copy) }
    val result = if (reason != "" or own.size() == 0) fresh else {
      val g = own.only()
      val group = board.groups.get(g)
      attached(fresh, g, m, index(group.order, source) + 1)
    }
    commit(result, reason, "duplicate")
  }
  /// Drag reorder removes the member before applying the nearest slot. Absolute members move
  /// freely instead, and Free has no positional layout; those drags leave order unchanged.
  action reorder(g: int, m: int, slot: int): bool = {
    val reason = memberReason(board, g, m)
    val result = if (reason != "") board else {
      val group = board.groups.get(g)
      if (board.members.get(m).absolute or group.flow == "Free") board
      else board.with("groups", board.groups.set(g, group.with("order", insert(group.order.select(x => x != m), m, slot))))
    }
    commit(result, reason, "reorder")
  }
  /// Arrows change ordered slots, with Wrap vertical arrows using the same column in adjacent rows.
  /// No slot in that row is a successful no-op. Absolute and Free arrows do not reorder.
  action arrow(g: int, m: int, direction: str): bool = {
    val reason = memberReason(board, g, m)
    val result = if (reason != "") board else {
      val group = board.groups.get(g)
      val target = arrowTarget(board, g, m, direction)
      if (board.members.get(m).absolute or group.flow == "Free" or target == -1) board
      else board.with("groups", board.groups.set(g, group.with("order", insert(group.order.select(x => x != m), m, target))))
    }
    commit(result, reason, "arrow")
  }
  /// Detach removes membership and freezes size. Drop-center/outside and final dropped position
  /// are geometry inputs, not modeled coordinates. Empty destination survives.
  action detach(g: int, m: int): bool = {
    val reason = memberReason(board, g, m)
    val result = if (reason != "") board else without(board.with("members", board.members.set(m,
      { ...freeze(board, g, m), fill: Set(), absolute: false })), m)
    commit(result, reason, "detach")
  }
  /// Turning absolute on freezes the rendered size and keeps order; off reinserts at the nearest slot.
  /// Retained Fill intent is inactive while absolute. Absolute members never contribute to Hug or rows.
  action toggleAbsolute(g: int, m: int, slot: int): bool = {
    val reason = memberReason(board, g, m)
    val result = if (reason != "") board else {
      val member = freeze(board, g, m)
      val group = board.groups.get(g)
      { ...board, members: board.members.set(m, member.with("absolute", not(member.absolute))),
        groups: board.groups.set(g, group.with("order", if (member.absolute)
          insert(group.order.select(x => x != m), m, slot) else group.order)) }
    }
    commit(result, reason, "toggleAbsolute")
  }
  /// A flow change preserves explicit sizing choices. Entering Wrap fixes width at current extent;
  /// entering Row/Column from Free reactivates Auto and fixes its new primary axis if necessary.
  /// Row/Column creation presets hug both axes; switching an existing group is not a reset to presets.
  action setFlow(g: int, flow: str): bool = {
    val reason = if (not(hasGroup(board, g))) "group_missing" else if (not(FLOWS.contains(flow))) "invalid_value" else ""
    val result = if (reason != "") board else {
      val group = board.groups.get(g)
      val entered = group.with("flow", flow)
      val required = (if (flow == "Wrap") Set(0) else Set()).union(
        if (flow != "Free" and group.gap != "Number") Set(primary(entered)) else Set())
      val members = if (flow == "Free") ITEMS.mapBy(m =>
        if (hasMember(board, g, m)) freeze(board, g, m) else board.members.get(m)) else board.members
      { ...board, members: members, groups: board.groups.set(g, entered.with("fixed", group.fixed.union(required))) }
    }
    commit(result, reason, "setFlow")
  }
  /// Numeric gaps may be negative. Auto retains the last number and fixes the current primary axis.
  /// Free records the setting without applying layout or forcing an axis.
  action setGap(g: int, gap: str, number: int): bool = {
    val reason = if (not(hasGroup(board, g))) "group_missing" else if (not(GAPS.contains(gap))) "invalid_value" else ""
    val result = if (reason != "") board else {
      val group = board.groups.get(g)
      board.with("groups", board.groups.set(g, { ...group, gap: gap,
        number: if (gap == "Number") number else group.number,
        fixed: if (gap != "Number" and group.flow != "Free") group.fixed.union(Set(primary(group))) else group.fixed }))
    }
    commit(result, reason, "setGap")
  }
  /// Direct axis mode change enforces Wrap and converts Fill to Fixed on Hug.
  action setAxisMode(g: int, a: int, fixed: bool): bool = {
    val reason = axisReason(board, g, a, fixed, 1)
    commit(if (reason == "") axisChanged(board, g, a, fixed, measured(board, g, a)) else board, reason, "setAxisMode")
  }
  /// An edge drag fixes only the dragged axis. Existing Fixed members keep their size and may overflow.
  action resizeEdge(g: int, a: int, dimension: int): bool = {
    val reason = axisReason(board, g, a, true, dimension)
    commit(if (reason == "") axisChanged(board, g, a, true, extent(dimension)) else board, reason, "resizeEdge")
  }
  /// Hug button/double-click performs the same conversion as choosing Hug for that axis.
  action hugAxis(g: int, a: int): bool = {
    val reason = axisReason(board, g, a, false, 1)
    commit(if (reason == "") axisChanged(board, g, a, false, measured(board, g, a)) else board, reason, "hugAxis")
  }
  /// Fill is available only on a Fixed container axis. Fixed freezes the current size observation.
  action setMemberMode(g: int, m: int, a: int, fill: bool): bool = {
    val missing = memberReason(board, g, m)
    val reason = if (missing != "") missing else if (fill and not(board.groups.get(g).fixed.contains(a))) "fixed_axis_required" else ""
    val result = if (reason != "") board else {
      val member = board.members.get(m)
      board.with("members", board.members.set(m, { ...member,
        fill: if (fill) member.fill.union(Set(a)) else member.fill.exclude(Set(a)),
        size: member.size.set(a, rendered(board, g, m, a)) }))
    }
    commit(result, reason, "setMemberMode")
  }
  /// Visual configuration only. Alignment, row gap, padding and clipping paint are not modeled;
  /// capacity inputs are already net of padding. Changing these does not mutate member order or sizes.
  action configure(g: int, clip: bool, alignX: int, alignY: int, padding: int, rowGap: int): bool = {
    val reason = if (not(hasGroup(board, g))) "group_missing" else ""
    val result = if (reason != "") board else board.with("groups", board.groups.set(g,
      { ...board.groups.get(g), clip: clip, align: Map(0 -> alignX, 1 -> alignY), padding: padding, rowGap: rowGap }))
    commit(result, reason, "configure")
  }
  /// Ungroup keeps all live items and their rendered size observations, including absolute members.
  /// Positions remain an external geometry obligation, not verified by this integer model.
  action ungroup(g: int): bool = {
    val reason = if (not(hasGroup(board, g))) "group_missing" else ""
    val result = if (reason != "") board else {
      val members = ITEMS.mapBy(m => if (hasMember(board, g, m))
        { ...freeze(board, g, m), fill: Set(), absolute: false } else board.members.get(m))
      { ...board, members: members, groups: board.groups.keys().exclude(Set(g)).mapBy(k => board.groups.get(k)) }
    }
    commit(result, reason, "ungroup")
  }
  /// Delete removes the item before derived layout runs; it never removes an empty group.
  action deleteMember(m: int): bool = {
    val reason = if (not(board.live.contains(m))) "item_missing" else ""
    commit(without(board, m).with("live", board.live.exclude(Set(m))), reason, "deleteMember")
  }
  /// Finite input choices for create; sample bounds are not product restrictions.
  action create_any = {
    nondet g = oneOf(GROUPS)
    nondet selection = oneOf(SELECTIONS)
    nondet width = oneOf(SIZES)
    create(g, selection, width)
  }
  /// Finite input choices for add; sample bounds are not product restrictions.
  action add_any = {
    nondet g = oneOf(GROUPS)
    nondet m = oneOf(ITEMS)
    add(g, m)
  }
  /// Finite input choices for dragIn; sample bounds are not product restrictions.
  action dragIn_any = {
    nondet g = oneOf(GROUPS)
    nondet m = oneOf(ITEMS)
    nondet slot = oneOf(0.to(3))
    dragIn(g, m, slot)
  }
  /// Finite input choices for paste; sample bounds are not product restrictions.
  action paste_any = {
    nondet g = oneOf(Set(-1, 0, 1))
    nondet anchor = oneOf(Set(-1, 0, 2))
    nondet m = oneOf(ITEMS)
    nondet width = oneOf(SIZES)
    paste(g, anchor, m, width)
  }
  /// Finite input choices for duplicate; sample bounds are not product restrictions.
  action duplicate_any = {
    nondet source = oneOf(ITEMS)
    nondet m = oneOf(ITEMS)
    duplicate(source, m)
  }
  /// Finite input choices for reorder; sample bounds are not product restrictions.
  action reorder_any = {
    nondet g = oneOf(GROUPS)
    nondet m = oneOf(ITEMS)
    nondet slot = oneOf(0.to(3))
    reorder(g, m, slot)
  }
  /// Finite input choices for arrow; sample bounds are not product restrictions.
  action arrow_any = {
    nondet g = oneOf(GROUPS)
    nondet m = oneOf(ITEMS)
    nondet direction = oneOf(Set("Left", "Right", "Up", "Down"))
    arrow(g, m, direction)
  }
  /// Finite input choices for detach; sample bounds are not product restrictions.
  action detach_any = {
    nondet g = oneOf(GROUPS)
    nondet m = oneOf(ITEMS)
    detach(g, m)
  }
  /// Finite input choices for toggleAbsolute; sample bounds are not product restrictions.
  action toggleAbsolute_any = {
    nondet g = oneOf(GROUPS)
    nondet m = oneOf(ITEMS)
    nondet slot = oneOf(0.to(3))
    toggleAbsolute(g, m, slot)
  }
  /// Finite input choices for setFlow; sample bounds are not product restrictions.
  action setFlow_any = {
    nondet g = oneOf(GROUPS)
    nondet flow = oneOf(FLOWS)
    setFlow(g, flow)
  }
  /// Finite input choices for setGap; sample bounds are not product restrictions.
  action setGap_any = {
    nondet g = oneOf(GROUPS)
    nondet gap = oneOf(GAPS)
    nondet number = oneOf(Set(-1, 0, 1))
    setGap(g, gap, number)
  }
  /// Finite input choices for setAxisMode; sample bounds are not product restrictions.
  action setAxisMode_any = {
    nondet g = oneOf(GROUPS)
    nondet a = oneOf(AXES)
    nondet fixed = oneOf(Set(false, true))
    setAxisMode(g, a, fixed)
  }
  /// Finite input choices for resizeEdge; sample bounds are not product restrictions.
  action resizeEdge_any = {
    nondet g = oneOf(GROUPS)
    nondet a = oneOf(AXES)
    nondet dimension = oneOf(Set(0, 1, 2, 3))
    resizeEdge(g, a, dimension)
  }
  /// Finite input choices for hugAxis; sample bounds are not product restrictions.
  action hugAxis_any = {
    nondet g = oneOf(GROUPS)
    nondet a = oneOf(AXES)
    hugAxis(g, a)
  }
  /// Finite input choices for setMemberMode; sample bounds are not product restrictions.
  action setMemberMode_any = {
    nondet g = oneOf(GROUPS)
    nondet m = oneOf(ITEMS)
    nondet a = oneOf(AXES)
    nondet fill = oneOf(Set(false, true))
    setMemberMode(g, m, a, fill)
  }
  /// Finite input choices for configure; sample bounds are not product restrictions.
  action configure_any = {
    nondet g = oneOf(GROUPS)
    nondet clip = oneOf(Set(false, true))
    nondet alignX = oneOf(Set(0, 1, 2))
    nondet alignY = oneOf(Set(0, 1, 2))
    nondet padding = oneOf(Set(0, 1))
    nondet rowGap = oneOf(Set(-1, 0, 1))
    configure(g, clip, alignX, alignY, padding, rowGap)
  }
  /// Finite input choices for ungroup; sample bounds are not product restrictions.
  action ungroup_any = {
    nondet g = oneOf(GROUPS)
    ungroup(g)
  }
  /// Finite input choices for deleteMember; sample bounds are not product restrictions.
  action deleteMember_any = {
    nondet m = oneOf(ITEMS)
    deleteMember(m)
  }

  // ---- dock operations ----
  /// Single group selection, including an ordinary click on one of its members, shows its dock.
  /// Switching selection always closes the prior popover and discards field focus.
  action selectGroup(g: int): bool = apply(board, if (hasGroup(board, g)) "" else "group_missing", "selectGroup",
    if (hasGroup(board, g)) { panel: GroupDock({ group: g, alignOpen: false }), textFocus: false } else dock)
  /// Double-click or primary-modifier click enters member editing directly, replacing group controls.
  action editMember(m: int): bool = {
    val own = owners(board, m)
    apply(board, if (own.size() == 0) "member_missing" else "", "editMember",
      if (own.size() > 0) { panel: MemberDock({ group: own.only(), member: m }), textFocus: false } else dock)
  }
  /// Empty-canvas, loose-item and mixed/multiple selection hide the contextual dock.
  /// Selection identities outside the dock belong to the canvas selection owner.
  action clearSelection = apply(board, "", "clearSelection", { panel: Hidden, textFocus: false })
  /// Toggle the sole second level. Choices keep it open; Esc closes it before selection.
  action toggleAlign = if (alignEnabled)
    apply(board, "", "toggleAlign", { panel: GroupDock({ group: dockGroup, alignOpen: not(alignShown) }), textFocus: false })
    else commit(board, "control_inactive", "toggleAlign")
  /// A pointer press outside both popover and trigger closes the popover
  /// and still reaches its target. The trigger only calls toggleAlign; it never also dismisses.
  /// The host then performs the target operation, including selection change. Closed is a no-op.
  action dismissAlign = apply(board, "", "dismissAlign", if (groupDockShown)
    dock.with("panel", GroupDock({ group: dockGroup, alignOpen: false })) else dock)
  /// Focus ownership only; text drafts, caret movement and parsing stay in the widget.
  /// Enter/blur submits a valid draft via useDock and releases focus; Esc discards it without a board edit.
  action focusText(active: bool): bool = apply(board, "", "focusText", dock.with("textFocus", active))
  /// Esc consumes one level: text draft, align popover, member editing, group selection.
  /// At Hidden it is a no-op here. A separate palette owns Escape while that palette has focus.
  action escape = apply(board, "", "escape", if (dock.textFocus) dock.with("textFocus", false)
    else if (alignShown) dock.with("panel", GroupDock({ group: dockGroup, alignOpen: false }))
    else if (memberDockShown) { panel: GroupDock({ group: dockGroup, alignOpen: false }), textFocus: false }
    else { panel: Hidden, textFocus: false })
  /// The one control-to-operation mapping. Unmentioned fields retain their current values.
  /// W/H edits fix that axis; clicking an active Hug toggle freezes its current size.
  action useDock(c: Control): bool = {
    val reason = controlReason(c)
    if (reason != "") commit(board, reason, "useDock") else {
      val g = dockGroup
      val member = dockMember
      val config = board.groups.get(g)
      match c {
        | Name(n) => commit(board.with("groups", board.groups.set(g, config.with("name", n))), "", "rename")
        | Flow(f) => setFlow(g, f)
        | Gap(n) => setGap(g, "Number", n)
        | RowGap(n) => configure(g, config.clip, config.align.get(0), config.align.get(1), config.padding, n)
        | Padding(n) => configure(g, config.clip, config.align.get(0), config.align.get(1), n, config.rowGap)
        | Align(xy) => configure(g, config.clip, xy._1, xy._2, config.padding, config.rowGap)
        | Auto(mode) => setGap(g, mode, config.number)
        | Dimension(an) => resizeEdge(g, an._1, an._2)
        | Hug(a) => setAxisMode(g, a, not(config.fixed.contains(a)))
        | Clip => configure(g, not(config.clip), config.align.get(0), config.align.get(1), config.padding, config.rowGap)
        | Ungroup => ungroup(g)
        | MemberMode(af) => setMemberMode(g, member, af._1, af._2)
        | Absolute(slot) => toggleAbsolute(g, member, slot)
        | Earlier => arrow(g, member, if (config.flow == "Column") "Up" else "Left")
        | Later => arrow(g, member, if (config.flow == "Column") "Down" else "Right")
        | Detach => detach(g, member)
      }
    }
  }
  /// Arrows operate on the edited flowing member only. Unavailable directions reject,
  /// including the ends, a missing Wrap column, Free, Absolute and the unused axis in Row/Column.
  def arrowEnabled(direction: str): bool = if (not(memberLayoutEnabled) or dock.textFocus) false
    else arrowTarget(board, dockGroup, dockMember, direction) >= 0
  /// Keyboard aliases resolve the same group/member arguments as the dock.
  /// Primary is Cmd on macOS, Ctrl on Windows. identity is a fresh group/item id where required;
  /// selection and width are canvas/clipboard inputs, not an implicit remembered destination.
  /// G requests the existing separate palette. Whole-group duplicate/copy/delete and uniform scale
  /// are delegated commands; this model makes no claim about their geometry or clipboard encoding.
  action pressKey(key: str, identity: int, selection: List[int], width: int): bool = {
    if (key == "Escape") escape
    else if (dock.textFocus) commit(board, "text_owns_key", "pressKey")
    else if (Set("Left", "Right", "Up", "Down").contains(key)) {
      if (arrowEnabled(key)) arrow(dockGroup, dockMember, key) else commit(board, "no_neighbor", "pressKey")
    } else if (key == "Primary+G") create(identity, selection, width)
    else if (key == "Primary+Shift+G") useDock(Ungroup)
    else if (key == "G") commit(board, "", "requestAddPalette")
    else if (key == "Primary+V") paste(dockGroup, dockMember, identity, width)
    else if (key == "Primary+D" and memberDockShown) duplicate(dockMember, identity)
    else if (key == "Delete" and memberDockShown) deleteMember(dockMember)
    else if (key == "Primary+D") commit(board, "", "requestDuplicateSelection")
    else if (key == "Primary+C") commit(board, "", "requestCopySelection")
    else if (key == "Delete") commit(board, "", "requestDeleteSelection")
    else commit(board, "unhandled_key", "pressKey")
  }
  /// A double-click on an edge handle hugs that axis; Wrap width refuses.
  action doubleClickHandle(a: int): bool = if (groupDockShown) hugAxis(dockGroup, a)
    else commit(board, "dock_unavailable", "doubleClickHandle")
  /// A corner double-click hugs both legal axes, leaving Wrap width Fixed.
  action doubleClickCorner = if (not(groupDockShown)) commit(board, "dock_unavailable", "hugCorner") else {
    val g = dockGroup
    val widthHugged = if (board.groups.get(g).flow == "Wrap") board
      else axisChanged(board, g, 0, false, measured(board, g, 0))
    commit(axisChanged(widthHugged, g, 1, false, measured(widthHugged, g, 1)), "", "hugCorner")
  }
  /// Outside drops detach flowing/Free non-absolute members. Absolute members
  /// stay attached and move manually, including outside; explicit Detach still works for them.
  action dropMember(g: int, m: int, slot: int, outside: bool): bool = {
    val reason = memberReason(board, g, m)
    if (reason != "") commit(board, reason, "dropMember")
    else if (outside and not(board.members.get(m).absolute)) detach(g, m)
    else reorder(g, m, slot)
  }
  /// Model the geometric inside/outside decision rather than continuous pointer coordinates.
  action dropMember_any = {
    nondet g = oneOf(GROUPS)
    nondet m = oneOf(ITEMS)
    nondet slot = oneOf(0.to(3))
    nondet outside = oneOf(Set(false, true))
    dropMember(g, m, slot, outside)
  }
  /// Primary+Alt drag delegates the existing whole-group scale; ordinary edge drag is resizeEdge.
  action uniformScale = if (groupDockShown) commit(board, "", "requestUniformScale")
    else commit(board, "dock_unavailable", "uniformScale")
  /// Finite group-selection attempts include missing groups.
  action selectGroup_any = { nondet g = oneOf(GROUPS) selectGroup(g) }
  /// Finite double-click/deep-select attempts include loose members.
  action editMember_any = { nondet m = oneOf(ITEMS) editMember(m) }
  /// Model focus ownership, including text editing outside this dock.
  action focusText_any = { nondet active = oneOf(Set(false, true)) focusText(active) }
  /// Every artboard control and option, plus invalid submitted name/padding/size drafts.
  action useDock_any = { nondet c = oneOf(CONTROLS.union(Set(Name(-1), Padding(-1), Dimension((0, 0))))) useDock(c) }
  /// Explore recognized keys and one unhandled key without assigning it new behavior.
  action pressKey_any = {
    nondet key = oneOf(Set("Escape", "Left", "Right", "Up", "Down", "Primary+G", "Primary+Shift+G", "G", "Primary+V", "Primary+D", "Primary+C", "Delete", "Shift+A"))
    nondet identity = oneOf(GROUPS)
    nondet selection = oneOf(SELECTIONS)
    nondet width = oneOf(SIZES)
    pressKey(key, identity, selection, width)
  }
  /// Each handle axis can be double-clicked, including forbidden Wrap width.
  action doubleClickHandle_any = { nondet a = oneOf(AXES) doubleClickHandle(a) }

  // ---- init and step ----
  /// Start with one loose image and one loose textbox; the third identity is available for copying.
  action init = all {
    board' = { groups: Map(), live: Set(0, 2), members: Map(
      0 -> defaultMember("Image", 3), 1 -> defaultMember("Image", 1), 2 -> defaultMember("Textbox", 1)) },
    dock' = { panel: Hidden, textFocus: false },
    outcome' = Ok, event' = "init", effects' = { refusal: true, incoming: true, empty: true, resize: true, append: true },
  }
  /// Explore every operation; parameter choices live exclusively in the wrappers.
  action step = any {
    create_any, add_any, dragIn_any, paste_any, duplicate_any, reorder_any, arrow_any, detach_any, toggleAbsolute_any, setFlow_any, setGap_any, setAxisMode_any, resizeEdge_any, hugAxis_any, setMemberMode_any, configure_any, ungroup_any, deleteMember_any,
    selectGroup_any, editMember_any, clearSelection, toggleAlign, dismissAlign, focusText_any, escape, useDock_any, pressKey_any, doubleClickHandle_any, doubleClickCorner, dropMember_any, uniformScale
  }

  // ---- invariants ----
  /// A member appears once at most, in one board-local flat group; only live images/textboxes qualify.
  val inv_membership = board.groups.keys().forall(g => {
    val xs = board.groups.get(g).order
    xs.indices().forall(i => board.live.contains(xs[i]) and ITEMS.contains(xs[i]) and
      xs.indices().forall(j => i == j or xs[i] != xs[j]))
  }) and ITEMS.forall(m => owners(board, m).size() <= 1)
  /// Wrap always has Fixed width, even after a flow or mode change.
  val inv_wrap_fixed = board.groups.keys().forall(g => board.groups.get(g).flow == "Wrap" implies board.groups.get(g).fixed.contains(0))
  /// Auto has a Fixed primary axis whenever layout is active. Free merely stores Auto.
  val inv_auto_fixed = board.groups.keys().forall(g => {
    val group = board.groups.get(g)
    (group.flow != "Free" and group.gap != "Number") implies group.fixed.contains(primary(group))
  })
  /// Fill never survives on a Hug container axis, including retained absolute/Free sizing intent.
  val inv_fill_fixed = board.groups.keys().forall(g => board.groups.get(g).order.indices().forall(i =>
    board.members.get(board.groups.get(g).order[i]).fill.subseteq(board.groups.get(g).fixed)))
  /// Refused operations leave the entire board unchanged.
  val inv_rejected_unchanged = effects.refusal
  /// Incoming fitting never changes an existing destination member's stored size or modes.
  val inv_incoming_isolated = effects.incoming
  /// Removing/transferring the last member retains the source identity, config and last measured size.
  val inv_empty_persists = effects.empty
  /// Container edge resizing cannot scale stored member sizes or alter member modes.
  val inv_resize_not_scale = effects.resize
  /// Picker addition appends without reordering the existing destination prefix.
  val inv_append_order = effects.append
  /// Paint precedence is derived from member order, with absolute members after all flowing members.
  val inv_paint_order = board.groups.keys().forall(g => {
    val group = board.groups.get(g)
    val paint = flowing(board, g).concat(group.order.select(m => board.members.get(m).absolute))
    paint.length() == group.order.length() and
      paint.slice(0, flowing(board, g).length()) == flowing(board, g)
  })
  /// Row partition preserves all flowing members in order and excludes absolute members.
  val inv_ordered_rows = board.groups.keys().forall(g => rows(board, g).foldl([], (xs, row) => xs.concat(row)) == flowing(board, g))
  /// A Wrap row starts only when its first result member would overflow the preceding row.
  val inv_wrap_breaks = board.groups.keys().forall(g => {
    val group = board.groups.get(g)
    val rs = rows(board, g)
    group.flow == "Wrap" implies rs.indices().forall(i => {
      val row = rs[i]
      val used = row.foldl(0, (n, m) => n + base(board, m, 0)) + numeric(group) * max(0, row.length() - 1)
      (row.length() == 1 or used <= group.size.get(0)) and
      (if (i + 1 < rs.length()) used + numeric(group) + base(board, rs[i + 1].head(), 0) > group.size.get(0) else true)
    })
  })
  /// Fixed members never shrink to fit during reflow, including when their width exceeds capacity.
  val inv_fixed_overflow = board.groups.keys().forall(g => flowing(board, g).indices().forall(i => {
    val m = flowing(board, g)[i]
    AXES.forall(a => not(board.members.get(m).fill.contains(a)) implies rendered(board, g, m, a) == board.members.get(m).size.get(a))
  }))
  /// Fill's exact rational share is nonnegative; zero leftover is allowed, and Auto spends no gap first.
  val inv_fill_leftover = board.groups.keys().forall(g => rows(board, g).indices().forall(i => {
    val group = board.groups.get(g)
    val xs = rows(board, g)[i]
    val a = primary(group)
    val share = allocation(board, g, xs, a)
    share.space >= 0 and share.count >= 0 and
      (group.gap != "Number" implies share.space == max(0, group.size.get(a) - xs.foldl(0, (n, m) => n + base(board, m, a))))
  }))
  /// Model observations remain finite; identities and names persist until explicit ungroup.
  val inv_bounds = board.groups.keys().subseteq(GROUPS) and board.live.subseteq(ITEMS) and
    ITEMS.forall(m => AXES.forall(a => board.members.get(m).size.get(a) >= 0 and board.members.get(m).size.get(a) <= MAX_EXTENT)) and
    board.groups.keys().forall(g => AXES.forall(a => board.groups.get(g).size.get(a) >= 1 and board.groups.get(g).size.get(a) <= MAX_EXTENT))

  /// Dock identity always refers to a live group/member; a Free group never owns an open popover.
  val inv_dock_selection = match dock.panel {
    | Hidden => true
    | GroupDock(p) => hasGroup(board, p.group) and (p.alignOpen implies board.groups.get(p.group).flow != "Free")
    | MemberDock(p) => hasMember(board, p.group, p.member)
  }
  /// The designer-facing enable rules equal actual admission for every valid submitted value.
  /// Invalid drafts are rejected at submission, not treated as disabled fields.
  val inv_dock_enabled_iff_accepted = CONTROLS.forall(c => controlEnabled(c) == (controlReason(c) == ""))
  /// A breadcrumb uses full membership order, including Absolute members; empty groups show zero.
  val inv_dock_count = if (memberDockShown) dockMemberNumber >= 1 and dockMemberNumber <= dockCount
    else if (groupDockShown) dockCount == board.groups.get(dockGroup).order.length() else dockCount == 0
  /// No board shortcut can consume keys that belong to a text field.
  val inv_text_owns_arrows = dock.textFocus implies Set("Left", "Right", "Up", "Down").forall(k => not(arrowEnabled(k)))

  // ---- witnesses ----
  /// create succeeds in a reachable state.
  val wit_ok_create = event == "create" and outcome == Ok
  /// add succeeds in a reachable state.
  val wit_ok_add = event == "add" and outcome == Ok
  /// dragIn succeeds in a reachable state.
  val wit_ok_dragIn = event == "dragIn" and outcome == Ok
  /// paste succeeds in a reachable state.
  val wit_ok_paste = event == "paste" and outcome == Ok
  /// duplicate succeeds in a reachable state.
  val wit_ok_duplicate = event == "duplicate" and outcome == Ok
  /// reorder succeeds in a reachable state.
  val wit_ok_reorder = event == "reorder" and outcome == Ok
  /// arrow succeeds in a reachable state.
  val wit_ok_arrow = event == "arrow" and outcome == Ok
  /// detach succeeds in a reachable state.
  val wit_ok_detach = event == "detach" and outcome == Ok
  /// toggleAbsolute succeeds in a reachable state.
  val wit_ok_toggleAbsolute = event == "toggleAbsolute" and outcome == Ok
  /// setFlow succeeds in a reachable state.
  val wit_ok_setFlow = event == "setFlow" and outcome == Ok
  /// setGap succeeds in a reachable state.
  val wit_ok_setGap = event == "setGap" and outcome == Ok
  /// setAxisMode succeeds in a reachable state.
  val wit_ok_setAxisMode = event == "setAxisMode" and outcome == Ok
  /// resizeEdge succeeds in a reachable state.
  val wit_ok_resizeEdge = event == "resizeEdge" and outcome == Ok
  /// hugAxis succeeds in a reachable state.
  val wit_ok_hugAxis = event == "hugAxis" and outcome == Ok
  /// setMemberMode succeeds in a reachable state.
  val wit_ok_setMemberMode = event == "setMemberMode" and outcome == Ok
  /// configure succeeds in a reachable state.
  val wit_ok_configure = event == "configure" and outcome == Ok
  /// ungroup succeeds in a reachable state.
  val wit_ok_ungroup = event == "ungroup" and outcome == Ok
  /// deleteMember succeeds in a reachable state.
  val wit_ok_deleteMember = event == "deleteMember" and outcome == Ok
  /// A request reaches the group_missing refusal.
  val wit_rejected_group_missing = outcome == Rejected("group_missing")
  /// A request reaches the group_exists refusal.
  val wit_rejected_group_exists = outcome == Rejected("group_exists")
  /// A request reaches the item_missing refusal.
  val wit_rejected_item_missing = outcome == Rejected("item_missing")
  /// A request reaches the item_exists refusal.
  val wit_rejected_item_exists = outcome == Rejected("item_exists")
  /// A request reaches the member_missing refusal.
  val wit_rejected_member_missing = outcome == Rejected("member_missing")
  /// A request reaches the invalid_value refusal.
  val wit_rejected_invalid_value = outcome == Rejected("invalid_value")
  /// A request reaches the wrap_width_fixed refusal.
  val wit_rejected_wrap_width_fixed = outcome == Rejected("wrap_width_fixed")
  /// A request reaches the fixed_axis_required refusal.
  val wit_rejected_fixed_axis_required = outcome == Rejected("fixed_axis_required")
  /// An empty group remains available after removals.
  val wit_empty = board.groups.keys().exists(g => board.groups.get(g).order.length() == 0)
  /// Both groups exist, exercising transfer boundaries.
  val wit_two_groups = board.groups.keys().size() == 2
  /// Three members exercise middle insertion and same-column Wrap movement.
  val wit_three_members = board.groups.keys().exists(g => board.groups.get(g).order.length() == 3)
  /// A Wrap overflow actually causes a new row.
  val wit_wrap_rows = board.groups.keys().exists(g => board.groups.get(g).flow == "Wrap" and rows(board, g).length() > 1)
  /// A Fixed member overflows a resized container.
  val wit_overflow = board.groups.keys().exists(g => flowing(board, g).indices().exists(i => {
    val m = flowing(board, g)[i]
    not(board.members.get(m).fill.contains(0)) and board.members.get(m).size.get(0) > board.groups.get(g).size.get(0)
  }))
  /// Absolute and Free configurations are reachable.
  val wit_absolute = board.groups.keys().exists(g => board.groups.get(g).order.indices().exists(i => board.members.get(board.groups.get(g).order[i]).absolute))
  /// Manual layout is a flow of the same group type.
  val wit_free = board.groups.keys().exists(g => board.groups.get(g).flow == "Free")
  /// Negative gaps remain allowed.
  val wit_negative_gap = board.groups.keys().exists(g => board.groups.get(g).gap == "Number" and board.groups.get(g).number < 0)
  /// Fill and Auto compose in a reachable active layout.
  val wit_auto_fill = board.groups.keys().exists(g => board.groups.get(g).flow != "Free" and board.groups.get(g).gap != "Number" and
    flowing(board, g).indices().exists(i => board.members.get(flowing(board, g)[i]).fill.contains(primary(board.groups.get(g)))))
  /// Fixed siblings can exhaust all space while a Fill member remains.
  val wit_zero_fill = board.groups.keys().exists(g => board.groups.get(g).flow != "Free" and rows(board, g).indices().exists(i => {
    val share = allocation(board, g, rows(board, g)[i], primary(board.groups.get(g)))
    share.count > 0 and share.space == 0
  }))

  /// The dock can be hidden, shown for an empty group, and swapped to member editing.
  val wit_dock_hidden = dock.panel == Hidden
  /// Empty groups still expose their controls and remain destinations.
  val wit_dock_empty = groupDockShown and dockCount == 0
  /// The member artboard mode is reachable with a derived breadcrumb number.
  val wit_dock_member = memberDockShown and dockMemberNumber > 0
  /// The alignment grid and Auto row are reachable together.
  val wit_dock_align = alignShown and controlEnabled(Align((0, 0)))
  /// Auto makes six alignment cells inactive without changing the saved primary coordinate.
  val wit_dock_auto_align = alignShown and board.groups.get(dockGroup).gap != "Number"
  /// Inactive layout controls are still visible in Free.
  val wit_dock_free_dimmed = groupDockShown and controlShown(Gap(1)) and not(alignEnabled)
  /// The member selectors can display retained but inactive Fill on Absolute members.
  val wit_member_absolute_dimmed = memberDockShown and board.members.get(dockMember).absolute and not(controlEnabled(MemberMode((0, true))))
  /// Every control family has an enabled context, including both sizing axes and reorder directions.
  val wit_control_name = controlEnabled(Name(0))
  /// Flow segmentation is available on a selected group.
  val wit_control_flow = controlEnabled(Flow("Free"))
  /// Numeric gap is editable with Auto off.
  val wit_control_gap = controlEnabled(Gap(-1))
  /// Wrap exposes its independent row gap.
  val wit_control_row_gap = controlEnabled(RowGap(1))
  /// Padding is editable even in Free.
  val wit_control_padding = controlEnabled(Padding(0))
  /// Auto spacing can be chosen from the open popover.
  val wit_control_auto = controlEnabled(Auto("Around"))
  /// Group width and height have usable numeric entries.
  val wit_control_dimensions = AXES.forall(a => controlEnabled(Dimension((a, 3))))
  /// Both Hug toggles are reachable outside Wrap.
  val wit_control_hug = AXES.forall(a => controlEnabled(Hug(a)))
  /// Clip and Ungroup remain group actions.
  val wit_control_clip_ungroup = controlEnabled(Clip) and controlEnabled(Ungroup)
  /// Both member Fill axes can be enabled when the parent fixes both axes.
  val wit_control_fill = AXES.forall(a => controlEnabled(MemberMode((a, true))))
  /// Fixed is available without needing a Fixed parent.
  val wit_control_fixed = AXES.forall(a => controlEnabled(MemberMode((a, false))))
  /// Absolute and Detach are usable in member editing regardless of layout participation.
  val wit_control_absolute_detach = controlEnabled(Absolute(0)) and controlEnabled(Detach)
  /// Earlier has a real predecessor.
  val wit_control_earlier = controlEnabled(Earlier)
  /// Later has a real successor.
  val wit_control_later = controlEnabled(Later)
  /// Explicit group selection can succeed.
  val wit_ok_selectGroup = event == "selectGroup" and outcome == Ok
  /// Deep selection can succeed.
  val wit_ok_editMember = event == "editMember" and outcome == Ok
  /// Leaving selection can succeed.
  val wit_ok_clearSelection = event == "clearSelection" and outcome == Ok
  /// The alignment trigger can succeed.
  val wit_ok_toggleAlign = event == "toggleAlign" and outcome == Ok
  /// Outside dismissal can succeed.
  val wit_ok_dismissAlign = event == "dismissAlign" and outcome == Ok
  /// Text focus can be acquired and released.
  val wit_ok_focusText = event == "focusText" and outcome == Ok
  /// Esc can unwind the current context.
  val wit_ok_escape = event == "escape" and outcome == Ok
  /// The editable name has a real accepting operation.
  val wit_ok_rename = event == "rename" and outcome == Ok
  /// The separate Add palette remains reachable by G.
  val wit_ok_palette = event == "requestAddPalette" and outcome == Ok
  /// Whole-group scale still routes to its existing owner.
  val wit_ok_scale = event == "requestUniformScale" and outcome == Ok
  /// Whole-selection copy remains reachable outside member editing.
  val wit_ok_copy_selection = event == "requestCopySelection" and outcome == Ok
  /// Whole-selection duplicate remains reachable outside member editing.
  val wit_ok_duplicate_selection = event == "requestDuplicateSelection" and outcome == Ok
  /// Whole-selection delete remains reachable outside member editing.
  val wit_ok_delete_selection = event == "requestDeleteSelection" and outcome == Ok
  /// Wrong dock mode refuses rather than editing a stale identity.
  val wit_rejected_dock_unavailable = outcome == Rejected("dock_unavailable")
  /// Inactive layout controls refuse rather than mutate a dormant setting.
  val wit_rejected_control_inactive = outcome == Rejected("control_inactive")
  /// Hidden popover choices cannot be invoked through the dock.
  val wit_rejected_popover_closed = outcome == Rejected("popover_closed")
  /// Auto's primary alignment cells refuse clicks.
  val wit_rejected_primary_alignment = outcome == Rejected("primary_alignment_inactive")
  /// Endpoint/unused-axis arrows have no destination.
  val wit_rejected_no_neighbor = outcome == Rejected("no_neighbor")
  /// Typing a shortcut in a field cannot edit the board.
  val wit_rejected_text_key = outcome == Rejected("text_owns_key")
  /// Unassigned Figma shortcuts stay unassigned.
  val wit_rejected_unhandled_key = outcome == Rejected("unhandled_key")

  /// Corner hugging can succeed while respecting Wrap's width restriction.
  val wit_ok_corner = event == "hugCorner" and outcome == Ok

  // ---- scenarios ----
  /// Creation includes textbox, ignores lines, starts Wrap/Fixed-width/Hug-height, and re-add is a no-op.
  run creationAndReaddTest = init.then(create(0, [0, 2, 3], 3))
    .expect(board.groups.get(0).order == [0, 2] and board.groups.get(0).fixed == Set(0) and not(board.groups.get(0).clip))
    .then(add(0, 0)).expect(board.groups.get(0).order == [0, 2])
  /// A lines-only selection creates no group.
  run ineligibleSelectionTest = init.then(create(0, [3], 3)).expect(board.groups.keys().size() == 0 and outcome == Ok)
  /// Transfer is atomic, preserves identities, and leaves the source empty and reusable.
  run transferTest = init.then(create(0, [0], 3)).then(create(1, [2], 3)).then(add(1, 0))
    .expect(board.groups.get(0).order == [] and board.groups.get(1).order == [2, 0] and board.live == Set(0, 2))
    .then(add(0, 0)).expect(board.groups.get(0).order == [0] and board.groups.get(1).order == [2])
  /// Incoming fit shrinks only the incoming image; later container resize preserves that size and overflows.
  run incomingAndOverflowTest = init.then(create(0, [2], 2)).then(add(0, 0))
    .expect(board.members.get(0).size.get(0) == 2 and board.members.get(2).size.get(0) == 1)
    .then(resizeEdge(0, 0, 1)).expect(board.members.get(0).size.get(0) == 2 and wit_overflow)
  /// Picker append fills the final row; deleting the first member closes its ordered slot.
  /// Append-stable positions apply only with fixed sizes, numeric gaps and Start
  /// alignment. Auto or non-Start alignment recomputes positions.
  run wrapAppendAndRemovalTest = init.then(create(0, [2], 3)).then(paste(0, -1, 1, 1))
    .expect(rows(board, 0) == [[2, 1]])
    .then(add(0, 0)).expect(rows(board, 0) == [[2, 1], [0]])
    .then(deleteMember(2)).expect(board.groups.get(0).order == [1, 0])
  /// Paste targets explicit member or group context; board paste is never captured.
  run pasteContextTest = init.then(create(0, [0, 2], 3)).then(paste(0, 0, 1, 1))
    .expect(board.groups.get(0).order == [0, 1, 2]).then(deleteMember(1)).then(paste(0, -1, 1, 1))
    .expect(board.groups.get(0).order == [0, 2, 1]).then(deleteMember(1)).then(paste(-1, -1, 1, 1))
    .expect(owners(board, 1).size() == 0)
  /// Duplicate preserves textbox identity kind and follows its source.
  run duplicateTest = init.then(create(0, [2, 0], 3)).then(duplicate(2, 1))
    .expect(board.groups.get(0).order == [2, 1, 0] and board.members.get(1).kind == "Textbox")
  /// Drag-in and drag reorder share insertion slots; arrows move one earlier/later.
  run insertionAndArrowsTest = init.then(create(0, [2], 3)).then(dragIn(0, 0, 0))
    .expect(board.groups.get(0).order == [0, 2]).then(reorder(0, 0, 1))
    .expect(board.groups.get(0).order == [2, 0]).then(arrow(0, 0, "Left"))
    .expect(board.groups.get(0).order == [0, 2])
  /// Wrap Up/Down selects the matching column only when the adjacent row has one.
  run wrapVerticalArrowTest = init.then(create(0, [2], 3)).then(paste(0, -1, 1, 1)).then(add(0, 0))
    .then(arrow(0, 2, "Down")).expect(board.groups.get(0).order == [1, 0, 2])
  /// Auto fixes a Hug primary axis; returning to Hug restores negative numeric gap and freezes Fill.
  run autoHugConversionTest = init.then(create(0, [2], 3)).then(setFlow(0, "Row")).then(hugAxis(0, 0))
    .then(setGap(0, "Number", -1)).then(setGap(0, "Around", 0))
    .expect(board.groups.get(0).fixed.contains(0)).then(setMemberMode(0, 2, 0, true))
    .then(hugAxis(0, 0)).expect(board.members.get(2).fill.size() == 0 and board.groups.get(0).gap == "Number" and board.groups.get(0).number == -1)
  /// Wrap refuses Hug width; Fill refuses a Hug axis; neither rejection changes valid configuration.
  run editConstraintsTest = init.then(create(0, [2], 3)).then(hugAxis(0, 0))
    .expect(outcome == Rejected("wrap_width_fixed")).then(setMemberMode(0, 2, 1, true))
    .expect(outcome == Rejected("fixed_axis_required")).then(resizeEdge(0, 1, 2)).then(setMemberMode(0, 2, 1, true))
    .then(hugAxis(0, 1)).expect(board.members.get(2).size.get(1) == 2 and board.members.get(2).fill.size() == 0)
  /// Two Fill children split leftover exactly as a rational share; Auto leaves all numeric gap budget to Fill.
  run equalFillTest = init.then(create(0, [2], 3)).then(paste(0, -1, 1, 1)).then(setFlow(0, "Row"))
    .then(setMemberMode(0, 2, 0, true)).then(setMemberMode(0, 1, 0, true))
    .expect(allocation(board, 0, [2, 1], 0) == { space: 2, count: 2 })
    .then(setGap(0, "Between", 0)).expect(allocation(board, 0, [2, 1], 0) == { space: 3, count: 2 })
  /// Oversized Fixed content exhausts Fill space instead of shrinking Fixed content.
  run zeroLeftoverTest = init.then(create(0, [0, 2], 3)).then(setFlow(0, "Row"))
    .then(setMemberMode(0, 2, 0, true)).then(resizeEdge(0, 0, 1))
    .expect(rendered(board, 0, 2, 0) == 0 and rendered(board, 0, 0, 0) == 3)
  /// Absolute leaves flow without losing membership, then reinserts at the nearest slot.
  run absoluteReinsertTest = init.then(create(0, [0, 2], 3)).then(toggleAbsolute(0, 0, 0))
    .expect(flowing(board, 0) == [2]).then(reorder(0, 0, 1)).expect(board.groups.get(0).order == [0, 2])
    .then(toggleAbsolute(0, 0, 1)).expect(board.groups.get(0).order == [2, 0])
  /// Free retains order and sizes; reactivating layout enforces Auto's new primary axis.
  run freeAndFlowTest = init.then(create(0, [0, 2], 3)).then(setFlow(0, "Free")).then(hugAxis(0, 0))
    .then(setGap(0, "Evenly", 0)).then(reorder(0, 0, 1)).expect(board.groups.get(0).order == [0, 2])
    .then(setFlow(0, "Column")).expect(board.groups.get(0).fixed.contains(1))
    .then(setFlow(0, "Wrap")).expect(board.groups.get(0).fixed.contains(0))
  /// Clip/config do not affect order, rows, or member size observations.
  run clipConfigurationTest = init.then(create(0, [0, 2], 3)).then(configure(0, true, 2, 1, 0, -1))
    .expect(board.groups.get(0).order == [0, 2] and rows(board, 0) == [[0], [2]] and board.members.get(0).size.get(0) == 3)
  /// Detach last member keeps container configuration and dimension; ungroup removes only the group.
  run emptyAndUngroupTest = init.then(create(0, [0], 3)).then(detach(0, 0))
    .expect(board.groups.get(0).order == [] and board.groups.get(0).size.get(0) == 3 and board.groups.get(0).name == 0)
    .then(add(0, 0)).then(toggleAbsolute(0, 0, 0)).then(ungroup(0))
    .expect(board.groups.keys().size() == 0 and board.live == Set(0, 2) and board.members.get(0).size.get(0) == 3)
  /// Free keeps both axes selectable and does not apply incoming-fit or retained Fill.
  run freeIgnoresLayoutTest = init.then(create(0, [2], 1)).then(setFlow(0, "Free"))
    .then(add(0, 0)).expect(board.members.get(0).size.get(0) == 3)
    .then(setMemberMode(0, 0, 0, true)).expect(rendered(board, 0, 0, 0) == 3)
    .then(hugAxis(0, 0)).expect(board.members.get(0).fill == Set() and not(board.groups.get(0).fixed.contains(0)))
  /// Moving a Fill member to a Hug destination freezes the source size instead of keeping invalid Fill.
  run transferFillToHugTest = init.then(create(0, [0], 2)).then(resizeEdge(0, 0, 2)).then(setMemberMode(0, 0, 0, true))
    .then(create(1, [2], 3)).then(setFlow(1, "Row")).then(hugAxis(1, 0)).then(add(1, 0))
    .expect(board.members.get(0).fill == Set() and board.members.get(0).size.get(0) == 2)
  /// Absolute members have no contribution to Hug, and retained Fill remains inactive until reinsertion.
  run absoluteHugExclusionTest = init.then(create(0, [0, 2], 3)).then(setFlow(0, "Row"))
    .then(toggleAbsolute(0, 0, 0)).then(hugAxis(0, 0))
    .expect(board.groups.get(0).size.get(0) == 1 and board.members.get(0).size.get(0) == 3)
  /// Column uses Up/Down, ignores Left/Right; arrows at the end are successful no-ops.
  run columnArrowsTest = init.then(create(0, [0, 2], 3)).then(setFlow(0, "Column"))
    .then(arrow(0, 0, "Right")).expect(board.groups.get(0).order == [0, 2])
    .then(arrow(0, 0, "Down")).expect(board.groups.get(0).order == [2, 0])
    .then(arrow(0, 0, "Down")).expect(board.groups.get(0).order == [2, 0] and outcome == Ok)

  /// Esc first closes alignment, then clears selection; entering member editing replaces the dock.
  run dockEscapeTest = init.then(create(0, [0, 2], 3)).then(toggleAlign)
    .then(useDock(Align((2, 1)))).expect(alignShown)
    .then(escape).expect(groupDockShown and not(alignShown))
    .then(editMember(2)).expect(memberDockShown and dockMemberNumber == 2)
    .then(escape).expect(groupDockShown).then(escape).expect(dock.panel == Hidden)
  /// Flow, gap, padding, row gap, name, numeric dimensions and Clip each change their own field.
  run dockControlsTest = init.then(create(0, [0, 2], 3)).then(useDock(Name(1)))
    .then(useDock(Gap(-1))).then(useDock(RowGap(-1))).then(useDock(Padding(0)))
    .then(useDock(Dimension((1, 3)))).then(useDock(Clip))
    .expect(dockName == 1 and board.groups.get(0).number == -1 and board.groups.get(0).rowGap == -1 and
      board.groups.get(0).padding == 0 and board.groups.get(0).fixed.contains(1) and board.groups.get(0).clip)
    .then(useDock(Flow("Free"))).expect(not(controlEnabled(Gap(1))) and not(controlShown(RowGap(1))))
    .then(useDock(Gap(1))).expect(outcome == Rejected("control_inactive"))
  /// Auto remembers the number and primary alignment, and its choices leave the popover open.
  run dockAutoTest = init.then(create(0, [0, 2], 3)).then(useDock(Gap(-1))).then(toggleAlign)
    .then(useDock(Align((2, 0)))).then(useDock(Auto("Between")))
    .expect(alignShown and not(controlEnabled(Gap(0))) and controlEnabled(Align((2, 1))) and not(controlEnabled(Align((0, 1)))))
    .then(useDock(Align((0, 1)))).expect(outcome == Rejected("primary_alignment_inactive"))
    .then(useDock(Align((2, 1)))).then(useDock(Auto("Number")))
    .expect(board.groups.get(0).number == -1 and board.groups.get(0).align == Map(0 -> 2, 1 -> 1))
    .then(useDock(Flow("Free"))).expect(not(alignShown))
  /// Wrap width cannot hug; a Hug toggle freezes or releases its axis without a size jump.
  run dockSizingTest = init.then(create(0, [2], 3)).then(useDock(Hug(0)))
    .expect(outcome == Rejected("wrap_width_fixed") and not(controlEnabled(Hug(0))))
    .then(useDock(Hug(1))).expect(not(axisIsHug(1)))
    .then(editMember(2)).then(useDock(MemberMode((1, true)))).expect(memberIsFill(1))
    .then(escape).then(useDock(Hug(1))).then(editMember(2))
    .expect(not(memberIsFill(1)) and not(controlEnabled(MemberMode((1, true)))))
    .then(useDock(MemberMode((1, true)))).expect(outcome == Rejected("fixed_axis_required"))
  /// Absolute preserves Fill intent, dims it and order controls, and can still detach.
  run dockAbsoluteTest = init.then(create(0, [0, 2], 3)).then(editMember(0))
    .then(useDock(MemberMode((0, true)))).then(useDock(Absolute(0)))
    .expect(memberIsFill(0) and not(controlEnabled(MemberMode((0, false)))) and not(controlEnabled(Later)))
    .then(useDock(Absolute(0))).expect(controlEnabled(Later))
    .then(useDock(Later)).expect(dockMemberNumber == 2 and not(controlEnabled(Later)))
    .then(useDock(Earlier)).expect(dockMemberNumber == 1)
    .then(useDock(Detach)).expect(dock.panel == Hidden and not(hasMember(board, 0, 0)))
  /// Focus consumes the first Esc; shortcuts cannot group or reorder while typing.
  run dockTextFocusTest = init.then(create(0, [0, 2], 3)).then(toggleAlign).then(focusText(true))
    .then(pressKey("Primary+G", 1, [0], 1)).expect(outcome == Rejected("text_owns_key"))
    .then(pressKey("Escape", 1, [], 1)).expect(alignShown and not(dock.textFocus))
    .then(escape).expect(groupDockShown and not(alignShown))
    .then(dismissAlign).expect(groupDockShown)
  /// Paste/duplicate select their new member; subsequent Delete returns to the retained group.
  run dockClipboardTest = init.then(create(0, [0, 2], 3)).then(editMember(0))
    .then(pressKey("Primary+D", 1, [], 1)).expect(dockMember == 1 and dockMemberNumber == 2)
    .then(pressKey("Delete", 1, [], 1)).expect(groupDockShown)
    .then(editMember(2)).then(pressKey("Primary+V", 1, [], 1))
    .expect(dockMember == 1 and board.groups.get(0).order == [0, 2, 1])
    .then(escape).then(pressKey("Primary+Shift+G", 1, [], 1)).expect(dock.panel == Hidden)
  /// Selection changes dismiss alignment; a transferred edited member keeps its editing context.
  run dockSelectionTest = init.then(create(0, [0], 3)).then(create(1, [2], 3)).then(toggleAlign)
    .then(selectGroup(0)).expect(not(alignShown)).then(editMember(0)).then(add(1, 0))
    .expect(dockGroup == 1 and dockMember == 0).then(clearSelection).expect(dock.panel == Hidden)
  /// Numeric gap/Auto conversion survives the UI path and fixes a Hug primary axis as before.
  run dockAutoFromHugTest = init.then(create(0, [2], 3)).then(useDock(Flow("Column")))
    .then(toggleAlign).then(useDock(Auto("Around"))).expect(board.groups.get(0).fixed.contains(1))
    .then(useDock(Hug(1))).expect(board.groups.get(0).gap == "Number" and alignShown)
  /// Empty groups keep the group dock after deletion; dimmed mode/order controls do not erase settings.
  run dockEmptyTest = init.then(create(0, [0], 3)).then(editMember(0))
    .then(pressKey("Delete", 1, [], 1)).expect(groupDockShown and dockCount == 0)
    .then(useDock(Ungroup)).expect(dock.panel == Hidden)
  /// A Wrap corner keeps width Fixed; Absolute outside drops retain membership until explicit Detach.
  run dockCornerAndDropTest = init.then(create(0, [0, 2], 3)).then(useDock(Dimension((1, 3))))
    .then(doubleClickCorner).expect(board.groups.get(0).fixed == Set(0))
    .then(editMember(0)).then(useDock(Absolute(0))).then(dropMember(0, 0, 0, true))
    .expect(memberDockShown and hasMember(board, 0, 0))
    .then(useDock(Absolute(0))).then(dropMember(0, 0, 0, true))
    .expect(dock.panel == Hidden and not(hasMember(board, 0, 0)))
}
