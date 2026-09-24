//! Deterministic grouping edits. Geometry supplies size observations; callers own pixels,
//! persistence and history. Membership follows `.specs/grouping/grouping.yml`;
//! size observations and conversions follow Spatial Layout.
use crate::data::{GapMode, GroupFlow};
use std::collections::{BTreeMap, BTreeSet};

pub type Id = i64;
/// A refused edit names its reason and leaves board and dock unchanged.
pub type Outcome = Result<(), &'static str>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemberKind {
    Image,
    Textbox,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}
impl Direction {
    fn horizontal(self) -> bool {
        matches!(self, Self::Left | Self::Right)
    }
    fn forward(self) -> bool {
        matches!(self, Self::Right | Self::Down)
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct Member {
    pub kind: MemberKind,
    pub size: [f32; 2],
    pub fill: [bool; 2],
    pub absolute: bool,
}
impl Member {
    pub fn fixed(kind: MemberKind, size: [f32; 2]) -> Self {
        Self {
            kind,
            size,
            fill: [false; 2],
            absolute: false,
        }
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    pub order: Vec<Id>,
    pub flow: GroupFlow,
    pub fixed: [bool; 2],
    /// Outer container dimensions, including padding.
    pub size: [f32; 2],
    pub auto: GapMode,
    /// Kept while Auto spacing is active for a later return to Number.
    pub gap: f32,
    pub row_gap: f32,
    pub padding: f32,
    pub align: [usize; 2],
    pub clip: bool,
    pub name: String,
}
impl Group {
    pub fn new(name: Id, width: f32, gap: f32) -> Self {
        Self {
            order: vec![],
            flow: GroupFlow::Wrap,
            fixed: [true, false],
            size: [width + gap * 2.0, 1.0 + gap * 2.0],
            auto: GapMode::Number,
            gap,
            row_gap: gap,
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
        usize::from(self.flow == GroupFlow::Column)
    }
    pub fn numeric_gap(&self) -> f32 {
        if self.auto == GapMode::Number {
            self.gap
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
        if self.members[&m].fill[a] {
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
        if group.flow != GroupFlow::Wrap {
            return vec![xs];
        }
        let mut rows = Vec::new();
        let mut row = Vec::new();
        let mut used = 0.0;
        for m in xs {
            // A cross-Fill image takes its width from its row's height, so row
            // breaks give it the zero minimum width of primary Fill.
            let member = &self.members[&m];
            let w = if member.kind == MemberKind::Image && member.fill[1] {
                0.0
            } else {
                self.base(m, 0)
            };
            let result = if row.is_empty() {
                w
            } else {
                used + group.numeric_gap() + w
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
                - group.numeric_gap() * xs.len().saturating_sub(1) as f32)
                .max(0.0),
            xs.iter().filter(|m| self.members[m].fill[a]).count(),
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
    fn wrap_arrow_target(&self, b: &Board, g: Id, m: Id, direction: Direction) -> Option<Id>;
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
        }
    }
    /// Accepts an edited board. Group sizes follow it and the dock keeps to existing targets.
    fn apply(&mut self, mut b: Board, dock: Dock) -> Outcome {
        Self::measure_sizes(&self.geometry, &mut b);
        self.dock = dock.normalized(&b);
        self.board = b;
        Ok(())
    }
    fn commit(&mut self, b: Board) -> Outcome {
        self.apply(b, self.dock.clone())
    }
    /// Paste and duplicate keep member editing on the new member.
    fn edit_new_member(&self, b: &Board, m: Id) -> Dock {
        match self.dock.panel {
            Panel::MemberDock { group, .. } if b.owner(m) == Some(group) => {
                Dock::for_member(group, m)
            }
            _ => self.dock.clone(),
        }
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
        let mut member = b.members[&m].clone();
        // Freeze against the board before this operation. Batch intake must not
        // see its earlier members leave and reallocate the source Fill.
        if let Some(owner) = self.board.owner(m) {
            member.size = self.freeze(&self.board, owner, m).size;
        }
        let group = &b.groups[&g];
        member.fill = [0, 1].map(|a| member.fill[a] && group.fixed[a]);
        if member.fill == [false; 2]
            && !member.absolute
            && group.fixed[0]
            && group.flow != GroupFlow::Free
        {
            member.size = self.geometry.fit(&member, group.capacity(0));
        }
        b.without(m);
        b.members.insert(m, member);
        insert(&mut b.groups.get_mut(&g).unwrap().order, m, slot);
    }
    fn group_admission(&self, g: Id) -> Outcome {
        if self.board.groups.contains_key(&g) {
            Ok(())
        } else {
            Err("group_missing")
        }
    }
    fn member_admission(&self, g: Id, m: Id) -> Outcome {
        self.group_admission(g)?;
        if self.board.groups[&g].order.contains(&m) {
            Ok(())
        } else {
            Err("member_missing")
        }
    }
    fn intake_admission(&self, g: Id, m: Id) -> Outcome {
        self.group_admission(g)?;
        if self.board.live.contains(&m) {
            Ok(())
        } else {
            Err("item_missing")
        }
    }
    fn axis_admission(&self, g: Id, a: usize, mode: AxisMode) -> Outcome {
        if matches!(mode, AxisMode::Fixed(n) if !n.is_finite() || n <= 0.0) {
            return Err("invalid_value");
        }
        self.group_admission(g)?;
        if a > 1 {
            Err("invalid_value")
        } else if mode == AxisMode::Hug && a == 0 && self.board.groups[&g].flow == GroupFlow::Wrap {
            Err("wrap_width_fixed")
        } else {
            Ok(())
        }
    }
    pub fn create(&mut self, g: Id, selection: &[Id], width: f32) -> Outcome {
        if self.board.groups.contains_key(&g) {
            return Err("group_exists");
        }
        if !width.is_finite() || width <= 0.0 {
            return Err("invalid_value");
        }
        let mut b = self.board.clone();
        let xs: Vec<_> = selection
            .iter()
            .copied()
            .filter(|m| b.live.contains(m))
            .collect();
        if xs.is_empty() {
            return self.commit(b);
        }
        let width = xs.iter().fold(width, |w, &m| {
            w.max(b.owner(m).map_or(b.members[&m].size[0], |owner| {
                self.geometry.rendered(&b, owner, m, 0)
            }))
        });
        b.groups.insert(g, Group::new(g, width, self.default_gap));
        for m in xs {
            let member = b.members.get_mut(&m).unwrap();
            member.fill = [false; 2];
            member.absolute = false;
            let slot = b.groups[&g].order.len() as i64;
            self.attach(&mut b, g, m, slot);
        }
        self.apply(b, Dock::for_group(g))
    }
    /// Appends items in order.
    pub fn add(&mut self, g: Id, ms: &[Id]) -> Outcome {
        for &m in ms {
            self.intake_admission(g, m)?;
        }
        let mut b = self.board.clone();
        for &m in ms {
            let slot = b.groups[&g].order.len() as i64;
            self.attach(&mut b, g, m, slot);
        }
        self.commit(b)
    }
    pub fn drag_in(&mut self, g: Id, m: Id, slot: i64) -> Outcome {
        self.intake_admission(g, m)?;
        let mut b = self.board.clone();
        self.attach(&mut b, g, m, slot);
        self.commit(b)
    }
    pub fn paste(&mut self, m: Id, width: f32) -> Outcome {
        if self.board.live.contains(&m) {
            return Err("item_exists");
        }
        let mut b = self.board.clone();
        b.live.insert(m);
        let mut member = self.geometry.pasted_member(m, width);
        member.fill = [false; 2];
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
        let dock = self.edit_new_member(&b, m);
        self.apply(b, dock)
    }
    pub fn duplicate(&mut self, source: Id, m: Id) -> Outcome {
        if !self.board.live.contains(&source) {
            return Err("item_missing");
        }
        if self.board.live.contains(&m) {
            return Err("item_exists");
        }
        let mut b = self.board.clone();
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
        let dock = self.edit_new_member(&b, m);
        self.apply(b, dock)
    }
    pub fn reorder(&mut self, g: Id, m: Id, slot: i64) -> Outcome {
        self.member_admission(g, m)?;
        let mut b = self.board.clone();
        if !b.members[&m].absolute && b.groups[&g].flow != GroupFlow::Free {
            let order = &mut b.groups.get_mut(&g).unwrap().order;
            order.retain(|&x| x != m);
            insert(order, m, slot);
        }
        self.commit(b)
    }
    pub fn arrow(&mut self, g: Id, m: Id, direction: Direction) -> Outcome {
        self.member_admission(g, m)?;
        let mut b = self.board.clone();
        if let Some(slot) = self.arrow_target(g, m, direction) {
            let order = &mut b.groups.get_mut(&g).unwrap().order;
            order.retain(|&x| x != m);
            insert(order, m, slot as i64);
        }
        self.commit(b)
    }
    pub fn detach(&mut self, g: Id, m: Id) -> Outcome {
        self.member_admission(g, m)?;
        let mut b = self.board.clone();
        let mut member = self.freeze(&b, g, m);
        member.fill = [false; 2];
        member.absolute = false;
        b.members.insert(m, member);
        b.without(m);
        self.commit(b)
    }
    pub fn toggle_absolute(&mut self, g: Id, m: Id, slot: i64) -> Outcome {
        self.member_admission(g, m)?;
        let mut b = self.board.clone();
        let mut member = self.freeze(&b, g, m);
        if member.absolute {
            let order = &mut b.groups.get_mut(&g).unwrap().order;
            order.retain(|&x| x != m);
            insert(order, m, slot);
        }
        member.absolute = !member.absolute;
        b.members.insert(m, member);
        self.commit(b)
    }
    pub fn set_flow(&mut self, g: Id, flow: GroupFlow) -> Outcome {
        self.group_admission(g)?;
        let mut b = self.board.clone();
        if flow == GroupFlow::Free {
            for &m in &self.board.groups[&g].order {
                b.members.insert(m, self.freeze(&self.board, g, m));
            }
        }
        let group = b.groups.get_mut(&g).unwrap();
        group.flow = flow;
        if flow == GroupFlow::Wrap {
            group.fixed[0] = true;
        }
        if flow != GroupFlow::Free && group.auto != GapMode::Number {
            group.fixed[group.primary()] = true;
        }
        self.commit(b)
    }
    /// Number spacing uses the gap; the Auto modes keep it for a later return to Number.
    pub fn set_gap(&mut self, g: Id, auto: GapMode, gap: f32) -> Outcome {
        self.group_admission(g)?;
        if !gap.is_finite() {
            return Err("invalid_value");
        }
        let mut b = self.board.clone();
        let group = b.groups.get_mut(&g).unwrap();
        group.auto = auto;
        group.gap = gap;
        if auto != GapMode::Number && group.flow != GroupFlow::Free {
            group.fixed[group.primary()] = true;
        }
        self.commit(b)
    }
    /// Hug freezes Fill members at their rendered size and returns primary-axis Auto to Number.
    pub fn set_axis_mode(&mut self, g: Id, a: usize, mode: AxisMode) -> Outcome {
        self.axis_admission(g, a, mode)?;
        let mut b = self.board.clone();
        match mode {
            AxisMode::Fixed(size) => {
                let group = b.groups.get_mut(&g).unwrap();
                group.fixed[a] = true;
                group.size[a] = size;
            }
            AxisMode::Hug => {
                for &m in &self.board.groups[&g].order {
                    if b.members[&m].fill[a] {
                        let mut member = self.freeze(&self.board, g, m);
                        member.fill[a] = false;
                        b.members.insert(m, member);
                    }
                }
                let group = b.groups.get_mut(&g).unwrap();
                group.fixed[a] = false;
                if group.flow != GroupFlow::Free && a == group.primary() {
                    group.auto = GapMode::Number;
                }
            }
        }
        self.commit(b)
    }
    fn member_mode_admission(&self, g: Id, m: Id, a: usize, fill: bool) -> Outcome {
        if self.board.groups[&g].flow == GroupFlow::Free || self.board.members[&m].absolute {
            Err("control_inactive")
        } else if a > 1 {
            Err("invalid_value")
        } else if fill && !self.board.groups[&g].fixed[a] {
            Err("fixed_axis_required")
        } else {
            Ok(())
        }
    }
    pub fn set_member_mode(&mut self, g: Id, m: Id, a: usize, fill: bool) -> Outcome {
        self.member_admission(g, m)?;
        self.member_mode_admission(g, m, a, fill)?;
        let mut b = self.board.clone();
        let mut member = self.freeze(&b, g, m);
        member.fill[a] = fill;
        b.members.insert(m, member);
        self.commit(b)
    }
    pub fn ungroup(&mut self, g: Id) -> Outcome {
        self.group_admission(g)?;
        let mut b = self.board.clone();
        for &m in &self.board.groups[&g].order {
            let mut member = self.freeze(&self.board, g, m);
            member.fill = [false; 2];
            member.absolute = false;
            b.members.insert(m, member);
        }
        b.groups.remove(&g);
        self.commit(b)
    }
    pub fn delete_member(&mut self, m: Id) -> Outcome {
        if !self.board.live.contains(&m) {
            return Err("item_missing");
        }
        let mut b = self.board.clone();
        b.without(m);
        b.live.remove(&m);
        let member = b.members.get_mut(&m).unwrap();
        member.absolute = false;
        member.fill = [false; 2];
        self.commit(b)
    }
}
#[cfg(test)]
mod tests;
