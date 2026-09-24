use super::*;
use crate::placement::group_pixels::Pixels;
use serde_json::{Value, json};

fn init() -> State<Pixels> {
    State::init(
        Board {
            groups: BTreeMap::new(),
            live: BTreeSet::from([0, 2]),
            members: (0..3)
                .map(|m| (m, Member::fixed("Textbox", [1.0; 2])))
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
fn decode(v: &Value) -> Value {
    match v {
        Value::Array(xs) => Value::Array(xs.iter().map(decode).collect()),
        Value::Object(o) => {
            if let Some(n) = o.get("#bigint") {
                return json!(n.as_str().unwrap().parse::<i64>().unwrap());
            }
            if let Some(xs) = o.get("#set") {
                let mut xs: Vec<_> = xs.as_array().unwrap().iter().map(decode).collect();
                xs.sort_by_key(Value::to_string);
                return json!({"set": xs});
            }
            if let Some(xs) = o.get("#map") {
                let mut xs: Vec<_> = xs.as_array().unwrap().iter().map(decode).collect();
                xs.sort_by_key(|v| v[0].to_string());
                return json!({"map": xs});
            }
            if let Some(xs) = o.get("#tup") {
                return decode(xs);
            }
            Value::Object(o.iter().map(|(k, v)| (k.clone(), decode(v))).collect())
        }
        _ => v.clone(),
    }
}
fn map(xs: impl IntoIterator<Item = (Id, Value)>) -> Value {
    let mut entries: Vec<_> = xs.into_iter().map(|(k, v)| json!([k, v])).collect();
    entries.sort_by_key(|v| v[0].to_string());
    json!({"map": entries})
}
fn set(xs: impl IntoIterator<Item = i64>) -> Value {
    let mut xs: Vec<_> = xs.into_iter().collect();
    xs.sort_by_key(ToString::to_string);
    json!({"set": xs})
}
fn outcome<G>(s: &State<G>) -> Value {
    match s.outcome {
        Outcome::Ok => json!({"tag":"Ok", "value":[]}),
        Outcome::Rejected(reason) => {
            let canonical = match reason {
                "group_exists" => "group-exists",
                "group_missing" => "group-missing",
                "item_missing" => "item-missing",
                "item_exists" => "item-exists",
                "member_missing" => "member-missing",
                "control_inactive" => "control-inactive",
                "dock_unavailable" => "dock-unavailable",
                "no_neighbor" => "no-neighbor",
                "text_owns_key" => "text-owns-key",
                "unhandled_key" => "unhandled-key",
                other => panic!("Unexpected board outcome {other}"),
            };
            json!({"tag":"Rejected", "value":canonical})
        }
    }
}
/// Replay supplies Spatial Layout's resolved identity. Pixel behavior has separate real-value tests.
#[derive(Default)]
struct ReplayGeometry {
    pixels: Pixels,
    target: Option<Id>,
}
impl Geometry for ReplayGeometry {
    fn wrap_arrow_target(&self, _: &Board, _: Id, _: Id, _: &str) -> Option<Id> {
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
struct Transition {
    board: Board,
    dock: Dock,
    op: String,
}
impl Transition {
    fn refusal<G>(&self, s: &State<G>) -> bool {
        s.outcome == Outcome::Ok || (s.board == self.board && s.dock == self.dock)
    }
    fn empty<G>(&self, s: &State<G>) -> bool {
        !["create", "add", "insert", "detach", "delete"].contains(&self.op.as_str())
            || self.board.groups.iter().all(|(g, old)| {
                s.board
                    .groups
                    .get(g)
                    .is_some_and(|new| !new.order.is_empty() || new.flow == old.flow)
            })
    }
    fn append<G>(&self, s: &State<G>) -> bool {
        self.op != "add"
            || self.board.groups.iter().all(|(g, old)| {
                let new = &s.board.groups[g].order;
                new.len() <= old.order.len() || new.starts_with(&old.order)
            })
    }
}
fn routed(op: &str, picks: &Value) -> String {
    match op {
        "control" => match picks["c"]["value"]["tag"].as_str().unwrap() {
            "Flow" => "flow",
            "Ungroup" => "ungroup",
            "Absolute" => "absolute",
            "Earlier" | "Later" => "arrow",
            _ => "detach",
        },
        "key" => match picks["keypress"]["value"].as_str().unwrap() {
            "Escape" => "escape",
            "Primary+G" => "create",
            "Primary+V" => "paste",
            "Primary+D" => "duplicate",
            "Primary+Shift+G" => "ungroup",
            "Delete" => "delete",
            "Left" | "Right" | "Up" | "Down" => "arrow",
            _ => "key",
        },
        "drop" if picks["outside"]["value"] == true => "detach",
        "drop" => "reorder",
        _ => op,
    }
    .into()
}
fn board_observation(s: &State<ReplayGeometry>, t: &Transition) -> Value {
    let panel = match s.dock.panel {
        Panel::Hidden => json!({"tag":"Hidden", "value": []}),
        Panel::GroupDock { group, align_open } => {
            json!({"tag":"GroupDock", "value":{"group":group,"alignOpen":align_open}})
        }
        Panel::MemberDock { group, member } => {
            json!({"tag":"MemberDock", "value":{"group":group,"member":member}})
        }
    };
    json!({
        "board": {
            "groups": map(s.board.groups.iter().map(|(&id, g)| (id, json!({"order":g.order,"flow":g.flow})))),
            "live":set(s.board.live.iter().copied()),
            "members":map(s.board.members.iter().map(|(&id,m)| (id,json!({"absolute":m.absolute})))),
        },
        "dock":{"panel":panel,"textFocus":s.dock.text_focus},
        "outcome":outcome(s),
        "effects":{"refusal":t.refusal(s),"empty":t.empty(s),"append":t.append(s)},
    })
}
fn replay() {
    let module = "board";
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(".specs/traces");
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .expect("Generate traces with sh .specs/grouping/replay.sh")
        .map(|e| e.unwrap().path())
        .filter(|p| {
            p.file_name()
                .unwrap()
                .to_str()
                .is_some_and(|n| n.starts_with(module) && n.ends_with(".itf.json"))
        })
        .collect();
    files.sort();
    assert!(!files.is_empty(), "No {module} traces found");
    let mut lengths = BTreeMap::<usize, usize>::new();
    for path in &files {
        let trace: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        let states = trace["states"].as_array().unwrap();
        *lengths.entry(states.len() - 1).or_default() += 1;
        let mut s = State::init(init().board, ReplayGeometry::default(), 1.0);
        for (step, expected) in states.iter().enumerate() {
            let op = expected["mbt::actionTaken"]
                .as_str()
                .unwrap()
                .trim_end_matches("_any");
            let picks = decode(&expected["mbt::nondetPicks"]);
            let before = Transition {
                board: s.board.clone(),
                dock: s.dock.clone(),
                op: routed(op, &picks),
            };
            let pick = |k: &str| {
                assert_eq!(picks[k]["tag"], "Some", "{op}: missing {k}");
                &picks[k]["value"]
            };
            let n = |k: &str| pick(k).as_i64().unwrap();
            let flag = |k: &str| pick(k).as_bool().unwrap();
            let text = |k: &str| pick(k).as_str().unwrap();
            let selection = || {
                pick("selection")
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|n| n.as_i64().unwrap())
                    .collect::<Vec<_>>()
            };
            s.geometry.target = if matches!(op, "arrow" | "key") {
                let target = n("target");
                (target >= 0).then_some(target)
            } else {
                None
            };
            match op {
                "init" => Outcome::Ok,
                "create" => s.create(n("g"), &selection(), 1.0),
                "add" => s.add(n("g"), n("m")),
                "detach" => s.detach(n("g"), n("m")),
                "ungroup" => s.ungroup(n("g")),
                "flow" => s.setFlow(n("g"), text("mode")),
                "insert" => s.dragIn(n("g"), n("m"), n("slot")),
                "paste" => s.paste(n("m"), 1.0),
                "duplicate" => s.duplicate(n("source"), n("m")),
                "reorder" => s.reorder(n("g"), n("m"), n("slot")),
                "arrow" => s.arrow(n("g"), n("m"), text("direction")),
                "absolute" => s.toggleAbsolute(n("g"), n("m"), n("slot")),
                "delete" => s.deleteMember(n("m")),
                "choose" => s.selectGroup(n("g")),
                "edit" => s.editMember(n("m")),
                "clear" => s.clearSelection(),
                "alignment" => s.toggleAlign(),
                "dismiss" => s.dismissAlign(),
                "focus" => s.focusText(flag("active")),
                "escape" => s.escape(),
                "control" => s.useDock(board_control(pick("c"))),
                "key" => s.pressKey(text("keypress"), n("identity"), &selection(), 1.0),
                "drop" => s.dropMember(n("g"), n("m"), n("slot"), flag("outside")),
                _ => panic!("Unknown {module} operation {op}"),
            };
            let expected: serde_json::Map<String, Value> = trace["vars"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_str().unwrap())
                .filter(|v| !v.starts_with("mbt::"))
                .map(|v| (v.into(), decode(&expected[v])))
                .collect();
            assert_eq!(
                board_observation(&s, &before),
                Value::Object(expected),
                "{} step {step}: {op}({picks})",
                path.display()
            );
        }
    }
    println!(
        "{module}: replayed {} traces, operation lengths {lengths:?}",
        files.len()
    );
}
#[test]
#[ignore = "needs traces from sh .specs/grouping/replay.sh"]
fn replay_every_board_trace() {
    replay();
}
fn board_control(v: &Value) -> Control {
    match v["tag"].as_str().unwrap() {
        "Flow" => Control::Flow(v["value"].as_str().unwrap().into()),
        "Ungroup" => Control::Ungroup,
        "Absolute" => Control::Absolute(v["value"].as_i64().unwrap()),
        "Earlier" => Control::Earlier,
        "Later" => Control::Later,
        "Detach" => Control::Detach,
        tag => panic!("Unknown board control {tag}"),
    }
}

mod sizing {
    use super::*;
    #[test]
    fn creationTest() {
        let mut s = sizing_init();
        s.create(0, &[0, 2], 1.0);
        let g = &s.board.groups[&0];
        assert_eq!(
            (&*g.flow, &g.fixed, &*g.gap),
            ("Wrap", &BTreeSet::from([0]), "Number")
        );
        assert_eq!(s.board.owner(0), Some(0));
        assert!(s.board.members[&0].fill.is_empty());
        assert_eq!(s.create(0, &[1], 1.0), Outcome::Rejected("group_exists"));
    }
    #[test]
    fn autoHugConversionTest() {
        let mut s = sizing_init();
        s.create(0, &[2], 1.0);
        s.setFlow(0, "Row");
        s.setAxisMode(0, 0, AxisMode::Hug);
        s.setGap(0, "Between", 1.0);
        assert!(s.board.groups[&0].fixed.contains(&0));
        s.setMemberMode(0, 2, 0, true);
        s.setAxisMode(0, 0, AxisMode::Hug);
        assert!(s.board.members[&2].fill.is_empty());
        assert_eq!(s.board.groups[&0].gap, "Number");
    }
    #[test]
    fn editConstraintsTest() {
        let mut s = sizing_init();
        s.create(0, &[2], 1.0);
        assert_eq!(
            s.setAxisMode(0, 0, AxisMode::Hug),
            Outcome::Rejected("wrap_width_fixed")
        );
        assert_eq!(
            s.setMemberMode(0, 2, 1, true),
            Outcome::Rejected("fixed_axis_required")
        );
        assert!(s.board.members[&2].fill.is_empty());
        s.setAxisMode(0, 1, AxisMode::Fixed(1.0));
        s.setMemberMode(0, 2, 1, true);
        assert_eq!(s.board.members[&2].fill, BTreeSet::from([1]));
    }
    #[test]
    fn freeAndFlowTest() {
        let mut s = sizing_init();
        s.create(0, &[0, 2], 1.0);
        s.setMemberMode(0, 2, 0, true);
        s.setFlow(0, "Free");
        s.setAxisMode(0, 0, AxisMode::Hug);
        s.setGap(0, "Between", 1.0);
        assert!(s.board.groups[&0].fixed.is_empty() && s.board.members[&2].fill.is_empty());
        assert_eq!(
            s.setMemberMode(0, 0, 1, true),
            Outcome::Rejected("control_inactive")
        );
        s.setFlow(0, "Column");
        assert_eq!(s.board.groups[&0].fixed, BTreeSet::from([1]));
        s.setFlow(0, "Wrap");
        assert_eq!(s.board.groups[&0].fixed, BTreeSet::from([0, 1]));
    }
    #[test]
    fn transferFillToHugTest() {
        let mut s = sizing_init();
        s.create(0, &[0], 1.0);
        s.setMemberMode(0, 0, 0, true);
        s.create(1, &[2], 1.0);
        s.setFlow(1, "Row");
        s.setAxisMode(1, 0, AxisMode::Hug);
        s.add(1, 0);
        assert_eq!(s.board.owner(0), Some(1));
        assert!(s.board.members[&0].fill.is_empty());
        assert_eq!(s.board.owner(2), Some(1));
    }
    #[test]
    fn leaveTest() {
        let mut s = sizing_init();
        s.create(0, &[0, 2], 1.0);
        s.setMemberMode(0, 0, 0, true);
        s.detach(0, 0);
        assert_eq!(s.board.owner(0), None);
        assert!(s.board.members[&0].fill.is_empty());
        assert!(s.board.groups.contains_key(&0));
        s.ungroup(0);
        assert!(s.board.groups.is_empty());
        assert_eq!(s.board.owner(2), None);
    }
}
mod board {
    use super::*;
    #[test]
    fn creationAndReaddTest() {
        let mut s = init();
        s.create(0, &[0, 2], 1.0);
        assert_eq!(s.board.groups[&0].order, [0, 2]);
        assert_eq!(
            s.dock.panel,
            Panel::GroupDock {
                group: 0,
                align_open: false
            }
        );
        assert_eq!(s.add(0, 0), Outcome::Ok);
        assert_eq!(s.board.groups[&0].order, [0, 2]);
        assert_eq!(s.add(0, 1), Outcome::Rejected("item_missing"));
        s.ungroup(0);
        s.create(0, &[2, 0], 1.0);
        assert_eq!(s.board.groups[&0].order, [2, 0]);
    }
    #[test]
    fn pasteAndDuplicateTest() {
        let mut s = init();
        s.create(0, &[0, 2], 1.0);
        s.editMember(0);
        s.pressKey("Primary+V", 1, &[], 1.0);
        assert_eq!(s.board.groups[&0].order, [0, 1, 2]);
        assert_eq!(
            s.dock.panel,
            Panel::MemberDock {
                group: 0,
                member: 1
            }
        );
        s.pressKey("Delete", 0, &[], 1.0);
        assert_eq!(s.board.live, BTreeSet::from([0, 2]));
        assert_eq!(
            s.dock.panel,
            Panel::GroupDock {
                group: 0,
                align_open: false
            }
        );
        assert_eq!(
            s.pressKey("Primary+D", 1, &[], 1.0),
            Outcome::Rejected("unhandled_key")
        );
        assert_eq!(s.board.live, BTreeSet::from([0, 2]));
    }
    #[test]
    fn arrowTest() {
        let mut s = init();
        s.paste(1, 1.0);
        s.create(0, &[0, 1, 2], 1.0);
        s.setFlow(0, "Column");
        s.editMember(0);
        s.pressKey("Down", 0, &[], 1.0);
        assert_eq!(s.board.groups[&0].order, [1, 0, 2]);
        assert_eq!(
            s.pressKey("Right", 0, &[], 1.0),
            Outcome::Rejected("no_neighbor")
        );
        s.useDock(Control::Absolute(0));
        assert_eq!(s.useDock(Control::Later), Outcome::Rejected("no_neighbor"));
        assert_eq!(s.board.groups[&0].order, [1, 0, 2]);
        s.useDock(Control::Absolute(2));
        assert_eq!(s.board.groups[&0].order, [1, 2, 0]);
        assert!(!s.board.members[&0].absolute);
        s.setFlow(0, "Free");
        assert_eq!(
            s.useDock(Control::Earlier),
            Outcome::Rejected("no_neighbor")
        );
    }
    #[test]
    fn verticalTargetTest() {
        let mut s = State::init(init().board, ReplayGeometry::default(), 1.0);
        s.paste(1, 1.0);
        s.create(0, &[0, 1, 2], 1.0);
        s.editMember(0);
        s.geometry.target = Some(2);
        assert_eq!(s.pressKey("Down", 0, &[], 1.0), Outcome::Ok);
        assert_eq!(s.board.groups[&0].order, [1, 2, 0]);
        s.geometry.target = None;
        assert_eq!(
            s.pressKey("Down", 0, &[], 1.0),
            Outcome::Rejected("no_neighbor")
        );
        assert_eq!(s.board.groups[&0].order, [1, 2, 0]);
    }
    #[test]
    fn transferAndDetachTest() {
        let mut s = init();
        s.create(0, &[0], 1.0);
        s.create(1, &[2], 1.0);
        s.editMember(0);
        s.add(1, 0);
        assert!(s.board.groups[&0].order.is_empty());
        assert_eq!(s.board.groups[&1].order, [2, 0]);
        assert_eq!(
            s.dock.panel,
            Panel::MemberDock {
                group: 1,
                member: 0
            }
        );
        s.useDock(Control::Detach);
        assert_eq!(s.dock.panel, Panel::Hidden);
        assert_eq!(s.board.owner(0), None);
        assert!(s.board.groups.contains_key(&1));
        s.selectGroup(0);
        assert_eq!(s.dockCount(), 0);
    }
    #[test]
    fn escapeTest() {
        let mut s = init();
        s.create(0, &[0, 2], 1.0);
        s.toggleAlign();
        s.focusText(true);
        assert_eq!(
            s.pressKey("Left", 0, &[], 1.0),
            Outcome::Rejected("text_owns_key")
        );
        s.escape();
        assert!(s.dock.align_shown() && !s.dock.text_focus);
        s.escape();
        assert_eq!(
            s.dock.panel,
            Panel::GroupDock {
                group: 0,
                align_open: false
            }
        );
        s.editMember(2);
        s.escape();
        assert!(s.dock.group_shown());
        s.escape();
        assert_eq!(s.dock.panel, Panel::Hidden);
    }
    #[test]
    fn freeAndUngroupTest() {
        let mut s = init();
        s.create(0, &[0, 2], 1.0);
        s.editMember(2);
        s.useDock(Control::Absolute(0));
        s.escape();
        s.toggleAlign();
        s.useDock(Control::Flow("Free".into()));
        assert!(!s.dock.align_shown());
        assert_eq!(s.toggleAlign(), Outcome::Rejected("control_inactive"));
        s.pressKey("Primary+Shift+G", 0, &[], 1.0);
        assert!(s.board.groups.is_empty());
        assert_eq!(s.dock.panel, Panel::Hidden);
        assert!(!s.board.members[&2].absolute);
    }
    #[test]
    fn dropTest() {
        let mut s = init();
        s.create(0, &[0, 2], 1.0);
        s.toggleAbsolute(0, 2, 0);
        s.dropMember(0, 2, 0, true);
        assert_eq!(s.board.groups[&0].order, [0, 2]);
        s.dropMember(0, 0, 0, true);
        assert_eq!(s.board.groups[&0].order, [2]);
    }
}
#[test]
fn stored_dock_values() {
    let mut s = init();
    s.create(0, &[0, 2], 3.0);
    for c in [
        Control::Name("1".into()),
        Control::Gap(-1.0),
        Control::RowGap(-1.0),
        Control::Padding(0.0),
        Control::Dimension(1, 3.0),
        Control::Clip,
    ] {
        assert_eq!(s.useDock(c), Outcome::Ok);
    }
    let g = &s.board.groups[&0];
    assert_eq!(
        (&g.name, g.number, g.rowGap, g.padding, g.clip),
        (&"1".to_string(), -1.0, -1.0, 0.0, true)
    );
    assert!(g.fixed.contains(&1));
    assert_eq!(
        s.useDock(Control::Dimension(0, 0.0)),
        Outcome::Rejected("invalid_value")
    );
    let before = s.board.clone();
    let dock = s.dock.clone();
    for c in [
        Control::Name(" ".into()),
        Control::Padding(-1.0),
        Control::Dimension(1, f32::NAN),
    ] {
        assert_eq!(s.useDock(c), Outcome::Rejected("invalid_value"));
        assert_eq!(s.board, before);
        assert_eq!(s.dock, dock);
    }
    assert_eq!(
        s.setMemberMode(0, 0, 2, false),
        Outcome::Rejected("invalid_value")
    );
    s.toggleAbsolute(0, 0, 0);
    assert_eq!(
        s.setMemberMode(0, 0, 0, false),
        Outcome::Rejected("control_inactive")
    );
    s.useDock(Control::Flow("Free".into()));
    assert!(!s.controlEnabled(&Control::Gap(1.0)));
    assert!(!s.controlShown(&Control::RowGap(1.0)));
    assert_eq!(
        s.useDock(Control::Gap(1.0)),
        Outcome::Rejected("control_inactive")
    );
}
#[test]
fn stored_auto_variant_and_number() {
    let mut s = init();
    s.create(0, &[0, 2], 3.0);
    s.useDock(Control::Gap(-1.0));
    s.toggleAlign();
    s.useDock(Control::Align([2, 0]));
    s.useDock(Control::Auto("Between".into()));
    assert!(s.dock.align_shown());
    assert!(!s.controlEnabled(&Control::Gap(0.0)));
    assert!(s.controlEnabled(&Control::Align([2, 1])));
    assert!(!s.controlEnabled(&Control::Align([0, 1])));
    assert_eq!(
        s.useDock(Control::Align([0, 1])),
        Outcome::Rejected("primary_alignment_inactive")
    );
    s.useDock(Control::Align([2, 1]));
    s.useDock(Control::Auto("Number".into()));
    assert_eq!(s.board.groups[&0].number, -1.0);
    assert_eq!(s.board.groups[&0].align, [2, 1]);
    s.useDock(Control::Flow("Free".into()));
    assert!(!s.dock.align_shown());
}
#[test]
fn corner_hugs_each_legal_axis() {
    let mut s = init();
    s.create(0, &[0, 2], 1.0);
    s.useDock(Control::Dimension(1, 3.0));
    s.doubleClickCorner();
    assert_eq!(s.board.groups[&0].fixed, BTreeSet::from([0]));
    s.setFlow(0, "Row");
    s.doubleClickCorner();
    assert!(s.board.groups[&0].fixed.is_empty());
}

#[test]
fn incoming_fit_preserves_existing_members_and_later_resize_does_not_refit() {
    let mut s = init();
    s.board
        .members
        .insert(0, Member::fixed("Image", [200.0, 80.0]));
    s.board
        .members
        .insert(2, Member::fixed("Image", [200.0, 100.0]));
    s.create(0, &[0], 200.0);
    s.useDock(Control::Dimension(0, 102.0));
    s.add(0, 2);
    assert_eq!(s.board.members[&0].size, [200.0, 80.0]);
    assert_eq!(s.board.members[&2].size, [100.0, 50.0]);
    s.useDock(Control::Dimension(0, 52.0));
    assert_eq!(s.board.members[&2].size, [100.0, 50.0]);
    assert_eq!(s.geometry.rendered(&s.board, 0, 2, 0), 100.0);
}

mod layout {
    use super::*;

    fn state(xs: &[(f32, bool)], capacity: f32, gap: f32, wrap: bool, auto: bool) -> State<Pixels> {
        let mut group = Group::new(0, capacity, gap);
        group.padding = 0.0;
        group.size[0] = capacity;
        group.flow = if wrap { "Wrap" } else { "Row" }.into();
        group.gap = if auto { "Between" } else { "Number" }.into();
        group.order = (0..xs.len() as Id).collect();
        State::init(
            Board {
                groups: BTreeMap::from([(0, group)]),
                live: (0..xs.len() as Id).collect(),
                members: xs
                    .iter()
                    .enumerate()
                    .map(|(m, &(width, fill))| {
                        let mut member = Member::fixed("Textbox", [width, 1.0]);
                        if fill {
                            member.fill.insert(0);
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
                + g.numeric() * row.len().saturating_sub(1) as f32
        };
        // Rows partition membership.
        assert_eq!(rows.concat(), g.order);
        assert!(rows.iter().all(|r| !r.is_empty()));
        assert!(g.flow == "Wrap" || rows.len() <= 1);
        for (r, row) in rows.iter().enumerate() {
            // A new row is necessary exactly when the next member exceeds capacity.
            assert!(
                g.flow != "Wrap" || used(row) <= g.size[0] || row.len() == 1,
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
                - if g.gap == "Number" {
                    g.number * row.len().saturating_sub(1) as f32
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
                let expected = if b.members[&m].fill.contains(&0) {
                    share
                } else {
                    b.members[&m].size[0]
                };
                assert!((pixels.sizes[&m].x - expected).abs() < 0.001);
                // Vertical arrows keep the column.
                if g.flow == "Wrap" {
                    for (direction, adjacent) in [("Up", r.checked_sub(1)), ("Down", Some(r + 1))] {
                        let expected = adjacent
                            .and_then(|r| rows.get(r))
                            .and_then(|r| r.get(c))
                            .copied();
                        assert_eq!(s.arrowTarget(0, m, direction).map(|i| g.order[i]), expected);
                    }
                }
            }
        }
        // Hug covers every fixed member.
        if g.number >= 0.0 {
            let xs: Vec<_> = g.order.iter().map(|&m| (b.base(m, 0), false)).collect();
            let mut hug = state(&xs, g.size[0], g.numeric(), false, false);
            hug.board.groups.get_mut(&0).unwrap().fixed.clear();
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
                "wrapRowsTest",
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
                "fillShareTest",
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
                "fillShareTest Auto",
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
                "overflowTest",
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
                "hugTest",
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
                "hugTest negative",
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
                s.board.groups.get_mut(&0).unwrap().fixed.clear();
                assert_eq!(s.geometry.measured(&s.board, 0, 0), width, "{name}");
            }
            if name == "wrapRowsTest" {
                assert_eq!(s.arrowTarget(0, 0, "Down"), Some(2));
                assert_eq!(s.arrowTarget(0, 1, "Down"), None);
            }
            check(&mut s);
        }
    }
}

mod enablement {
    use super::*;

    fn axes(bits: usize) -> BTreeSet<usize> {
        (0..2).filter(|a| bits & (1 << a) != 0).collect()
    }
    #[test]
    fn enablement_scenarios() {
        for (name, panel, flow, gap, fixed, align, order, fill, enabled, disabled, hidden) in [
            (
                "memberArtboardTest",
                Panel::MemberDock {
                    group: 0,
                    member: 2,
                },
                "Wrap",
                "Number",
                3,
                [0, 0],
                vec![0, 1, 2],
                2,
                vec![Control::MemberMode(1, true), Control::Earlier],
                vec![Control::Later],
                vec![],
            ),
            (
                "autoAlignTest",
                Panel::GroupDock {
                    group: 0,
                    align_open: true,
                },
                "Row",
                "Between",
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
            s.create(0, &order, 1.0);
            s.dock.panel = panel;
            let g = s.board.groups.get_mut(&0).unwrap();
            g.flow = flow.into();
            g.gap = gap.into();
            g.fixed = axes(fixed);
            g.align = align;
            s.board.members.get_mut(&2).unwrap().fill = axes(fill);
            for c in enabled {
                assert!(s.controlEnabled(&c), "{name} {c:?}");
            }
            for c in disabled {
                assert!(!s.controlEnabled(&c), "{name} {c:?}");
            }
            for c in hidden {
                assert!(!s.controlShown(&c), "{name} {c:?}");
            }
        }
    }
}
