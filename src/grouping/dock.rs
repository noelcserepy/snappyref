//! Context and admission for the two contextual dock modes. Drafts and pixels belong to the UI.
use super::*;

#[derive(Clone, Debug, Default, PartialEq)]
pub enum Panel {
    #[default]
    Hidden,
    GroupDock {
        group: Id,
    },
    MemberDock {
        group: Id,
        member: Id,
    },
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Dock {
    pub panel: Panel,
    pub text_focus: bool,
}
impl Dock {
    pub fn for_group(group: Id) -> Self {
        Self {
            panel: Panel::GroupDock { group },
            text_focus: false,
        }
    }
    pub fn for_member(group: Id, member: Id) -> Self {
        Self {
            panel: Panel::MemberDock { group, member },
            text_focus: false,
        }
    }
    pub fn group(&self) -> Option<Id> {
        match self.panel {
            Panel::Hidden => None,
            Panel::GroupDock { group, .. } | Panel::MemberDock { group, .. } => Some(group),
        }
    }
    pub fn member(&self) -> Option<Id> {
        match self.panel {
            Panel::MemberDock { member, .. } => Some(member),
            _ => None,
        }
    }
    pub fn group_shown(&self) -> bool {
        matches!(self.panel, Panel::GroupDock { .. })
    }
    pub fn normalized(mut self, b: &Board) -> Self {
        self.panel = match self.panel {
            Panel::Hidden => return self,
            Panel::GroupDock { group } if b.groups.contains_key(&group) => return self,
            Panel::MemberDock { group, member } if b.owner(member) == Some(group) => return self,
            Panel::MemberDock { group, member } => {
                if let Some(group) = b.owner(member) {
                    Panel::MemberDock { group, member }
                } else if !b.live.contains(&member) && b.groups.contains_key(&group) {
                    Panel::GroupDock { group }
                } else {
                    Panel::Hidden
                }
            }
            _ => Panel::Hidden,
        };
        self.text_focus = false;
        self
    }
}
#[derive(Clone, Debug, PartialEq)]
pub enum Control {
    Name(String),
    Flow(GroupFlow),
    Gap(f32),
    RowGap(f32),
    Padding(f32),
    Align([usize; 2]),
    Auto(GapMode),
    Dimension(usize, f32),
    Hug(usize),
    Clip,
    Ungroup,
    MemberMode(usize, bool),
    Absolute(i64),
    Detach,
}
impl Control {
    fn member_control(&self) -> bool {
        matches!(
            self,
            Self::MemberMode(..) | Self::Absolute(_) | Self::Detach
        )
    }
}
impl<G: Geometry> State<G> {
    /// Counts and ordinals include Absolute members.
    pub fn dock_count(&self) -> usize {
        self.dock
            .group()
            .map_or(0, |g| self.board.groups[&g].order.len())
    }
    pub fn dock_member_number(&self) -> usize {
        self.dock.member().map_or(0, |m| {
            self.board.groups[&self.dock.group().unwrap()]
                .order
                .iter()
                .position(|&x| x == m)
                .unwrap()
                + 1
        })
    }
    pub fn control_shown(&self, c: &Control) -> bool {
        if c.member_control() {
            return self.dock.member().is_some();
        }
        match c {
            Control::RowGap(_) => {
                self.dock.group_shown()
                    && self.board.groups[&self.dock.group().unwrap()].flow == GroupFlow::Wrap
            }
            _ => self.dock.group_shown(),
        }
    }
    /// A control is enabled exactly when its submission is admitted.
    pub fn control_enabled(&self, c: &Control) -> bool {
        self.control_admission(c).is_ok()
    }
    /// The Hug button toggles: a fixed axis hugs, a hugging axis freezes at its current size.
    fn hug_target(g: &Group, a: usize) -> AxisMode {
        if g.fixed.get(a).copied().unwrap_or(false) {
            AxisMode::Hug
        } else {
            AxisMode::Fixed(g.size.get(a).copied().unwrap_or(0.0))
        }
    }
    fn control_admission(&self, c: &Control) -> Outcome {
        if (c.member_control() && self.dock.member().is_none())
            || (!c.member_control() && !self.dock.group_shown())
        {
            return Err("dock_unavailable");
        }
        let id = self.dock.group().unwrap();
        let g = &self.board.groups[&id];
        let free = g.flow == GroupFlow::Free;
        match c {
            Control::Name(n) if n.trim().is_empty() => Err("invalid_value"),
            Control::Padding(n) if !n.is_finite() || *n < 0.0 => Err("invalid_value"),
            Control::Gap(n) | Control::RowGap(n) if !n.is_finite() => Err("invalid_value"),
            Control::Gap(_) if free || g.auto != GapMode::Number => Err("control_inactive"),
            Control::RowGap(_) if g.flow != GroupFlow::Wrap => Err("control_inactive"),
            Control::Align(_) | Control::Auto(_) if free => Err("control_inactive"),
            Control::Align(xy)
                if g.auto != GapMode::Number && xy[g.primary()] != g.align[g.primary()] =>
            {
                Err("primary_alignment_inactive")
            }
            Control::Dimension(a, n) => self.axis_admission(id, *a, AxisMode::Fixed(*n)),
            Control::Hug(a) => self.axis_admission(id, *a, Self::hug_target(g, *a)),
            Control::MemberMode(a, fill) => {
                self.member_mode_admission(id, self.dock.member().unwrap(), *a, *fill)
            }
            _ => Ok(()),
        }
    }
    /// Dock-only changes leave the board and its measured sizes alone.
    fn set_dock(&mut self, dock: Dock) {
        self.dock = dock.normalized(&self.board);
    }
    pub fn select_group(&mut self, g: Id) -> Outcome {
        self.group_admission(g)?;
        self.set_dock(Dock::for_group(g));
        Ok(())
    }
    pub fn edit_member(&mut self, m: Id) -> Outcome {
        let g = self.board.owner(m).ok_or("member_missing")?;
        self.set_dock(Dock::for_member(g, m));
        Ok(())
    }
    pub fn clear_selection(&mut self) {
        self.set_dock(Dock::default());
    }
    pub fn focus_text(&mut self, active: bool) {
        let mut d = self.dock.clone();
        d.text_focus = active;
        self.set_dock(d);
    }
    /// Dismisses one level: text focus, then member editing, then the group.
    pub fn escape(&mut self) {
        let mut d = self.dock.clone();
        if d.text_focus {
            d.text_focus = false;
        } else if d.member().is_some() {
            d = Dock::for_group(d.group().unwrap());
        } else {
            d = Dock::default();
        }
        self.set_dock(d);
    }
    pub fn use_dock(&mut self, c: Control) -> Outcome {
        self.control_admission(&c)?;
        let g = self.dock.group().unwrap();
        let config = self.board.groups[&g].clone();
        match c {
            Control::Flow(f) => self.set_flow(g, f),
            Control::Gap(n) => self.set_gap(g, GapMode::Number, n),
            Control::Auto(mode) => self.set_gap(g, mode, config.gap),
            Control::Dimension(a, n) => self.set_axis_mode(g, a, AxisMode::Fixed(n)),
            Control::Hug(a) => self.set_axis_mode(g, a, Self::hug_target(&config, a)),
            Control::Name(_)
            | Control::RowGap(_)
            | Control::Padding(_)
            | Control::Align(_)
            | Control::Clip => {
                let mut b = self.board.clone();
                let group = b.groups.get_mut(&g).unwrap();
                match c {
                    Control::Name(n) => group.name = n.trim().into(),
                    Control::RowGap(n) => group.row_gap = n,
                    Control::Padding(n) => group.padding = n,
                    Control::Align(xy) => group.align = xy,
                    Control::Clip => group.clip = !group.clip,
                    _ => unreachable!(),
                }
                self.commit(b)
            }
            Control::Ungroup => self.ungroup(g),
            Control::MemberMode(a, fill) => {
                self.set_member_mode(g, self.dock.member().unwrap(), a, fill)
            }
            Control::Absolute(slot) => self.toggle_absolute(g, self.dock.member().unwrap(), slot),
            Control::Detach => self.detach(g, self.dock.member().unwrap()),
        }
    }
    pub fn arrow_target(&self, g: Id, m: Id, direction: Direction) -> Option<usize> {
        let group = &self.board.groups[&g];
        if self.board.members[&m].absolute || group.flow == GroupFlow::Free {
            return None;
        }
        let xs = self.board.flowing(g);
        let i = xs.iter().position(|&x| x == m)?;
        let target = if group.flow == GroupFlow::Wrap && !direction.horizontal() {
            self.geometry
                .wrap_arrow_target(&self.board, g, m, direction)?
        } else if (group.flow == GroupFlow::Column) != direction.horizontal() {
            let neighbor = if direction.forward() {
                i + 1
            } else {
                i.checked_sub(1)?
            };
            *xs.get(neighbor)?
        } else {
            return None;
        };
        group.order.iter().position(|&x| x == target)
    }
    /// Arrow keys move the edited member. Text focus owns them while it is active.
    pub fn press_arrow(&mut self, direction: Direction) -> Outcome {
        if self.dock.text_focus {
            return Err("text_owns_key");
        }
        let (Some(g), Some(m)) = (self.dock.group(), self.dock.member()) else {
            return Err("no_neighbor");
        };
        if self.arrow_target(g, m, direction).is_none() {
            return Err("no_neighbor");
        }
        self.arrow(g, m, direction)
    }
    pub fn double_click_handle(&mut self, a: usize) -> Outcome {
        if !self.dock.group_shown() {
            return Err("dock_unavailable");
        }
        self.set_axis_mode(self.dock.group().unwrap(), a, AxisMode::Hug)
    }
    pub fn double_click_corner(&mut self) -> Outcome {
        if !self.dock.group_shown() {
            return Err("dock_unavailable");
        }
        let g = self.dock.group().unwrap();
        if self.board.groups[&g].flow != GroupFlow::Wrap {
            self.set_axis_mode(g, 0, AxisMode::Hug)?;
        }
        self.set_axis_mode(g, 1, AxisMode::Hug)
    }

    pub fn drop_member(&mut self, g: Id, m: Id, slot: i64, outside: bool) -> Outcome {
        if outside && self.member_admission(g, m).is_ok() && !self.board.members[&m].absolute {
            self.detach(g, m)
        } else {
            self.reorder(g, m, slot)
        }
    }
}
