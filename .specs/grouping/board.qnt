// grouping: derived from grouping.yml operations, outcomes, and guarantees.
// Two groups and three placement identities exercise transfer, ordering, and fresh intake.
// The selected group or edited member determines the dock and keyboard context.
// Sizes, coordinates, layout algorithms, and presentation belong to spatial-layout and group-dock.
// Insertion slots and vertical Wrap target identities come from spatial-layout.
// The model samples those provider results without inventing rows or numeric sizing categories.
module board {
  pure val FLOWS = Set("Row", "Column", "Wrap", "Free")
  pure val DIRECTIONS = Set("Left", "Right", "Up", "Down")
  pure def max(a: int, b: int): int = if (a > b) a else b
  pure def min(a: int, b: int): int = if (a < b) a else b
  pure def hasItem(xs: List[int], m: int): bool = xs.indices().exists(i => xs[i] == m)
  pure def index(xs: List[int], m: int): int = xs.indices().fold(0, (n, i) => if (xs[i] == m) i else n)
  /// Insert at a slot clamped to the list ends.
  pure def insertAt(xs: List[int], m: int, slot: int): List[int] = {
    val k = min(xs.length(), max(0, slot))
    xs.slice(0, k).concat([m]).concat(xs.slice(k, xs.length()))
  }
  /// Sole element of a singleton set. TLC lacks getOnlyElement.
  pure def only(s: Set[int]): int = s.fold(-1, (_, x) => x)


  pure def earlier(flow: str): str = if (flow == "Column") "Up" else "Left"
  pure def later(flow: str): str = if (flow == "Column") "Down" else "Right"
  /// Horizontal Wrap and primary-axis Row/Column movement use membership order.
  pure def neighbor(xs: List[int], flow: str, absolute: bool, m: int, direction: str, target: int): int = {
    val horizontal = direction == "Left" or direction == "Right"
    val i = index(xs, m)
    val j = if (direction == "Left" or direction == "Up") i - 1 else i + 1
    if (flow == "Free" or absolute or not(hasItem(xs, m))) -1
    else if (flow == "Row" and not(horizontal)) -1
    else if (flow == "Column" and horizontal) -1
    else if (flow == "Wrap" and not(horizontal)) target
    else if (j >= 0 and j < xs.length()) xs[j] else -1
  }

  // ---- parameters ----
  pure val GROUPS = Set(0, 1)
  pure val ITEMS = Set(0, 1, 2)
  pure val SELECTIONS = Set([], [0], [1], [2], [0, 1], [1, 0], [0, 2], [2, 0], [1, 2], [2, 1],
    [0, 1, 2], [0, 2, 1], [1, 0, 2], [1, 2, 0], [2, 0, 1], [2, 1, 0])
  /// Insertion slots a drop can resolve to; the list ends clamp larger values.
  pure val SLOTS = 0.to(2)
  /// Keys that carry no identity or selection payload, plus one key with no binding.
  pure val PLAIN_KEYS = Set("Escape", "Left", "Right", "Up", "Down", "Primary+Shift+G", "Delete", "Shift+A")

  // ---- types ----
  type Outcome = Ok | Rejected(str)
  type Group = { order: List[int], flow: str }
  type Member = { absolute: bool }
  type Board = { groups: int -> Group, live: Set[int], members: int -> Member }
  type Panel = Hidden | GroupDock({ group: int, alignOpen: bool }) | MemberDock({ group: int, member: int })
  type Dock = { panel: Panel, textFocus: bool }
  /// The dock controls whose effect is membership or order. Sizing controls belong to spatial-layout.
  type Control = Flow(str) | Ungroup | Absolute(int) | Earlier | Later | Detach

  // ---- state ----
  /// The board owns membership lists; member records never duplicate their owner.
  var board: Board
  /// One contextual dock, with the popover owned by its group mode. Text focus captures shortcuts.
  var dock: Dock
  /// Every attempted operation succeeds or rejects without changing the board or the dock.
  var outcome: Outcome
  /// Transition evidence: a refusal changed nothing; an emptied group survived; add appended.
  var effects: { refusal: bool, empty: bool, append: bool }

  // ---- derived ----
  pure def owners(b: Board, m: int): Set[int] = b.groups.keys().filter(g => hasItem(b.groups.get(g).order, m))
  pure def hasGroup(b: Board, g: int): bool = b.groups.keys().contains(g)
  pure def hasMember(b: Board, g: int, m: int): bool = hasGroup(b, g) and hasItem(b.groups.get(g).order, m)
  /// Members that take part in flow layout, in order. Absolute members keep their slot but sit out.
  pure def flowing(b: Board, g: int): List[int] = b.groups.get(g).order.select(m => not(b.members.get(m).absolute))
  pure def memberReason(b: Board, g: int, m: int): str =
    if (not(hasGroup(b, g))) "group-missing" else if (not(hasMember(b, g, m))) "member-missing" else ""
  pure def without(b: Board, m: int): Board = b.with("groups", b.groups.keys().mapBy(g =>
    b.groups.get(g).with("order", b.groups.get(g).order.select(x => x != m))))
  /// Intake and transfer: leave any other group, then take the slot. Absolute survives a transfer.
  pure def attached(b: Board, g: int, m: int, slot: int): Board = {
    val removed = without(b, m)
    removed.with("groups", removed.groups.set(g, removed.groups.get(g).with("order", insertAt(removed.groups.get(g).order, m, slot))))
  }
  pure def plain(b: Board, m: int): Board = b.with("members", b.members.set(m, { absolute: false }))
  pure def arrowTarget(b: Board, g: int, m: int, direction: str, target: int): int =
    neighbor(flowing(b, g), b.groups.get(g).flow, b.members.get(m).absolute, m, direction, target)
  /// Creation flow preset. Sizing presets belong to spatial-layout.
  pure val PRESET_FLOW = "Wrap"

  // ---- dock observations ----
  /// The grouping interface has one selected group or one edited member.
  /// Mixed or multiple selections use Hidden, like no selection; bulk editing is not implied.
  val groupDockShown = match dock.panel { | GroupDock(_) => true | _ => false }
  val memberDockShown = match dock.panel { | MemberDock(_) => true | _ => false }
  val dockGroup = match dock.panel { | GroupDock(p) => p.group | MemberDock(p) => p.group | _ => -1 }
  val dockMember = match dock.panel { | MemberDock(p) => p.member | _ => -1 }
  val alignShown = match dock.panel { | GroupDock(p) => p.alignOpen | _ => false }
  /// Display values derive from the board; the dock never keeps a second copy.
  val dockCount = if (dockGroup == -1) 0 else board.groups.get(dockGroup).order.length()
  val dockMemberNumber = if (memberDockShown) index(board.groups.get(dockGroup).order, dockMember) + 1 else 0
  val alignEnabled = groupDockShown and board.groups.get(dockGroup).flow != "Free"
  /// Arrows operate on the edited flowing member only, never while a text field has focus.
  def arrowEnabled(direction: str, target: int): bool =
    memberDockShown and not(dock.textFocus) and arrowTarget(board, dockGroup, dockMember, direction, target) >= 0
  /// Admission for grouping controls submitted through control.
  def controlReason(c: Control): str = {
    val memberControl = match c { | Flow(_) => false | Ungroup => false | _ => true }
    if ((memberControl and not(memberDockShown)) or (not(memberControl) and not(groupDockShown))) "dock-unavailable" else match c {
      | Earlier => if (arrowTarget(board, dockGroup, dockMember, earlier(board.groups.get(dockGroup).flow), -1) < 0) "no-neighbor" else ""
      | Later => if (arrowTarget(board, dockGroup, dockMember, later(board.groups.get(dockGroup).flow), -1) < 0) "no-neighbor" else ""
      | _ => ""
    }
  }
  /// Visibility comes from the current editing mode, independently of operation admission.
  def controlShown(c: Control): bool = match c {
    | Flow(_) => groupDockShown | Ungroup => groupDockShown | _ => memberDockShown
  }
  /// Earlier and Later require a flowing neighbor; other shown valid controls are active.
  def controlEnabled(c: Control): bool = if (not(controlShown(c))) false else match c {
    | Earlier => board.groups.get(dockGroup).flow != "Free" and not(board.members.get(dockMember).absolute) and
        index(flowing(board, dockGroup), dockMember) > 0
    | Later => board.groups.get(dockGroup).flow != "Free" and not(board.members.get(dockMember).absolute) and
        index(flowing(board, dockGroup), dockMember) + 1 < flowing(board, dockGroup).length()
    | _ => true
  }
  pure val CONTROLS = Set(Ungroup, Earlier, Later, Detach, Absolute(0), Absolute(2)).union(FLOWS.map(f => Flow(f)))

  // ---- commit ----
  /// After Detach the canvas keeps the loose item selected and the dock hides. After Delete the
  /// dock returns to the retained group. Transfer keeps editing the member in its destination.
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
  action apply(result: Board, reason: str, op: str, nextDock: Dock): bool = {
    val applied = if (reason == "") result else board
    val appliedDock = if (reason == "") normalizeDock(applied, nextDock) else dock
    all {
      board' = applied,
      dock' = appliedDock,
      outcome' = if (reason == "") Ok else Rejected(reason),
      effects' = {
        refusal: reason == "" or (applied == board and appliedDock == dock),
        empty: not(Set("create", "add", "insert", "detach", "delete").contains(op)) or
          board.groups.keys().forall(g => hasGroup(applied, g) and
            (applied.groups.get(g).order.length() == 0 implies applied.groups.get(g) == board.groups.get(g).with("order", []))),
        append: op != "add" or board.groups.keys().forall(g => {
          val before = board.groups.get(g).order
          val after = applied.groups.get(g).order
          after.length() <= before.length() or after.slice(0, before.length()) == before
        }),
      },
    }
  }
  /// Every board operation keeps the dock valid. Create selects its new group; a member paste or
  /// duplicate while editing selects the new member; a group paste keeps the group selected.
  action commit(result: Board, reason: str, op: str): bool = {
    val addedGroups = result.groups.keys().exclude(board.groups.keys())
    val addedItems = result.live.exclude(board.live)
    val updated = if (op == "create" and addedGroups.size() == 1)
      { panel: GroupDock({ group: addedGroups.only(), alignOpen: false }), textFocus: false }
      else match dock.panel {
        | MemberDock(p) => if (Set("paste", "duplicate").contains(op) and addedItems.size() == 1 and hasMember(result, p.group, addedItems.only()))
            { panel: MemberDock({ group: p.group, member: addedItems.only() }), textFocus: false } else dock
        | _ => dock
      }
    apply(result, reason, op, if (reason == "") updated else dock)
  }

  // ---- board operations ----
  /// Create from the live part of an ordered selection; selected members transfer and
  /// lose Absolute. An empty selection is a successful no-op; a used group identity rejects.
  action create(g: int, selection: List[int]): bool = {
    val xs = selection.select(m => board.live.contains(m))
    val reason = if (hasGroup(board, g)) "group-exists" else ""
    val fresh = board.with("groups", board.groups.put(g, { order: [], flow: PRESET_FLOW }))
    val result = xs.foldl(fresh, (b, m) => attached(plain(b, m), g, m, b.groups.get(g).order.length()))
    commit(if (xs.length() == 0) board else result, reason, "create")
  }
  /// Picker and context addition append. Also transfer and repeat-safe re-add.
  action add(g: int, m: int): bool = {
    val reason = if (not(hasGroup(board, g))) "group-missing" else if (not(board.live.contains(m))) "item-missing" else ""
    commit(if (reason == "" and not(hasMember(board, g, m))) attached(board, g, m, board.groups.get(g).order.length()) else board, reason, "add")
  }
  /// Drag-in takes the nearest slot from hit geometry; a member already there is a no-op.
  action insert(g: int, m: int, slot: int): bool = {
    val reason = if (not(hasGroup(board, g))) "group-missing" else if (not(board.live.contains(m))) "item-missing" else ""
    commit(if (reason == "" and not(hasMember(board, g, m))) attached(board, g, m, slot) else board, reason, "insert")
  }
  /// Paste lands where the dock points: after the edited member, at the selected group's end,
  /// or loose on the board. No remembered destination captures intake.
  action paste(m: int): bool = {
    val reason = if (board.live.contains(m)) "item-exists" else ""
    val fresh = plain(board.with("live", board.live.union(Set(m))), m)
    val result = match dock.panel {
      | MemberDock(p) => attached(fresh, p.group, m, index(board.groups.get(p.group).order, p.member) + 1)
      | GroupDock(p) => attached(fresh, p.group, m, board.groups.get(p.group).order.length())
      | Hidden => fresh
    }
    commit(if (reason == "") result else board, reason, "paste")
  }
  /// Duplicate copies member settings to a fresh identity right after the source; a loose source
  /// duplicates on the board. An exhausted sample identity rejects with item_exists.
  action duplicate(source: int, m: int): bool = {
    val reason = if (not(board.live.contains(source))) "item-missing" else if (board.live.contains(m)) "item-exists" else ""
    val own = owners(board, source)
    val fresh = { ...board, live: board.live.union(Set(m)), members: board.members.set(m, board.members.get(source)) }
    val result = if (own.size() == 0) fresh else attached(fresh, own.only(), m, index(board.groups.get(own.only()).order, source) + 1)
    commit(if (reason == "") result else board, reason, "duplicate")
  }
  /// Drag reorder removes the member first, then takes the nearest slot. Absolute members move
  /// freely and Free has no positional order, so those drags leave order unchanged.
  action reorder(g: int, m: int, slot: int): bool = {
    val reason = memberReason(board, g, m)
    val group = board.groups.get(g)
    val result = if (reason != "" or board.members.get(m).absolute or group.flow == "Free") board
      else board.with("groups", board.groups.set(g, group.with("order", insertAt(group.order.select(x => x != m), m, slot))))
    commit(result, reason, "reorder")
  }
  /// An arrow swaps the member with its flowing neighbor. No neighbor is a successful no-op.
  action arrow(g: int, m: int, direction: str, target: int): bool = {
    val reason = memberReason(board, g, m)
    val group = board.groups.get(g)
    val t = if (reason == "") arrowTarget(board, g, m, direction, target) else -1
    val result = if (t == -1) board
      else board.with("groups", board.groups.set(g, group.with("order", insertAt(group.order.select(x => x != m), m, index(group.order, t)))))
    commit(result, reason, "arrow")
  }
  /// Detach removes membership and clears Absolute. The emptied group survives.
  action detach(g: int, m: int): bool = {
    val reason = memberReason(board, g, m)
    commit(if (reason == "") without(plain(board, m), m) else board, reason, "detach")
  }
  /// Absolute on keeps the slot and leaves the flow; off reinserts at the nearest slot.
  action absolute(g: int, m: int, slot: int): bool = {
    val reason = memberReason(board, g, m)
    val group = board.groups.get(g)
    val result = if (board.members.get(m).absolute)
      plain(board, m).with("groups", board.groups.set(g, group.with("order", insertAt(group.order.select(x => x != m), m, slot))))
      else board.with("members", board.members.set(m, { absolute: true }))
    commit(if (reason == "") result else board, reason, "absolute")
  }
  /// Flow conversion keeps membership order and normalizes the selected group mode.
  action flow(g: int, mode: str): bool = {
    val reason = if (not(hasGroup(board, g))) "group-missing" else ""
    commit(if (reason == "") board.with("groups", board.groups.set(g, board.groups.get(g).with("flow", mode))) else board, reason, "flow")
  }
  /// Ungroup keeps every item, including Absolute ones, and removes the group.
  action ungroup(g: int): bool = {
    val reason = if (not(hasGroup(board, g))) "group-missing" else ""
    val result = { groups: board.groups.keys().exclude(Set(g)).mapBy(k => board.groups.get(k)), live: board.live,
      members: ITEMS.mapBy(m => if (hasMember(board, g, m)) { absolute: false } else board.members.get(m)) }
    commit(if (reason == "") result else board, reason, "ungroup")
  }
  /// Delete removes the item; it never removes an emptied group.
  action delete(m: int): bool = {
    val reason = if (not(board.live.contains(m))) "item-missing" else ""
    commit(if (reason == "") without(plain(board, m), m).with("live", board.live.exclude(Set(m))) else board, reason, "delete")
  }
  action create_any = { nondet g = oneOf(GROUPS) nondet selection = oneOf(SELECTIONS) create(g, selection) }
  action add_any = { nondet g = oneOf(GROUPS) nondet m = oneOf(ITEMS) add(g, m) }
  action insert_any = { nondet g = oneOf(GROUPS) nondet m = oneOf(ITEMS) nondet slot = oneOf(SLOTS) insert(g, m, slot) }
  action paste_any = { nondet m = oneOf(ITEMS) paste(m) }
  action duplicate_any = { nondet source = oneOf(ITEMS) nondet m = oneOf(ITEMS) duplicate(source, m) }
  action arrow_any = {
    nondet g = oneOf(GROUPS) nondet m = oneOf(ITEMS) nondet direction = oneOf(DIRECTIONS)
    nondet target = oneOf(if (hasGroup(board, g) and board.groups.get(g).flow == "Wrap" and Set("Up", "Down").contains(direction)) Set(-1).union(ITEMS.filter(x => x != m and hasMember(board, g, x) and not(board.members.get(x).absolute))) else Set(-1))
    arrow(g, m, direction, target)
  }
  action detach_any = { nondet g = oneOf(GROUPS) nondet m = oneOf(ITEMS) detach(g, m) }
  action reorder_any = { nondet g = oneOf(GROUPS) nondet m = oneOf(ITEMS) nondet slot = oneOf(SLOTS) reorder(g, m, slot) }
  action absolute_any = { nondet g = oneOf(GROUPS) nondet m = oneOf(ITEMS) nondet slot = oneOf(SLOTS) absolute(g, m, slot) }
  action flow_any = { nondet g = oneOf(GROUPS) nondet mode = oneOf(FLOWS) flow(g, mode) }
  action ungroup_any = { nondet g = oneOf(GROUPS) ungroup(g) }
  action delete_any = { nondet m = oneOf(ITEMS) delete(m) }

  // ---- dock operations ----
  /// A single group selection, including an ordinary click on one of its members, shows its dock.
  /// Switching selection closes the popover and drops field focus.
  action choose(g: int): bool = apply(board, if (hasGroup(board, g)) "" else "group-missing", "choose",
    if (hasGroup(board, g)) { panel: GroupDock({ group: g, alignOpen: false }), textFocus: false } else dock)
  /// Double-click or primary-modifier click enters member editing directly.
  action edit(m: int): bool = {
    val own = owners(board, m)
    apply(board, if (own.size() == 0) "member-missing" else "", "edit",
      if (own.size() > 0) { panel: MemberDock({ group: own.only(), member: m }), textFocus: false } else dock)
  }
  /// Empty canvas, a loose item, or a mixed or multiple selection hides the dock.
  action clear = apply(board, "", "clear", { panel: Hidden, textFocus: false })
  /// The only second level. Choices keep it open; Esc closes it before selection.
  action alignment = if (alignEnabled)
    apply(board, "", "alignment", { panel: GroupDock({ group: dockGroup, alignOpen: not(alignShown) }), textFocus: false })
    else commit(board, "control-inactive", "alignment")
  /// A press outside popover and trigger closes the popover and still reaches its target.
  action dismiss = apply(board, "", "dismiss", if (groupDockShown)
    dock.with("panel", GroupDock({ group: dockGroup, alignOpen: false })) else dock)
  /// Focus ownership only; drafts, caret and parsing stay in the widget.
  action focus(active: bool): bool = apply(board, "", "focus", dock.with("textFocus", active))
  /// Esc consumes one level: text draft, popover, member editing, group selection.
  action escape = apply(board, "", "escape", if (dock.textFocus) dock.with("textFocus", false)
    else if (alignShown) dock.with("panel", GroupDock({ group: dockGroup, alignOpen: false }))
    else if (memberDockShown) { panel: GroupDock({ group: dockGroup, alignOpen: false }), textFocus: false }
    else { panel: Hidden, textFocus: false })
  /// The control-to-operation mapping for this module's controls.
  action control(c: Control): bool = {
    val reason = controlReason(c)
    if (reason != "") commit(board, reason, "control") else match c {
      | Flow(f) => flow(dockGroup, f)
      | Ungroup => ungroup(dockGroup)
      | Absolute(slot) => absolute(dockGroup, dockMember, slot)
      | Earlier => arrow(dockGroup, dockMember, earlier(board.groups.get(dockGroup).flow), -1)
      | Later => arrow(dockGroup, dockMember, later(board.groups.get(dockGroup).flow), -1)
      | Detach => detach(dockGroup, dockMember)
    }
  }
  /// Keyboard aliases resolve the same arguments as the dock. Primary is Cmd on macOS, Ctrl on
  /// Windows. identity is a fresh group or item id; selection is the canvas selection.
  /// Keys with no dock meaning fall through to their existing owner: G opens the palette and
  /// whole-selection copy, duplicate and delete stay with the canvas-interaction.
  action key(keypress: str, identity: int, selection: List[int], target: int): bool = {
    if (keypress == "Escape") escape
    else if (dock.textFocus) commit(board, "text-owns-key", "key")
    else if (DIRECTIONS.contains(keypress)) {
      if (arrowEnabled(keypress, target)) arrow(dockGroup, dockMember, keypress, target) else commit(board, "no-neighbor", "key")
    } else if (keypress == "Primary+G") create(identity, selection)
    else if (keypress == "Primary+Shift+G") control(Ungroup)
    else if (keypress == "Primary+V") paste(identity)
    else if (keypress == "Primary+D" and memberDockShown) duplicate(dockMember, identity)
    else if (keypress == "Delete" and memberDockShown) delete(dockMember)
    else commit(board, "unhandled-key", "key")
  }
  /// Outside drops detach non-absolute members, while Absolute members stay attached outside.
  action drop(g: int, m: int, slot: int, outside: bool): bool =
    if (outside and hasMember(board, g, m) and not(board.members.get(m).absolute)) detach(g, m) else reorder(g, m, slot)
  action choose_any = { nondet g = oneOf(GROUPS) choose(g) }
  action edit_any = { nondet m = oneOf(ITEMS) edit(m) }
  action focus_any = { nondet active = oneOf(Set(false, true)) focus(active) }
  action control_any = { nondet c = oneOf(CONTROLS) control(c) }
  action key_any = {
    nondet keypress = oneOf(PLAIN_KEYS.union(Set("Primary+G", "Primary+V", "Primary+D")))
    nondet identity = oneOf(if (keypress == "Primary+G") GROUPS else if (Set("Primary+V", "Primary+D").contains(keypress)) ITEMS else Set(0))
    nondet selection = oneOf(if (keypress == "Primary+G") SELECTIONS else Set([]))
    nondet target = oneOf(if (memberDockShown and board.groups.get(dockGroup).flow == "Wrap" and Set("Up", "Down").contains(keypress)) Set(-1).union(ITEMS.filter(m => m != dockMember and hasMember(board, dockGroup, m) and not(board.members.get(m).absolute))) else Set(-1))
    key(keypress, identity, selection, target)
  }
  action drop_any = {
    nondet g = oneOf(GROUPS) nondet m = oneOf(ITEMS) nondet slot = oneOf(SLOTS) nondet outside = oneOf(Set(false, true))
    drop(g, m, slot, outside)
  }

  // ---- init and step ----
  /// Two loose items; the third identity is free for paste and duplicate.
  action init = all {
    board' = { groups: Map(), live: Set(0, 2), members: ITEMS.mapBy(m => { absolute: false }) },
    dock' = { panel: Hidden, textFocus: false },
    outcome' = Ok, effects' = { refusal: true, empty: true, append: true },
  }
  action step = any {
    create_any, add_any, insert_any, paste_any, duplicate_any, reorder_any, arrow_any, detach_any, absolute_any,
    flow_any, ungroup_any, delete_any, choose_any, edit_any, clear, alignment,
    dismiss, focus_any, escape, control_any, key_any, drop_any,
  }

  // ---- invariants ----
  /// A member appears at most once, in one flat group, and is live.
  val inv_membership = board.groups.keys().forall(g => {
    val xs = board.groups.get(g).order
    xs.indices().forall(i => board.live.contains(xs[i]) and xs.indices().forall(j => i == j or xs[i] != xs[j]))
  }) and ITEMS.forall(m => owners(board, m).size() <= 1)
  /// Only a member can be Absolute.
  val inv_absolute = ITEMS.forall(m => board.members.get(m).absolute implies owners(board, m).size() == 1)
  /// Refused operations leave board and dock unchanged.
  val inv_refusal = effects.refusal
  /// Removing or transferring the last member keeps the group and its settings.
  val inv_retention = effects.empty
  /// Picker addition appends without reordering the existing prefix.
  val inv_append = effects.append
  /// The dock always refers to an existing group or member; a Free group never owns an open popover.
  val inv_selection = match dock.panel {
    | Hidden => true
    | GroupDock(p) => hasGroup(board, p.group) and (p.alignOpen implies board.groups.get(p.group).flow != "Free")
    | MemberDock(p) => hasMember(board, p.group, p.member)
  }
  /// The breadcrumb ordinal counts full membership order, Absolute included; empty groups show zero.
  val inv_ordinal = if (memberDockShown) dockMemberNumber >= 1 and dockMemberNumber <= dockCount
    else if (groupDockShown) dockCount == board.groups.get(dockGroup).order.length() else dockCount == 0
  /// No board shortcut consumes keys that belong to a text field.
  val inv_typing = dock.textFocus implies DIRECTIONS.forall(k => not(arrowEnabled(k, -1)))

  /// The mode-specific enabled state agrees with the same admission used by control.
  val inv_enablement = CONTROLS.forall(c => controlEnabled(c) == (controlReason(c) == "") and
    (controlEnabled(c) implies controlShown(c)))

  // ---- witnesses ----
  /// An accepted create call is available in this context.
  val wit_can_create = GROUPS.exists(g => not(hasGroup(board, g))) and board.live.size() > 0
  /// An accepted add call is available in this context.
  val wit_can_add = board.groups.keys().size() > 0 and board.live.size() > 0
  /// An accepted insert call is available in this context.
  val wit_can_insert = board.groups.keys().size() > 0 and board.live.size() > 0
  /// An accepted paste call is available in this context.
  val wit_can_paste = ITEMS.exists(m => not(board.live.contains(m)))
  /// An accepted duplicate call is available in this context.
  val wit_can_duplicate = board.live.size() > 0 and ITEMS.exists(m => not(board.live.contains(m)))
  /// An accepted reorder call is available in this context.
  val wit_can_reorder = board.groups.keys().exists(g => board.groups.get(g).order.length() > 0)
  /// An accepted arrow call is available in this context.
  val wit_can_arrow = board.groups.keys().exists(g => board.groups.get(g).order.length() > 0)
  /// An accepted detach call is available in this context.
  val wit_can_detach = board.groups.keys().exists(g => board.groups.get(g).order.length() > 0)
  /// An accepted absolute call is available in this context.
  val wit_can_absolute = board.groups.keys().exists(g => board.groups.get(g).order.length() > 0)
  /// An accepted flow call is available in this context.
  val wit_can_flow = board.groups.keys().size() > 0
  /// An accepted ungroup call is available in this context.
  val wit_can_ungroup = board.groups.keys().size() > 0
  /// An accepted delete call is available in this context.
  val wit_can_delete = board.live.size() > 0
  /// An accepted choose call is available in this context.
  val wit_can_choose = board.groups.keys().size() > 0
  /// An accepted edit call is available in this context.
  val wit_can_edit = board.groups.keys().exists(g => board.groups.get(g).order.length() > 0)
  /// An accepted clear call is available in this context.
  val wit_can_clear = dock.panel != Hidden
  /// An accepted alignment call is available in this context.
  val wit_can_alignment = alignEnabled
  /// An accepted dismiss call is available in this context.
  val wit_can_dismiss = alignShown
  /// An accepted focus call is available in this context.
  val wit_can_focus = groupDockShown
  /// An accepted escape call is available in this context.
  val wit_can_escape = memberDockShown or alignShown or dock.textFocus
  /// An accepted control call is available in this context.
  val wit_can_control = CONTROLS.exists(c => controlReason(c) == "")
  /// An accepted key call is available in this context.
  val wit_can_key = not(dock.textFocus) and groupDockShown
  /// An accepted drop call is available in this context.
  val wit_can_drop = board.groups.keys().exists(g => board.groups.get(g).order.length() > 0)
  /// A call actually returned group-exists.
  val wit_rejected_group_exists = outcome == Rejected("group-exists")
  /// A call actually returned group-missing.
  val wit_rejected_group_missing = outcome == Rejected("group-missing")
  /// A call actually returned item-missing.
  val wit_rejected_item_missing = outcome == Rejected("item-missing")
  /// A call actually returned item-exists.
  val wit_rejected_item_exists = outcome == Rejected("item-exists")
  /// A call actually returned member-missing.
  val wit_rejected_member_missing = outcome == Rejected("member-missing")
  /// A call actually returned control-inactive.
  val wit_rejected_control_inactive = outcome == Rejected("control-inactive")
  /// A call actually returned no-neighbor.
  val wit_rejected_no_neighbor = outcome == Rejected("no-neighbor")
  /// A call actually returned dock-unavailable.
  val wit_rejected_dock_unavailable = outcome == Rejected("dock-unavailable")
  /// A call actually returned text-owns-key.
  val wit_rejected_text_owns_key = outcome == Rejected("text-owns-key")
  /// A call actually returned unhandled-key.
  val wit_rejected_unhandled_key = outcome == Rejected("unhandled-key")
  /// Both groups hold members at once.
  val wit_two_groups = board.groups.keys().forall(g => board.groups.get(g).order.length() > 0) and board.groups.keys().size() == 2
  /// An empty group retains its dock.
  val wit_empty_group_docked = groupDockShown and dockCount == 0
  /// A member is being edited while both groups exist.
  val wit_transfer_context = memberDockShown and board.groups.keys().size() == 2
  /// The initially unused placement identity is being edited.
  val wit_fresh_member_edited = memberDockShown and dockMember == 1
  /// A group dock is available while fewer than two items are live.
  val wit_deleted_item_group_docked = groupDockShown and board.live.size() < 2
  /// A loose live placement exists while the dock is hidden.
  val wit_loose_item_hidden_dock = dock.panel == Hidden and board.live.exists(m => owners(board, m).size() == 0)
  /// A Free group has its group dock.
  val wit_free_group_docked = groupDockShown and board.groups.get(dockGroup).flow == "Free"
  /// A Wrap member is being edited.
  val wit_wrap_member_edited = memberDockShown and board.groups.get(dockGroup).flow == "Wrap"
  /// An Absolute member occupies the middle of three ordered members.
  val wit_absolute_in_middle = board.groups.keys().exists(g => { val xs = board.groups.get(g).order xs.length() == 3 and board.members.get(xs[1]).absolute })
  /// A text editor owns typing during member editing.
  val wit_text_in_member_editing = dock.textFocus and memberDockShown
  /// The group alignment popover is open.
  val wit_align_open = alignShown

  // ---- scenarios ----
  /// Create selects the new group; adding a member again is a no-op; the third identity rejects.
  run creationAndReaddTest = init.then(create(0, [0, 2]))
    .expect(board.groups.get(0).order == [0, 2] and dock.panel == GroupDock({ group: 0, alignOpen: false }))
    .then(add(0, 0)).expect(outcome == Ok and board.groups.get(0).order == [0, 2])
    .then(add(0, 1)).expect(outcome == Rejected("item-missing"))
    .then(ungroup(0)).then(create(0, [2, 0])).expect(board.groups.get(0).order == [2, 0])
  /// Paste after the edited member and select it; duplicate from the group dock does nothing.
  run pasteAndDuplicateTest = init.then(create(0, [0, 2])).then(edit(0)).then(key("Primary+V", 1, [], -1))
    .expect(board.groups.get(0).order == [0, 1, 2] and dock.panel == MemberDock({ group: 0, member: 1 }))
    .then(key("Delete", 0, [], -1)).expect(board.live == Set(0, 2) and dock.panel == GroupDock({ group: 0, alignOpen: false }))
    .then(key("Primary+D", 1, [], -1)).expect(outcome == Rejected("unhandled-key") and board.live == Set(0, 2))
  /// Arrows follow the primary axis; Free and Absolute never reorder; the end refuses.
  run arrowTest = init.then(paste(1)).then(create(0, [0, 1, 2])).then(flow(0, "Column")).then(edit(0))
    .then(key("Down", 0, [], -1)).expect(board.groups.get(0).order == [1, 0, 2])
    .then(key("Right", 0, [], -1)).expect(outcome == Rejected("no-neighbor"))
    .then(control(Absolute(0))).then(control(Later)).expect(outcome == Rejected("no-neighbor") and board.groups.get(0).order == [1, 0, 2])
    .then(control(Absolute(2))).expect(board.groups.get(0).order == [1, 2, 0] and not(board.members.get(0).absolute))
    .then(flow(0, "Free")).then(control(Earlier)).expect(outcome == Rejected("no-neighbor"))
  /// A vertical Wrap key uses the provider's target identity, not a synthetic row.
  run verticalTargetTest = init.then(paste(1)).then(create(0, [0, 1, 2])).then(edit(0))
    .then(key("Down", 0, [], 2)).expect(board.groups.get(0).order == [1, 2, 0])
    .then(key("Down", 0, [], -1)).expect(outcome == Rejected("no-neighbor") and board.groups.get(0).order == [1, 2, 0])
  /// Transfer keeps editing in the destination; detach hides the dock and the empty group survives.
  run transferAndDetachTest = init.then(create(0, [0])).then(create(1, [2])).then(edit(0)).then(add(1, 0))
    .expect(board.groups.get(0).order == [] and board.groups.get(1).order == [2, 0] and dock.panel == MemberDock({ group: 1, member: 0 }))
    .then(control(Detach)).expect(dock.panel == Hidden and owners(board, 0) == Set() and hasGroup(board, 1))
    .then(choose(0)).expect(dockCount == 0)
  /// Esc walks text focus, popover, member editing and selection; text focus blocks arrows.
  run escapeTest = init.then(create(0, [0, 2])).then(alignment).then(focus(true)).then(key("Left", 0, [], -1))
    .expect(outcome == Rejected("text-owns-key")).then(escape).expect(alignShown and not(dock.textFocus))
    .then(escape).expect(dock.panel == GroupDock({ group: 0, alignOpen: false }))
    .then(edit(2)).then(escape).expect(groupDockShown).then(escape).expect(dock.panel == Hidden)
  /// Free closes the popover and refuses to reopen it; ungroup frees Absolute members too.
  run freeAndUngroupTest = init.then(create(0, [0, 2])).then(edit(2)).then(control(Absolute(0))).then(escape)
    .then(alignment).then(control(Flow("Free"))).expect(not(alignShown))
    .then(alignment).expect(outcome == Rejected("control-inactive"))
    .then(key("Primary+Shift+G", 0, [], -1)).expect(board.groups.keys() == Set() and dock.panel == Hidden and not(board.members.get(2).absolute))
  /// An outside drop detaches a flowing member but keeps an Absolute one attached.
  run dropTest = init.then(create(0, [0, 2])).then(absolute(0, 2, 0)).then(drop(0, 2, 0, true))
    .expect(board.groups.get(0).order == [0, 2]).then(drop(0, 0, 0, true)).expect(board.groups.get(0).order == [2])
}
