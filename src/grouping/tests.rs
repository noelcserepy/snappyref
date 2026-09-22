use super::*;
use serde_json::{Value, json};

struct QuintGeometry;
impl Geometry for QuintGeometry {
    fn extent(&self, n: f32) -> f32 {
        n.clamp(0.0, 3.0)
    }
    fn rendered(&self, b: &Board, g: Id, m: Id, a: usize) -> f32 {
        let group = &b.groups[&g];
        let member = &b.members[&m];
        if member.absolute || group.flow == "Free" || !member.fill.contains(&a) {
            member.size[a]
        } else if a != group.primary() {
            group.size[a]
        } else {
            let xs = if group.flow == "Wrap" {
                b.rows(g)
                    .into_iter()
                    .find(|row| row.contains(&m))
                    .unwrap_or_default()
            } else {
                b.flowing(g)
            };
            let (space, count) = b.allocation(g, &xs, a);
            if count == 0 {
                0.0
            } else {
                self.extent((space / count as f32).floor())
            }
        }
    }
    fn measured(&self, b: &Board, g: Id, a: usize) -> f32 {
        let group = &b.groups[&g];
        let xs = b.flowing(g);
        if group.fixed.contains(&a) || xs.is_empty() {
            group.size[a]
        } else if group.flow != "Free" && a == group.primary() {
            self.extent(
                xs.iter().map(|&m| self.rendered(b, g, m, a)).sum::<f32>()
                    + group.numeric() * xs.len().saturating_sub(1) as f32,
            )
            .max(1.0)
        } else {
            xs.iter()
                .map(|&m| self.rendered(b, g, m, a))
                .fold(1.0, f32::max)
        }
    }
}
fn init() -> State<QuintGeometry> {
    State::init(
        Board {
            groups: BTreeMap::new(),
            live: BTreeSet::from([0, 2]),
            members: BTreeMap::from([
                (0, Member::fixed("Image", [3.0; 2])),
                (1, Member::fixed("Image", [1.0; 2])),
                (2, Member::fixed("Textbox", [1.0; 2])),
            ]),
        },
        QuintGeometry,
        1.0,
    )
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
fn dimensions(size: [f32; 2]) -> Value {
    map(size
        .into_iter()
        .enumerate()
        .map(|(a, n)| (a as Id, json!(n as i64))))
}
fn observation(s: &State<QuintGeometry>) -> Value {
    let groups = map(s.board.groups.iter().map(|(&id, g)| (id, json!({
        "order": g.order, "flow": g.flow, "fixed": set(g.fixed.iter().map(|&a| a as i64)), "size": dimensions(g.size),
        "gap": g.gap, "number": g.number as i64, "rowGap": g.rowGap as i64, "padding": g.padding as i64,
        "align": map(g.align.iter().enumerate().map(|(a, n)| (a as Id, json!(n)))), "clip": g.clip, "name": g.name.parse::<i64>().unwrap(),
    }))));
    let members = map(s.board.members.iter().map(|(&id, m)| (id, json!({"kind": m.kind, "size": dimensions(m.size), "fill": set(m.fill.iter().map(|&a| a as i64)), "absolute": m.absolute}))));
    let panel = match s.dock.panel {
        Panel::Hidden => json!({"tag":"Hidden", "value": []}),
        Panel::GroupDock { group, align_open } => {
            json!({"tag":"GroupDock", "value":{"group":group,"alignOpen":align_open}})
        }
        Panel::MemberDock { group, member } => {
            json!({"tag":"MemberDock", "value":{"group":group,"member":member}})
        }
    };
    json!({"dock":{"panel":panel,"textFocus":s.dock.text_focus}, "board": {"groups": groups, "live": set(s.board.live.iter().copied()), "members": members},
        "outcome": match s.outcome { Outcome::Ok => json!({"tag":"Ok", "value":[]}), Outcome::Rejected(r) => json!({"tag":"Rejected", "value":r}) },
        "event": s.event, "effects": {"refusal": s.effects.refusal, "incoming": s.effects.incoming, "empty": s.effects.empty, "resize": s.effects.resize, "append": s.effects.append}})
}
#[test]
fn replay_every_grouping_trace() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("specs/traces");
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .expect("Generate traces with quint run --mbt into specs/traces (implement-quint-spec)")
        .map(|e| e.unwrap().path())
        .filter(|p| p.to_string_lossy().ends_with(".itf.json"))
        .collect();
    files.sort();
    assert!(!files.is_empty(), "No grouping traces found");
    let mut lengths = BTreeMap::<usize, usize>::new();
    for path in &files {
        let trace: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        let states = trace["states"].as_array().unwrap();
        *lengths.entry(states.len() - 1).or_default() += 1;
        let mut s = init();
        for (step, expected) in states.iter().enumerate() {
            let op = expected["mbt::actionTaken"]
                .as_str()
                .unwrap()
                .trim_end_matches("_any");
            let picks = decode(&expected["mbt::nondetPicks"]);
            let n = |k: &str| picks[k]["value"].as_i64().unwrap();
            let f = |k: &str| n(k) as f32;
            let a = |k: &str| n(k) as usize;
            let flag = |k: &str| picks[k]["value"].as_bool().unwrap();
            let text = |k: &str| picks[k]["value"].as_str().unwrap();
            match op {
                "init" => {}
                "selectGroup" => {
                    s.selectGroup(n("g"));
                }
                "editMember" => {
                    s.editMember(n("m"));
                }
                "clearSelection" => {
                    s.clearSelection();
                }
                "toggleAlign" => {
                    s.toggleAlign();
                }
                "dismissAlign" => {
                    s.dismissAlign();
                }
                "focusText" => {
                    s.focusText(flag("active"));
                }
                "escape" => {
                    s.escape();
                }
                "useDock" => {
                    s.useDock(control(&picks["c"]["value"]));
                }
                "pressKey" => {
                    let xs: Vec<_> = picks["selection"]["value"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|n| n.as_i64().unwrap())
                        .collect();
                    s.pressKey(text("key"), n("identity"), &xs, f("width"));
                }
                "doubleClickHandle" => {
                    s.doubleClickHandle(a("a"));
                }
                "doubleClickCorner" => {
                    s.doubleClickCorner();
                }
                "dropMember" => {
                    s.dropMember(n("g"), n("m"), n("slot"), flag("outside"));
                }
                "uniformScale" => {
                    s.uniformScale();
                }
                "create" => {
                    let xs: Vec<_> = picks["selection"]["value"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|n| n.as_i64().unwrap())
                        .collect();
                    s.create(n("g"), &xs, f("width"));
                }
                "add" => {
                    s.add(n("g"), n("m"));
                }
                "dragIn" => {
                    s.dragIn(n("g"), n("m"), n("slot"));
                }
                "paste" => {
                    s.paste(n("g"), n("anchor"), n("m"), f("width"));
                }
                "duplicate" => {
                    s.duplicate(n("source"), n("m"));
                }
                "reorder" => {
                    s.reorder(n("g"), n("m"), n("slot"));
                }
                "arrow" => {
                    s.arrow(n("g"), n("m"), text("direction"));
                }
                "detach" => {
                    s.detach(n("g"), n("m"));
                }
                "toggleAbsolute" => {
                    s.toggleAbsolute(n("g"), n("m"), n("slot"));
                }
                "setFlow" => {
                    s.setFlow(n("g"), text("flow"));
                }
                "setGap" => {
                    s.setGap(n("g"), text("gap"), f("number"));
                }
                "setAxisMode" => {
                    s.setAxisMode(n("g"), a("a"), flag("fixed"));
                }
                "resizeEdge" => {
                    s.resizeEdge(n("g"), a("a"), f("dimension"));
                }
                "hugAxis" => {
                    s.hugAxis(n("g"), a("a"));
                }
                "setMemberMode" => {
                    s.setMemberMode(n("g"), n("m"), a("a"), flag("fill"));
                }
                "configure" => {
                    s.configure(
                        n("g"),
                        flag("clip"),
                        a("alignX"),
                        a("alignY"),
                        f("padding"),
                        f("rowGap"),
                    );
                }
                "ungroup" => {
                    s.ungroup(n("g"));
                }
                "deleteMember" => {
                    s.deleteMember(n("m"));
                }
                _ => panic!("Unknown operation {op}"),
            }
            let expected = json!({"dock": decode(&expected["dock"]), "board": decode(&expected["board"]), "outcome": decode(&expected["outcome"]), "event": expected["event"], "effects": expected["effects"]});
            assert_eq!(
                observation(&s),
                expected,
                "{} step {step}: {op}({picks})",
                path.display()
            );
        }
    }
    println!(
        "Replayed {} traces, operation lengths {lengths:?}",
        files.len()
    );
}

#[test]
fn creationAndReaddTest() {
    let mut s = init();
    s.create(0, &[0, 2, 3], 3.0);
    assert_eq!(s.board.groups[&0].order, [0, 2]);
    assert_eq!(s.board.groups[&0].fixed, BTreeSet::from([0]));
    assert!(!s.board.groups[&0].clip);
    s.add(0, 0);
    assert_eq!(s.board.groups[&0].order, [0, 2]);
}
#[test]
fn ineligibleSelectionTest() {
    let mut s = init();
    s.create(0, &[3], 3.0);
    assert!(s.board.groups.is_empty());
    assert_eq!(s.outcome, Outcome::Ok);
}
#[test]
fn transferTest() {
    let mut s = init();
    s.create(0, &[0], 3.0);
    s.create(1, &[2], 3.0);
    s.add(1, 0);
    assert!(s.board.groups[&0].order.is_empty());
    assert_eq!(s.board.groups[&1].order, [2, 0]);
    assert_eq!(s.board.live, BTreeSet::from([0, 2]));
    s.add(0, 0);
    assert_eq!(s.board.groups[&0].order, [0]);
    assert_eq!(s.board.groups[&1].order, [2]);
}
#[test]
fn incomingAndOverflowTest() {
    let mut s = init();
    s.create(0, &[2], 2.0);
    s.add(0, 0);
    assert_eq!(s.board.members[&0].size[0], 2.0);
    assert_eq!(s.board.members[&2].size[0], 1.0);
    s.resizeEdge(0, 0, 1.0);
    assert_eq!(s.board.members[&0].size[0], 2.0);
    assert!(s.geometry.rendered(&s.board, 0, 0, 0) > s.board.groups[&0].size[0]);
}
#[test]
fn wrapAppendAndRemovalTest() {
    let mut s = init();
    s.create(0, &[2], 3.0);
    s.paste(0, -1, 1, 1.0);
    assert_eq!(s.board.rows(0), vec![vec![2, 1]]);
    s.add(0, 0);
    assert_eq!(s.board.rows(0), vec![vec![2, 1], vec![0]]);
    s.deleteMember(2);
    assert_eq!(s.board.groups[&0].order, [1, 0]);
}
#[test]
fn pasteContextTest() {
    let mut s = init();
    s.create(0, &[0, 2], 3.0);
    s.paste(0, 0, 1, 1.0);
    assert_eq!(s.board.groups[&0].order, [0, 1, 2]);
    s.deleteMember(1);
    s.paste(0, -1, 1, 1.0);
    assert_eq!(s.board.groups[&0].order, [0, 2, 1]);
    s.deleteMember(1);
    s.paste(-1, -1, 1, 1.0);
    assert_eq!(s.board.owner(1), None);
}
#[test]
fn duplicateTest() {
    let mut s = init();
    s.create(0, &[2, 0], 3.0);
    s.duplicate(2, 1);
    assert_eq!(s.board.groups[&0].order, [2, 1, 0]);
    assert_eq!(s.board.members[&1].kind, "Textbox");
}
#[test]
fn insertionAndArrowsTest() {
    let mut s = init();
    s.create(0, &[2], 3.0);
    s.dragIn(0, 0, 0);
    assert_eq!(s.board.groups[&0].order, [0, 2]);
    s.reorder(0, 0, 1);
    assert_eq!(s.board.groups[&0].order, [2, 0]);
    s.arrow(0, 0, "Left");
    assert_eq!(s.board.groups[&0].order, [0, 2]);
}
#[test]
fn wrapVerticalArrowTest() {
    let mut s = init();
    s.create(0, &[2], 3.0);
    s.paste(0, -1, 1, 1.0);
    s.add(0, 0);
    s.arrow(0, 2, "Down");
    assert_eq!(s.board.groups[&0].order, [1, 0, 2]);
}
#[test]
fn autoHugConversionTest() {
    let mut s = init();
    s.create(0, &[2], 3.0);
    s.setFlow(0, "Row");
    s.hugAxis(0, 0);
    s.setGap(0, "Number", -1.0);
    s.setGap(0, "Around", 0.0);
    assert!(s.board.groups[&0].fixed.contains(&0));
    s.setMemberMode(0, 2, 0, true);
    s.hugAxis(0, 0);
    assert!(s.board.members[&2].fill.is_empty());
    assert_eq!(s.board.groups[&0].gap, "Number");
    assert_eq!(s.board.groups[&0].number, -1.0);
}
#[test]
fn editConstraintsTest() {
    let mut s = init();
    s.create(0, &[2], 3.0);
    assert_eq!(s.hugAxis(0, 0), Outcome::Rejected("wrap_width_fixed"));
    assert_eq!(
        s.setMemberMode(0, 2, 1, true),
        Outcome::Rejected("fixed_axis_required")
    );
    s.resizeEdge(0, 1, 2.0);
    s.setMemberMode(0, 2, 1, true);
    s.hugAxis(0, 1);
    assert_eq!(s.board.members[&2].size[1], 2.0);
    assert!(s.board.members[&2].fill.is_empty());
}
#[test]
fn equalFillTest() {
    let mut s = init();
    s.create(0, &[2], 3.0);
    s.paste(0, -1, 1, 1.0);
    s.setFlow(0, "Row");
    s.setMemberMode(0, 2, 0, true);
    s.setMemberMode(0, 1, 0, true);
    assert_eq!(s.board.allocation(0, &[2, 1], 0), (2.0, 2));
    s.setGap(0, "Between", 0.0);
    assert_eq!(s.board.allocation(0, &[2, 1], 0), (3.0, 2));
}
#[test]
fn zeroLeftoverTest() {
    let mut s = init();
    s.create(0, &[0, 2], 3.0);
    s.setFlow(0, "Row");
    s.setMemberMode(0, 2, 0, true);
    s.resizeEdge(0, 0, 1.0);
    assert_eq!(s.geometry.rendered(&s.board, 0, 2, 0), 0.0);
    assert_eq!(s.geometry.rendered(&s.board, 0, 0, 0), 3.0);
}
#[test]
fn absoluteReinsertTest() {
    let mut s = init();
    s.create(0, &[0, 2], 3.0);
    s.toggleAbsolute(0, 0, 0);
    assert_eq!(s.board.flowing(0), [2]);
    s.reorder(0, 0, 1);
    assert_eq!(s.board.groups[&0].order, [0, 2]);
    s.toggleAbsolute(0, 0, 1);
    assert_eq!(s.board.groups[&0].order, [2, 0]);
}
#[test]
fn freeAndFlowTest() {
    let mut s = init();
    s.create(0, &[0, 2], 3.0);
    s.setFlow(0, "Free");
    s.hugAxis(0, 0);
    s.setGap(0, "Evenly", 0.0);
    s.reorder(0, 0, 1);
    assert_eq!(s.board.groups[&0].order, [0, 2]);
    s.setFlow(0, "Column");
    assert!(s.board.groups[&0].fixed.contains(&1));
    s.setFlow(0, "Wrap");
    assert!(s.board.groups[&0].fixed.contains(&0));
}
#[test]
fn clipConfigurationTest() {
    let mut s = init();
    s.create(0, &[0, 2], 3.0);
    s.configure(0, true, 2, 1, 0.0, -1.0);
    assert_eq!(s.board.groups[&0].order, [0, 2]);
    assert_eq!(s.board.rows(0), vec![vec![0], vec![2]]);
    assert_eq!(s.board.members[&0].size[0], 3.0);
}
#[test]
fn emptyAndUngroupTest() {
    let mut s = init();
    s.create(0, &[0], 3.0);
    s.detach(0, 0);
    assert!(s.board.groups[&0].order.is_empty());
    assert_eq!(s.board.groups[&0].size[0], 3.0);
    assert_eq!(s.board.groups[&0].name, "0");
    s.add(0, 0);
    s.toggleAbsolute(0, 0, 0);
    s.ungroup(0);
    assert!(s.board.groups.is_empty());
    assert_eq!(s.board.live, BTreeSet::from([0, 2]));
    assert_eq!(s.board.members[&0].size[0], 3.0);
}
#[test]
fn freeIgnoresLayoutTest() {
    let mut s = init();
    s.create(0, &[2], 1.0);
    s.setFlow(0, "Free");
    s.add(0, 0);
    assert_eq!(s.board.members[&0].size[0], 3.0);
    s.setMemberMode(0, 0, 0, true);
    assert_eq!(s.geometry.rendered(&s.board, 0, 0, 0), 3.0);
    s.hugAxis(0, 0);
    assert!(s.board.members[&0].fill.is_empty());
    assert!(!s.board.groups[&0].fixed.contains(&0));
}
#[test]
fn transferFillToHugTest() {
    let mut s = init();
    s.create(0, &[0], 2.0);
    s.resizeEdge(0, 0, 2.0);
    s.setMemberMode(0, 0, 0, true);
    s.create(1, &[2], 3.0);
    s.setFlow(1, "Row");
    s.hugAxis(1, 0);
    s.add(1, 0);
    assert!(s.board.members[&0].fill.is_empty());
    assert_eq!(s.board.members[&0].size[0], 2.0);
}
#[test]
fn absoluteHugExclusionTest() {
    let mut s = init();
    s.create(0, &[0, 2], 3.0);
    s.setFlow(0, "Row");
    s.toggleAbsolute(0, 0, 0);
    s.hugAxis(0, 0);
    assert_eq!(s.board.groups[&0].size[0], 1.0);
    assert_eq!(s.board.members[&0].size[0], 3.0);
}
#[test]
fn columnArrowsTest() {
    let mut s = init();
    s.create(0, &[0, 2], 3.0);
    s.setFlow(0, "Column");
    s.arrow(0, 0, "Right");
    assert_eq!(s.board.groups[&0].order, [0, 2]);
    s.arrow(0, 0, "Down");
    assert_eq!(s.board.groups[&0].order, [2, 0]);
    s.arrow(0, 0, "Down");
    assert_eq!(s.board.groups[&0].order, [2, 0]);
    assert_eq!(s.outcome, Outcome::Ok);
}

fn control(v: &Value) -> Control {
    let x = &v["value"];
    let n = || x.as_i64().unwrap();
    let a = || x[0].as_u64().unwrap() as usize;
    match v["tag"].as_str().unwrap() {
        "Name" => Control::Name(if n() < 0 {
            String::new()
        } else {
            n().to_string()
        }),
        "Flow" => Control::Flow(x.as_str().unwrap().into()),
        "Gap" => Control::Gap(n() as f32),
        "RowGap" => Control::RowGap(n() as f32),
        "Padding" => Control::Padding(n() as f32),
        "Align" => Control::Align([a(), x[1].as_u64().unwrap() as usize]),
        "Auto" => Control::Auto(x.as_str().unwrap().into()),
        "Dimension" => Control::Dimension(a(), x[1].as_i64().unwrap() as f32),
        "Hug" => Control::Hug(n() as usize),
        "Clip" => Control::Clip,
        "Ungroup" => Control::Ungroup,
        "MemberMode" => Control::MemberMode(a(), x[1].as_bool().unwrap()),
        "Absolute" => Control::Absolute(n()),
        "Earlier" => Control::Earlier,
        "Later" => Control::Later,
        "Detach" => Control::Detach,
        tag => panic!("Unknown control {tag}"),
    }
}

#[test]
fn dockEscapeTest() {
    let mut s = init();
    s.create(0, &[0, 2], 3.0);
    s.toggleAlign();
    s.useDock(Control::Align([2, 1]));
    assert!(s.dock.align_shown());
    s.escape();
    assert!(s.dock.group_shown() && !s.dock.align_shown());
    s.editMember(2);
    assert_eq!(s.dockMemberNumber(), 2);
    s.escape();
    assert!(s.dock.group_shown());
    s.escape();
    assert_eq!(s.dock.panel, Panel::Hidden);
}
#[test]
fn dockControlsTest() {
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
    s.useDock(Control::Flow("Free".into()));
    assert!(!s.controlEnabled(&Control::Gap(1.0)));
    assert!(!s.controlShown(&Control::RowGap(1.0)));
    assert_eq!(
        s.useDock(Control::Gap(1.0)),
        Outcome::Rejected("control_inactive")
    );
}
#[test]
fn dockAutoTest() {
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
fn dockSizingTest() {
    let mut s = init();
    s.create(0, &[2], 3.0);
    assert_eq!(
        s.useDock(Control::Hug(0)),
        Outcome::Rejected("wrap_width_fixed")
    );
    assert!(!s.controlEnabled(&Control::Hug(0)));
    s.useDock(Control::Hug(1));
    assert!(s.board.groups[&0].fixed.contains(&1));
    s.editMember(2);
    s.useDock(Control::MemberMode(1, true));
    assert!(s.board.members[&2].fill.contains(&1));
    s.escape();
    s.useDock(Control::Hug(1));
    s.editMember(2);
    assert!(!s.board.members[&2].fill.contains(&1));
    assert!(!s.controlEnabled(&Control::MemberMode(1, true)));
    assert_eq!(
        s.useDock(Control::MemberMode(1, true)),
        Outcome::Rejected("fixed_axis_required")
    );
}
#[test]
fn dockAbsoluteTest() {
    let mut s = init();
    s.create(0, &[0, 2], 3.0);
    s.editMember(0);
    s.useDock(Control::MemberMode(0, true));
    s.useDock(Control::Absolute(0));
    assert!(s.board.members[&0].fill.contains(&0));
    assert!(!s.controlEnabled(&Control::MemberMode(0, false)));
    assert!(!s.controlEnabled(&Control::Later));
    s.useDock(Control::Absolute(0));
    assert!(s.controlEnabled(&Control::Later));
    s.useDock(Control::Later);
    assert_eq!(s.dockMemberNumber(), 2);
    assert!(!s.controlEnabled(&Control::Later));
    s.useDock(Control::Earlier);
    assert_eq!(s.dockMemberNumber(), 1);
    s.useDock(Control::Detach);
    assert_eq!(s.dock.panel, Panel::Hidden);
    assert_eq!(s.board.owner(0), None);
}
#[test]
fn dockTextFocusTest() {
    let mut s = init();
    s.create(0, &[0, 2], 3.0);
    s.toggleAlign();
    s.focusText(true);
    assert_eq!(
        s.pressKey("Primary+G", 1, &[0], 1.0),
        Outcome::Rejected("text_owns_key")
    );
    s.pressKey("Escape", 1, &[], 1.0);
    assert!(s.dock.align_shown() && !s.dock.text_focus);
    s.escape();
    assert!(s.dock.group_shown() && !s.dock.align_shown());
    s.dismissAlign();
    assert!(s.dock.group_shown());
}
#[test]
fn dockClipboardTest() {
    let mut s = init();
    s.create(0, &[0, 2], 3.0);
    s.editMember(0);
    s.pressKey("Primary+D", 1, &[], 1.0);
    assert_eq!(s.dock.member(), Some(1));
    assert_eq!(s.dockMemberNumber(), 2);
    s.pressKey("Delete", 1, &[], 1.0);
    assert!(s.dock.group_shown());
    s.editMember(2);
    s.pressKey("Primary+V", 1, &[], 1.0);
    assert_eq!(s.dock.member(), Some(1));
    assert_eq!(s.board.groups[&0].order, [0, 2, 1]);
    s.escape();
    s.pressKey("Primary+Shift+G", 1, &[], 1.0);
    assert_eq!(s.dock.panel, Panel::Hidden);
}
#[test]
fn dockSelectionTest() {
    let mut s = init();
    s.create(0, &[0], 3.0);
    s.create(1, &[2], 3.0);
    s.toggleAlign();
    s.selectGroup(0);
    assert!(!s.dock.align_shown());
    s.editMember(0);
    s.add(1, 0);
    assert_eq!(s.dock.group(), Some(1));
    assert_eq!(s.dock.member(), Some(0));
    s.clearSelection();
    assert_eq!(s.dock.panel, Panel::Hidden);
}
#[test]
fn dockAutoFromHugTest() {
    let mut s = init();
    s.create(0, &[2], 3.0);
    s.useDock(Control::Flow("Column".into()));
    s.toggleAlign();
    s.useDock(Control::Auto("Around".into()));
    assert!(s.board.groups[&0].fixed.contains(&1));
    s.useDock(Control::Hug(1));
    assert_eq!(s.board.groups[&0].gap, "Number");
    assert!(s.dock.align_shown());
}
#[test]
fn dockEmptyTest() {
    let mut s = init();
    s.create(0, &[0], 3.0);
    s.editMember(0);
    s.pressKey("Delete", 1, &[], 1.0);
    assert!(s.dock.group_shown());
    assert_eq!(s.dockCount(), 0);
    s.useDock(Control::Ungroup);
    assert_eq!(s.dock.panel, Panel::Hidden);
}
#[test]
fn dockCornerAndDropTest() {
    let mut s = init();
    s.create(0, &[0, 2], 3.0);
    s.useDock(Control::Dimension(1, 3.0));
    s.doubleClickCorner();
    assert_eq!(s.board.groups[&0].fixed, BTreeSet::from([0]));
    s.editMember(0);
    s.useDock(Control::Absolute(0));
    s.dropMember(0, 0, 0, true);
    assert!(s.dock.member().is_some());
    assert_eq!(s.board.owner(0), Some(0));
    s.useDock(Control::Absolute(0));
    s.dropMember(0, 0, 0, true);
    assert_eq!(s.dock.panel, Panel::Hidden);
    assert_eq!(s.board.owner(0), None);
}
