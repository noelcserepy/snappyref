use super::*;
use crate::data::{GapMode, GroupFlow};
use crate::placement::group_pixels::Pixels;

fn init() -> State<Pixels> {
    State::init(
        Board {
            groups: BTreeMap::new(),
            live: BTreeSet::from([0, 2]),
            members: (0..3)
                .map(|m| (m, Member::fixed(MemberKind::Textbox, [1.0; 2])))
                .collect(),
        },
        Pixels::default(),
        1.0,
    )
}
fn sizing_init() -> State<Pixels> {
    let mut s = init();
    s.board.live.insert(1);
    s
}
/// Pixels with a supplied vertical Wrap target, so arrow tests pick the neighbor directly.
#[derive(Default)]
struct StubTarget {
    pixels: Pixels,
    target: Option<Id>,
}
impl Geometry for StubTarget {
    fn wrap_arrow_target(&self, _: &Board, _: Id, _: Id, _: Direction) -> Option<Id> {
        self.target
    }
    fn pasted_member(&self, id: Id, width: f32) -> Member {
        self.pixels.pasted_member(id, width)
    }
    fn fit(&self, member: &Member, width: f32) -> [f32; 2] {
        self.pixels.fit(member, width)
    }
    fn rendered(&self, b: &Board, g: Id, m: Id, a: usize) -> f32 {
        self.pixels.rendered(b, g, m, a)
    }
    fn measured(&self, b: &Board, g: Id, a: usize) -> f32 {
        self.pixels.measured(b, g, a)
    }
}

mod sizing {
    use super::*;
    #[test]
    fn creation() {
        let mut s = sizing_init();
        s.create(0, &[0, 2], 1.0).unwrap();
        let g = &s.board.groups[&0];
        assert_eq!(
            (g.flow, g.fixed, g.auto),
            (GroupFlow::Wrap, [true, false], GapMode::Number)
        );
        assert_eq!(s.board.owner(0), Some(0));
        assert_eq!(s.board.members[&0].fill, [false; 2]);
        assert_eq!(s.create(0, &[1], 1.0), Err("group_exists"));
    }
    #[test]
    fn auto_hug_conversion() {
        let mut s = sizing_init();
        s.create(0, &[2], 1.0).unwrap();
        s.set_flow(0, GroupFlow::Row).unwrap();
        s.set_axis_mode(0, 0, AxisMode::Hug).unwrap();
        s.set_gap(0, GapMode::Between, 1.0).unwrap();
        assert!(s.board.groups[&0].fixed[0]);
        s.set_member_mode(0, 2, 0, true).unwrap();
        s.set_axis_mode(0, 0, AxisMode::Hug).unwrap();
        assert_eq!(s.board.members[&2].fill, [false; 2]);
        assert_eq!(s.board.groups[&0].auto, GapMode::Number);
    }
    #[test]
    fn edit_constraints() {
        let mut s = sizing_init();
        s.create(0, &[2], 1.0).unwrap();
        assert_eq!(
            s.set_axis_mode(0, 0, AxisMode::Hug),
            Err("wrap_width_fixed")
        );
        assert_eq!(s.set_member_mode(0, 2, 1, true), Err("fixed_axis_required"));
        assert_eq!(s.board.members[&2].fill, [false; 2]);
        s.set_axis_mode(0, 1, AxisMode::Fixed(1.0)).unwrap();
        s.set_member_mode(0, 2, 1, true).unwrap();
        assert_eq!(s.board.members[&2].fill, [false, true]);
    }
    #[test]
    fn free_and_flow() {
        let mut s = sizing_init();
        s.create(0, &[0, 2], 1.0).unwrap();
        s.set_member_mode(0, 2, 0, true).unwrap();
        s.set_flow(0, GroupFlow::Free).unwrap();
        s.set_axis_mode(0, 0, AxisMode::Hug).unwrap();
        s.set_gap(0, GapMode::Between, 1.0).unwrap();
        assert_eq!(s.board.groups[&0].fixed, [false; 2]);
        assert_eq!(s.board.members[&2].fill, [false; 2]);
        assert_eq!(s.set_member_mode(0, 0, 1, true), Err("control_inactive"));
        s.set_flow(0, GroupFlow::Column).unwrap();
        assert_eq!(s.board.groups[&0].fixed, [false, true]);
        s.set_flow(0, GroupFlow::Wrap).unwrap();
        assert_eq!(s.board.groups[&0].fixed, [true; 2]);
    }
    #[test]
    fn transfer_fill_to_hug() {
        let mut s = sizing_init();
        s.create(0, &[0], 1.0).unwrap();
        s.set_member_mode(0, 0, 0, true).unwrap();
        s.create(1, &[2], 1.0).unwrap();
        s.set_flow(1, GroupFlow::Row).unwrap();
        s.set_axis_mode(1, 0, AxisMode::Hug).unwrap();
        s.add(1, &[0]).unwrap();
        assert_eq!(s.board.owner(0), Some(1));
        assert_eq!(s.board.members[&0].fill, [false; 2]);
        assert_eq!(s.board.owner(2), Some(1));
    }
    #[test]
    fn leave() {
        let mut s = sizing_init();
        s.create(0, &[0, 2], 1.0).unwrap();
        s.set_member_mode(0, 0, 0, true).unwrap();
        s.detach(0, 0).unwrap();
        assert_eq!(s.board.owner(0), None);
        assert_eq!(s.board.members[&0].fill, [false; 2]);
        assert!(s.board.groups.contains_key(&0));
        s.ungroup(0).unwrap();
        assert!(s.board.groups.is_empty());
        assert_eq!(s.board.owner(2), None);
    }
}
mod board {
    use super::*;
    #[test]
    fn creation_and_readd() {
        let mut s = init();
        s.create(0, &[0, 2], 1.0).unwrap();
        assert_eq!(s.board.groups[&0].order, [0, 2]);
        assert_eq!(s.dock.panel, Panel::GroupDock { group: 0 });
        assert_eq!(s.add(0, &[0]), Ok(()));
        assert_eq!(s.board.groups[&0].order, [0, 2]);
        assert_eq!(s.add(0, &[1]), Err("item_missing"));
        s.ungroup(0).unwrap();
        s.create(0, &[2, 0], 1.0).unwrap();
        assert_eq!(s.board.groups[&0].order, [2, 0]);
    }
    #[test]
    fn paste_and_duplicate() {
        let mut s = init();
        s.create(0, &[0, 2], 1.0).unwrap();
        s.edit_member(0).unwrap();
        s.paste(1, 1.0).unwrap();
        assert_eq!(s.board.groups[&0].order, [0, 1, 2]);
        assert_eq!(
            s.dock.panel,
            Panel::MemberDock {
                group: 0,
                member: 1
            }
        );
        s.delete_member(1).unwrap();
        assert_eq!(s.board.live, BTreeSet::from([0, 2]));
        assert_eq!(s.dock.panel, Panel::GroupDock { group: 0 });
        s.edit_member(0).unwrap();
        s.duplicate(0, 1).unwrap();
        assert_eq!(s.board.groups[&0].order, [0, 1, 2]);
        assert_eq!(
            s.dock.panel,
            Panel::MemberDock {
                group: 0,
                member: 1
            }
        );
    }
    #[test]
    fn arrow() {
        let mut s = init();
        s.paste(1, 1.0).unwrap();
        s.create(0, &[0, 1, 2], 1.0).unwrap();
        s.set_flow(0, GroupFlow::Column).unwrap();
        s.edit_member(0).unwrap();
        s.press_arrow(Direction::Down).unwrap();
        assert_eq!(s.board.groups[&0].order, [1, 0, 2]);
        assert_eq!(s.press_arrow(Direction::Right), Err("no_neighbor"));
        s.use_dock(Control::Absolute(0)).unwrap();
        assert_eq!(s.press_arrow(Direction::Down), Err("no_neighbor"));
        assert_eq!(s.board.groups[&0].order, [1, 0, 2]);
        s.use_dock(Control::Absolute(2)).unwrap();
        assert_eq!(s.board.groups[&0].order, [1, 2, 0]);
        assert!(!s.board.members[&0].absolute);
        s.set_flow(0, GroupFlow::Free).unwrap();
        assert_eq!(s.press_arrow(Direction::Up), Err("no_neighbor"));
    }
    #[test]
    fn vertical_target() {
        let mut s = State::init(init().board, StubTarget::default(), 1.0);
        s.paste(1, 1.0).unwrap();
        s.create(0, &[0, 1, 2], 1.0).unwrap();
        s.edit_member(0).unwrap();
        s.geometry.target = Some(2);
        assert_eq!(s.press_arrow(Direction::Down), Ok(()));
        assert_eq!(s.board.groups[&0].order, [1, 2, 0]);
        s.geometry.target = None;
        assert_eq!(s.press_arrow(Direction::Down), Err("no_neighbor"));
        assert_eq!(s.board.groups[&0].order, [1, 2, 0]);
    }
    #[test]
    fn transfer_and_detach() {
        let mut s = init();
        s.create(0, &[0], 1.0).unwrap();
        s.create(1, &[2], 1.0).unwrap();
        s.edit_member(0).unwrap();
        s.add(1, &[0]).unwrap();
        assert!(s.board.groups[&0].order.is_empty());
        assert_eq!(s.board.groups[&1].order, [2, 0]);
        assert_eq!(
            s.dock.panel,
            Panel::MemberDock {
                group: 1,
                member: 0
            }
        );
        s.use_dock(Control::Detach).unwrap();
        assert_eq!(s.dock.panel, Panel::Hidden);
        assert_eq!(s.board.owner(0), None);
        assert!(s.board.groups.contains_key(&1));
        s.select_group(0).unwrap();
        assert_eq!(s.dock_count(), 0);
    }
    #[test]
    fn escape() {
        let mut s = init();
        s.create(0, &[0, 2], 1.0).unwrap();
        s.focus_text(true);
        assert_eq!(s.press_arrow(Direction::Left), Err("text_owns_key"));
        s.escape();
        assert_eq!(s.dock.panel, Panel::GroupDock { group: 0 });
        s.edit_member(2).unwrap();
        s.escape();
        assert!(s.dock.group_shown());
        s.escape();
        assert_eq!(s.dock.panel, Panel::Hidden);
    }
    #[test]
    fn free_and_ungroup() {
        let mut s = init();
        s.create(0, &[0, 2], 1.0).unwrap();
        s.edit_member(2).unwrap();
        s.use_dock(Control::Absolute(0)).unwrap();
        s.escape();
        s.use_dock(Control::Flow(GroupFlow::Free)).unwrap();
        assert!(!s.control_enabled(&Control::Align([1, 1])));
        s.use_dock(Control::Ungroup).unwrap();
        assert!(s.board.groups.is_empty());
        assert_eq!(s.dock.panel, Panel::Hidden);
        assert!(!s.board.members[&2].absolute);
    }
    #[test]
    fn drop() {
        let mut s = init();
        s.create(0, &[0, 2], 1.0).unwrap();
        s.toggle_absolute(0, 2, 0).unwrap();
        s.drop_member(0, 2, 0, true).unwrap();
        assert_eq!(s.board.groups[&0].order, [0, 2]);
        s.drop_member(0, 0, 0, true).unwrap();
        assert_eq!(s.board.groups[&0].order, [2]);
    }
}
#[test]
fn stored_dock_values() {
    let mut s = init();
    s.create(0, &[0, 2], 3.0).unwrap();
    for c in [
        Control::Name("1".into()),
        Control::Gap(-1.0),
        Control::RowGap(-1.0),
        Control::Padding(0.0),
        Control::Dimension(1, 3.0),
        Control::Clip,
    ] {
        assert_eq!(s.use_dock(c), Ok(()));
    }
    let g = &s.board.groups[&0];
    assert_eq!(
        (&g.name, g.gap, g.row_gap, g.padding, g.clip),
        (&"1".to_string(), -1.0, -1.0, 0.0, true)
    );
    assert!(g.fixed[1]);
    assert_eq!(s.use_dock(Control::Dimension(0, 0.0)), Err("invalid_value"));
    let before = s.board.clone();
    let dock = s.dock.clone();
    for c in [
        Control::Name(" ".into()),
        Control::Padding(-1.0),
        Control::Dimension(1, f32::NAN),
    ] {
        assert_eq!(s.use_dock(c), Err("invalid_value"));
        assert_eq!(s.board, before);
        assert_eq!(s.dock, dock);
    }
    assert_eq!(s.set_member_mode(0, 0, 2, false), Err("invalid_value"));
    s.toggle_absolute(0, 0, 0).unwrap();
    assert_eq!(s.set_member_mode(0, 0, 0, false), Err("control_inactive"));
    s.use_dock(Control::Flow(GroupFlow::Free)).unwrap();
    assert!(!s.control_enabled(&Control::Gap(1.0)));
    assert!(!s.control_shown(&Control::RowGap(1.0)));
    assert_eq!(s.use_dock(Control::Gap(1.0)), Err("control_inactive"));
}
#[test]
fn stored_auto_variant_and_number() {
    let mut s = init();
    s.create(0, &[0, 2], 3.0).unwrap();
    s.use_dock(Control::Gap(-1.0)).unwrap();
    s.use_dock(Control::Align([2, 0])).unwrap();
    s.use_dock(Control::Auto(GapMode::Between)).unwrap();
    assert!(!s.control_enabled(&Control::Gap(0.0)));
    assert!(s.control_enabled(&Control::Align([2, 1])));
    assert!(!s.control_enabled(&Control::Align([0, 1])));
    assert_eq!(
        s.use_dock(Control::Align([0, 1])),
        Err("primary_alignment_inactive")
    );
    s.use_dock(Control::Align([2, 1])).unwrap();
    s.use_dock(Control::Auto(GapMode::Number)).unwrap();
    assert_eq!(s.board.groups[&0].gap, -1.0);
    assert_eq!(s.board.groups[&0].align, [2, 1]);
    s.use_dock(Control::Flow(GroupFlow::Free)).unwrap();
    assert!(!s.control_enabled(&Control::Auto(GapMode::Number)));
}
#[test]
fn corner_hugs_each_legal_axis() {
    let mut s = init();
    s.create(0, &[0, 2], 1.0).unwrap();
    s.use_dock(Control::Dimension(1, 3.0)).unwrap();
    s.double_click_corner().unwrap();
    assert_eq!(s.board.groups[&0].fixed, [true, false]);
    s.set_flow(0, GroupFlow::Row).unwrap();
    s.double_click_corner().unwrap();
    assert_eq!(s.board.groups[&0].fixed, [false; 2]);
}

#[test]
fn incoming_fit_preserves_existing_members_and_later_resize_does_not_refit() {
    let mut s = init();
    s.board
        .members
        .insert(0, Member::fixed(MemberKind::Image, [200.0, 80.0]));
    s.board
        .members
        .insert(2, Member::fixed(MemberKind::Image, [200.0, 100.0]));
    s.create(0, &[0], 200.0).unwrap();
    s.use_dock(Control::Dimension(0, 102.0)).unwrap();
    s.add(0, &[2]).unwrap();
    assert_eq!(s.board.members[&0].size, [200.0, 80.0]);
    assert_eq!(s.board.members[&2].size, [100.0, 50.0]);
    s.use_dock(Control::Dimension(0, 52.0)).unwrap();
    assert_eq!(s.board.members[&2].size, [100.0, 50.0]);
    assert_eq!(s.geometry.rendered(&s.board, 0, 2, 0), 100.0);
}

mod layout {
    use super::*;

    fn state(xs: &[(f32, bool)], capacity: f32, gap: f32, wrap: bool, auto: bool) -> State<Pixels> {
        let mut group = Group::new(0, capacity, gap);
        group.padding = 0.0;
        group.size[0] = capacity;
        group.flow = if wrap {
            GroupFlow::Wrap
        } else {
            GroupFlow::Row
        };
        group.auto = if auto {
            GapMode::Between
        } else {
            GapMode::Number
        };
        group.order = (0..xs.len() as Id).collect();
        State::init(
            Board {
                groups: BTreeMap::from([(0, group)]),
                live: (0..xs.len() as Id).collect(),
                members: xs
                    .iter()
                    .enumerate()
                    .map(|(m, &(width, fill))| {
                        let mut member = Member::fixed(MemberKind::Textbox, [width, 1.0]);
                        if fill {
                            member.fill[0] = true;
                        }
                        (m as Id, member)
                    })
                    .collect(),
            },
            Pixels::default(),
            gap,
        )
    }
    fn check(s: &mut State<Pixels>) {
        let b = &s.board;
        let g = &b.groups[&0];
        let rows = b.rows(0);
        let pixels = s.geometry.layout(b, 0);
        let used = |row: &[Id]| {
            row.iter().map(|&m| b.base(m, 0)).sum::<f32>()
                + g.numeric_gap() * row.len().saturating_sub(1) as f32
        };
        // Rows partition membership.
        assert_eq!(rows.concat(), g.order);
        assert!(rows.iter().all(|r| !r.is_empty()));
        assert!(g.flow == GroupFlow::Wrap || rows.len() <= 1);
        for (r, row) in rows.iter().enumerate() {
            // A new row is necessary exactly when the next member exceeds capacity.
            assert!(
                g.flow != GroupFlow::Wrap || used(row) <= g.size[0] || row.len() == 1,
                "{g:?} {row:?}"
            );
            if r + 1 < rows.len() {
                let mut extended = row.clone();
                extended.push(rows[r + 1][0]);
                assert!(used(&extended) > g.size[0]);
            }
            let (space, count) = b.allocation(0, row, 0);
            // Auto spacing leaves all residual capacity for Fill.
            let expected = (g.size[0]
                - row.iter().map(|&m| b.base(m, 0)).sum::<f32>()
                - if g.auto == GapMode::Number {
                    g.gap * row.len().saturating_sub(1) as f32
                } else {
                    0.0
                })
            .max(0.0);
            assert!((space - expected).abs() < 0.001);
            let share = if count == 0 {
                0.0
            } else {
                space / count as f32
            };
            // Division preserves fractional pixels.
            assert!(share >= 0.0 && share * count as f32 <= space + 0.001);
            for (c, &m) in row.iter().enumerate() {
                // Fixed widths remain unchanged; Fill gets its share.
                let expected = if b.members[&m].fill[0] {
                    share
                } else {
                    b.members[&m].size[0]
                };
                assert!((pixels.sizes[&m].x - expected).abs() < 0.001);
                // Vertical arrows keep the column.
                if g.flow == GroupFlow::Wrap {
                    for (direction, adjacent) in [
                        (Direction::Up, r.checked_sub(1)),
                        (Direction::Down, Some(r + 1)),
                    ] {
                        let expected = adjacent
                            .and_then(|r| rows.get(r))
                            .and_then(|r| r.get(c))
                            .copied();
                        assert_eq!(
                            s.arrow_target(0, m, direction).map(|i| g.order[i]),
                            expected
                        );
                    }
                }
            }
        }
        // Hug covers every fixed member.
        if g.gap >= 0.0 {
            let xs: Vec<_> = g.order.iter().map(|&m| (b.base(m, 0), false)).collect();
            let mut hug = state(&xs, g.size[0], g.numeric_gap(), false, false);
            hug.board.groups.get_mut(&0).unwrap().fixed = [false; 2];
            let width = hug.geometry.measured(&hug.board, 0, 0);
            assert!(xs.iter().all(|&(w, _)| w <= width + 0.001));
        }
        // Removing the last member preserves earlier rows.
        if let Some(last) = s.board.groups.get_mut(&0).unwrap().order.pop() {
            let before = s.board.rows(0);
            let prefix = before.len().saturating_sub(1);
            assert_eq!(&before[..prefix], &rows[..prefix]);
            s.board.groups.get_mut(&0).unwrap().order.push(last);
        }
    }
    #[test]
    fn every_small_layout_input_and_wide_samples() {
        // Exercise small widths and list lengths before wider fractional samples.
        let mut count = 0;
        for n in 0..=4 {
            for code in 0..8usize.pow(n) {
                let mut code = code;
                let xs: Vec<_> = (0..n)
                    .map(|_| {
                        let x = code % 8;
                        code /= 8;
                        ((x / 2) as f32, x % 2 == 1)
                    })
                    .collect();
                for capacity in 1..=3 {
                    for gap in -1..=1 {
                        for wrap in [false, true] {
                            for auto in [false, true] {
                                check(&mut state(&xs, capacity as f32, gap as f32, wrap, auto));
                                count += 1;
                            }
                        }
                    }
                }
            }
        }
        assert_eq!(count, 168_516);
        // Reproducible samples
        // span every capacity and both integral and fractional pixel inputs.
        let mut seed = 1_u64;
        let mut next = || {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            (seed >> 32) as usize
        };
        for capacity in 1..=1000 {
            for fractional in [false, true] {
                let xs: Vec<_> = (0..next() % 5)
                    .map(|_| {
                        let width = next() % 1001;
                        (
                            if fractional {
                                (width as f32 + 0.25).min(1000.0)
                            } else {
                                width as f32
                            },
                            next() % 2 == 1,
                        )
                    })
                    .collect();
                for gap in -1..=1 {
                    for wrap in [false, true] {
                        for auto in [false, true] {
                            check(&mut state(&xs, capacity as f32, gap as f32, wrap, auto));
                            count += 1;
                        }
                    }
                }
            }
        }
        println!(
            "layout: {count} inputs, 168516 exhaustive small inputs and 24000 wide/fractional samples"
        );
    }
    #[test]
    fn layout_scenarios() {
        // Real-value examples cover wrapping, Fill exhaustion, overflow and Hug.
        for (name, xs, capacity, gap, wrap, auto, rows, rendered, hug) in [
            (
                "wrap rows",
                vec![(1., false); 3],
                3.,
                1.,
                true,
                false,
                vec![vec![0, 1], vec![2]],
                vec![1., 1., 1.],
                None,
            ),
            (
                "fill share",
                vec![(1., false), (0., true), (0., true)],
                3.,
                1.,
                false,
                false,
                vec![vec![0, 1, 2]],
                vec![1., 0., 0.],
                None,
            ),
            (
                "fill share auto",
                vec![(1., false), (0., true), (0., true)],
                3.,
                1.,
                false,
                true,
                vec![vec![0, 1, 2]],
                vec![1., 1., 1.],
                None,
            ),
            (
                "overflow",
                vec![(1., false), (5., false)],
                3.,
                0.,
                true,
                false,
                vec![vec![0], vec![1]],
                vec![1., 5.],
                None,
            ),
            (
                "hug",
                vec![(1., false), (2., false)],
                3.,
                1.,
                false,
                false,
                vec![vec![0, 1]],
                vec![1., 2.],
                Some(4.),
            ),
            (
                "hug negative",
                vec![(1., false), (2., false)],
                3.,
                -1.,
                false,
                false,
                vec![vec![0, 1]],
                vec![1., 2.],
                Some(2.),
            ),
        ] {
            let mut s = state(&xs, capacity, gap, wrap, auto);
            assert_eq!(s.board.rows(0), rows, "{name}");
            let pixels = s.geometry.layout(&s.board, 0);
            for (m, w) in rendered.iter().enumerate() {
                assert_eq!(pixels.sizes[&(m as Id)].x, *w, "{name}");
            }
            if let Some(width) = hug {
                s.board.groups.get_mut(&0).unwrap().fixed = [false; 2];
                assert_eq!(s.geometry.measured(&s.board, 0, 0), width, "{name}");
            }
            if name == "wrap rows" {
                assert_eq!(s.arrow_target(0, 0, Direction::Down), Some(2));
                assert_eq!(s.arrow_target(0, 1, Direction::Down), None);
            }
            check(&mut s);
        }
    }
}

mod enablement {
    use super::*;

    fn axes(bits: usize) -> [bool; 2] {
        [0, 1].map(|a| bits & (1 << a) != 0)
    }
    #[test]
    fn enablement_scenarios() {
        for (name, panel, flow, gap, fixed, align, order, fill, enabled, disabled, hidden) in [
            (
                "member artboard",
                Panel::MemberDock {
                    group: 0,
                    member: 2,
                },
                GroupFlow::Wrap,
                GapMode::Number,
                3,
                [0, 0],
                vec![0, 1, 2],
                2,
                vec![Control::MemberMode(1, true)],
                vec![],
                vec![],
            ),
            (
                "auto align",
                Panel::GroupDock { group: 0 },
                GroupFlow::Row,
                GapMode::Between,
                1,
                [1, 0],
                vec![0, 2],
                0,
                vec![Control::Align([1, 2])],
                vec![Control::Align([0, 0]), Control::Gap(1.0)],
                vec![Control::RowGap(1.0)],
            ),
        ] {
            let mut s = sizing_init();
            s.create(0, &order, 1.0).unwrap();
            s.dock.panel = panel;
            let g = s.board.groups.get_mut(&0).unwrap();
            g.flow = flow;
            g.auto = gap;
            g.fixed = axes(fixed);
            g.align = align;
            s.board.members.get_mut(&2).unwrap().fill = axes(fill);
            for c in enabled {
                assert!(s.control_enabled(&c), "{name} {c:?}");
            }
            for c in disabled {
                assert!(!s.control_enabled(&c), "{name} {c:?}");
            }
            for c in hidden {
                assert!(!s.control_shown(&c), "{name} {c:?}");
            }
        }
    }
}
