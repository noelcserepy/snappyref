use super::CanvasState;
use crate::data::{Group, GroupFlow, GroupId, RectGeometry};
use crate::grouping::{AxisMode, Direction, Geometry};
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
    /// Clip crops a member's paint, text editor and hits to its container.
    pub(super) fn member_clip_screen_rect(&self, rect: egui::Rect, id: i64) -> Option<egui::Rect> {
        let group = self
            .groups()
            .iter()
            .find(|g| g.layout.clip && g.members.contains(&id))?;
        let bounds = group.container.unrotated_bounds();
        Some(egui::Rect::from_min_max(
            self.world_to_screen(bounds.min, rect),
            self.world_to_screen(bounds.max, rect),
        ))
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
        self.interaction.group_dock = crate::grouping::Dock {
            panel: crate::grouping::Panel::GroupDock { group: id },
            text_focus: false,
        };
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
        state.create(id, selected, bounds.width().max(1.0))?;
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
        state.board.groups.get_mut(&id).unwrap().name = group.name.clone();
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
        self.group_edit(|state| state.add(destination, ids))?;
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
            if old.flow != group.flow {
                s.set_flow(g, group.flow)?;
            }
            if old.gap != group.gap || old.layout.auto != group.layout.auto {
                s.set_gap(g, group.layout.auto, group.gap)?;
            }
            for a in 0..2 {
                // A resized axis is fixed at its new outer extent.
                let resized = old.container.scaled_size()[a] != group.container.scaled_size()[a];
                if resized || old.layout.fixed[a] != group.layout.fixed[a] {
                    let mode = if resized {
                        AxisMode::Fixed(group.container.scaled_size()[a])
                    } else if group.layout.fixed[a] {
                        AxisMode::Fixed(s.geometry.measured(&s.board, g, a))
                    } else {
                        AxisMode::Hug
                    };
                    s.set_axis_mode(g, a, mode)?;
                }
            }
            let stored = s.board.groups.get_mut(&g).unwrap();
            stored.clip = group.layout.clip;
            stored.align = group.layout.align;
            stored.padding = group.padding;
            stored.row_gap = group.layout.row_gap;
            s.refresh_sizes();
            Ok(())
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
                        let _ = s.ungroup(id);
                    }
                    Ok(())
                })
                .is_ok();
        self.interaction.selected_group_ids.clear();
        self.interaction.editing_group_id = None;
        changed
    }
    pub(super) fn remove_group_members(&mut self, ids: &[i64]) {
        let _ = self.group_edit(|s| {
            for &m in ids {
                let _ = s.delete_member(m);
            }
            Ok(())
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
            s.refresh_sizes();
            let _ = self.apply_group_state(s, None);
        }
    }
    pub(super) fn reflow_group_for_member(&mut self, id: i64) {
        if self.group_for_member(id).is_some() {
            let mut s = self.grouping_state();
            s.refresh_sizes();
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
                let outside = !group.container.unrotated_bounds().contains(rect.center);
                let slot = self.group_insertion_slot(g, m, rect.center);
                let _ = self.group_edit(|s| s.drop_member(g, m, slot, outside));
            } else if let Some(g) = self
                .groups()
                .iter()
                .rev()
                .find(|g| g.container.unrotated_bounds().contains(rect.center))
                .map(|g| g.id)
            {
                let slot = self.group_insertion_slot(g, m, rect.center);
                let _ = self.group_edit(|s| s.drag_in(g, m, slot));
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
    pub(super) fn group_insertion_slot(&self, g: i64, m: i64, point: egui::Pos2) -> i64 {
        let group = self.groups().iter().find(|x| x.id == g).unwrap();
        let mut order = group.members.iter().filter(|&&id| id != m);
        match self.group_insertion_anchor(group, m, point) {
            Some(anchor) => order
                .position(|&id| id == anchor)
                .expect("anchor is a member") as i64,
            None => order.count() as i64,
        }
    }
    /// The flowing member a drop at `point` lands before, or `None` past the
    /// end of flow. Absolute members sit outside flow, so they never decide it.
    fn group_insertion_anchor(&self, group: &Group, m: i64, point: egui::Pos2) -> Option<i64> {
        group
            .members
            .iter()
            .copied()
            .filter(|&id| id != m && !group_member_is_absolute(group, id))
            .find(|&id| {
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
    }
}

impl CanvasState {
    /// Selected Groups follow the selection. Input moves either every selected placement or, from
    /// a textbox, every selected textbox, so the first of each kind is a candidate reference.
    pub(super) fn group_transform_origin(&self) -> Vec<(i64, RectGeometry)> {
        if self.interaction.editing_group_id.is_some()
            || self.interaction.selected_group_ids.is_empty()
        {
            return Vec::new();
        }
        let placement = self.interaction.selected_placement_ids.first();
        let textbox = self
            .interaction
            .selected_annotation_ids
            .iter()
            .find(|&&id| self.group_member_geometry(id).is_some());
        [placement, textbox]
            .into_iter()
            .flatten()
            .filter_map(|&id| Some((id, self.group_member_geometry(id)?)))
            .collect()
    }
    pub(super) fn follow_group_member_transforms(&mut self, origin: Vec<(i64, RectGeometry)>) {
        let Some((reference, before, after)) = origin.into_iter().find_map(|(id, before)| {
            let after = self.group_member_geometry(id)?;
            (after.center != before.center || after.scale != before.scale)
                .then_some((id, before, after))
        }) else {
            return;
        };
        let moved_by_input = |state: &Self, m: i64| {
            if state.placement_for_id(reference).is_some() {
                state.is_selected(m)
            } else {
                state.is_annotation_selected(m)
            }
        };
        let factor = after.scale / before.scale;
        let follow = |center: egui::Pos2| after.center + (center - before.center) * factor;
        for id in self.selected_group_ids() {
            let Some(group) = self.document.groups.iter_mut().find(|g| g.id == id) else {
                continue;
            };
            group.container.center = follow(group.container.center);
            group.container.size *= factor;
            group.gap *= factor;
            group.layout.row_gap *= factor;
            group.padding *= factor;
            // Release persists the members, so a drag never records per-frame edits.
            for m in group.members.clone() {
                if moved_by_input(self, m) {
                    continue;
                }
                if let Some(p) = self
                    .document
                    .placements
                    .iter_mut()
                    .find(|p| p.state.id == m)
                {
                    p.state.geometry.center = follow(p.state.geometry.center);
                    p.state.geometry.scale *= factor;
                } else if let Some(a) = self.document.annotations.iter_mut().find(|a| a.id == m)
                    && let crate::data::CanvasAnnotationGeometry::Rect(r) = &mut a.geometry
                {
                    r.center = follow(r.center);
                    if factor != 1.0 {
                        r.size *= factor;
                        self.invalidate_textbox_layout_cache(m);
                    }
                }
            }
        }
        self.interaction.group_transform_changed = true;
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
    /// Reserved on open, so Create needs nothing from the app after confirm.
    create_id: GroupId,
}

impl CanvasState {
    pub(crate) fn group_picker_open(&self) -> bool {
        self.interaction.group_picker.is_some()
    }
    pub fn open_group_picker(&mut self, create_id: GroupId) {
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
            create_id,
        });
    }

    fn remember_group_destination(&mut self, id: GroupId) {
        self.interaction.last_group_destination = Some(id);
    }

    pub(super) fn show_group_picker(&mut self, ctx: &egui::Context) {
        use crate::ui::shortcut::{self, Key};
        use crate::ui::theme;
        let Some(mut picker) = self.interaction.group_picker.take() else {
            return;
        };
        let input_id = egui::Id::new("group-picker-query");
        let just_opened = picker.focus;
        let mut confirmed = None;
        let mut cancel = ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
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
                crate::ui::card::frame()
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
                            ui.spacing_mut().item_spacing.x = shortcut::HINT_GAP;
                            shortcut::hint(ui, &[Key::Up, Key::Down], "Choose");
                            shortcut::hint(ui, &[Key::Enter], "Confirm");
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
                    }
                }
                Some(GroupPickerChoice::Create) => {
                    if let Ok(Some(id)) = self.create_named_group(
                        picker.create_id,
                        picker.query.trim().to_owned(),
                        &picker.members,
                    ) {
                        self.remember_group_destination(id);
                    }
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
                let anchor = self.group_insertion_anchor(group, m, point);
                let last = group
                    .members
                    .iter()
                    .copied()
                    .rev()
                    .find(|&id| id != m && !group_member_is_absolute(group, id));
                if let Some(r) = anchor
                    .or(last)
                    .and_then(|id| self.group_member_geometry(id))
                {
                    let bounds = r.visual_bounds();
                    let at_end = anchor.is_none();
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
                egui::Stroke::new(
                    1.0_f32,
                    if self.interaction.editing_group_id == Some(group.id) {
                        color.gamma_multiply(0.55)
                    } else {
                        color
                    },
                ),
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
    pub(super) fn paint_edited_group_member(&self, painter: &egui::Painter, viewport: egui::Rect) {
        use crate::ui::theme;
        let dock = self.current_group_dock();
        if let Some(member) = dock.member()
            && let Some(geometry) = self.group_member_geometry(member)
        {
            let bounds = geometry.visual_bounds();
            let rect = egui::Rect::from_min_max(
                self.world_to_screen(bounds.min, viewport),
                self.world_to_screen(bounds.max, viewport),
            );
            painter.rect_stroke(
                rect,
                0,
                egui::Stroke::new(2.0_f32, theme::ACCENT),
                egui::StrokeKind::Inside,
            );
        }
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
        if !ui.ctx().egui_wants_keyboard_input() && self.current_group_dock().member().is_some() {
            for (key, direction) in [
                (egui::Key::ArrowLeft, Direction::Left),
                (egui::Key::ArrowRight, Direction::Right),
                (egui::Key::ArrowUp, Direction::Up),
                (egui::Key::ArrowDown, Direction::Down),
            ] {
                if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, key)) {
                    let _ = self.group_edit(|s| s.press_arrow(direction));
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
            && let Some(hit) = super::interaction::hit_test_member(self, viewport, p)
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
            && let Some(hit) = super::interaction::hit_test_member(self, viewport, p)
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
            for m in self
                .groups()
                .iter()
                .filter(|g| self.interaction.selected_group_ids.contains(&g.id))
                .flat_map(|g| g.members.clone())
                .collect::<Vec<_>>()
            {
                if self.placement_for_id(m).is_some() {
                    self.mark_dirty(m);
                } else {
                    self.mark_annotation_for_persistence(m);
                }
            }
        }
        if let Some(gesture) = self.interaction.group_gesture.clone() {
            if released
                && ui.input(|i| {
                    i.pointer
                        .button_double_clicked(egui::PointerButton::Primary)
                })
                && let GroupGestureKind::Resize(edges) = gesture.kind
            {
                let _ = self.group_edit(|s| {
                    if edges.iter().all(|&edge| edge != 0) {
                        s.double_click_corner()
                    } else {
                        s.double_click_handle(usize::from(edges[1] != 0))
                    }
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
                                group.container.size[a] =
                                    (group.container.size[a] + delta[a] * edges[a] as f32).max(1.0);
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
                let _ = self.group_edit(|s| {
                    if edges.iter().all(|&edge| edge != 0) {
                        s.double_click_corner()
                    } else {
                        s.double_click_handle(usize::from(edges[1] != 0))
                    }
                });
                return true;
            }
            let scale = selected
                && ui.input(|i| i.modifiers.command && i.modifiers.alt)
                && rect.contains(pointer);
            // Alt pans and Shift extends selection, so both fall through to canvas input.
            let (alt, shift) = ui.input(|i| (i.modifiers.alt, i.modifiers.shift));
            let hit_group_member = !shift
                && super::interaction::hit_test_canvas_item(self, viewport, pointer).is_some_and(
                    |hit| matches!(hit, super::CanvasHit::Annotation(m) | super::CanvasHit::Placement(m) if group.members.contains(&m)),
                );
            // Pressing a member of a wider selection drags that selection through canvas input.
            let wider_selection = selected
                && (self.interaction.selected_group_ids.len() > 1
                    || self
                        .interaction
                        .selected_placement_ids
                        .iter()
                        .chain(&self.interaction.selected_annotation_ids)
                        .any(|id| !group.members.contains(id)));
            let body = !alt
                && (!hit_member
                    || (hit_group_member
                        && !wider_selection
                        && self.interaction.editing_group_id != Some(group.id)))
                && (rect.contains(pointer)
                    || egui::Rect::from_min_max(rect.min - egui::vec2(0., 24.), rect.right_top())
                        .contains(pointer));
            if resize {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeNwSe);
            }
            if (resize || scale || body)
                && ui.input(|i| i.pointer.button_pressed(egui::PointerButton::Primary))
            {
                if scale && !self.current_group_dock().group_shown() {
                    return true;
                }
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

fn group_member_is_absolute(group: &Group, id: i64) -> bool {
    group
        .layout
        .members
        .get(&id)
        .is_some_and(|mode| mode.absolute)
}
