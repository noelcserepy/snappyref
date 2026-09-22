//! Deterministic grouping edits. Geometry supplies size observations; callers own pixels,
//! persistence and history. Operation names match specs/grouping/spec.qnt.
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
    pub size: [f32; 2],
    pub gap: String,
    pub number: f32,
    pub rowGap: f32,
    pub padding: f32,
    pub align: [usize; 2],
    pub clip: bool,
    pub name: Id,
}
impl Group {
    pub fn new(name: Id, width: f32, gap: f32) -> Self {
        Self {
            order: vec![],
            flow: "Wrap".into(),
            fixed: BTreeSet::from([0]),
            size: [width, 1.0],
            gap: "Number".into(),
            number: gap,
            rowGap: gap,
            padding: gap,
            align: [0, 0],
            clip: false,
            name,
        }
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
            if !row.is_empty() && result > group.size[0] {
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
            (group.size[a]
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

/// Pixel layout and the finite Quint fixture provide different size observations.
/// Neither identities nor the fixture's extent saturation limit the product.
pub trait Geometry {
    fn pasted_member(&self, _id: Id, width: f32) -> Member {
        Member::fixed("Image", [width; 2])
    }
    fn extent(&self, n: f32) -> f32 {
        n.max(0.0)
    }
    fn fit(&self, member: &Member, width: f32) -> [f32; 2] {
        [member.size[0].min(width), member.size[1]]
    }
    fn rendered(&self, b: &Board, g: Id, m: Id, a: usize) -> f32;
    fn measured(&self, b: &Board, g: Id, a: usize) -> f32;
}
#[derive(Clone, Debug, PartialEq)]
pub struct Effects {
    pub refusal: bool,
    pub incoming: bool,
    pub empty: bool,
    pub resize: bool,
    pub append: bool,
}
impl Default for Effects {
    fn default() -> Self {
        Self {
            refusal: true,
            incoming: true,
            empty: true,
            resize: true,
            append: true,
        }
    }
}
pub struct State<G> {
    pub board: Board,
    pub outcome: Outcome,
    pub event: &'static str,
    pub effects: Effects,
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
            geometry,
            default_gap,
            outcome: Outcome::Ok,
            event: "init",
            effects: Effects::default(),
        }
    }
    fn commit(&mut self, mut b: Board, reason: &'static str, op: &'static str) -> Outcome {
        if !reason.is_empty() {
            b = self.board.clone();
        } else {
            let sizes: Vec<_> = b
                .groups
                .keys()
                .map(|&g| {
                    (
                        g,
                        [
                            self.geometry.measured(&b, g, 0),
                            self.geometry.measured(&b, g, 1),
                        ],
                    )
                })
                .collect();
            for (g, size) in sizes {
                b.groups.get_mut(&g).unwrap().size = size;
            }
        }
        self.effects = Effects {
            refusal: reason.is_empty() || b == self.board,
            incoming: !["add", "dragIn", "paste"].contains(&op)
                || self.board.live.iter().all(|&m| {
                    self.board.owner(m) != b.owner(m) || self.board.members[&m] == b.members[&m]
                }),
            empty: !["add", "dragIn", "detach", "deleteMember"].contains(&op)
                || self.board.groups.iter().all(|(g, old)| {
                    b.groups.get(g).is_some_and(|new| {
                        let mut empty = old.clone();
                        empty.order.clear();
                        !new.order.is_empty() || *new == empty
                    })
                }),
            resize: op != "resizeEdge" || b.members == self.board.members,
            append: op != "add"
                || self.board.groups.iter().all(|(g, old)| {
                    let new = &b.groups[g].order;
                    new.len() <= old.order.len() || new.starts_with(&old.order)
                }),
        };
        self.board = b;
        self.event = op;
        self.outcome = if reason.is_empty() {
            Outcome::Ok
        } else {
            Outcome::Rejected(reason)
        };
        self.outcome.clone()
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
            member.size = self.geometry.fit(&member, group.size[0]);
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
    fn axis_reason(&self, g: Id, a: usize, fixed: bool, dimension: f32) -> &'static str {
        if !self.board.groups.contains_key(&g) {
            "group_missing"
        } else if a > 1 || !dimension.is_finite() || dimension <= 0.0 {
            "invalid_value"
        } else if !fixed && a == 0 && self.board.groups[&g].flow == "Wrap" {
            "wrap_width_fixed"
        } else {
            ""
        }
    }
    fn axis_changed(&self, b: &mut Board, g: Id, a: usize, fixed: bool, dimension: f32) {
        if !fixed {
            for &m in &self.board.groups[&g].order {
                if b.members[&m].fill.contains(&a) {
                    let dimension = self.geometry.rendered(&self.board, g, m, a);
                    let member = b.members.get_mut(&m).unwrap();
                    member.fill.remove(&a);
                    member.size[a] = dimension;
                }
            }
        }
        let group = b.groups.get_mut(&g).unwrap();
        if fixed {
            group.fixed.insert(a);
        } else {
            group.fixed.remove(&a);
        }
        group.size[a] = dimension;
        if !fixed && group.flow != "Free" && a == group.primary() {
            group.gap = "Number".into();
        }
    }
    pub fn create(&mut self, g: Id, selection: &[Id], width: f32) -> Outcome {
        let reason = if self.board.groups.contains_key(&g) {
            "group_exists"
        } else if width < 1.0 {
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
            let width = xs.iter().fold(self.geometry.extent(width), |w, &m| {
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
    pub fn paste(&mut self, g: Id, anchor: Id, m: Id, width: f32) -> Outcome {
        let reason = if self.board.live.contains(&m) {
            "item_exists"
        } else if g != -1 && !self.board.groups.contains_key(&g) {
            "group_missing"
        } else if g != -1 && anchor != -1 && !self.board.groups[&g].order.contains(&anchor) {
            "member_missing"
        } else {
            ""
        };
        let mut b = self.board.clone();
        if reason.is_empty() {
            b.live.insert(m);
            b.members.insert(m, self.geometry.pasted_member(m, width));
            if g != -1 {
                let slot = if anchor == -1 {
                    b.groups[&g].order.len()
                } else {
                    b.groups[&g]
                        .order
                        .iter()
                        .position(|&x| x == anchor)
                        .unwrap()
                        + 1
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
        if reason.is_empty() && !b.members[&m].absolute && b.groups[&g].flow != "Free" {
            let group = &b.groups[&g];
            let xs = b.flowing(g);
            let i = xs.iter().position(|&x| x == m).unwrap();
            let horizontal = ["Left", "Right"].contains(&direction);
            let delta: i64 = if ["Left", "Up"].contains(&direction) {
                -1
            } else {
                1
            };
            let target = if group.flow == "Wrap" && !horizontal {
                let rows = b.rows(g);
                let r = rows.iter().position(|row| row.contains(&m)).unwrap();
                let column = rows[r].iter().position(|&x| x == m).unwrap();
                rows.get((r as i64 + delta) as usize)
                    .and_then(|row| row.get(column))
                    .copied()
            } else if (group.flow == "Column") != horizontal {
                xs.get((i as i64 + delta) as usize).copied()
            } else {
                None
            };
            if let Some(target) = target {
                let slot = group.order.iter().position(|&x| x == target).unwrap();
                let order = &mut b.groups.get_mut(&g).unwrap().order;
                order.retain(|&x| x != m);
                insert(order, m, slot as i64);
            }
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
            let mut member = self.freeze(&b, g, m);
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
    pub fn setGap(&mut self, g: Id, gap: &str, number: f32) -> Outcome {
        let reason = if !self.board.groups.contains_key(&g) {
            "group_missing"
        } else if !["Number", "Between", "Evenly", "Around"].contains(&gap) {
            "invalid_value"
        } else {
            ""
        };
        let mut b = self.board.clone();
        if reason.is_empty() {
            let group = b.groups.get_mut(&g).unwrap();
            group.gap = gap.into();
            if gap == "Number" {
                group.number = number;
            } else if group.flow != "Free" {
                group.fixed.insert(group.primary());
            }
        }
        self.commit(b, reason, "setGap")
    }
    pub fn setAxisMode(&mut self, g: Id, a: usize, fixed: bool) -> Outcome {
        let reason = self.axis_reason(g, a, fixed, 1.0);
        let mut b = self.board.clone();
        if reason.is_empty() {
            let size = self.geometry.measured(&b, g, a);
            self.axis_changed(&mut b, g, a, fixed, size);
        }
        self.commit(b, reason, "setAxisMode")
    }
    pub fn resizeEdge(&mut self, g: Id, a: usize, dimension: f32) -> Outcome {
        let reason = self.axis_reason(g, a, true, dimension);
        let mut b = self.board.clone();
        if reason.is_empty() {
            self.axis_changed(&mut b, g, a, true, self.geometry.extent(dimension));
        }
        self.commit(b, reason, "resizeEdge")
    }
    pub fn hugAxis(&mut self, g: Id, a: usize) -> Outcome {
        let reason = self.axis_reason(g, a, false, 1.0);
        let mut b = self.board.clone();
        if reason.is_empty() {
            let size = self.geometry.measured(&b, g, a);
            self.axis_changed(&mut b, g, a, false, size);
        }
        self.commit(b, reason, "hugAxis")
    }
    pub fn setMemberMode(&mut self, g: Id, m: Id, a: usize, fill: bool) -> Outcome {
        let missing = self.member_reason(g, m);
        let reason = if !missing.is_empty() {
            missing
        } else if fill && !self.board.groups[&g].fixed.contains(&a) {
            "fixed_axis_required"
        } else {
            ""
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
    pub fn configure(
        &mut self,
        g: Id,
        clip: bool,
        alignX: usize,
        alignY: usize,
        padding: f32,
        rowGap: f32,
    ) -> Outcome {
        let reason = if !self.board.groups.contains_key(&g) {
            "group_missing"
        } else {
            ""
        };
        let mut b = self.board.clone();
        if reason.is_empty() {
            let group = b.groups.get_mut(&g).unwrap();
            group.clip = clip;
            group.align = [alignX, alignY];
            group.padding = padding;
            group.rowGap = rowGap;
        }
        self.commit(b, reason, "configure")
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
        b.without(m);
        b.live.remove(&m);
        self.commit(b, reason, "deleteMember")
    }
}
#[cfg(test)]
mod tests;
