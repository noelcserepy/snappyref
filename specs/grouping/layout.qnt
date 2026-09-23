// SnappyRef group layout algorithm
// The pure layout of one group's flowing members on its primary axis: Wrap row breaking, Fill
// share, Hug measure and the Wrap vertical arrow target. This module answers "does the layout
// function satisfy its rules for every input", not "what sequences of edits are possible";
// sizing.qnt and board.qnt own those. Inputs are chosen once at init and never change, so the
// checker enumerates inputs instead of paths. Widths, capacity and gap are integers standing for
// pixels; the app computes the same rules in floats and its property test covers rounding.
// Ranges: quint run and TLC use 0..3, quint verify (Apalache) checks widths and capacity to 1000.
// Counter-axis sizes, padding (capacity is already net of it), alignment, rotation and aspect
// fitting are geometry in the app and not modeled.
module layout {
  // ---- parameters ----
  /// WIDE bounds fixed widths and capacity in the exhaustive check; 1000 stands for any pixel count.
  pure val WIDE = 1000
  /// Numeric gaps may be negative; -1 stands for any overlap.
  pure val GAPS = (-1).to(1)
  /// Four members are enough for two rows of two, a middle member, and a lone overflowing member.
  pure val MAX_MEMBERS = 4

  // ---- types ----
  /// A flowing member: its stored width on the primary axis and whether it Fills that axis.
  type Member = { w: int, fill: bool }
  /// One group's layout inputs. wrap breaks rows; auto means an Auto gap, which spends no gap before Fill.
  type Input = { xs: List[Member], capacity: int, gap: int, wrap: bool, auto: bool }

  // ---- algorithm ----
  pure def max(a: int, b: int): int = if (a > b) a else b
  /// A Fill member has zero minimum width when rows are formed and when leftover is computed.
  pure def base(m: Member): int = if (m.fill) 0 else m.w
  /// The gap that layout spends between neighbors. Auto spends none.
  pure def spent(i: Input): int = if (i.auto) 0 else i.gap
  /// Width a row uses: bases plus the spent gap between neighbors.
  pure def used(i: Input, row: List[Member]): int =
    row.foldl(0, (n, m) => n + base(m)) + spent(i) * max(0, row.length() - 1)
  /// Wrap starts a new row only when the next member would overflow the current one.
  /// A single member never breaks, so a Fixed member wider than the capacity overflows in place.
  /// Row and Column keep every member in one row.
  pure def rows(i: Input): List[List[Member]] =
    if (i.xs.length() == 0) []
    else if (not(i.wrap)) [i.xs]
    else {
      val p = i.xs.foldl({ done: [], row: [] }, (s, m) =>
        if (s.row.length() > 0 and used(i, s.row.append(m)) > i.capacity)
          { done: s.done.append(s.row), row: [m] }
        else { done: s.done, row: s.row.append(m) })
      p.done.append(p.row)
    }
  /// Space left in a row for its Fill members, never negative, and how many share it.
  pure def allocation(i: Input, row: List[Member]): { space: int, count: int } =
    { space: max(0, i.capacity - used(i, row)), count: row.select(m => m.fill).length() }
  /// Each Fill member's share, floored. Fixed members keep their stored width.
  pure def share(i: Input, row: List[Member]): int = {
    val a = allocation(i, row)
    if (a.count == 0) 0 else a.space / a.count
  }
  pure def rendered(i: Input, row: List[Member], m: Member): int = if (m.fill) share(i, row) else m.w
  /// Hug measures the single row of Fixed members with its gaps; Fill never survives on a Hug axis.
  pure def hug(i: Input): int = used(i, i.xs)
  /// Wrap Up/Down move to the same column of the adjacent row, or nowhere. Members are named by index.
  pure def arrowTarget(rs: List[List[int]], m: int, down: bool): int = {
    val r = rs.indices().fold(-1, (n, j) => if (rs[j].indices().exists(k => rs[j][k] == m)) j else n)
    val adjacent = if (down) r + 1 else r - 1
    if (r < 0 or adjacent < 0 or adjacent >= rs.length()) -1
    else {
      val column = rs[r].indices().fold(0, (n, k) => if (rs[r][k] == m) k else n)
      if (column < rs[adjacent].length()) rs[adjacent][column] else -1
    }
  }

  // ---- state ----
  /// The input chosen at init.
  var input: Input
  /// The rows the algorithm produced for that input.
  var out: List[List[Member]]

  // ---- init and step ----
  /// Choose any input once. Members beyond n are dropped so lists of every length appear.
  action init = {
    nondet n = oneOf(0.to(MAX_MEMBERS))
    nondet w0 = oneOf(0.to(WIDE))
    nondet w1 = oneOf(0.to(WIDE))
    nondet w2 = oneOf(0.to(WIDE))
    nondet w3 = oneOf(0.to(WIDE))
    nondet f0 = oneOf(Set(false, true))
    nondet f1 = oneOf(Set(false, true))
    nondet f2 = oneOf(Set(false, true))
    nondet f3 = oneOf(Set(false, true))
    nondet capacity = oneOf(1.to(WIDE))
    nondet gap = oneOf(GAPS)
    nondet wrap = oneOf(Set(false, true))
    nondet auto = oneOf(Set(false, true))
    val i = { xs: [{ w: w0, fill: f0 }, { w: w1, fill: f1 }, { w: w2, fill: f2 }, { w: w3, fill: f3 }].slice(0, n),
              capacity: capacity, gap: gap, wrap: wrap, auto: auto }
    all { input' = i, out' = rows(i) }
  }
  /// Layout is a function; nothing happens after init.
  action step = all { input' = input, out' = out }

  // ---- derived ----
  pure def joined(rs: List[List[Member]]): List[Member] = rs.foldl([], (acc, r) => acc.concat(r))
  /// Rows as member indices, the shape arrowTarget reads.
  def indexRows: List[List[int]] = {
    val p = out.foldl({ rs: [], next: 0 }, (s, row) =>
      { rs: s.rs.append(row.indices().fold([], (xs, k) => xs.append(s.next + k))), next: s.next + row.length() })
    p.rs
  }

  // ---- invariants ----
  /// Rows keep every member once, in order, and no row is empty.
  val inv_partition = joined(out) == input.xs and out.indices().forall(i => out[i].length() > 0)
  /// Only Wrap has more than one row.
  val inv_single_row = input.wrap or out.length() <= 1
  /// A Wrap row fits its capacity unless it holds one member that cannot fit anywhere. Row and Column overflow freely.
  val inv_fits = not(input.wrap) or out.indices().forall(i => used(input, out[i]) <= input.capacity or out[i].length() == 1)
  /// A row starts only because its first member would have overflowed the previous row.
  val inv_break_needed = out.indices().forall(i => i + 1 >= out.length() or
    used(input, out[i].append(out[i + 1].head())) > input.capacity)
  /// A Fixed member renders at its stored width even when that exceeds the capacity.
  val inv_fixed_never_shrinks = out.indices().forall(i => out[i].indices().forall(k =>
    out[i][k].fill or rendered(input, out[i], out[i][k]) == out[i][k].w))
  /// Fill shares are nonnegative and never exceed the row's leftover; zero leftover is allowed.
  val inv_share = out.indices().forall(i => {
    val a = allocation(input, out[i])
    a.space >= 0 and share(input, out[i]) * a.count <= a.space
  })
  /// Auto leaves the whole leftover to Fill; only a numeric gap is subtracted first.
  val inv_auto_spends_no_gap = out.indices().forall(i =>
    allocation(input, out[i]).space == max(0, input.capacity - out[i].foldl(0, (n, m) => n + base(m)) -
      (if (input.auto) 0 else input.gap * max(0, out[i].length() - 1))))
  /// Appending a member changes nothing before the last previous row.
  val inv_append_stable = input.xs.length() == 0 or {
    val before = rows(input.with("xs", input.xs.slice(0, input.xs.length() - 1)))
    before.length() == 0 or before.slice(0, before.length() - 1) == out.slice(0, before.length() - 1)
  }
  /// With a nonnegative gap, Hug is at least as wide as each member.
  val inv_hug_covers = input.gap < 0 or input.xs.indices().forall(k => base(input.xs[k]) <= hug(input))
  /// A vertical arrow lands in the adjacent row at the same column, or nowhere when that column is missing.
  val inv_arrow_column = {
    val rs = indexRows
    rs.indices().forall(r => rs[r].indices().forall(c => Set(false, true).forall(down => {
      val t = arrowTarget(rs, rs[r][c], down)
      val adjacent = if (down) r + 1 else r - 1
      if (adjacent < 0 or adjacent >= rs.length() or c >= rs[adjacent].length()) t == -1
      else t == rs[adjacent][c]
    })))
  }

  // ---- witnesses ----
  /// Wrap produces a second row.
  val wit_two_rows = out.length() > 1
  /// A lone Fixed member overflows its row.
  val wit_overflow = out.indices().exists(i => used(input, out[i]) > input.capacity)
  /// Fixed siblings exhaust the space while a Fill member remains.
  val wit_zero_share = out.indices().exists(i => allocation(input, out[i]).count > 0 and allocation(input, out[i]).space == 0)
  /// Two Fill members split a leftover.
  val wit_shared = out.indices().exists(i => allocation(input, out[i]).count > 1 and share(input, out[i]) > 0)
  /// A negative gap packs a row tighter than its widths.
  val wit_negative_gap = input.gap < 0 and out.indices().exists(i => out[i].length() > 1)
  /// A vertical arrow finds a target in one row and none in another.
  val wit_arrow_hit = indexRows.length() > 1 and arrowTarget(indexRows, 0, true) >= 0
  val wit_arrow_miss = indexRows.length() > 1 and indexRows.indices().exists(r =>
    indexRows[r].indices().exists(c => arrowTarget(indexRows, indexRows[r][c], true) == -1 and r + 1 < indexRows.length()))

  // ---- scenarios ----
  pure def fixed(w: int): Member = { w: w, fill: false }
  pure def filler: Member = { w: 0, fill: true }
  /// Three members of width 1 in capacity 3 with gap 1 wrap as two then one; Down from the first lands on the third.
  run wrapRowsTest = {
    val i = { xs: [fixed(1), fixed(1), fixed(1)], capacity: 3, gap: 1, wrap: true, auto: false }
    all { input' = i, out' = rows(i) }
  }.expect(out == [[fixed(1), fixed(1)], [fixed(1)]] and arrowTarget(indexRows, 0, true) == 2 and arrowTarget(indexRows, 1, true) == -1)
  /// Two Fill members in capacity 3 beside a Fixed 1 with numeric gap 1 get (3 - 1 - 2) / 2 = 0 each; under Auto (3 - 1) / 2 = 1.
  run fillShareTest = {
    val i = { xs: [fixed(1), filler, filler], capacity: 3, gap: 1, wrap: false, auto: false }
    all { input' = i, out' = rows(i) }
  }.expect(share(input, out[0]) == 0)
   .then({ val i = input.with("auto", true) all { input' = i, out' = rows(i) } })
   .expect(share(input, out[0]) == 1)
  /// A Fixed member wider than the capacity stays at its width and alone in its row.
  run overflowTest = {
    val i = { xs: [fixed(1), fixed(5)], capacity: 3, gap: 0, wrap: true, auto: false }
    all { input' = i, out' = rows(i) }
  }.expect(out == [[fixed(1)], [fixed(5)]] and rendered(input, out[1], fixed(5)) == 5)
  /// Hug of widths 1 and 2 with gap 1 is 4; with gap -1 it is 2.
  run hugTest = {
    val i = { xs: [fixed(1), fixed(2)], capacity: 3, gap: 1, wrap: false, auto: false }
    all { input' = i, out' = rows(i) }
  }.expect(hug(input) == 4 and hug(input.with("gap", -1)) == 2)
}
