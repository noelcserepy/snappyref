use super::CanvasState;
use crate::data::{Group, GroupFlow, GroupId, RectGeometry};
use eframe::egui;

impl CanvasState {
    pub fn select_pasted_groups(&mut self, ids: Vec<GroupId>) {
        self.interaction.selected_group_ids = ids;
        self.interaction.editing_group_id = None;
    }
    pub fn groups(&self) -> &[Group] {
        &self.document.groups
    }
    pub fn selected_group_ids(&self) -> Vec<GroupId> {
        self.interaction.selected_group_ids.clone()
    }
    pub fn restore_groups(&mut self, groups: Vec<Group>) {
        self.document.groups = groups;
        self.document.groups_dirty = false;
        self.interaction.selected_group_ids.clear();
        self.interaction.editing_group_id = None;
    }
    pub fn replace_groups(&mut self, groups: Vec<Group>) {
        if self.document.groups != groups {
            self.document.groups = groups;
            self.document.groups_dirty = true;
        }
    }
    pub fn group_for_member(&self, id: i64) -> Option<GroupId> {
        self.groups()
            .iter()
            .find(|g| g.members.contains(&id))
            .map(|g| g.id)
    }
    pub fn select_group(&mut self, id: GroupId) {
        let Some(group) = self.groups().iter().find(|g| g.id == id).cloned() else {
            return;
        };
        self.interaction.selected_group_ids = vec![id];
        self.interaction.selected_placement_ids = group
            .members
            .iter()
            .copied()
            .filter(|id| self.placement_for_id(*id).is_some())
            .collect();
        self.interaction.selected_annotation_ids = group
            .members
            .iter()
            .copied()
            .filter(|id| self.annotation_for_id(*id).is_some())
            .collect();
        self.interaction.editing_group_id = None;
    }
    pub(super) fn expand_group_selection(&mut self) {
        if self.interaction.editing_group_id.is_some() {
            return;
        }
        let selected = self
            .selected_placement_ids()
            .into_iter()
            .chain(self.selected_annotation_ids())
            .collect::<Vec<_>>();
        let groups = self
            .groups()
            .iter()
            .filter(|g| g.members.iter().any(|id| selected.contains(id)))
            .cloned()
            .collect::<Vec<_>>();
        for group in groups {
            if !self.interaction.selected_group_ids.contains(&group.id) {
                self.interaction.selected_group_ids.push(group.id);
            }
            for id in group.members {
                if self.placement_for_id(id).is_some() {
                    if !self.interaction.selected_placement_ids.contains(&id) {
                        self.interaction.selected_placement_ids.push(id);
                    }
                } else if !self.interaction.selected_annotation_ids.contains(&id) {
                    self.interaction.selected_annotation_ids.push(id);
                }
            }
        }
    }

    pub fn create_group(&mut self, id: GroupId) -> Result<Option<GroupId>, String> {
        let mut selected = self.selected_placement_ids();
        selected.extend(self.selected_annotation_ids());
        self.create_named_group(id, format!("Group {}", self.groups().len() + 1), &selected)
    }
    pub(crate) fn create_named_group(
        &mut self,
        id: GroupId,
        name: String,
        selected: &[i64],
    ) -> Result<Option<GroupId>, String> {
        let name = name.trim().to_owned();
        if name.is_empty() {
            return Err("Group name cannot be blank".into());
        }
        let Some(bounds) = selected
            .iter()
            .filter_map(|&m| self.group_member_geometry(m))
            .map(|r| r.visual_bounds())
            .reduce(|a, b| a.union(b))
        else {
            return Ok(None);
        };
        let mut state = self.grouping_state();
        if let crate::grouping::Outcome::Rejected(reason) =
            state.create(id, selected, bounds.width().max(1.0))
        {
            return Err(reason.into());
        }
        if !state.board.groups.contains_key(&id) {
            return Ok(None);
        }
        let padding = self.placement_gap;
        let group = Group {
            id,
            name,
            members: Vec::new(),
            container: RectGeometry::from_bounds(bounds.expand(padding), 0),
            flow: GroupFlow::Wrap,
            gap: padding,
            padding,
            layout: crate::data::GroupLayoutSettings {
                row_gap: padding,
                ..Default::default()
            },
        };
        self.apply_group_state(state, Some(group))?;
        self.select_group(id);
        Ok(Some(id))
    }
    pub fn add_members_to_group(
        &mut self,
        destination: GroupId,
        ids: &[i64],
    ) -> Result<bool, String> {
        let before = self.groups().to_vec();
        self.group_edit(|state| {
            for &m in ids {
                let result = state.add(destination, m);
                if result != crate::grouping::Outcome::Ok {
                    return result;
                }
            }
            crate::grouping::Outcome::Ok
        })?;
        Ok(self.groups() != before)
    }
    pub fn edit_group(&mut self, group: Group) -> Result<(), String> {
        if ![group.gap, group.padding, group.layout.row_gap]
            .iter()
            .all(|v| v.is_finite())
            || group.padding < 0.0
            || !group.container.size.is_finite()
            || group.container.size.min_elem() <= 0.0
        {
            return Err("invalid_value".into());
        }
        let old = self
            .groups()
            .iter()
            .find(|g| g.id == group.id)
            .cloned()
            .ok_or("group_missing")?;
        if old.members != group.members {
            return Err("Use reorder to change membership".into());
        }
        let g = group.id;
        self.group_edit(|s| {
            let mut result = crate::grouping::Outcome::Ok;
            macro_rules! edit {
                ($op:expr) => {
                    if result == crate::grouping::Outcome::Ok {
                        result = $op;
                    }
                };
            }
            if old.flow != group.flow {
                edit!(s.setFlow(g, group.flow.as_str()));
            }
            if old.gap != group.gap || old.layout.auto != group.layout.auto {
                edit!(s.setGap(g, &group.layout.auto, group.gap));
            }
            for a in 0..2 {
                if old.layout.fixed[a] != group.layout.fixed[a] {
                    edit!(s.setAxisMode(g, a, group.layout.fixed[a]));
                }
                if old.container.scaled_size()[a] != group.container.scaled_size()[a]
                    || (old.padding != group.padding && s.board.groups[&g].fixed.contains(&a))
                {
                    edit!(s.resizeEdge(
                        g,
                        a,
                        group.container.scaled_size()[a] - group.padding * 2.0
                    ));
                }
            }
            edit!(s.configure(
                g,
                group.layout.clip,
                group.layout.align[0],
                group.layout.align[1],
                group.padding,
                group.layout.row_gap
            ));
            result
        })?;
        if let Some(saved) = self.document.groups.iter_mut().find(|g| g.id == group.id)
            && saved.name != group.name
        {
            saved.name = group.name;
            self.document.groups_dirty = true;
        }
        Ok(())
    }
    pub fn ungroup_selected(&mut self) -> bool {
        let ids = self.selected_group_ids();
        let changed = !ids.is_empty()
            && self
                .group_edit(|s| {
                    for id in ids {
                        s.ungroup(id);
                    }
                    crate::grouping::Outcome::Ok
                })
                .is_ok();
        self.interaction.selected_group_ids.clear();
        self.interaction.editing_group_id = None;
        changed
    }
    pub(super) fn remove_group_members(&mut self, ids: &[i64]) {
        let _ = self.group_edit(|s| {
            for &m in ids {
                s.deleteMember(m);
            }
            crate::grouping::Outcome::Ok
        });
    }
    pub(crate) fn reflow_dirty_group_members(&mut self) {
        if self.document.groups_dirty {
            return;
        }
        let dirty = self
            .document
            .dirty_placement_ids
            .iter()
            .chain(&self.document.dirty_annotation_ids)
            .copied()
            .collect::<Vec<_>>();
        if dirty.iter().any(|&id| self.group_for_member(id).is_some()) {
            let mut s = self.grouping_state();
            let ids = s.board.groups.keys().copied().collect::<Vec<_>>();
            for g in ids {
                let c = s.board.groups[&g].clone();
                s.configure(g, c.clip, c.align[0], c.align[1], c.padding, c.rowGap);
            }
            let _ = self.apply_group_state(s, None);
        }
    }
    pub(super) fn reflow_group_for_member(&mut self, id: i64) {
        if self.group_for_member(id).is_some() {
            let mut s = self.grouping_state();
            let ids = s.board.groups.keys().copied().collect::<Vec<_>>();
            for g in ids {
                let c = s.board.groups[&g].clone();
                s.configure(g, c.clip, c.align[0], c.align[1], c.padding, c.rowGap);
            }
            let _ = self.apply_group_state(s, None);
        }
    }
    pub(super) fn finish_group_member_drag(&mut self, ids: &[i64]) {
        if self.interaction.editing_group_id.is_none() && !self.selected_group_ids().is_empty() {
            return;
        }
        for &m in ids {
            let Some(rect) = self.group_member_geometry(m) else {
                continue;
            };
            let owner = self.group_for_member(m);
            if let Some(g) = owner {
                let group = self.groups().iter().find(|x| x.id == g).unwrap();
                let mode = group.layout.members.get(&m).copied().unwrap_or_default();
                if mode.absolute {
                    self.reflow_group_for_member(m);
                    continue;
                }
                if !group.container.unrotated_bounds().contains(rect.center) {
                    let _ = self.group_edit(|s| s.detach(g, m));
                } else if group.flow == GroupFlow::Free {
                    self.reflow_group_for_member(m);
                } else {
                    let slot = self.group_insertion_slot(g, m, rect.center);
                    let _ = self.group_edit(|s| s.reorder(g, m, slot));
                }
            } else if let Some(g) = self
                .groups()
                .iter()
                .rev()
                .find(|g| g.container.unrotated_bounds().contains(rect.center))
                .map(|g| g.id)
            {
                let slot = self.group_insertion_slot(g, m, rect.center);
                let _ = self.group_edit(|s| s.dragIn(g, m, slot));
            }
        }
        if self.interaction.editing_group_id.is_some()
            && !ids
                .iter()
                .any(|&m| self.group_for_member(m) == self.interaction.editing_group_id)
        {
            self.interaction.editing_group_id = None;
        }
    }
    fn group_insertion_slot(&self, g: i64, m: i64, point: egui::Pos2) -> i64 {
        let group = self.groups().iter().find(|x| x.id == g).unwrap();
        let members = group
            .members
            .iter()
            .copied()
            .filter(|&id| id != m)
            .collect::<Vec<_>>();
        members
            .iter()
            .position(|&id| {
                self.group_member_geometry(id)
                    .is_some_and(|r| match group.flow {
                        GroupFlow::Column => point.y < r.center.y,
                        GroupFlow::Wrap => {
                            point.y < r.visual_bounds().min.y
                                || (point.y <= r.visual_bounds().max.y && point.x < r.center.x)
                        }
                        _ => point.x < r.center.x,
                    })
            })
            .unwrap_or(members.len()) as i64
    }
}

impl CanvasState {
    pub fn leave_group_member_editing(&mut self) {
        if let Some(id) = self.interaction.editing_group_id {
            self.select_group(id);
        }
    }
    pub(super) fn group_transform_origins(&self) -> Vec<(i64, RectGeometry)> {
        if self.interaction.editing_group_id.is_some() {
            return Vec::new();
        }
        self.selected_group_ids()
            .iter()
            .filter_map(|id| self.groups().iter().find(|g| g.id == *id))
            .filter_map(|g| {
                g.members
                    .first()
                    .and_then(|id| self.placement_for_id(*id))
                    .map(|p| (g.id, p.state.geometry))
            })
            .collect()
    }
    pub(super) fn follow_group_member_transforms(&mut self, origins: Vec<(i64, RectGeometry)>) {
        for (id, before) in origins {
            let Some(group) = self.groups().iter().find(|g| g.id == id) else {
                continue;
            };
            let Some(after) = group
                .members
                .first()
                .and_then(|id| self.placement_for_id(*id))
                .map(|p| p.state.geometry)
            else {
                continue;
            };
            if after.center == before.center && after.scale == before.scale {
                continue;
            }
            let factor = after.scale / before.scale;
            let group = self
                .document
                .groups
                .iter_mut()
                .find(|g| g.id == id)
                .unwrap();
            group.container.center =
                after.center + (group.container.center - before.center) * factor;
            group.container.size *= factor;
            group.gap *= factor;
            group.layout.row_gap *= factor;
            group.padding *= factor;
            self.interaction.group_transform_changed = true;
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum GroupPickerChoice {
    Existing(GroupId),
    Create,
}

pub(super) struct GroupPicker {
    query: String,
    highlighted: Option<GroupPickerChoice>,
    focus: bool,
    members: Vec<i64>,
}

impl CanvasState {
    pub fn open_group_picker(&mut self) {
        if self.selected_placement_ids().is_empty() && self.selected_annotation_ids().is_empty() {
            return;
        }
        self.interaction.group_picker = Some(GroupPicker {
            query: String::new(),
            highlighted: self
                .interaction
                .last_group_destination
                .filter(|id| self.groups().iter().any(|g| g.id == *id))
                .or_else(|| self.groups().first().map(|g| g.id))
                .map(GroupPickerChoice::Existing),
            focus: true,
            members: self
                .selected_placement_ids()
                .into_iter()
                .chain(self.selected_annotation_ids())
                .filter(|&id| self.group_member_geometry(id).is_some())
                .collect(),
        });
    }

    pub(crate) fn take_group_creation(&mut self) -> Option<(String, Vec<i64>)> {
        self.interaction.pending_group_creation.take()
    }

    pub(crate) fn remember_group_destination(&mut self, id: GroupId) {
        self.interaction.last_group_destination = Some(id);
    }

    pub(super) fn show_group_picker(&mut self, ctx: &egui::Context) {
        use crate::ui::theme;
        let Some(mut picker) = self.interaction.group_picker.take() else {
            return;
        };
        let input_id = egui::Id::new("group-picker-query");
        let just_opened = picker.focus;
        let mut confirmed = None;
        let mut cancel = ctx.input(|i| i.key_pressed(egui::Key::Escape));
        let viewport = ctx.viewport_rect();
        let width = 420.0_f32.min((viewport.width() - theme::SPACE_6 * 2.0).max(180.0));
        let entrance =
            crate::ui::motion::entrance_on_appear(ctx, egui::Id::new("group-picker-entrance"));
        let settle = crate::ui::motion::settle_offset(entrance);
        // Same warm, borderless panel and text-first input as the search palette.
        let panel = egui::Area::new(egui::Id::new("group-picker"))
            .order(egui::Order::Foreground)
            .pivot(egui::Align2::CENTER_TOP)
            .fixed_pos(egui::pos2(
                viewport.center().x,
                viewport.top() + 56.0 - settle,
            ))
            .show(ctx, |ui| {
                ui.set_opacity(entrance);
                egui::Frame::new()
                    .fill(theme::PALETTE_SURFACE)
                    .stroke(egui::Stroke::NONE)
                    .corner_radius(theme::RADIUS_PANEL)
                    .shadow(egui::epaint::Shadow {
                        offset: [0, 12],
                        blur: 40,
                        spread: 0,
                        color: egui::Color32::from_black_alpha(180),
                    })
                    .inner_margin(theme::SPACE_4)
                    .show(ui, |ui| {
                        ui.set_width(width - theme::SPACE_4 * 2.0);
                        ui.spacing_mut().item_spacing.y = theme::SPACE_2;
                        ui.label(theme::editorial_caption("Add to group"));
                        let search = ui.add(
                            egui::TextEdit::singleline(&mut picker.query)
                                .id(input_id)
                                .desired_width(f32::INFINITY)
                                .hint_text("Find or create a group")
                                .frame(egui::Frame::NONE),
                        );
                        if picker.focus {
                            search.request_focus();
                            picker.focus = false;
                        }
                        ui.separator();
                        let query = picker.query.trim().to_lowercase();
                        let mut choices: Vec<_> = self
                            .groups()
                            .iter()
                            .filter(|g| g.name.to_lowercase().contains(&query))
                            .map(|g| (GroupPickerChoice::Existing(g.id), g.name.clone()))
                            .collect();
                        if !query.is_empty()
                            && !self.groups().iter().any(|g| g.name.to_lowercase() == query)
                        {
                            choices.push((
                                GroupPickerChoice::Create,
                                format!("Create new \"{}\"", picker.query.trim()),
                            ));
                        }
                        if !choices
                            .iter()
                            .any(|(choice, _)| Some(*choice) == picker.highlighted)
                        {
                            picker.highlighted = choices.first().map(|(choice, _)| *choice);
                        }
                        let navigating = ctx.input(|i| {
                            i.key_pressed(egui::Key::ArrowDown) || i.key_pressed(egui::Key::ArrowUp)
                        });
                        if !choices.is_empty() {
                            let current = choices
                                .iter()
                                .position(|(c, _)| Some(*c) == picker.highlighted)
                                .unwrap_or(0);
                            if ctx.input(|i| i.key_pressed(egui::Key::ArrowDown)) {
                                picker.highlighted = Some(choices[(current + 1) % choices.len()].0);
                            }
                            if ctx.input(|i| i.key_pressed(egui::Key::ArrowUp)) {
                                picker.highlighted =
                                    Some(choices[(current + choices.len() - 1) % choices.len()].0);
                            }
                        }
                        egui::ScrollArea::vertical()
                            .max_height((viewport.height() - 220.0).clamp(60.0, 240.0))
                            .show(ui, |ui| {
                                if choices.is_empty() {
                                    ui.label(
                                        egui::RichText::new("Type a name to create a group")
                                            .color(theme::TEXT_SUBTLE),
                                    );
                                }
                                for (choice, label) in choices {
                                    let selected = picker.highlighted == Some(choice);
                                    ui.visuals_mut().widgets.inactive.bg_fill =
                                        egui::Color32::TRANSPARENT;
                                    ui.visuals_mut().widgets.hovered.bg_fill =
                                        theme::SURFACE_ELEVATED;
                                    ui.visuals_mut().widgets.active.bg_fill =
                                        theme::SURFACE_PRESSED;
                                    ui.visuals_mut().selection.bg_fill = theme::SURFACE_ELEVATED;
                                    let row = ui.add(
                                        egui::Button::new("")
                                            .left_text(label.as_str())
                                            .selected(selected)
                                            .stroke(egui::Stroke::NONE)
                                            .corner_radius(theme::RADIUS_COMPACT)
                                            .min_size(egui::vec2(ui.available_width(), 36.0))
                                            .truncate(),
                                    );
                                    row.widget_info(|| {
                                        egui::WidgetInfo::labeled(
                                            egui::WidgetType::Button,
                                            ui.is_enabled(),
                                            &label,
                                        )
                                    });
                                    if selected && navigating {
                                        row.scroll_to_me(None);
                                    }
                                    if row.clicked() {
                                        confirmed = Some(choice);
                                    }
                                }
                            });
                        if ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
                            confirmed = picker.highlighted;
                        }
                        ui.separator();
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new("↑ ↓  choose    enter  confirm")
                                    .monospace()
                                    .small()
                                    .color(theme::TEXT_SUBTLE),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui.add(egui::Button::new("Cancel").frame(false)).clicked() {
                                        cancel = true;
                                    }
                                },
                            );
                        });
                    });
            });
        cancel |= !just_opened
            && ctx.input(|i| {
                i.pointer.any_pressed()
                    && i.pointer
                        .interact_pos()
                        .is_some_and(|p| !panel.response.rect.contains(p))
            });
        if cancel || confirmed.is_some() {
            ctx.memory_mut(|m| m.surrender_focus(input_id));
        }
        if !cancel {
            match confirmed {
                Some(GroupPickerChoice::Existing(id)) => {
                    if self.add_members_to_group(id, &picker.members).is_ok() {
                        self.remember_group_destination(id);
                        self.select_group(id);
                    }
                }
                Some(GroupPickerChoice::Create) => {
                    self.interaction.pending_group_creation =
                        Some((picker.query.trim().to_owned(), picker.members));
                }
                None => self.interaction.group_picker = Some(picker),
            }
        }
    }
    pub(super) fn paint_groups(&self, painter: &egui::Painter, viewport: egui::Rect) {
        use crate::ui::theme;
        if painter.ctx().input(|i| i.pointer.primary_down())
            && self.interaction.gesture.is_some()
            && self.selected_group_ids().is_empty()
            && let Some(pointer) = painter.ctx().input(|i| i.pointer.latest_pos())
        {
            let point = self.screen_to_world(pointer, viewport);
            if let Some(group) = self.groups().iter().rev().find(|g| {
                g.flow != GroupFlow::Free && g.container.unrotated_bounds().contains(point)
            }) && let Some(m) = self
                .selected_placement_ids()
                .into_iter()
                .chain(self.selected_annotation_ids())
                .next()
            {
                let members = group
                    .members
                    .iter()
                    .copied()
                    .filter(|&id| {
                        id != m
                            && !group
                                .layout
                                .members
                                .get(&id)
                                .is_some_and(|mode| mode.absolute)
                    })
                    .collect::<Vec<_>>();
                let slot = self.group_insertion_slot(group.id, m, point) as usize;
                if let Some(r) = members
                    .get(slot)
                    .or_else(|| members.last())
                    .and_then(|&id| self.group_member_geometry(id))
                {
                    let bounds = r.visual_bounds();
                    let at_end = slot >= members.len();
                    let (a, b) = if group.flow == GroupFlow::Column {
                        let y = if at_end { bounds.max.y } else { bounds.min.y };
                        (egui::pos2(bounds.min.x, y), egui::pos2(bounds.max.x, y))
                    } else {
                        let x = if at_end { bounds.max.x } else { bounds.min.x };
                        (egui::pos2(x, bounds.min.y), egui::pos2(x, bounds.max.y))
                    };
                    painter.line_segment(
                        [
                            self.world_to_screen(a, viewport),
                            self.world_to_screen(b, viewport),
                        ],
                        egui::Stroke::new(2.0_f32, theme::ACCENT),
                    );
                }
            }
        }

        for group in self.groups() {
            let bounds = group.container.unrotated_bounds();
            let rect = egui::Rect::from_min_max(
                self.world_to_screen(bounds.min, viewport),
                self.world_to_screen(bounds.max, viewport),
            );
            let color = if self.interaction.selected_group_ids.contains(&group.id)
                || self.interaction.editing_group_id == Some(group.id)
            {
                theme::ACCENT
            } else {
                theme::TEXT_SUBTLE
            };
            painter.rect_stroke(
                rect,
                theme::RADIUS_NONE,
                egui::Stroke::new(1.0_f32, color),
                egui::StrokeKind::Outside,
            );
            painter.text(
                rect.left_top() - egui::vec2(0., theme::SPACE_2),
                egui::Align2::LEFT_BOTTOM,
                &group.name,
                egui::FontId::proportional(13.),
                color,
            );
        }
    }
    pub(super) fn show_group_controls(
        &mut self,
        ui: &mut egui::Ui,
        viewport: egui::Rect,
    ) -> Option<egui::Rect> {
        use crate::ui::theme;
        if !ui.input(|i| i.pointer.primary_down())
            && let Some(before) = self.interaction.group_controls_start.take()
            && self
                .groups()
                .iter()
                .find(|g| g.id == before.id)
                .is_some_and(|g| g != &before)
        {
            self.document.groups_dirty = true;
            for id in before.members {
                self.mark_dirty(id);
            }
        }
        // The editor can disappear when a canvas click selects another item.
        // Finish its draft here even when it will not render a lost-focus frame.
        if let Some((id, _)) = &self.interaction.group_name_edit {
            let editing = self.interaction.selected_group_ids == [*id]
                && ui
                    .ctx()
                    .memory(|m| m.has_focus(egui::Id::new(("group-name", *id))));
            if !editing {
                let (id, name) = self.interaction.group_name_edit.take().unwrap();
                if let Some(mut group) = self.groups().iter().find(|g| g.id == id).cloned()
                    && !name.trim().is_empty()
                    && group.name != name.trim()
                {
                    group.name = name.trim().to_owned();
                    let _ = self.edit_group(group);
                }
            }
        }
        let active = self
            .interaction
            .editing_group_id
            .or_else(|| self.interaction.selected_group_ids.first().copied());
        let cache = egui::Id::new("group-dock-last");
        if let Some(id) = active {
            ui.ctx().data_mut(|d| d.insert_temp(cache, id));
        }
        let entrance = ui.ctx().animate_bool_with_time(
            egui::Id::new("group-dock-visible"),
            active.is_some(),
            theme::MOTION_SETTLE_SECONDS,
        );
        if entrance == 0.0 && active.is_none() {
            return None;
        }
        let id = active.or_else(|| ui.ctx().data(|d| d.get_temp::<i64>(cache)))?;
        let mut group = self.groups().iter().find(|g| g.id == id)?.clone();
        let member = self.interaction.editing_group_id.and_then(|_| {
            self.selected_placement_ids()
                .into_iter()
                .chain(self.selected_annotation_ids())
                .find(|m| group.members.contains(m))
        });
        let mut changed = false;
        let mut ungroup = false;
        let mut member_action = None;
        let popover_id = egui::Id::new(("group-align", id));
        let mut align_open = ui
            .ctx()
            .data(|d| d.get_temp::<bool>(popover_id))
            .unwrap_or(false);
        let mut align_anchor = None;
        let response = egui::Area::new(egui::Id::new("group-controls"))
            .pivot(egui::Align2::CENTER_BOTTOM)
            .fixed_pos(egui::pos2(
                viewport.center().x,
                viewport.bottom() - theme::SPACE_3 + (1.0 - entrance) * 52.0,
            ))
            .order(egui::Order::Foreground)
            .show(ui.ctx(), |ui| {
                ui.set_opacity(entrance);
                if active.is_none() {
                    ui.disable();
                }
                egui::Frame::new()
                    .fill(theme::PALETTE_SURFACE)
                    .corner_radius(theme::RADIUS_PANEL)
                    .inner_margin(theme::SPACE_3)
                    .show(ui, |ui| {
                        ui.set_width((viewport.width() - theme::SPACE_6 * 2.0).max(160.0));
                        egui::ScrollArea::horizontal()
                            .max_height(theme::CONTROL_HEIGHT)
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.set_height(theme::CONTROL_HEIGHT);
                                    ui.spacing_mut().item_spacing.x = theme::SPACE_2;
                                    if let Some(m) = member {
                                        let mode = group
                                            .layout
                                            .members
                                            .get(&m)
                                            .copied()
                                            .unwrap_or_default();
                                        ui.label(format!(
                                            "{} / {}",
                                            group.name,
                                            group.members.iter().position(|&id| id == m).unwrap()
                                                + 1
                                        ));
                                        for (a, label) in [(0, "W"), (1, "H")] {
                                            ui.label(label);
                                            if ui.selectable_label(!mode.fill[a], "Fixed").clicked()
                                            {
                                                member_action = Some((0, a, false));
                                            }
                                            if ui
                                                .add_enabled(
                                                    group.layout.fixed[a],
                                                    egui::Button::new("Fill")
                                                        .selected(mode.fill[a]),
                                                )
                                                .clicked()
                                            {
                                                member_action = Some((0, a, true));
                                            }
                                        }
                                        if ui.selectable_label(mode.absolute, "Absolute").clicked()
                                        {
                                            member_action = Some((1, 0, false));
                                        }
                                        if ui.button("Earlier").clicked() {
                                            member_action = Some((2, 0, false));
                                        }
                                        if ui.button("Later").clicked() {
                                            member_action = Some((2, 0, true));
                                        }
                                        if ui.button("Detach").clicked() {
                                            member_action = Some((3, 0, false));
                                        }
                                        ui.label(
                                            egui::RichText::new("Esc leaves member editing")
                                                .small()
                                                .color(theme::TEXT_MUTED),
                                        );
                                    } else {
                                        let mut name = self
                                            .interaction
                                            .group_name_edit
                                            .as_ref()
                                            .filter(|(g, _)| *g == id)
                                            .map(|(_, name)| name.clone())
                                            .unwrap_or_else(|| group.name.clone());
                                        let edit = ui.add(
                                            egui::TextEdit::singleline(&mut name)
                                                .id(egui::Id::new(("group-name", id)))
                                                .desired_width(110.0),
                                        );
                                        if edit.has_focus() {
                                            self.interaction.group_name_edit =
                                                Some((id, name.clone()));
                                        }
                                        if edit.lost_focus() {
                                            if !name.trim().is_empty() && group.name != name.trim()
                                            {
                                                group.name = name.trim().into();
                                                changed = true;
                                            }
                                            self.interaction.group_name_edit = None;
                                        }
                                        ui.label(
                                            egui::RichText::new(group.members.len().to_string())
                                                .color(theme::TEXT_MUTED),
                                        );
                                        for (flow, label) in [
                                            (GroupFlow::Row, "Row"),
                                            (GroupFlow::Column, "Column"),
                                            (GroupFlow::Wrap, "Wrap"),
                                            (GroupFlow::Free, "Free"),
                                        ] {
                                            let button = ui.add(
                                                egui::Button::new(format!("   {label}"))
                                                    .selected(group.flow == flow),
                                            );
                                            button.widget_info(|| {
                                                egui::WidgetInfo::labeled(
                                                    egui::WidgetType::Button,
                                                    ui.is_enabled(),
                                                    label,
                                                )
                                            });
                                            let cells: &[(f32, f32)] = match flow {
                                                GroupFlow::Row => {
                                                    &[(0.0, 3.0), (3.0, 3.0), (6.0, 3.0)]
                                                }
                                                GroupFlow::Column => {
                                                    &[(3.0, 0.0), (3.0, 3.0), (3.0, 6.0)]
                                                }
                                                GroupFlow::Wrap => &[
                                                    (0.0, 0.0),
                                                    (4.0, 0.0),
                                                    (0.0, 5.0),
                                                    (4.0, 5.0),
                                                ],
                                                GroupFlow::Free => {
                                                    &[(0.0, 1.0), (5.0, 0.0), (3.0, 6.0)]
                                                }
                                            };
                                            let origin = egui::pos2(
                                                button.rect.left() + 4.0,
                                                button.rect.center().y - 4.0,
                                            );
                                            for &(x, y) in cells {
                                                ui.painter().rect_filled(
                                                    egui::Rect::from_min_size(
                                                        origin + egui::vec2(x, y),
                                                        egui::vec2(2.0, 2.0),
                                                    ),
                                                    theme::RADIUS_NONE,
                                                    if group.flow == flow {
                                                        theme::ACCENT
                                                    } else {
                                                        theme::TEXT_MUTED
                                                    },
                                                );
                                            }
                                            if button.clicked() && group.flow != flow {
                                                group.flow = flow;
                                                changed = true;
                                            }
                                        }
                                        ui.add_enabled_ui(group.flow != GroupFlow::Free, |ui| {
                                            ui.label("Gap");
                                            changed |= ui
                                                .add_enabled(
                                                    group.layout.auto == "Number",
                                                    egui::DragValue::new(&mut group.gap).speed(1.0),
                                                )
                                                .changed();
                                            if group.flow == GroupFlow::Wrap {
                                                ui.label("Row gap");
                                                changed |= ui
                                                    .add(
                                                        egui::DragValue::new(
                                                            &mut group.layout.row_gap,
                                                        )
                                                        .speed(1.0),
                                                    )
                                                    .changed();
                                            }
                                        });
                                        ui.label("Padding");
                                        changed |= ui
                                            .add(
                                                egui::DragValue::new(&mut group.padding)
                                                    .range(0.0..=10000.0)
                                                    .speed(1.0),
                                            )
                                            .changed();
                                        let align = ui.add_enabled(
                                            group.flow != GroupFlow::Free,
                                            egui::Button::new("Align    "),
                                        );
                                        align.widget_info(|| {
                                            egui::WidgetInfo::labeled(
                                                egui::WidgetType::Button,
                                                ui.is_enabled(),
                                                "Align",
                                            )
                                        });
                                        for y in 0..3 {
                                            for x in 0..3 {
                                                ui.painter().circle_filled(
                                                    egui::pos2(
                                                        align.rect.right() - 13.0 + x as f32 * 3.0,
                                                        align.rect.center().y - 3.0
                                                            + y as f32 * 3.0,
                                                    ),
                                                    1.0,
                                                    if group.layout.align == [x, y] {
                                                        theme::ACCENT
                                                    } else {
                                                        theme::TEXT_MUTED
                                                    },
                                                );
                                            }
                                        }
                                        if align.clicked() {
                                            align_open = !align_open;
                                        }
                                        align_anchor = Some(align.rect.center_top());
                                        for (a, label) in [(0, "W"), (1, "H")] {
                                            ui.label(label);
                                            if group.layout.fixed[a] {
                                                let mut dimension =
                                                    group.container.scaled_size()[a];
                                                if ui
                                                    .add(
                                                        egui::DragValue::new(&mut dimension)
                                                            .range(
                                                                (group.padding * 2.0 + 1.0)
                                                                    ..=100000.0,
                                                            )
                                                            .speed(1.0),
                                                    )
                                                    .changed()
                                                {
                                                    group.container.size[a] =
                                                        dimension / group.container.scale;
                                                    changed = true;
                                                }
                                            }
                                            let permitted = a != 0 || group.flow != GroupFlow::Wrap;
                                            if ui
                                                .add_enabled(
                                                    permitted,
                                                    egui::Button::new("Hug")
                                                        .selected(!group.layout.fixed[a]),
                                                )
                                                .clicked()
                                            {
                                                group.layout.fixed[a] = !group.layout.fixed[a];
                                                changed = true;
                                            }
                                        }
                                        changed |= ui
                                            .toggle_value(&mut group.layout.clip, "Clip")
                                            .changed();
                                        ungroup = ui.button("Ungroup").clicked();
                                    }
                                });
                            });
                    });
            });
        if align_open
            && member.is_none()
            && let Some(anchor) = align_anchor
        {
            let popup = egui::Area::new(popover_id)
                .pivot(egui::Align2::CENTER_BOTTOM)
                .fixed_pos(anchor - egui::vec2(0.0, theme::SPACE_4))
                .order(egui::Order::Foreground)
                .show(ui.ctx(), |ui| {
                    egui::Frame::new()
                        .fill(theme::PALETTE_SURFACE)
                        .inner_margin(theme::SPACE_3)
                        .corner_radius(theme::RADIUS_PANEL)
                        .show(ui, |ui| {
                            for y in 0..3 {
                                ui.horizontal(|ui| {
                                    for x in 0..3 {
                                        let primary = usize::from(group.flow == GroupFlow::Column);
                                        let enabled = group.layout.auto == "Number"
                                            || [x, y][primary] == group.layout.align[primary];
                                        if ui
                                            .add_enabled(
                                                enabled,
                                                egui::Button::new("•")
                                                    .selected(group.layout.align == [x, y])
                                                    .min_size(egui::vec2(28.0, 28.0)),
                                            )
                                            .on_hover_text(format!(
                                                "Align {}, {}",
                                                ["left", "center", "right"][x],
                                                ["top", "center", "bottom"][y]
                                            ))
                                            .clicked()
                                        {
                                            group.layout.align = [x, y];
                                            changed = true;
                                        }
                                    }
                                });
                            }
                            for (value, label) in [
                                ("Number", "Off"),
                                ("Between", "Between"),
                                ("Evenly", "Evenly"),
                                ("Around", "Around"),
                            ] {
                                changed |= ui
                                    .selectable_value(
                                        &mut group.layout.auto,
                                        value.to_owned(),
                                        label,
                                    )
                                    .changed();
                            }
                        });
                });
            if ui.input(|i| {
                i.key_pressed(egui::Key::Escape)
                    || (i.pointer.any_pressed()
                        && i.pointer.interact_pos().is_some_and(|p| {
                            !popup.response.rect.contains(p) && !response.response.rect.contains(p)
                        }))
            }) {
                align_open = false;
            }
        }
        ui.ctx().data_mut(|d| d.insert_temp(popover_id, align_open));
        if let (Some(m), Some((action, a, value))) = (member, member_action) {
            let direction = if group.flow == GroupFlow::Column {
                if value { "Down" } else { "Up" }
            } else if value {
                "Right"
            } else {
                "Left"
            };
            let slot =
                self.group_insertion_slot(id, m, self.group_member_geometry(m).unwrap().center);
            let _ = self.group_edit(|s| match action {
                0 => s.setMemberMode(id, m, a, value),
                1 => s.toggleAbsolute(id, m, slot),
                2 => s.arrow(id, m, direction),
                _ => s.detach(id, m),
            });
        }
        if changed {
            let _ = self.edit_group(group);
        }
        if ungroup {
            self.ungroup_selected();
        }

        Some(response.response.rect)
    }
}

#[derive(Clone)]
pub(super) struct GroupGesture {
    group: Group,
    members: Vec<(i64, RectGeometry)>,
    start: egui::Pos2,
    kind: GroupGestureKind,
}
#[derive(Clone, Copy)]
enum GroupGestureKind {
    Move,
    Resize([i8; 2]),
    Scale,
}
impl CanvasState {
    pub(super) fn handle_group_pointer(
        &mut self,
        ui: &egui::Ui,
        viewport: egui::Rect,
        input: egui::Rect,
        overlay: Option<egui::Rect>,
    ) -> bool {
        if !ui.ctx().egui_wants_keyboard_input()
            && let Some(g) = self.interaction.editing_group_id
            && let Some(m) = self
                .selected_placement_ids()
                .into_iter()
                .chain(self.selected_annotation_ids())
                .next()
        {
            for (key, direction) in [
                (egui::Key::ArrowLeft, "Left"),
                (egui::Key::ArrowRight, "Right"),
                (egui::Key::ArrowUp, "Up"),
                (egui::Key::ArrowDown, "Down"),
            ] {
                if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, key)) {
                    let _ = self.group_edit(|s| s.arrow(g, m, direction));
                }
            }
        }
        let pointer = ui.input(|i| i.pointer.latest_pos());
        if ui.input(|i| {
            i.modifiers.command
                && !i.modifiers.alt
                && i.pointer.button_pressed(egui::PointerButton::Primary)
        }) && let Some(p) = pointer
            && input.contains(p)
            && !overlay.is_some_and(|r| r.contains(p))
            && let Some(hit) = super::interaction::hit_test_canvas_item(self, viewport, p)
        {
            let m = match hit {
                super::CanvasHit::Placement(id) | super::CanvasHit::Annotation(id) => id,
            };
            if let Some(g) = self.group_for_member(m) {
                self.interaction.editing_group_id = Some(g);
                self.interaction.selected_group_ids.clear();
                self.interaction.selected_placement_ids.clear();
                self.interaction.selected_annotation_ids.clear();
                if self.placement_for_id(m).is_some() {
                    self.interaction.selected_placement_ids.push(m);
                } else {
                    self.interaction.selected_annotation_ids.push(m);
                }
            }
        }
        let released = ui.input(|i| i.pointer.button_released(egui::PointerButton::Primary));
        if released
            && !self
                .interaction
                .group_gesture
                .as_ref()
                .is_some_and(|gesture| matches!(gesture.kind, GroupGestureKind::Resize(_)))
            && ui.input(|i| {
                i.pointer
                    .button_double_clicked(egui::PointerButton::Primary)
            })
            && let Some(p) = pointer
            && input.contains(p)
            && !overlay.is_some_and(|r| r.contains(p))
            && let Some(hit) = super::interaction::hit_test_canvas_item(self, viewport, p)
        {
            let m = match hit {
                super::CanvasHit::Placement(id) | super::CanvasHit::Annotation(id) => id,
            };
            if let Some(g) = self.group_for_member(m)
                && self.interaction.editing_group_id != Some(g)
            {
                self.interaction.group_gesture = None;
                self.interaction.editing_group_id = Some(g);
                self.interaction.selected_group_ids.clear();
                self.interaction.selected_placement_ids.clear();
                self.interaction.selected_annotation_ids.clear();
                if self.placement_for_id(m).is_some() {
                    self.interaction.selected_placement_ids.push(m);
                } else {
                    self.interaction.selected_annotation_ids.push(m);
                }
                return true;
            }
        }

        if released && self.interaction.group_transform_changed {
            self.document.groups_dirty = true;
            self.interaction.group_transform_changed = false;
        }
        if let Some(gesture) = self.interaction.group_gesture.clone() {
            if released
                && ui.input(|i| {
                    i.pointer
                        .button_double_clicked(egui::PointerButton::Primary)
                })
                && let GroupGestureKind::Resize(edges) = gesture.kind
            {
                let id = gesture.group.id;
                let _ = self.group_edit(|s| {
                    for (a, edge) in edges.iter().enumerate() {
                        if *edge != 0 {
                            let result = s.hugAxis(id, a);
                            if result != crate::grouping::Outcome::Ok {
                                return result;
                            }
                        }
                    }
                    crate::grouping::Outcome::Ok
                });
                self.interaction.group_gesture = None;
                return true;
            }
            if let Some(pointer) = pointer {
                let delta = self.screen_to_world(pointer, viewport) - gesture.start;
                let mut group = gesture.group.clone();
                match gesture.kind {
                    GroupGestureKind::Resize(edges) => {
                        for a in 0..2 {
                            if edges[a] != 0 {
                                group.container.size[a] = (group.container.size[a]
                                    + delta[a] * edges[a] as f32)
                                    .max(group.padding * 2.0 + 1.0);
                                group.layout.fixed[a] = true;
                            }
                        }
                        let id = group.id;
                        let _ = self.edit_group(group);
                        if let Some(current) = self.groups().iter().find(|g| g.id == id).cloned() {
                            let before = gesture.group.container.unrotated_bounds();
                            let after = current.container.unrotated_bounds();
                            let mut offset = egui::Vec2::ZERO;
                            for a in 0..2 {
                                if edges[a] < 0 {
                                    offset[a] = before.max[a] - after.max[a];
                                }
                            }
                            if offset != egui::Vec2::ZERO {
                                self.document
                                    .groups
                                    .iter_mut()
                                    .find(|g| g.id == id)
                                    .unwrap()
                                    .container
                                    .center += offset;
                                for m in &current.members {
                                    if let Some(mut r) = self.group_member_geometry(*m) {
                                        r.center += offset;
                                        self.set_group_member_geometry(*m, r);
                                    }
                                }
                            }
                        }
                    }
                    GroupGestureKind::Move => {
                        group.container.center += delta;
                        for (id, mut geometry) in gesture.members.clone() {
                            geometry.center += delta;
                            self.set_group_member_geometry(id, geometry);
                        }
                        if let Some(target) =
                            self.document.groups.iter_mut().find(|g| g.id == group.id)
                        {
                            *target = group;
                        }
                    }
                    GroupGestureKind::Scale => {
                        let factor =
                            (1. + delta.x / gesture.group.container.scaled_size().x).max(0.05);
                        if let Ok(layout) = crate::placement::group_layout::scale_group(
                            &group,
                            &gesture.members,
                            factor,
                        ) {
                            group.container = layout.container;
                            group.gap = layout.gap;
                            group.layout.row_gap *= factor;
                            group.padding = layout.padding;
                            for (id, geometry) in layout.members {
                                self.set_group_member_geometry(id, geometry);
                            }
                            if let Some(target) =
                                self.document.groups.iter_mut().find(|g| g.id == group.id)
                            {
                                *target = group;
                            }
                        }
                    }
                }
                // A pointer gesture becomes one document change on release.
                self.document.groups_dirty = false;
                for (id, _) in &gesture.members {
                    self.document
                        .dirty_placement_ids
                        .retain(|dirty| dirty != id);
                    self.document
                        .dirty_annotation_ids
                        .retain(|dirty| dirty != id);
                }
            }
            if released {
                if self
                    .groups()
                    .iter()
                    .find(|g| g.id == gesture.group.id)
                    .is_some_and(|g| g != &gesture.group)
                {
                    self.document.groups_dirty = true;
                    for (id, _) in gesture.members {
                        if self.placement_for_id(id).is_some() {
                            self.mark_dirty(id);
                        } else {
                            self.mark_annotation_for_persistence(id);
                        }
                    }
                }
                self.interaction.group_gesture = None;
            }
            return true;
        }
        let Some(pointer) = pointer else { return false };
        if !input.contains(pointer) || overlay.is_some_and(|r| r.contains(pointer)) {
            return false;
        }
        let hit_member =
            super::interaction::hit_test_canvas_item(self, viewport, pointer).is_some();
        for group in self.groups().iter().rev() {
            let bounds = group.container.unrotated_bounds();
            let rect = egui::Rect::from_min_max(
                self.world_to_screen(bounds.min, viewport),
                self.world_to_screen(bounds.max, viewport),
            );
            let selected = self.interaction.selected_group_ids.contains(&group.id);
            let edges = [
                if (pointer.x - rect.left()).abs() < 6.0 {
                    -1
                } else if (pointer.x - rect.right()).abs() < 6.0 {
                    1
                } else {
                    0
                },
                if (pointer.y - rect.top()).abs() < 6.0 {
                    -1
                } else if (pointer.y - rect.bottom()).abs() < 6.0 {
                    1
                } else {
                    0
                },
            ];
            let resize = selected && rect.expand(6.0).contains(pointer) && edges != [0, 0];
            if resize
                && ui.input(|i| {
                    i.pointer
                        .button_double_clicked(egui::PointerButton::Primary)
                })
            {
                let g = group.id;
                let _ = self.group_edit(|s| {
                    for (a, edge) in edges.iter().enumerate() {
                        if *edge != 0 {
                            let result = s.hugAxis(g, a);
                            if result != crate::grouping::Outcome::Ok {
                                return result;
                            }
                        }
                    }
                    crate::grouping::Outcome::Ok
                });
                return true;
            }
            let scale = selected
                && ui.input(|i| i.modifiers.command && i.modifiers.alt)
                && rect.contains(pointer);
            let hit_group_member=super::interaction::hit_test_canvas_item(self,viewport,pointer).is_some_and(|hit|matches!(hit,super::CanvasHit::Annotation(m)|super::CanvasHit::Placement(m) if group.members.contains(&m)));
            let body = (!hit_member
                || (hit_group_member && self.interaction.editing_group_id != Some(group.id)))
                && (rect.contains(pointer)
                    || egui::Rect::from_min_max(rect.min - egui::vec2(0., 24.), rect.right_top())
                        .contains(pointer));
            if resize {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeNwSe);
            }
            if (resize || scale || body)
                && ui.input(|i| i.pointer.button_pressed(egui::PointerButton::Primary))
            {
                let group = group.clone();
                let members = group
                    .members
                    .iter()
                    .filter_map(|id| self.group_member_geometry(*id).map(|r| (*id, r)))
                    .collect();
                self.select_group(group.id);
                self.interaction.group_gesture = Some(GroupGesture {
                    group,
                    members,
                    start: self.screen_to_world(pointer, viewport),
                    kind: if scale {
                        GroupGestureKind::Scale
                    } else if resize {
                        GroupGestureKind::Resize(edges)
                    } else {
                        GroupGestureKind::Move
                    },
                });
                return true;
            }
        }
        false
    }
}
