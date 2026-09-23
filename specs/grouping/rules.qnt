// SnappyRef grouping rules shared by the other modules
// Pure definitions only: the guards that admit or refuse a request and the small list helpers.
// sizing.qnt applies them as operations, enablement.qnt compares them with the dock's enable
// rules, board.qnt uses the neighbor rule for arrows. One owner per guard keeps the three modules
// refusing the same requests for the same reasons.
module rules {
  // ---- parameters ----
  pure val AXES = Set(0, 1)
  pure val FLOWS = Set("Row", "Column", "Wrap", "Free")
  /// Between, Evenly and Around distribute space visually and are one Auto category here.
  pure val GAPS = Set("Number", "Auto")
  pure val DIRECTIONS = Set("Left", "Right", "Up", "Down")

  // ---- helpers ----
  pure def max(a: int, b: int): int = if (a > b) a else b
  pure def min(a: int, b: int): int = if (a < b) a else b
  pure def hasItem(xs: List[int], m: int): bool = xs.indices().exists(i => xs[i] == m)
  pure def index(xs: List[int], m: int): int = xs.indices().fold(0, (n, i) => if (xs[i] == m) i else n)
  /// Insert at a slot clamped to the list ends.
  pure def insert(xs: List[int], m: int, slot: int): List[int] = {
    val k = min(xs.length(), max(0, slot))
    xs.slice(0, k).concat([m]).concat(xs.slice(k, xs.length()))
  }
  /// Sole element of a singleton set. TLC lacks getOnlyElement.
  pure def only(s: Set[int]): int = s.fold(-1, (_, x) => x)

  // ---- guards ----
  pure def primary(flow: str): int = if (flow == "Column") 1 else 0
  /// Axis modes a configuration forces: Wrap fixes width, Auto fixes the primary axis, Free forces nothing.
  pure def requiredFixed(flow: str, gap: str): Set[int] =
    (if (flow == "Wrap") Set(0) else Set()).union(
      if (flow != "Free" and gap == "Auto") Set(primary(flow)) else Set())
  /// Wrap width cannot become Hug; refuse rather than silently change the flow.
  pure def axisReason(flow: str, a: int, fixed: bool): str =
    if (not(AXES.contains(a))) "invalid_value"
    else if (not(fixed) and a == 0 and flow == "Wrap") "wrap_width_fixed" else ""
  /// Fill is available only on a Fixed container axis.
  pure def fillReason(fixed: Set[int], a: int, fill: bool): str =
    if (not(AXES.contains(a))) "invalid_value"
    else if (fill and not(fixed.contains(a))) "fixed_axis_required" else ""
  /// Free and Absolute members keep Fill dormant; the dock shows the choice dimmed.
  pure def memberModeReason(flow: str, absolute: bool, fixed: Set[int], a: int, fill: bool): str =
    if (flow == "Free" or absolute) "control_inactive" else fillReason(fixed, a, fill)
  /// The flowing neighbor an arrow moves next to, or -1. Row answers Left/Right, Column Up/Down,
  /// Free and Absolute nothing. Wrap Up/Down land in the adjacent row's same column; layout.qnt
  /// proves that over real rows. Without widths every Wrap member sits in its own row, so all
  /// four Wrap directions resolve to the linear neighbor here.
  pure def neighbor(xs: List[int], flow: str, absolute: bool, m: int, direction: str): int = {
    val horizontal = direction == "Left" or direction == "Right"
    val i = index(xs, m)
    val j = if (direction == "Left" or direction == "Up") i - 1 else i + 1
    if (flow == "Free" or absolute or not(hasItem(xs, m))) -1
    else if (flow == "Row" and not(horizontal)) -1
    else if (flow == "Column" and horizontal) -1
    else if (j >= 0 and j < xs.length()) xs[j] else -1
  }
  pure def arrowReason(xs: List[int], flow: str, absolute: bool, m: int, direction: str): str =
    if (neighbor(xs, flow, absolute, m, direction) < 0) "no_neighbor" else ""
  /// The dock's Earlier and Later chevrons follow the primary axis.
  pure def earlier(flow: str): str = if (flow == "Column") "Up" else "Left"
  pure def later(flow: str): str = if (flow == "Column") "Down" else "Right"
}
