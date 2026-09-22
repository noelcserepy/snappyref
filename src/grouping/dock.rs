//! Context and admission for the two contextual dock modes. Drafts and pixels belong to the UI.
use super::*;

#[derive(Clone, Debug, Default, PartialEq)]
pub enum Panel {
    #[default]
    Hidden,
    GroupDock {
        group: Id,
        align_open: bool,
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
    pub fn align_shown(&self) -> bool {
        matches!(
            self.panel,
            Panel::GroupDock {
                align_open: true,
                ..
            }
        )
    }
    pub fn normalized(mut self, b: &Board) -> Self {
        self.panel = match self.panel {
            Panel::Hidden => return self,
            Panel::GroupDock { group, align_open } if b.groups.contains_key(&group) => {
                self.panel = Panel::GroupDock {
                    group,
                    align_open: align_open && b.groups[&group].flow != "Free",
                };
                return self;
            }
            Panel::MemberDock { group, member } if b.owner(member) == Some(group) => return self,
            Panel::MemberDock { group, member } => {
                if let Some(group) = b.owner(member) {
                    Panel::MemberDock { group, member }
                } else if !b.live.contains(&member) && b.groups.contains_key(&group) {
                    Panel::GroupDock {
                        group,
                        align_open: false,
                    }
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
    Flow(String),
    Gap(f32),
    RowGap(f32),
    Padding(f32),
    Align([usize; 2]),
    Auto(String),
    Dimension(usize, f32),
    Hug(usize),
    Clip,
    Ungroup,
    MemberMode(usize, bool),
    Absolute(i64),
    Earlier,
    Later,
    Detach,
}
impl Control {
    fn member_control(&self) -> bool {
        matches!(
            self,
            Self::MemberMode(..) | Self::Absolute(_) | Self::Earlier | Self::Later | Self::Detach
        )
    }
}
impl<G: Geometry> State<G> {
    pub fn dockCount(&self) -> usize {
        self.dock
            .group()
            .map_or(0, |g| self.board.groups[&g].order.len())
    }
    pub fn dockMemberNumber(&self) -> usize {
        self.dock.member().map_or(0, |m| {
            self.board.groups[&self.dock.group().unwrap()]
                .order
                .iter()
                .position(|&x| x == m)
                .unwrap()
                + 1
        })
    }
    pub fn memberLayoutEnabled(&self) -> bool {
        self.dock.member().is_some_and(|m| {
            !self.board.members[&m].absolute
                && self.board.groups[&self.dock.group().unwrap()].flow != "Free"
        })
    }
    pub fn alignEnabled(&self) -> bool {
        self.dock.group_shown() && self.board.groups[&self.dock.group().unwrap()].flow != "Free"
    }
    pub fn controlShown(&self, c: &Control) -> bool {
        if c.member_control() {
            return self.dock.member().is_some();
        }
        match c {
            Control::Align(_) | Control::Auto(_) => self.dock.align_shown(),
            Control::RowGap(_) => {
                self.dock.group_shown()
                    && self.board.groups[&self.dock.group().unwrap()].flow == "Wrap"
            }
            _ => self.dock.group_shown(),
        }
    }
    pub fn controlEnabled(&self, c: &Control) -> bool {
        if !self.controlShown(c) {
            return false;
        }
        let g = &self.board.groups[&self.dock.group().unwrap()];
        match c {
            Control::Gap(_) => g.flow != "Free" && g.gap == "Number",
            Control::Align(xy) => {
                g.flow != "Free" && (g.gap == "Number" || xy[g.primary()] == g.align[g.primary()])
            }
            Control::Auto(_) => g.flow != "Free",
            Control::Hug(a) => !(*a == 0 && g.flow == "Wrap"),
            Control::MemberMode(a, fill) => {
                self.memberLayoutEnabled() && (!fill || g.fixed.contains(a))
            }
            Control::Earlier | Control::Later => {
                self.memberLayoutEnabled()
                    && self
                        .arrowTarget(
                            self.dock.group().unwrap(),
                            self.dock.member().unwrap(),
                            self.order_direction(matches!(c, Control::Later)),
                        )
                        .is_some()
            }
            _ => true,
        }
    }
    fn order_direction(&self, later: bool) -> &'static str {
        match (
            self.board.groups[&self.dock.group().unwrap()].flow == "Column",
            later,
        ) {
            (true, true) => "Down",
            (true, false) => "Up",
            (false, true) => "Right",
            (false, false) => "Left",
        }
    }
    pub fn controlReason(&self, c: &Control) -> &'static str {
        if (c.member_control() && self.dock.member().is_none())
            || (!c.member_control() && !self.dock.group_shown())
        {
            return "dock_unavailable";
        }
        let id = self.dock.group().unwrap();
        let g = &self.board.groups[&id];
        match c {
            Control::Name(n) if n.trim().is_empty() => "invalid_value",
            Control::Padding(n) if !n.is_finite() || *n < 0.0 => "invalid_value",
            Control::Gap(n) | Control::RowGap(n) if !n.is_finite() => "invalid_value",
            Control::Gap(_) if g.flow == "Free" || g.gap != "Number" => "control_inactive",
            Control::RowGap(_) if g.flow != "Wrap" => "control_inactive",
            Control::Align(_) | Control::Auto(_) if !self.dock.align_shown() => "popover_closed",
            Control::Align(_) | Control::Auto(_) if g.flow == "Free" => "control_inactive",
            Control::Align(xy) if g.gap != "Number" && xy[g.primary()] != g.align[g.primary()] => {
                "primary_alignment_inactive"
            }
            Control::Dimension(a, n) => self.axis_reason(id, *a, true, *n),
            Control::Hug(a) => self.axis_reason(id, *a, !g.fixed.contains(a), 1.0),
            Control::MemberMode(..) if !self.memberLayoutEnabled() => "control_inactive",
            Control::MemberMode(a, true) if !g.fixed.contains(a) => "fixed_axis_required",
            Control::Earlier | Control::Later if !self.controlEnabled(c) => "no_neighbor",
            _ => "",
        }
    }
    pub fn selectGroup(&mut self, g: Id) -> Outcome {
        let reason = if self.board.groups.contains_key(&g) {
            ""
        } else {
            "group_missing"
        };
        self.apply(
            self.board.clone(),
            reason,
            "selectGroup",
            Dock {
                panel: Panel::GroupDock {
                    group: g,
                    align_open: false,
                },
                text_focus: false,
            },
        )
    }
    pub fn editMember(&mut self, m: Id) -> Outcome {
        let Some(g) = self.board.owner(m) else {
            return self.commit(self.board.clone(), "member_missing", "editMember");
        };
        self.apply(
            self.board.clone(),
            "",
            "editMember",
            Dock {
                panel: Panel::MemberDock {
                    group: g,
                    member: m,
                },
                text_focus: false,
            },
        )
    }
    pub fn clearSelection(&mut self) -> Outcome {
        self.apply(self.board.clone(), "", "clearSelection", Dock::default())
    }
    pub fn toggleAlign(&mut self) -> Outcome {
        if !self.alignEnabled() {
            return self.commit(self.board.clone(), "control_inactive", "toggleAlign");
        }
        self.apply(
            self.board.clone(),
            "",
            "toggleAlign",
            Dock {
                panel: Panel::GroupDock {
                    group: self.dock.group().unwrap(),
                    align_open: !self.dock.align_shown(),
                },
                text_focus: false,
            },
        )
    }
    pub fn dismissAlign(&mut self) -> Outcome {
        let mut d = self.dock.clone();
        if let Panel::GroupDock { align_open, .. } = &mut d.panel {
            *align_open = false;
        }
        self.apply(self.board.clone(), "", "dismissAlign", d)
    }
    pub fn focusText(&mut self, active: bool) -> Outcome {
        let mut d = self.dock.clone();
        d.text_focus = active;
        self.apply(self.board.clone(), "", "focusText", d)
    }
    pub fn escape(&mut self) -> Outcome {
        let mut d = self.dock.clone();
        if d.text_focus {
            d.text_focus = false;
        } else if d.align_shown() || d.member().is_some() {
            d.panel = Panel::GroupDock {
                group: d.group().unwrap(),
                align_open: false,
            };
        } else {
            d = Dock::default();
        }
        self.apply(self.board.clone(), "", "escape", d)
    }
    pub fn useDock(&mut self, c: Control) -> Outcome {
        let reason = self.controlReason(&c);
        if !reason.is_empty() {
            return self.commit(self.board.clone(), reason, "useDock");
        }
        let g = self.dock.group().unwrap();
        let config = self.board.groups[&g].clone();
        match c {
            Control::Name(n) => {
                let mut b = self.board.clone();
                b.groups.get_mut(&g).unwrap().name = n.trim().into();
                self.commit(b, "", "rename")
            }
            Control::Flow(f) => self.setFlow(g, &f),
            Control::Gap(n) => self.setGap(g, "Number", n),
            Control::RowGap(n) => self.configure(
                g,
                config.clip,
                config.align[0],
                config.align[1],
                config.padding,
                n,
            ),
            Control::Padding(n) => self.configure(
                g,
                config.clip,
                config.align[0],
                config.align[1],
                n,
                config.rowGap,
            ),
            Control::Align(xy) => {
                self.configure(g, config.clip, xy[0], xy[1], config.padding, config.rowGap)
            }
            Control::Auto(mode) => self.setGap(g, &mode, config.number),
            Control::Dimension(a, n) => self.resizeEdge(g, a, n),
            Control::Hug(a) => self.setAxisMode(g, a, !config.fixed.contains(&a)),
            Control::Clip => self.configure(
                g,
                !config.clip,
                config.align[0],
                config.align[1],
                config.padding,
                config.rowGap,
            ),
            Control::Ungroup => self.ungroup(g),
            Control::MemberMode(a, fill) => {
                self.setMemberMode(g, self.dock.member().unwrap(), a, fill)
            }
            Control::Absolute(slot) => self.toggleAbsolute(g, self.dock.member().unwrap(), slot),
            Control::Earlier | Control::Later => self.arrow(
                g,
                self.dock.member().unwrap(),
                self.order_direction(matches!(c, Control::Later)),
            ),
            Control::Detach => self.detach(g, self.dock.member().unwrap()),
        }
    }
    pub fn arrowTarget(&self, g: Id, m: Id, direction: &str) -> Option<usize> {
        let group = &self.board.groups[&g];
        if self.board.members[&m].absolute || group.flow == "Free" {
            return None;
        }
        let xs = self.board.flowing(g);
        let i = xs.iter().position(|&x| x == m)?;
        let horizontal = ["Left", "Right"].contains(&direction);
        let delta = if ["Left", "Up"].contains(&direction) {
            -1
        } else {
            1
        };
        let target = if group.flow == "Wrap" && !horizontal {
            let rows = self.board.rows(g);
            let r = rows.iter().position(|row| row.contains(&m))?;
            let col = rows[r].iter().position(|&x| x == m)?;
            *rows.get((r as i64 + delta) as usize)?.get(col)?
        } else if (group.flow == "Column") != horizontal {
            *xs.get((i as i64 + delta) as usize)?
        } else {
            return None;
        };
        group.order.iter().position(|&x| x == target)
    }
    pub fn arrowEnabled(&self, direction: &str) -> bool {
        self.memberLayoutEnabled()
            && !self.dock.text_focus
            && self
                .arrowTarget(
                    self.dock.group().unwrap(),
                    self.dock.member().unwrap(),
                    direction,
                )
                .is_some()
    }
    pub fn pressKey(&mut self, key: &str, identity: Id, selection: &[Id], width: f32) -> Outcome {
        if key == "Escape" {
            return self.escape();
        }
        if self.dock.text_focus {
            return self.commit(self.board.clone(), "text_owns_key", "pressKey");
        }
        match key {
            "Left" | "Right" | "Up" | "Down" => {
                if self.arrowEnabled(key) {
                    self.arrow(self.dock.group().unwrap(), self.dock.member().unwrap(), key)
                } else {
                    self.commit(self.board.clone(), "no_neighbor", "pressKey")
                }
            }
            "Primary+G" => self.create(identity, selection, width),
            "Primary+Shift+G" => self.useDock(Control::Ungroup),
            "G" => self.commit(self.board.clone(), "", "requestAddPalette"),
            "Primary+V" => self.paste(
                self.dock.group().unwrap_or(-1),
                self.dock.member().unwrap_or(-1),
                identity,
                width,
            ),
            "Primary+D" if self.dock.member().is_some() => {
                self.duplicate(self.dock.member().unwrap(), identity)
            }
            "Delete" if self.dock.member().is_some() => {
                self.deleteMember(self.dock.member().unwrap())
            }
            "Primary+D" => self.commit(self.board.clone(), "", "requestDuplicateSelection"),
            "Primary+C" => self.commit(self.board.clone(), "", "requestCopySelection"),
            "Delete" => self.commit(self.board.clone(), "", "requestDeleteSelection"),
            _ => self.commit(self.board.clone(), "unhandled_key", "pressKey"),
        }
    }
    pub fn doubleClickHandle(&mut self, a: usize) -> Outcome {
        if self.dock.group_shown() {
            self.hugAxis(self.dock.group().unwrap(), a)
        } else {
            self.commit(self.board.clone(), "dock_unavailable", "doubleClickHandle")
        }
    }
    pub fn doubleClickCorner(&mut self) -> Outcome {
        if !self.dock.group_shown() {
            return self.commit(self.board.clone(), "dock_unavailable", "hugCorner");
        }
        let g = self.dock.group().unwrap();
        let mut b = self.board.clone();
        if b.groups[&g].flow != "Wrap" {
            let n = self.geometry.measured(&b, g, 0);
            self.axis_changed(&mut b, g, 0, false, n);
        }
        let n = self.geometry.measured(&b, g, 1);
        self.axis_changed(&mut b, g, 1, false, n);
        self.commit(b, "", "hugCorner")
    }
    pub fn dropMember(&mut self, g: Id, m: Id, slot: i64, outside: bool) -> Outcome {
        let reason = self.member_reason(g, m);
        if !reason.is_empty() {
            return self.commit(self.board.clone(), reason, "dropMember");
        }
        if outside && !self.board.members[&m].absolute {
            self.detach(g, m)
        } else {
            self.reorder(g, m, slot)
        }
    }
    pub fn uniformScale(&mut self) -> Outcome {
        if self.dock.group_shown() {
            self.commit(self.board.clone(), "", "requestUniformScale")
        } else {
            self.commit(self.board.clone(), "dock_unavailable", "uniformScale")
        }
    }
}
