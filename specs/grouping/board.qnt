// SnappyRef group membership, order and the contextual dock
// Which items belong to which group and in what order, how creation, intake, transfer, arrows,
// drags, Absolute, Detach, Ungroup, paste, duplicate and delete change that, and what the dock
// shows afterwards: one selected group or one edited member, the alignment popover, and text
// focus that owns the keyboard. Two groups and three items cover transfer and a fresh identity.
// Sizes are absent, so Wrap rows are one member each and every Wrap arrow is the linear
// neighbor; layout.qnt owns real rows and same-column arrows. Modes are sizing.qnt's, control
// enablement is enablement.qnt's. Item kinds are absent: lines and arrows never enter a
// selection here; ui.md keeps that filter. Ordinary group drag, uniform scale, whole-selection
// copy, duplicate and delete are delegated commands with no claim beyond dispatch.
module board {
  import rules.* from "./rules"
  // ---- parameters ----
  pure val GROUPS = Set(0, 1)
  pure val ITEMS = Set(0, 1, 2)
  pure val ITEM_LIST = [0, 1, 2]
  pure val SELECTIONS = ITEMS.powerset()
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
  /// The dock controls whose effect is membership or order. The others are enablement.qnt's.
  type Control = Flow(str) | Ungroup | Absolute(int) | Earlier | Later | Detach

  // ---- state ----
  /// The board owns membership lists; member records never duplicate their owner.
  var board: Board
  /// One contextual dock, with the popover owned by its group mode. Text focus captures shortcuts.
  var dock: Dock
  /// Every attempted operation succeeds or rejects without changing the board or the dock.
  var outcome: Outcome
  /// Last accepted operation, or "rejected", for coverage.
  var event: str
  /// Transition evidence: a refusal changed nothing; an emptied group survived; add appended.
  var effects: { refusal: bool, empty: bool, append: bool }

  // ---- derived ----
  pure def owners(b: Board, m: int): Set[int] = b.groups.keys().filter(g => hasItem(b.groups.get(g).order, m))
  pure def hasGroup(b: Board, g: int): bool = b.groups.keys().contains(g)
  pure def hasMember(b: Board, g: int, m: int): bool = hasGroup(b, g) and hasItem(b.groups.get(g).order, m)
  /// Members that take part in flow layout, in order. Absolute members keep their slot but sit out.
  pure def flowing(b: Board, g: int): List[int] = b.groups.get(g).order.select(m => not(b.members.get(m).absolute))
  pure def memberReason(b: Board, g: int, m: int): str =
    if (not(hasGroup(b, g))) "group_missing" else if (not(hasMember(b, g, m))) "member_missing" else ""
  pure def without(b: Board, m: int): Board = b.with("groups", b.groups.keys().mapBy(g =>
    b.groups.get(g).with("order", b.groups.get(g).order.select(x => x != m))))
  /// Intake and transfer: leave any other group, then take the slot. Absolute survives a transfer.
  pure def attached(b: Board, g: int, m: int, slot: int): Board = {
    val removed = without(b, m)
    removed.with("groups", removed.groups.set(g, removed.groups.get(g).with("order", insert(removed.groups.get(g).order, m, slot))))
  }
  pure def plain(b: Board, m: int): Board = b.with("members", b.members.set(m, { absolute: false }))
  pure def target(b: Board, g: int, m: int, direction: str): int =
    neighbor(flowing(b, g), b.groups.get(g).flow, b.members.get(m).absolute, m, direction)
  /// Creation flow preset. Sizing presets are sizing.qnt's.
  pure val PRESET_FLOW = "Wrap"

  // ---- dock observations ----
  /// The artboards show exactly one selected group or one edited member.
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
  def arrowEnabled(direction: str): bool =
    memberDockShown and not(dock.textFocus) and target(board, dockGroup, dockMember, direction) >= 0
  /// Admission for this module's controls. enablement.qnt states the same rules for every control.
  def controlReason(c: Control): str = {
    val memberControl = match c { | Flow(_) => false | Ungroup => false | _ => true }
    if ((memberControl and not(memberDockShown)) or (not(memberControl) and not(groupDockShown))) "dock_unavailable" else match c {
      | Earlier => arrowReason(flowing(board, dockGroup), board.groups.get(dockGroup).flow, board.members.get(dockMember).absolute, dockMember, earlier(board.groups.get(dockGroup).flow))
      | Later => arrowReason(flowing(board, dockGroup), board.groups.get(dockGroup).flow, board.members.get(dockMember).absolute, dockMember, later(board.groups.get(dockGroup).flow))
      | _ => ""
    }
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
      event' = if (reason == "") op else "rejected",
      effects' = {
        refusal: reason == "" or (applied == board and appliedDock == dock),
        empty: not(Set("create", "add", "dragIn", "detach", "deleteMember").contains(op)) or
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
  /// Create from the live part of a selection, in board order; selected members transfer and
  /// lose Absolute. An empty selection is a successful no-op; a used group identity rejects.
  action create(g: int, selection: Set[int]): bool = {
    val xs = ITEM_LIST.select(m => selection.contains(m) and board.live.contains(m))
    val reason = if (hasGroup(board, g)) "group_exists" else ""
    val fresh = board.with("groups", board.groups.put(g, { order: [], flow: PRESET_FLOW }))
    val result = xs.foldl(fresh, (b, m) => attached(plain(b, m), g, m, b.groups.get(g).order.length()))
    commit(if (xs.length() == 0) board else result, reason, "create")
  }
  /// Picker and context addition append. Also transfer and repeat-safe re-add.
  action add(g: int, m: int): bool = {
    val reason = if (not(hasGroup(board, g))) "group_missing" else if (not(board.live.contains(m))) "item_missing" else ""
    commit(if (reason == "" and not(hasMember(board, g, m))) attached(board, g, m, board.groups.get(g).order.length()) else board, reason, "add")
  }
  /// Drag-in takes the nearest slot from hit geometry; a member already there is a no-op.
  action dragIn(g: int, m: int, slot: int): bool = {
    val reason = if (not(hasGroup(board, g))) "group_missing" else if (not(board.live.contains(m))) "item_missing" else ""
    commit(if (reason == "" and not(hasMember(board, g, m))) attached(board, g, m, slot) else board, reason, "dragIn")
  }
  /// Paste lands where the dock points: after the edited member, at the selected group's end,
  /// or loose on the board. No remembered destination captures intake.
  action paste(m: int): bool = {
    val reason = if (board.live.contains(m)) "item_exists" else ""
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
    val reason = if (not(board.live.contains(source))) "item_missing" else if (board.live.contains(m)) "item_exists" else ""
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
      else board.with("groups", board.groups.set(g, group.with("order", insert(group.order.select(x => x != m), m, slot))))
    commit(result, reason, "reorder")
  }
  /// An arrow swaps the member with its flowing neighbor. No neighbor is a successful no-op.
  action arrow(g: int, m: int, direction: str): bool = {
    val reason = memberReason(board, g, m)
    val group = board.groups.get(g)
    val t = if (reason == "") target(board, g, m, direction) else -1
    val result = if (t == -1) board
      else board.with("groups", board.groups.set(g, group.with("order", insert(group.order.select(x => x != m), m, index(group.order, t)))))
    commit(result, reason, "arrow")
  }
  /// Detach removes membership and clears Absolute. The emptied group survives.
  action detach(g: int, m: int): bool = {
    val reason = memberReason(board, g, m)
    commit(if (reason == "") without(plain(board, m), m) else board, reason, "detach")
  }
  /// Absolute on keeps the slot and leaves the flow; off reinserts at the nearest slot.
  action toggleAbsolute(g: int, m: int, slot: int): bool = {
    val reason = memberReason(board, g, m)
    val group = board.groups.get(g)
    val result = if (board.members.get(m).absolute)
      plain(board, m).with("groups", board.groups.set(g, group.with("order", insert(group.order.select(x => x != m), m, slot))))
      else board.with("members", board.members.set(m, { absolute: true }))
    commit(if (reason == "") result else board, reason, "toggleAbsolute")
  }
  action setFlow(g: int, flow: str): bool = {
    val reason = if (not(hasGroup(board, g))) "group_missing" else if (not(FLOWS.contains(flow))) "invalid_value" else ""
    commit(if (reason == "") board.with("groups", board.groups.set(g, board.groups.get(g).with("flow", flow))) else board, reason, "setFlow")
  }
  /// Ungroup keeps every item, including Absolute ones, and removes the group.
  action ungroup(g: int): bool = {
    val reason = if (not(hasGroup(board, g))) "group_missing" else ""
    val result = { groups: board.groups.keys().exclude(Set(g)).mapBy(k => board.groups.get(k)), live: board.live,
      members: ITEMS.mapBy(m => if (hasMember(board, g, m)) { absolute: false } else board.members.get(m)) }
    commit(if (reason == "") result else board, reason, "ungroup")
  }
  /// Delete removes the item; it never removes an emptied group.
  action deleteMember(m: int): bool = {
    val reason = if (not(board.live.contains(m))) "item_missing" else ""
    commit(if (reason == "") without(plain(board, m), m).with("live", board.live.exclude(Set(m))) else board, reason, "deleteMember")
  }
  action create_any = { nondet g = oneOf(GROUPS) nondet selection = oneOf(SELECTIONS) create(g, selection) }
  action add_any = { nondet g = oneOf(GROUPS) nondet m = oneOf(ITEMS) add(g, m) }
  action dragIn_any = { nondet g = oneOf(GROUPS) nondet m = oneOf(ITEMS) nondet slot = oneOf(SLOTS) dragIn(g, m, slot) }
  action paste_any = { nondet m = oneOf(ITEMS) paste(m) }
  action duplicate_any = { nondet source = oneOf(ITEMS) nondet m = oneOf(ITEMS) duplicate(source, m) }
  action arrow_any = { nondet g = oneOf(GROUPS) nondet m = oneOf(ITEMS) nondet direction = oneOf(DIRECTIONS) arrow(g, m, direction) }
  action detach_any = { nondet g = oneOf(GROUPS) nondet m = oneOf(ITEMS) detach(g, m) }
  action reorder_any = { nondet g = oneOf(GROUPS) nondet m = oneOf(ITEMS) nondet slot = oneOf(SLOTS) reorder(g, m, slot) }
  action toggleAbsolute_any = { nondet g = oneOf(GROUPS) nondet m = oneOf(ITEMS) nondet slot = oneOf(SLOTS) toggleAbsolute(g, m, slot) }
  action setFlow_any = { nondet g = oneOf(GROUPS) nondet flow = oneOf(FLOWS) setFlow(g, flow) }
  action ungroup_any = { nondet g = oneOf(GROUPS) ungroup(g) }
  action deleteMember_any = { nondet m = oneOf(ITEMS) deleteMember(m) }

  // ---- dock operations ----
  /// A single group selection, including an ordinary click on one of its members, shows its dock.
  /// Switching selection closes the popover and drops field focus.
  action selectGroup(g: int): bool = apply(board, if (hasGroup(board, g)) "" else "group_missing", "selectGroup",
    if (hasGroup(board, g)) { panel: GroupDock({ group: g, alignOpen: false }), textFocus: false } else dock)
  /// Double-click or primary-modifier click enters member editing directly.
  action editMember(m: int): bool = {
    val own = owners(board, m)
    apply(board, if (own.size() == 0) "member_missing" else "", "editMember",
      if (own.size() > 0) { panel: MemberDock({ group: own.only(), member: m }), textFocus: false } else dock)
  }
  /// Empty canvas, a loose item, or a mixed or multiple selection hides the dock.
  action clearSelection = apply(board, "", "clearSelection", { panel: Hidden, textFocus: false })
  /// The only second level. Choices keep it open; Esc closes it before selection.
  action toggleAlign = if (alignEnabled)
    apply(board, "", "toggleAlign", { panel: GroupDock({ group: dockGroup, alignOpen: not(alignShown) }), textFocus: false })
    else commit(board, "control_inactive", "toggleAlign")
  /// A press outside popover and trigger closes the popover and still reaches its target.
  action dismissAlign = apply(board, "", "dismissAlign", if (groupDockShown)
    dock.with("panel", GroupDock({ group: dockGroup, alignOpen: false })) else dock)
  /// Focus ownership only; drafts, caret and parsing stay in the widget.
  action focusText(active: bool): bool = apply(board, "", "focusText", dock.with("textFocus", active))
  /// Esc consumes one level: text draft, popover, member editing, group selection.
  action escape = apply(board, "", "escape", if (dock.textFocus) dock.with("textFocus", false)
    else if (alignShown) dock.with("panel", GroupDock({ group: dockGroup, alignOpen: false }))
    else if (memberDockShown) { panel: GroupDock({ group: dockGroup, alignOpen: false }), textFocus: false }
    else { panel: Hidden, textFocus: false })
  /// The control-to-operation mapping for this module's controls.
  action useDock(c: Control): bool = {
    val reason = controlReason(c)
    if (reason != "") commit(board, reason, "useDock") else match c {
      | Flow(f) => setFlow(dockGroup, f)
      | Ungroup => ungroup(dockGroup)
      | Absolute(slot) => toggleAbsolute(dockGroup, dockMember, slot)
      | Earlier => arrow(dockGroup, dockMember, earlier(board.groups.get(dockGroup).flow))
      | Later => arrow(dockGroup, dockMember, later(board.groups.get(dockGroup).flow))
      | Detach => detach(dockGroup, dockMember)
    }
  }
  /// Keyboard aliases resolve the same arguments as the dock. Primary is Cmd on macOS, Ctrl on
  /// Windows. identity is a fresh group or item id; selection is the canvas selection.
  /// Keys with no dock meaning fall through to their existing owner: G opens the palette and
  /// whole-selection copy, duplicate and delete stay with the canvas (ui.md).
  action pressKey(key: str, identity: int, selection: Set[int]): bool = {
    if (key == "Escape") escape
    else if (dock.textFocus) commit(board, "text_owns_key", "pressKey")
    else if (DIRECTIONS.contains(key)) {
      if (arrowEnabled(key)) arrow(dockGroup, dockMember, key) else commit(board, "no_neighbor", "pressKey")
    } else if (key == "Primary+G") create(identity, selection)
    else if (key == "Primary+Shift+G") useDock(Ungroup)
    else if (key == "Primary+V") paste(identity)
    else if (key == "Primary+D" and memberDockShown) duplicate(dockMember, identity)
    else if (key == "Delete" and memberDockShown) deleteMember(dockMember)
    else commit(board, "unhandled_key", "pressKey")
  }
  /// Outside drops detach flowing and Free non-absolute members. Absolute members stay attached
  /// and move manually, including outside; explicit Detach still works for them. Pure dispatch,
  /// so the random and exhaustive searches leave it to dropTest.
  action dropMember(g: int, m: int, slot: int, outside: bool): bool =
    if (outside and hasMember(board, g, m) and not(board.members.get(m).absolute)) detach(g, m) else reorder(g, m, slot)
  action selectGroup_any = { nondet g = oneOf(GROUPS) selectGroup(g) }
  action editMember_any = { nondet m = oneOf(ITEMS) editMember(m) }
  action focusText_any = { nondet active = oneOf(Set(false, true)) focusText(active) }
  action useDock_any = { nondet c = oneOf(CONTROLS) useDock(c) }
  action pressPlainKey = { nondet key = oneOf(PLAIN_KEYS) pressKey(key, 0, Set()) }
  action pressCreateKey = { nondet identity = oneOf(GROUPS) nondet selection = oneOf(SELECTIONS) pressKey("Primary+G", identity, selection) }
  action pressPasteKey = { nondet identity = oneOf(ITEMS) pressKey("Primary+V", identity, Set()) }
  action pressDuplicateKey = { nondet identity = oneOf(ITEMS) pressKey("Primary+D", identity, Set()) }
  action pressKey_any = any { pressPlainKey, pressCreateKey, pressPasteKey, pressDuplicateKey }

  // ---- init and step ----
  /// Two loose items; the third identity is free for paste and duplicate.
  action init = all {
    board' = { groups: Map(), live: Set(0, 2), members: ITEMS.mapBy(m => { absolute: false }) },
    dock' = { panel: Hidden, textFocus: false },
    outcome' = Ok, event' = "init", effects' = { refusal: true, empty: true, append: true },
  }
  action step = any {
    create_any, add_any, dragIn_any, paste_any, duplicate_any, reorder_any, arrow_any, detach_any, toggleAbsolute_any,
    setFlow_any, ungroup_any, deleteMember_any, selectGroup_any, editMember_any, clearSelection, toggleAlign,
    dismissAlign, focusText_any, escape, useDock_any, pressKey_any,
  }

  // ---- invariants ----
  /// A member appears at most once, in one flat group, and is live.
  val inv_membership = board.groups.keys().forall(g => {
    val xs = board.groups.get(g).order
    xs.indices().forall(i => board.live.contains(xs[i]) and xs.indices().forall(j => i == j or xs[i] != xs[j]))
  }) and ITEMS.forall(m => owners(board, m).size() <= 1)
  /// Only a member can be Absolute.
  val inv_loose_plain = ITEMS.forall(m => board.members.get(m).absolute implies owners(board, m).size() == 1)
  /// Refused operations leave board and dock unchanged.
  val inv_rejected_unchanged = effects.refusal
  /// Removing or transferring the last member keeps the group and its settings.
  val inv_empty_persists = effects.empty
  /// Picker addition appends without reordering the existing prefix.
  val inv_append_order = effects.append
  /// The dock always refers to an existing group or member; a Free group never owns an open popover.
  val inv_dock_selection = match dock.panel {
    | Hidden => true
    | GroupDock(p) => hasGroup(board, p.group) and (p.alignOpen implies board.groups.get(p.group).flow != "Free")
    | MemberDock(p) => hasMember(board, p.group, p.member)
  }
  /// The breadcrumb ordinal counts full membership order, Absolute included; empty groups show zero.
  val inv_dock_count = if (memberDockShown) dockMemberNumber >= 1 and dockMemberNumber <= dockCount
    else if (groupDockShown) dockCount == board.groups.get(dockGroup).order.length() else dockCount == 0
  /// No board shortcut consumes keys that belong to a text field.
  val inv_text_owns_arrows = dock.textFocus implies DIRECTIONS.forall(k => not(arrowEnabled(k)))

  // ---- witnesses ----
  val wit_ok_create = event == "create" and outcome == Ok and board.groups.keys().size() > 0
  val wit_ok_add = event == "add" and outcome == Ok
  val wit_ok_dragIn = event == "dragIn" and outcome == Ok
  val wit_ok_paste = event == "paste" and outcome == Ok
  val wit_ok_duplicate = event == "duplicate" and outcome == Ok
  val wit_ok_reorder = event == "reorder" and outcome == Ok
  val wit_ok_arrow = event == "arrow" and outcome == Ok
  val wit_ok_detach = event == "detach" and outcome == Ok
  val wit_ok_toggleAbsolute = event == "toggleAbsolute" and outcome == Ok
  val wit_ok_setFlow = event == "setFlow" and outcome == Ok
  val wit_ok_ungroup = event == "ungroup" and outcome == Ok
  val wit_ok_deleteMember = event == "deleteMember" and outcome == Ok
  val wit_ok_selectGroup = event == "selectGroup" and outcome == Ok
  val wit_ok_editMember = event == "editMember" and outcome == Ok
  val wit_ok_toggleAlign = event == "toggleAlign" and outcome == Ok
  val wit_rejected_group_exists = outcome == Rejected("group_exists")
  val wit_rejected_group_missing = outcome == Rejected("group_missing")
  val wit_rejected_item_missing = outcome == Rejected("item_missing")
  val wit_rejected_item_exists = outcome == Rejected("item_exists")
  val wit_rejected_member_missing = outcome == Rejected("member_missing")
  val wit_rejected_control_inactive = outcome == Rejected("control_inactive")
  val wit_rejected_no_neighbor = outcome == Rejected("no_neighbor")
  val wit_rejected_dock_unavailable = outcome == Rejected("dock_unavailable")
  val wit_rejected_text_owns_key = outcome == Rejected("text_owns_key")
  val wit_rejected_unhandled_key = outcome == Rejected("unhandled_key")
  /// Both groups hold members at once.
  val wit_two_groups = board.groups.keys().forall(g => board.groups.get(g).order.length() > 0) and board.groups.keys().size() == 2
  /// A group keeps its dock after its last member left.
  val wit_empty_group_docked = groupDockShown and dockCount == 0
  /// Editing survived a transfer into the other group.
  val wit_transfer_keeps_editing = event == "add" and outcome == Ok and memberDockShown and board.groups.keys().size() == 2
  /// A member paste selected the new member.
  val wit_paste_selects_member = event == "paste" and outcome == Ok and memberDockShown and dockMember == 1
  /// Delete returned to the group dock.
  val wit_delete_returns_to_group = event == "deleteMember" and outcome == Ok and groupDockShown
  /// Detach hid the dock.
  val wit_detach_hides = event == "detach" and outcome == Ok and dock.panel == Hidden
  /// Flow Free closed the popover.
  val wit_free_closes_popover = event == "setFlow" and outcome == Ok and groupDockShown and board.groups.get(dockGroup).flow == "Free"
  /// A Wrap Up or Down arrow moved a member; with one member per row that is the linear neighbor.
  val wit_wrap_vertical_arrow = event == "arrow" and outcome == Ok and memberDockShown and board.groups.get(dockGroup).flow == "Wrap"
  /// An Absolute member sits inside a group with three members, in the middle of the order.
  val wit_absolute_in_middle = board.groups.keys().exists(g => {
    val xs = board.groups.get(g).order
    xs.length() == 3 and board.members.get(xs[1]).absolute
  })
  val wit_escape_text = event == "escape" and not(dock.textFocus) and memberDockShown
  val wit_align_open = alignShown

  // ---- scenarios ----
  /// Create selects the new group; adding a member again is a no-op; the third identity rejects.
  run creationAndReaddTest = init.then(create(0, Set(0, 2)))
    .expect(board.groups.get(0).order == [0, 2] and dock.panel == GroupDock({ group: 0, alignOpen: false }))
    .then(add(0, 0)).expect(outcome == Ok and board.groups.get(0).order == [0, 2])
    .then(add(0, 1)).expect(outcome == Rejected("item_missing"))
  /// Paste after the edited member and select it; duplicate from the group dock does nothing.
  run pasteAndDuplicateTest = init.then(create(0, Set(0, 2))).then(editMember(0)).then(pressKey("Primary+V", 1, Set()))
    .expect(board.groups.get(0).order == [0, 1, 2] and dock.panel == MemberDock({ group: 0, member: 1 }))
    .then(pressKey("Delete", 0, Set())).expect(board.live == Set(0, 2) and dock.panel == GroupDock({ group: 0, alignOpen: false }))
    .then(pressKey("Primary+D", 1, Set())).expect(outcome == Rejected("unhandled_key") and board.live == Set(0, 2))
  /// Arrows follow the primary axis; Free and Absolute never reorder; the end refuses.
  run arrowTest = init.then(paste(1)).then(create(0, Set(0, 1, 2))).then(setFlow(0, "Column")).then(editMember(0))
    .then(pressKey("Down", 0, Set())).expect(board.groups.get(0).order == [1, 0, 2])
    .then(pressKey("Right", 0, Set())).expect(outcome == Rejected("no_neighbor"))
    .then(useDock(Absolute(0))).then(useDock(Later)).expect(outcome == Rejected("no_neighbor") and board.groups.get(0).order == [1, 0, 2])
    .then(useDock(Absolute(2))).expect(board.groups.get(0).order == [1, 2, 0] and not(board.members.get(0).absolute))
    .then(setFlow(0, "Free")).then(useDock(Earlier)).expect(outcome == Rejected("no_neighbor"))
  /// Transfer keeps editing in the destination; detach hides the dock and the empty group survives.
  run transferAndDetachTest = init.then(create(0, Set(0))).then(create(1, Set(2))).then(editMember(0)).then(add(1, 0))
    .expect(board.groups.get(0).order == [] and board.groups.get(1).order == [2, 0] and dock.panel == MemberDock({ group: 1, member: 0 }))
    .then(useDock(Detach)).expect(dock.panel == Hidden and owners(board, 0) == Set() and hasGroup(board, 1))
    .then(selectGroup(0)).expect(dockCount == 0)
  /// Esc walks text focus, popover, member editing and selection; text focus blocks arrows.
  run escapeTest = init.then(create(0, Set(0, 2))).then(toggleAlign).then(focusText(true)).then(pressKey("Left", 0, Set()))
    .expect(outcome == Rejected("text_owns_key")).then(escape).expect(alignShown and not(dock.textFocus))
    .then(escape).expect(dock.panel == GroupDock({ group: 0, alignOpen: false }))
    .then(editMember(2)).then(escape).expect(groupDockShown).then(escape).expect(dock.panel == Hidden)
  /// Free closes the popover and refuses to reopen it; ungroup frees Absolute members too.
  run freeAndUngroupTest = init.then(create(0, Set(0, 2))).then(editMember(2)).then(useDock(Absolute(0))).then(escape)
    .then(toggleAlign).then(useDock(Flow("Free"))).expect(not(alignShown))
    .then(toggleAlign).expect(outcome == Rejected("control_inactive"))
    .then(pressKey("Primary+Shift+G", 0, Set())).expect(board.groups.keys() == Set() and dock.panel == Hidden and not(board.members.get(2).absolute))
  /// An outside drop detaches a flowing member but keeps an Absolute one attached.
  run dropTest = init.then(create(0, Set(0, 2))).then(toggleAbsolute(0, 2, 0)).then(dropMember(0, 2, 0, true))
    .expect(board.groups.get(0).order == [0, 2]).then(dropMember(0, 0, 0, true)).expect(board.groups.get(0).order == [2])
}
