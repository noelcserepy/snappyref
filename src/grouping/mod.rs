//! Deterministic grouping edits. Geometry supplies size observations; callers own pixels,
//! persistence and history. Membership follows `.specs/grouping/grouping.yml`;
//! size observations and conversions follow Spatial Layout.
#![allow(non_snake_case)]
use std::collections::{BTreeMap, BTreeSet};

pub type Id = i64;
#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    Ok,
    Rejected(&'static str),
}
#[derive(Clone, Debug, PartialEq)]
pub struct Member {
    pub kind: String,
    pub size: [f32; 2],
    pub fill: BTreeSet<usize>,
    pub absolute: bool,
}
impl Member {
    pub fn fixed(kind: impl Into<String>, size: [f32; 2]) -> Self {
        Self {
            kind: kind.into(),
            size,
            fill: BTreeSet::new(),
            absolute: false,
        }
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    pub order: Vec<Id>,
    pub flow: String,
    pub fixed: BTreeSet<usize>,
    /// Outer container dimensions, including padding.
    pub size: [f32; 2],
    pub gap: String,
    pub number: f32,
    pub rowGap: f32,
    pub padding: f32,
    pub align: [usize; 2],
    pub clip: bool,
    pub name: String,
}
impl Group {
    pub fn new(name: Id, width: f32, gap: f32) -> Self {
        Self {
            order: vec![],
            flow: "Wrap".into(),
            fixed: BTreeSet::from([0]),
            size: [width + gap * 2.0, 1.0 + gap * 2.0],
            gap: "Number".into(),
            number: gap,
            rowGap: gap,
            padding: gap,
            align: [0, 0],
            clip: false,
            name: name.to_string(),
        }
    }
    pub fn capacity(&self, axis: usize) -> f32 {
        (self.size[axis] - self.padding * 2.0).max(0.0)
    }
    pub fn primary(&self) -> usize {
        usize::from(self.flow == "Column")
    }
    pub fn numeric(&self) -> f32 {
        if self.gap == "Number" {
            self.number
        } else {
            0.0
        }
    }
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Board {
    pub groups: BTreeMap<Id, Group>,
    pub live: BTreeSet<Id>,
    pub members: BTreeMap<Id, Member>,
}
impl Board {
    pub fn owner(&self, m: Id) -> Option<Id> {
        self.groups
            .iter()
            .find_map(|(&g, group)| group.order.contains(&m).then_some(g))
    }
    pub fn flowing(&self, g: Id) -> Vec<Id> {
        self.groups[&g]
            .order
            .iter()
            .copied()
            .filter(|m| !self.members[m].absolute)
            .collect()
    }
    pub fn base(&self, m: Id, a: usize) -> f32 {
        if self.members[&m].fill.contains(&a) {
            0.0
        } else {
            self.members[&m].size[a]
        }
    }
    pub fn rows(&self, g: Id) -> Vec<Vec<Id>> {
        let group = &self.groups[&g];
        let xs = self.flowing(g);
        if xs.is_empty() {
            return vec![];
        }
        if group.flow != "Wrap" {
            return vec![xs];
        }
        let mut rows = Vec::new();
        let mut row = Vec::new();
        let mut used = 0.0;
        for m in xs {
            let w = self.base(m, 0);
            let result = if row.is_empty() {
                w
            } else {
                used + group.numeric() + w
            };
            if !row.is_empty() && result > group.capacity(0) {
                rows.push(std::mem::take(&mut row));
                used = w;
            } else {
                used = result;
            }
            row.push(m);
        }
        rows.push(row);
        rows
    }
    pub fn allocation(&self, g: Id, xs: &[Id], a: usize) -> (f32, usize) {
        let group = &self.groups[&g];
        (
            (group.capacity(a)
                - xs.iter().map(|&m| self.base(m, a)).sum::<f32>()
                - group.numeric() * xs.len().saturating_sub(1) as f32)
                .max(0.0),
            xs.iter()
                .filter(|m| self.members[m].fill.contains(&a))
                .count(),
        )
    }
    fn without(&mut self, m: Id) {
        for g in self.groups.values_mut() {
            g.order.retain(|&x| x != m);
        }
    }
}

/// Pixel observations freeze rendered sizes when modes or membership change.
pub trait Geometry {
    /// Resolves the adjacent-row, same-column member for vertical Wrap movement.
    fn wrap_arrow_target(&self, b: &Board, g: Id, m: Id, direction: &str) -> Option<Id>;
    fn pasted_member(&self, id: Id, width: f32) -> Member;
    fn fit(&self, member: &Member, width: f32) -> [f32; 2];
    fn rendered(&self, b: &Board, g: Id, m: Id, a: usize) -> f32;
    fn measured(&self, b: &Board, g: Id, a: usize) -> f32;
}
/// An axis either hugs its content or is fixed at an outer extent in group pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AxisMode {
    Hug,
    Fixed(f32),
}
mod dock;
pub use dock::{Control, Dock, Panel};

pub struct State<G> {
    pub dock: Dock,
    pub board: Board,
    pub outcome: Outcome,
    pub geometry: G,
    pub default_gap: f32,
}
fn insert(xs: &mut Vec<Id>, m: Id, slot: i64) {
    xs.insert(slot.max(0).min(xs.len() as i64) as usize, m);
}
impl<G: Geometry> State<G> {
    pub fn init(board: Board, geometry: G, default_gap: f32) -> Self {
        Self {
            board,
            dock: Dock::default(),
            geometry,
            default_gap,
            outcome: Outcome::Ok,
        }
    }
    fn commit(&mut self, b: Board, reason: &'static str, op: &'static str) -> Outcome {
        let mut dock = self.dock.clone();
        if reason.is_empty() {
            let added: Vec<_> = b
                .groups
                .keys()
                .filter(|g| !self.board.groups.contains_key(g))
                .copied()
                .collect();
            if op == "create" && added.len() == 1 {
                dock = Dock {
                    panel: Panel::GroupDock {
                        group: added[0],
                        align_open: false,
                    },
                    text_focus: false,
                };
            } else if let Panel::MemberDock { group, .. } = dock.panel {
                let added: Vec<_> = b.live.difference(&self.board.live).copied().collect();
                if ["paste", "duplicate"].contains(&op)
                    && added.len() == 1
                    && b.owner(added[0]) == Some(group)
                {
                    dock = Dock {
                        panel: Panel::MemberDock {
                            group,
                            member: added[0],
                        },
                        text_focus: false,
                    };
                }
            }
        }
        self.apply(b, reason, dock)
    }
    fn apply(&mut self, mut b: Board, reason: &'static str, dock: Dock) -> Outcome {
        if !reason.is_empty() {
            b = self.board.clone();
        } else {
            Self::measure_sizes(&self.geometry, &mut b);
        }
        let applied_dock = if reason.is_empty() {
            dock.normalized(&b)
        } else {
            self.dock.clone()
        };
        self.dock = applied_dock;
        self.board = b;
        self.outcome = if reason.is_empty() {
            Outcome::Ok
        } else {
            Outcome::Rejected(reason)
        };
        self.outcome.clone()
    }
    fn measure_sizes(geometry: &G, b: &mut Board) {
        let sizes: Vec<_> = b
            .groups
            .keys()
            .map(|&g| (g, [geometry.measured(b, g, 0), geometry.measured(b, g, 1)]))
            .collect();
        for (g, size) in sizes {
            b.groups.get_mut(&g).unwrap().size = size;
        }
    }
    /// Refresh geometry after stored values or member pixels change, without a mode edit.
    pub fn refresh_sizes(&mut self) {
        Self::measure_sizes(&self.geometry, &mut self.board);
    }

    fn freeze(&self, b: &Board, g: Id, m: Id) -> Member {
        let mut member = b.members[&m].clone();
        member.size = [
            self.geometry.rendered(b, g, m, 0),
            self.geometry.rendered(b, g, m, 1),
        ];
        member
    }
    fn attach(&self, b: &mut Board, g: Id, m: Id, slot: i64) {
        if b.groups[&g].order.contains(&m) {
            return;
        }
        let mut member = b
            .owner(m)
            .map_or_else(|| b.members[&m].clone(), |owner| self.freeze(b, owner, m));
        let group = &b.groups[&g];
        member.fill = member.fill.intersection(&group.fixed).copied().collect();
        if member.fill.is_empty()
            && !member.absolute
            && group.fixed.contains(&0)
            && group.flow != "Free"
        {
            member.size = self.geometry.fit(&member, group.capacity(0));
        }
        b.without(m);
        b.members.insert(m, member);
        insert(&mut b.groups.get_mut(&g).unwrap().order, m, slot);
    }
    fn member_reason(&self, g: Id, m: Id) -> &'static str {
        match self.board.groups.get(&g) {
            None => "group_missing",
            Some(group) if !group.order.contains(&m) => "member_missing",
            _ => "",
        }
    }
    fn intake_reason(&self, g: Id, m: Id) -> &'static str {
        if !self.board.groups.contains_key(&g) {
            "group_missing"
        } else if !self.board.live.contains(&m) {
            "item_missing"
        } else {
            ""
        }
    }
    fn axis_reason(&self, g: Id, a: usize, mode: AxisMode) -> &'static str {
        match mode {
            AxisMode::Fixed(n) if !n.is_finite() || n <= 0.0 => "invalid_value",
            _ if !self.board.groups.contains_key(&g) => "group_missing",
            _ if a > 1 => "invalid_value",
            AxisMode::Hug if a == 0 && self.board.groups[&g].flow == "Wrap" => "wrap_width_fixed",
            _ => "",
        }
    }
    pub fn create(&mut self, g: Id, selection: &[Id], width: f32) -> Outcome {
        let reason = if self.board.groups.contains_key(&g) {
            "group_exists"
        } else if !width.is_finite() || width <= 0.0 {
            "invalid_value"
        } else {
            ""
        };
        let mut b = self.board.clone();
        let xs: Vec<_> = selection
            .iter()
            .copied()
            .filter(|m| b.live.contains(m))
            .collect();
        if reason.is_empty() && !xs.is_empty() {
            let width = xs.iter().fold(width, |w, &m| {
                w.max(b.owner(m).map_or(b.members[&m].size[0], |owner| {
                    self.geometry.rendered(&b, owner, m, 0)
                }))
            });
            b.groups.insert(g, Group::new(g, width, self.default_gap));
            for m in xs {
                let mut member = b
                    .owner(m)
                    .map_or_else(|| b.members[&m].clone(), |owner| self.freeze(&b, owner, m));
                member.fill.clear();
                member.absolute = false;
                b.members.insert(m, member);
                let slot = b.groups[&g].order.len() as i64;
                self.attach(&mut b, g, m, slot);
            }
        }
        self.commit(b, reason, "create")
    }
    pub fn add(&mut self, g: Id, m: Id) -> Outcome {
        let reason = self.intake_reason(g, m);
        let mut b = self.board.clone();
        if reason.is_empty() {
            let slot = b.groups[&g].order.len() as i64;
            self.attach(&mut b, g, m, slot);
        }
        self.commit(b, reason, "add")
    }
    pub fn dragIn(&mut self, g: Id, m: Id, slot: i64) -> Outcome {
        let reason = self.intake_reason(g, m);
        let mut b = self.board.clone();
        if reason.is_empty() {
            self.attach(&mut b, g, m, slot);
        }
        self.commit(b, reason, "dragIn")
    }
    pub fn paste(&mut self, m: Id, width: f32) -> Outcome {
        let reason = if self.board.live.contains(&m) {
            "item_exists"
        } else {
            ""
        };
        let mut b = self.board.clone();
        if reason.is_empty() {
            b.live.insert(m);
            let mut member = self.geometry.pasted_member(m, width);
            member.fill.clear();
            member.absolute = false;
            b.members.insert(m, member);
            if let Some(g) = self.dock.group() {
                let slot = if let Some(anchor) = self.dock.member() {
                    b.groups[&g]
                        .order
                        .iter()
                        .position(|&x| x == anchor)
                        .unwrap()
                        + 1
                } else {
                    b.groups[&g].order.len()
                };
                self.attach(&mut b, g, m, slot as i64);
            }
        }
        self.commit(b, reason, "paste")
    }
    pub fn duplicate(&mut self, source: Id, m: Id) -> Outcome {
        let reason = if !self.board.live.contains(&source) {
            "item_missing"
        } else if self.board.live.contains(&m) {
            "item_exists"
        } else {
            ""
        };
        let mut b = self.board.clone();
        if reason.is_empty() {
            let owner = b.owner(source);
            let copy = owner.map_or_else(
                || b.members[&source].clone(),
                |g| self.freeze(&b, g, source),
            );
            b.live.insert(m);
            b.members.insert(m, copy);
            if let Some(g) = owner {
                let slot = b.groups[&g]
                    .order
                    .iter()
                    .position(|&x| x == source)
                    .unwrap()
                    + 1;
                self.attach(&mut b, g, m, slot as i64);
            }
        }
        self.commit(b, reason, "duplicate")
    }
    pub fn reorder(&mut self, g: Id, m: Id, slot: i64) -> Outcome {
        let reason = self.member_reason(g, m);
        let mut b = self.board.clone();
        if reason.is_empty() && !b.members[&m].absolute && b.groups[&g].flow != "Free" {
            let order = &mut b.groups.get_mut(&g).unwrap().order;
            order.retain(|&x| x != m);
            insert(order, m, slot);
        }
        self.commit(b, reason, "reorder")
    }
    pub fn arrow(&mut self, g: Id, m: Id, direction: &str) -> Outcome {
        let reason = self.member_reason(g, m);
        let mut b = self.board.clone();
        if reason.is_empty()
            && let Some(slot) = self.arrowTarget(g, m, direction)
        {
            let order = &mut b.groups.get_mut(&g).unwrap().order;
            order.retain(|&x| x != m);
            insert(order, m, slot as i64);
        }
        self.commit(b, reason, "arrow")
    }
    pub fn detach(&mut self, g: Id, m: Id) -> Outcome {
        let reason = self.member_reason(g, m);
        let mut b = self.board.clone();
        if reason.is_empty() {
            let mut member = self.freeze(&b, g, m);
            member.fill.clear();
            member.absolute = false;
            b.members.insert(m, member);
            b.without(m);
        }
        self.commit(b, reason, "detach")
    }
    pub fn toggleAbsolute(&mut self, g: Id, m: Id, slot: i64) -> Outcome {
        let reason = self.member_reason(g, m);
        let mut b = self.board.clone();
        if reason.is_empty() {
            let mut member = if b.members[&m].absolute {
                b.members[&m].clone()
            } else {
                self.freeze(&b, g, m)
            };
            if member.absolute {
                let order = &mut b.groups.get_mut(&g).unwrap().order;
                order.retain(|&x| x != m);
                insert(order, m, slot);
            }
            member.absolute = !member.absolute;
            b.members.insert(m, member);
        }
        self.commit(b, reason, "toggleAbsolute")
    }
    pub fn setFlow(&mut self, g: Id, flow: &str) -> Outcome {
        let reason = if !self.board.groups.contains_key(&g) {
            "group_missing"
        } else if !["Row", "Column", "Wrap", "Free"].contains(&flow) {
            "invalid_value"
        } else {
            ""
        };
        let mut b = self.board.clone();
        if reason.is_empty() {
            if flow == "Free" {
                for &m in &self.board.groups[&g].order {
                    b.members.insert(m, self.freeze(&self.board, g, m));
                }
            }
            let group = b.groups.get_mut(&g).unwrap();
            group.flow = flow.into();
            if flow == "Wrap" {
                group.fixed.insert(0);
            }
            if flow != "Free" && group.gap != "Number" {
                group.fixed.insert(group.primary());
            }
        }
        self.commit(b, reason, "setFlow")
    }
    /// Number spacing uses the number; the Auto variants keep it for a later return to Number.
    pub fn setGap(&mut self, g: Id, gap: &str, number: f32) -> Outcome {
        let reason = if !self.board.groups.contains_key(&g) {
            "group_missing"
        } else if !["Number", "Between", "Evenly", "Around"].contains(&gap) || !number.is_finite() {
            "invalid_value"
        } else {
            ""
        };
        let mut b = self.board.clone();
        if reason.is_empty() {
            let group = b.groups.get_mut(&g).unwrap();
            group.gap = gap.into();
            group.number = number;
            if gap != "Number" && group.flow != "Free" {
                group.fixed.insert(group.primary());
            }
        }
        self.commit(b, reason, "setGap")
    }
    /// Hug freezes Fill members at their rendered size and returns primary-axis Auto to Number.
    pub fn setAxisMode(&mut self, g: Id, a: usize, mode: AxisMode) -> Outcome {
        let reason = self.axis_reason(g, a, mode);
        let mut b = self.board.clone();
        if reason.is_empty() {
            match mode {
                AxisMode::Fixed(size) => {
                    let group = b.groups.get_mut(&g).unwrap();
                    group.fixed.insert(a);
                    group.size[a] = size;
                }
                AxisMode::Hug => {
                    for &m in &self.board.groups[&g].order {
                        if b.members[&m].fill.contains(&a) {
                            let size = self.geometry.rendered(&self.board, g, m, a);
                            let member = b.members.get_mut(&m).unwrap();
                            member.fill.remove(&a);
                            member.size[a] = size;
                        }
                    }
                    let group = b.groups.get_mut(&g).unwrap();
                    group.fixed.remove(&a);
                    if group.flow != "Free" && a == group.primary() {
                        group.gap = "Number".into();
                    }
                }
            }
        }
        self.commit(b, reason, "setAxisMode")
    }
    fn member_mode_reason(&self, g: Id, m: Id, a: usize, fill: bool) -> &'static str {
        if self.board.groups[&g].flow == "Free" || self.board.members[&m].absolute {
            "control_inactive"
        } else if a > 1 {
            "invalid_value"
        } else if fill && !self.board.groups[&g].fixed.contains(&a) {
            "fixed_axis_required"
        } else {
            ""
        }
    }
    pub fn setMemberMode(&mut self, g: Id, m: Id, a: usize, fill: bool) -> Outcome {
        let missing = self.member_reason(g, m);
        let reason = if !missing.is_empty() {
            missing
        } else {
            self.member_mode_reason(g, m, a, fill)
        };
        let mut b = self.board.clone();
        if reason.is_empty() {
            let size = self.geometry.rendered(&b, g, m, a);
            let member = b.members.get_mut(&m).unwrap();
            if fill {
                member.fill.insert(a);
            } else {
                member.fill.remove(&a);
            }
            member.size[a] = size;
        }
        self.commit(b, reason, "setMemberMode")
    }
    pub fn ungroup(&mut self, g: Id) -> Outcome {
        let reason = if !self.board.groups.contains_key(&g) {
            "group_missing"
        } else {
            ""
        };
        let mut b = self.board.clone();
        if reason.is_empty() {
            for &m in &self.board.groups[&g].order {
                let mut member = self.freeze(&self.board, g, m);
                member.fill.clear();
                member.absolute = false;
                b.members.insert(m, member);
            }
            b.groups.remove(&g);
        }
        self.commit(b, reason, "ungroup")
    }
    pub fn deleteMember(&mut self, m: Id) -> Outcome {
        let reason = if !self.board.live.contains(&m) {
            "item_missing"
        } else {
            ""
        };
        let mut b = self.board.clone();
        if reason.is_empty() {
            b.without(m);
            b.live.remove(&m);
            let member = b.members.get_mut(&m).unwrap();
            member.absolute = false;
            member.fill.clear();
        }
        self.commit(b, reason, "deleteMember")
    }
}
#[cfg(test)]
mod tests;
