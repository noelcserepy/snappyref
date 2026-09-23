// SnappyRef dock control enablement
// For every dock control and every valid view, the designer's enable rule agrees with the
// admission rule the operations apply: a control is enabled exactly when its submission would
// be accepted. A view is what the dock reads: the panel mode, one group's flow, gap, Fixed axes,
// alignment and order, and each member's Fill and Absolute. init picks one view; step stutters,
// so the checker covers every view, reachable or not, that satisfies sizing.qnt's invariants.
// Submitted values are absent: a blank name, negative padding or non-positive size is refused by
// the field's parser while the field itself stays enabled; ui.md keeps those rules.
module enablement {
  import rules.* from "./rules"
  // ---- parameters ----
  pure val ITEMS = Set(0, 1, 2)
  pure val ALIGN = 0.to(2)
  /// Every ordering of every subset of the items.
  pure val ORDERS = Set([], [0], [1], [2], [0, 1], [1, 0], [0, 2], [2, 0], [1, 2], [2, 1],
    [0, 1, 2], [0, 2, 1], [1, 0, 2], [1, 2, 0], [2, 0, 1], [2, 1, 0])

  // ---- types ----
  type Panel = Hidden | GroupDock(bool) | MemberDock(int)
  type Member = { fill: Set[int], absolute: bool }
  type View = { panel: Panel, flow: str, gap: str, fixed: Set[int], align: int -> int, order: List[int], members: int -> Member }
  /// All control families. Numeric and text payloads are the field's, not the model's.
  type Control = Name | Flow(str) | Gap | RowGap | Padding | Align((int, int)) | Auto(str) | Dimension(int) | Hug(int)
    | Clip | Ungroup | MemberMode((int, bool)) | Absolute | Earlier | Later | Detach
  pure val CONTROLS = Set(Name, Gap, RowGap, Padding, Clip, Ungroup, Absolute, Earlier, Later, Detach)
    .union(FLOWS.map(f => Flow(f))).union(GAPS.map(g => Auto(g)))
    .union(AXES.map(a => Set(Dimension(a), Hug(a), MemberMode((a, false)), MemberMode((a, true)))).flatten())
    .union(ALIGN.map(x => ALIGN.map(y => Align((x, y)))).flatten())

  // ---- state ----
  var view: View

  // ---- derived ----
  val groupDockShown = match view.panel { | GroupDock(_) => true | _ => false }
  val memberDockShown = match view.panel { | MemberDock(_) => true | _ => false }
  val alignShown = match view.panel { | GroupDock(open) => open | _ => false }
  val member = match view.panel { | MemberDock(m) => m | _ => -1 }
  val edited = view.members.get(if (member == -1) 0 else member)
  val flowing = view.order.select(m => not(view.members.get(m).absolute))
  /// The state sizing.qnt maintains, plus a dock that points at a member and a popover only outside Free.
  val valid = (view.flow == "Wrap" implies view.fixed.contains(0)) and
    ((view.flow != "Free" and view.gap == "Auto") implies view.fixed.contains(primary(view.flow))) and
    ITEMS.forall(m => view.members.get(m).fill.subseteq(view.fixed)) and
    (memberDockShown implies hasItem(view.order, member)) and (alignShown implies view.flow != "Free")

  /// Row gap is present only in Wrap. Align options exist only while the popover is open.
  def controlShown(c: Control): bool = match c {
    | MemberMode(_) => memberDockShown
    | Absolute => memberDockShown
    | Earlier => memberDockShown
    | Later => memberDockShown
    | Detach => memberDockShown
    | Align(_) => alignShown
    | Auto(_) => alignShown
    | RowGap => groupDockShown and view.flow == "Wrap"
    | _ => groupDockShown
  }
  /// Free and Absolute members keep their settings but cannot edit sizing or order.
  val memberLayoutEnabled = memberDockShown and view.flow != "Free" and not(edited.absolute)
  /// The designer's rules. Numeric gap dims in Auto; W/H entry sets Fixed; Hug is a toggle;
  /// under Auto only the counter-axis cells on the saved primary coordinate stay active.
  def controlEnabled(c: Control): bool = if (not(controlShown(c))) false else match c {
    | Gap => view.flow != "Free" and view.gap == "Number"
    | Align(xy) => view.flow != "Free" and (view.gap == "Number" or
        (if (view.flow == "Column") xy._2 == view.align.get(1) else xy._1 == view.align.get(0)))
    | Auto(_) => view.flow != "Free"
    | Hug(a) => not(a == 0 and view.flow == "Wrap")
    | MemberMode(af) => memberLayoutEnabled and (not(af._2) or view.fixed.contains(af._1))
    | Earlier => memberLayoutEnabled and index(flowing, member) > 0
    | Later => memberLayoutEnabled and index(flowing, member) + 1 < flowing.length()
    | _ => true
  }
  /// Admission, composed from the guards the operations apply. Dimmed or hidden controls reject;
  /// they never change a different setting to enable themselves.
  def controlReason(c: Control): str = {
    val memberControl = match c {
      | MemberMode(_) => true | Absolute => true | Earlier => true | Later => true | Detach => true | _ => false
    }
    if ((memberControl and not(memberDockShown)) or (not(memberControl) and not(groupDockShown))) "dock_unavailable" else match c {
      | Gap => if (view.flow == "Free" or view.gap != "Number") "control_inactive" else ""
      | RowGap => if (view.flow != "Wrap") "control_inactive" else ""
      | Align(xy) => if (not(alignShown)) "popover_closed" else if (view.flow == "Free") "control_inactive"
          else if (view.gap == "Auto" and (if (primary(view.flow) == 0) xy._1 else xy._2) != view.align.get(primary(view.flow))) "primary_alignment_inactive" else ""
      | Auto(_) => if (not(alignShown)) "popover_closed" else if (view.flow == "Free") "control_inactive" else ""
      | Dimension(a) => axisReason(view.flow, a, true)
      | Hug(a) => axisReason(view.flow, a, not(view.fixed.contains(a)))
      | MemberMode(af) => memberModeReason(view.flow, edited.absolute, view.fixed, af._1, af._2)
      | Earlier => arrowReason(flowing, view.flow, edited.absolute, member, earlier(view.flow))
      | Later => arrowReason(flowing, view.flow, edited.absolute, member, later(view.flow))
      | _ => ""
    }
  }

  // ---- init and step ----
  /// Pick any view; the invariants hold vacuously for an invalid one.
  action init = {
    nondet panel = oneOf(Set(Hidden, GroupDock(false), GroupDock(true)).union(ITEMS.map(m => MemberDock(m))))
    nondet flow = oneOf(FLOWS)
    nondet gap = oneOf(GAPS)
    nondet fixed = oneOf(AXES.powerset())
    nondet alignX = oneOf(ALIGN)
    nondet alignY = oneOf(ALIGN)
    nondet order = oneOf(ORDERS)
    nondet fill0 = oneOf(AXES.powerset())
    nondet fill1 = oneOf(AXES.powerset())
    nondet fill2 = oneOf(AXES.powerset())
    nondet absolute0 = oneOf(Set(false, true))
    nondet absolute1 = oneOf(Set(false, true))
    nondet absolute2 = oneOf(Set(false, true))
    view' = { panel: panel, flow: flow, gap: gap, fixed: fixed, align: Map(0 -> alignX, 1 -> alignY), order: order,
      members: Map(0 -> { fill: fill0, absolute: absolute0 }, 1 -> { fill: fill1, absolute: absolute1 }, 2 -> { fill: fill2, absolute: absolute2 }) }
  }
  action step = all { view' = view }

  // ---- invariants ----
  /// The designer-facing enable rule equals admission for every control in every valid view.
  val inv_enabled_iff_accepted = valid implies CONTROLS.forall(c => controlEnabled(c) == (controlReason(c) == ""))
  /// Hidden controls are never enabled, so an unavailable dock cannot fire anything.
  val inv_hidden_disabled = CONTROLS.forall(c => controlEnabled(c) implies controlShown(c))

  // ---- witnesses ----
  val wit_valid = valid
  val wit_wrap_hug_dimmed = valid and groupDockShown and view.flow == "Wrap" and not(controlEnabled(Hug(0))) and controlEnabled(Hug(1))
  val wit_auto_cells = valid and alignShown and view.gap == "Auto" and view.flow != "Free" and
    ALIGN.map(x => ALIGN.map(y => Align((x, y)))).flatten().filter(c => controlEnabled(c)).size() == 3
  val wit_free_dims_member = valid and memberDockShown and view.flow == "Free" and not(controlEnabled(Earlier)) and controlEnabled(Detach)
  val wit_fill_needs_fixed = valid and memberDockShown and view.flow != "Free" and not(edited.absolute) and
    controlEnabled(MemberMode((0, false))) and not(controlEnabled(MemberMode((0, true))))
  val wit_later_enabled = valid and controlEnabled(Later)

  // ---- scenarios ----
  /// The member artboard: Wrap, Fixed width and height, H Fill on the edited member, Later dim at the end.
  run memberArtboardTest = {
    nondet unused = oneOf(Set(0))
    all { view' = { panel: MemberDock(2), flow: "Wrap", gap: "Number", fixed: Set(0, 1), align: Map(0 -> 0, 1 -> 0), order: [0, 1, 2],
      members: Map(0 -> { fill: Set(), absolute: false }, 1 -> { fill: Set(), absolute: false }, 2 -> { fill: Set(1), absolute: false }) } }
  }.expect(valid and controlEnabled(MemberMode((1, true))) and controlEnabled(Earlier) and not(controlEnabled(Later)))
  /// The group artboard with the popover open under Auto in Row: primary cells follow the saved X.
  run autoAlignTest = {
    nondet unused = oneOf(Set(0))
    all { view' = { panel: GroupDock(true), flow: "Row", gap: "Auto", fixed: Set(0), align: Map(0 -> 1, 1 -> 0), order: [0, 2],
      members: ITEMS.mapBy(m => { fill: Set(), absolute: false }) } }
  }.expect(valid and controlEnabled(Align((1, 2))) and not(controlEnabled(Align((0, 0)))) and not(controlEnabled(Gap)) and not(controlShown(RowGap)))
}
