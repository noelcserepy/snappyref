// SnappyRef group sizing modes
// Which layout modes a group and its members may hold and how edits move between them: flow
// (Row, Column, Wrap, Free), gap (Number or Auto), the group's Fixed versus Hug axes, and each
// member's Fill axes. Membership changes appear only where they change modes: creation resets,
// transfer keeps Fill that the destination can hold, leaving a group drops Fill.
// Two groups and three items exercise transfer between differently fixed groups.
// Sizes are absent: the stored numbers, the incoming fit, frozen Fill sizes and Hug measures are
// layout.qnt and the app's unit tests. Order, the dock and identities live in board.qnt.
// The numeric gap value and its retention under Auto are a stored value, not a mode; see ui.md.
module sizing {
  import rules.* from "./rules"
  // ---- parameters ----
  pure val GROUPS = Set(0, 1)
  pure val ITEMS = Set(0, 1, 2)
  pure val SELECTIONS = ITEMS.powerset()

  // ---- types ----
  type Outcome = Ok | Rejected(str)
  type Group = { flow: str, fixed: Set[int], gap: str }
  /// owner is the containing group, or -1 for a loose item.
  type Member = { owner: int, fill: Set[int] }
  type Board = { groups: int -> Group, members: int -> Member }

  // ---- state ----
  /// Existing groups and every item's owner and Fill axes.
  var board: Board
  /// Every attempted operation succeeds or rejects without changing the board.
  var outcome: Outcome
  /// Last operation name, for coverage.
  var event: str
  /// Transition evidence: a refusal changed nothing; a transfer changed no other member.
  var effects: { refusal: bool, isolated: bool }

  // ---- derived ----
  pure def hasGroup(b: Board, g: int): bool = b.groups.keys().contains(g)
  pure def hasMember(b: Board, g: int, m: int): bool = ITEMS.contains(m) and b.members.get(m).owner == g
  pure def memberReason(b: Board, g: int, m: int): str =
    if (not(hasGroup(b, g))) "group_missing" else if (not(hasMember(b, g, m))) "member_missing" else ""
  /// Creation presets: Wrap with a Fixed width, Hug height, numeric gap.
  pure val PRESET: Group = { flow: "Wrap", fixed: Set(0), gap: "Number" }
  pure def loose(m: Member): Member = { owner: -1, fill: Set() }
  /// Loose intake starts without Fill; a transfer keeps the Fill axes the destination fixes.
  pure def attached(b: Board, g: int, m: int): Board = {
    val member = b.members.get(m)
    b.with("members", b.members.set(m, { owner: g, fill: member.fill.intersect(b.groups.get(g).fixed) }))
  }
  pure def enforced(g: Group): Group = g.with("fixed", g.fixed.union(requiredFixed(g.flow, g.gap)))

  action commit(result: Board, reason: str, op: str): bool = {
    val applied = if (reason == "") result else board
    all {
      board' = applied,
      outcome' = if (reason == "") Ok else Rejected(reason),
      event' = if (reason == "") op else "rejected",
      effects' = {
        refusal: reason == "" or applied == board,
        isolated: op != "add" or ITEMS.forall(m =>
          board.members.get(m).owner == applied.members.get(m).owner implies board.members.get(m) == applied.members.get(m)),
      },
    }
  }

  // ---- operations ----
  /// Create from a selection with presets; selected members start Fixed. An empty selection is a no-op.
  action create(g: int, selection: Set[int]): bool = {
    val reason = if (hasGroup(board, g)) "group_exists" else ""
    val result = if (selection.size() == 0) board else {
      groups: board.groups.put(g, PRESET),
      members: ITEMS.mapBy(m => if (selection.contains(m)) { owner: g, fill: Set() } else board.members.get(m)) }
    commit(result, reason, "create")
  }
  /// Picker, drag-in and paste all attach the same way; a member already there is a no-op.
  action add(g: int, m: int): bool = {
    val reason = if (not(hasGroup(board, g))) "group_missing" else ""
    commit(if (reason == "" and not(hasMember(board, g, m))) attached(board, g, m) else board, reason, "add")
  }
  /// Leaving a group drops Fill; the frozen size is layout.
  action detach(g: int, m: int): bool = {
    val reason = memberReason(board, g, m)
    commit(if (reason == "") board.with("members", board.members.set(m, loose(board.members.get(m)))) else board, reason, "detach")
  }
  /// Ungroup frees every member and removes the group.
  action ungroup(g: int): bool = {
    val reason = if (not(hasGroup(board, g))) "group_missing" else ""
    val result = { groups: board.groups.keys().exclude(Set(g)).mapBy(k => board.groups.get(k)),
      members: ITEMS.mapBy(m => if (hasMember(board, g, m)) loose(board.members.get(m)) else board.members.get(m)) }
    commit(if (reason == "") result else board, reason, "ungroup")
  }
  /// A flow change keeps explicit sizing and adds the axes the new flow forces. Free keeps Fill dormant.
  action setFlow(g: int, flow: str): bool = {
    val reason = if (not(hasGroup(board, g))) "group_missing" else if (not(FLOWS.contains(flow))) "invalid_value" else ""
    commit(if (reason == "") board.with("groups", board.groups.set(g, enforced(board.groups.get(g).with("flow", flow)))) else board, reason, "setFlow")
  }
  /// Auto fixes the current primary axis; Free records the choice without forcing an axis.
  action setGap(g: int, gap: str): bool = {
    val reason = if (not(hasGroup(board, g))) "group_missing" else if (not(GAPS.contains(gap))) "invalid_value" else ""
    commit(if (reason == "") board.with("groups", board.groups.set(g, enforced(board.groups.get(g).with("gap", gap)))) else board, reason, "setGap")
  }
  /// Fixed comes from a typed size or an edge drag; Hug from the toggle or a handle double-click.
  /// Hugging an axis converts its Fill members to Fixed and returns a primary Auto gap to Number.
  action setAxisMode(g: int, a: int, fixed: bool): bool = {
    val reason = if (not(hasGroup(board, g))) "group_missing" else axisReason(board.groups.get(g).flow, a, fixed)
    val group = board.groups.get(g)
    val result = if (fixed) board.with("groups", board.groups.set(g, group.with("fixed", group.fixed.union(Set(a)))))
      else {
        groups: board.groups.set(g, { ...group, fixed: group.fixed.exclude(Set(a)),
          gap: if (group.flow != "Free" and a == primary(group.flow)) "Number" else group.gap }),
        members: ITEMS.mapBy(m => {
          val member = board.members.get(m)
          if (member.owner == g) member.with("fill", member.fill.exclude(Set(a))) else member }) }
    commit(if (reason == "") result else board, reason, "setAxisMode")
  }
  /// Fill needs a Fixed container axis; Fixed is always available. Free refuses the edit and keeps
  /// an earlier choice dormant. Absolute members are board.qnt's; they refuse the same way.
  action setMemberMode(g: int, m: int, a: int, fill: bool): bool = {
    val missing = memberReason(board, g, m)
    val reason = if (missing != "") missing else memberModeReason(board.groups.get(g).flow, false, board.groups.get(g).fixed, a, fill)
    val member = board.members.get(m)
    val result = board.with("members", board.members.set(m,
      member.with("fill", if (fill) member.fill.union(Set(a)) else member.fill.exclude(Set(a)))))
    commit(if (reason == "") result else board, reason, "setMemberMode")
  }
  action create_any = { nondet g = oneOf(GROUPS) nondet selection = oneOf(SELECTIONS) create(g, selection) }
  action add_any = { nondet g = oneOf(GROUPS) nondet m = oneOf(ITEMS) add(g, m) }
  action detach_any = { nondet g = oneOf(GROUPS) nondet m = oneOf(ITEMS) detach(g, m) }
  action ungroup_any = { nondet g = oneOf(GROUPS) ungroup(g) }
  action setFlow_any = { nondet g = oneOf(GROUPS) nondet flow = oneOf(FLOWS) setFlow(g, flow) }
  action setGap_any = { nondet g = oneOf(GROUPS) nondet gap = oneOf(GAPS) setGap(g, gap) }
  action setAxisMode_any = { nondet g = oneOf(GROUPS) nondet a = oneOf(AXES) nondet fixed = oneOf(Set(false, true)) setAxisMode(g, a, fixed) }
  action setMemberMode_any = {
    nondet g = oneOf(GROUPS) nondet m = oneOf(ITEMS) nondet a = oneOf(AXES) nondet fill = oneOf(Set(false, true))
    setMemberMode(g, m, a, fill)
  }

  // ---- init and step ----
  /// Three loose items and no group.
  action init = all {
    board' = { groups: Map(), members: ITEMS.mapBy(m => { owner: -1, fill: Set() }) },
    outcome' = Ok, event' = "init", effects' = { refusal: true, isolated: true },
  }
  action step = any { create_any, add_any, detach_any, ungroup_any, setFlow_any, setGap_any, setAxisMode_any, setMemberMode_any }

  // ---- invariants ----
  /// Wrap always has a Fixed width.
  val inv_wrap_fixed = board.groups.keys().forall(g => board.groups.get(g).flow == "Wrap" implies board.groups.get(g).fixed.contains(0))
  /// Auto has a Fixed primary axis whenever layout is active; Free merely stores Auto.
  val inv_auto_fixed = board.groups.keys().forall(g => {
    val group = board.groups.get(g)
    (group.flow != "Free" and group.gap == "Auto") implies group.fixed.contains(primary(group.flow))
  })
  /// Fill never survives on a Hug container axis, including dormant Free intent.
  val inv_fill_fixed = ITEMS.forall(m => {
    val member = board.members.get(m)
    member.owner == -1 or member.fill.subseteq(board.groups.get(member.owner).fixed)
  })
  /// A loose item carries no Fill, and every owner exists.
  val inv_owner = ITEMS.forall(m => {
    val member = board.members.get(m)
    if (member.owner == -1) member.fill == Set() else hasGroup(board, member.owner)
  })
  /// Refused operations leave the board unchanged.
  val inv_rejected_unchanged = effects.refusal
  /// A transfer changes no member that stays where it was.
  val inv_transfer_isolated = effects.isolated

  // ---- witnesses ----
  val wit_ok_create = event == "create" and outcome == Ok
  val wit_ok_add = event == "add" and outcome == Ok
  val wit_ok_detach = event == "detach" and outcome == Ok
  val wit_ok_ungroup = event == "ungroup" and outcome == Ok
  val wit_ok_setFlow = event == "setFlow" and outcome == Ok
  val wit_ok_setGap = event == "setGap" and outcome == Ok
  val wit_ok_setAxisMode = event == "setAxisMode" and outcome == Ok
  val wit_ok_setMemberMode = event == "setMemberMode" and outcome == Ok
  val wit_rejected_group_exists = outcome == Rejected("group_exists")
  val wit_rejected_group_missing = outcome == Rejected("group_missing")
  val wit_rejected_member_missing = outcome == Rejected("member_missing")
  val wit_rejected_wrap_width_fixed = outcome == Rejected("wrap_width_fixed")
  val wit_rejected_fixed_axis_required = outcome == Rejected("fixed_axis_required")
  val wit_rejected_control_inactive = outcome == Rejected("control_inactive")
  /// Both groups exist with different Fixed axes, so a transfer can drop Fill.
  val wit_transfer_drops_fill = event == "add" and outcome == Ok and ITEMS.exists(m =>
    board.members.get(m).owner != -1 and board.groups.keys().size() == 2 and board.members.get(m).fill == Set() and
    board.groups.keys().exists(g => board.groups.get(g).fixed != board.groups.get(board.members.get(m).owner).fixed))
  /// Fill and Auto compose in an active layout.
  val wit_auto_fill = board.groups.keys().exists(g => board.groups.get(g).flow != "Free" and board.groups.get(g).gap == "Auto" and
    ITEMS.exists(m => board.members.get(m).owner == g and board.members.get(m).fill.contains(primary(board.groups.get(g).flow))))
  /// Free keeps a dormant Fill and an Auto gap.
  val wit_free_dormant = board.groups.keys().exists(g => board.groups.get(g).flow == "Free" and board.groups.get(g).gap == "Auto" and
    ITEMS.exists(m => board.members.get(m).owner == g and board.members.get(m).fill != Set()))
  /// A Hug axis cleared Fill from a member.
  val wit_hug_clears_fill = event == "setAxisMode" and outcome == Ok and
    board.groups.keys().exists(g => board.groups.get(g).fixed == Set() and ITEMS.exists(m => board.members.get(m).owner == g))
  /// Both Fill axes on one member inside a group fixed on both axes.
  val wit_fill_both = ITEMS.exists(m => board.members.get(m).fill == AXES)

  // ---- scenarios ----
  /// Creation presets Wrap with Fixed width and Hug height; the selected members start Fixed.
  run creationTest = init.then(create(0, Set(0, 2))).expect(board.groups.get(0) == PRESET and board.members.get(0) == { owner: 0, fill: Set() })
    .then(create(0, Set(1))).expect(outcome == Rejected("group_exists"))
  /// Auto fixes a Hug primary axis; hugging it again restores Number and converts Fill to Fixed.
  run autoHugConversionTest = init.then(create(0, Set(2))).then(setFlow(0, "Row")).then(setAxisMode(0, 0, false))
    .then(setGap(0, "Auto")).expect(board.groups.get(0).fixed.contains(0))
    .then(setMemberMode(0, 2, 0, true)).then(setAxisMode(0, 0, false))
    .expect(board.members.get(2).fill == Set() and board.groups.get(0).gap == "Number")
  /// Wrap refuses Hug width; Fill refuses a Hug axis; neither rejection changes anything.
  run editConstraintsTest = init.then(create(0, Set(2))).then(setAxisMode(0, 0, false))
    .expect(outcome == Rejected("wrap_width_fixed")).then(setMemberMode(0, 2, 1, true))
    .expect(outcome == Rejected("fixed_axis_required") and board.members.get(2).fill == Set())
    .then(setAxisMode(0, 1, true)).then(setMemberMode(0, 2, 1, true)).expect(board.members.get(2).fill == Set(1))
  /// Free keeps both axes selectable, keeps an earlier Fill dormant and refuses new Fill;
  /// reactivating layout enforces Auto's new primary axis.
  run freeAndFlowTest = init.then(create(0, Set(0, 2))).then(setMemberMode(0, 2, 0, true)).then(setFlow(0, "Free")).then(setAxisMode(0, 0, false))
    .then(setGap(0, "Auto")).expect(board.groups.get(0).fixed == Set() and board.members.get(2).fill == Set())
    .then(setMemberMode(0, 0, 1, true)).expect(outcome == Rejected("control_inactive"))
    .then(setFlow(0, "Column")).expect(board.groups.get(0).fixed == Set(1))
    .then(setFlow(0, "Wrap")).expect(board.groups.get(0).fixed == Set(0, 1))
  /// Moving a Fill member to a Hug destination drops the Fill instead of keeping an invalid mode.
  run transferFillToHugTest = init.then(create(0, Set(0))).then(setMemberMode(0, 0, 0, true))
    .then(create(1, Set(2))).then(setFlow(1, "Row")).then(setAxisMode(1, 0, false)).then(add(1, 0))
    .expect(board.members.get(0) == { owner: 1, fill: Set() } and board.members.get(2).owner == 1)
  /// Detach and ungroup free members and drop Fill; the empty group survives detach.
  run leaveTest = init.then(create(0, Set(0, 2))).then(setMemberMode(0, 0, 0, true)).then(detach(0, 0))
    .expect(board.members.get(0) == { owner: -1, fill: Set() } and hasGroup(board, 0))
    .then(ungroup(0)).expect(board.groups.keys() == Set() and board.members.get(2).owner == -1)
}
