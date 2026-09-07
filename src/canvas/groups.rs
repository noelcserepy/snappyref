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
        self.interaction.selected_placement_ids = group.members;
        self.interaction.selected_annotation_ids.clear();
        self.interaction.editing_group_id = None;
    }
    pub(super) fn expand_group_selection(&mut self) {
        if self.interaction.editing_group_id.is_some() {
            return;
        }
        let selected = self.interaction.selected_placement_ids.clone();
        for group in self.document.groups.iter() {
            if group.members.iter().any(|id| selected.contains(id)) {
                if !self.interaction.selected_group_ids.contains(&group.id) {
                    self.interaction.selected_group_ids.push(group.id);
                }
                for id in &group.members {
                    if !self.interaction.selected_placement_ids.contains(id) {
                        self.interaction.selected_placement_ids.push(*id);
                    }
                }
            }
        }
    }
    pub fn create_group(&mut self, id: GroupId) -> Result<Option<GroupId>, String> {
        let members: Vec<_> = self
            .selected_placement_ids()
            .into_iter()
            .filter(|id| {
                self.placement_for_id(*id)
                    .is_some_and(|p| p.lifecycle == crate::data::PlacementLifecycle::Committed)
            })
            .collect();
        if members.is_empty() {
            return Ok(None);
        }
        let bounds = members
            .iter()
            .filter_map(|id| self.placement_for_id(*id))
            .map(|p| p.state.geometry.visual_bounds())
            .reduce(|a, b| a.union(b))
            .unwrap();
        let padding = self.placement_gap;
        let size = egui::vec2(
            bounds.width().max(1.) + padding * 2.,
            bounds.height().max(1.) + padding * 2.,
        );
        let group = Group {
            id,
            name: format!("Group {}", self.groups().len() + 1),
            members: Vec::new(),
            container: RectGeometry::new(bounds.center(), size, 1., 0., 0),
            flow: GroupFlow::Wrap,
            spacing: self.placement_gap,
            padding,
        };
        let before = self.document.groups.clone();
        self.document.groups.push(group);
        if let Err(error) = self.add_members_to_group(id, &members) {
            self.document.groups = before;
            return Err(error);
        }
        self.select_group(id);
        Ok(Some(id))
    }
    pub fn add_members_to_group(
        &mut self,
        destination: GroupId,
        ids: &[i64],
    ) -> Result<bool, String> {
        let Some(target) = self.groups().iter().find(|g| g.id == destination) else {
            return Err("Group no longer exists".into());
        };
        let incoming: Vec<_> = ids
            .iter()
            .copied()
            .filter(|id| !target.members.contains(id))
            .fold(Vec::new(), |mut out, id| {
                if !out.contains(&id) {
                    out.push(id);
                }
                out
            });
        if incoming.is_empty() {
            return Ok(false);
        }
        if incoming.iter().any(|id| {
            self.placement_for_id(*id)
                .is_none_or(|p| p.lifecycle != crate::data::PlacementLifecycle::Committed)
        }) {
            return Err("Only existing Board placements can be grouped".into());
        }
        let mut groups = self.document.groups.clone();
        for group in &mut groups {
            if group.id == destination {
                group.members.extend(&incoming);
            } else {
                group.members.retain(|id| !incoming.contains(id));
            }
        }
        self.apply_group_layouts(groups, &incoming, Some(destination))?;
        Ok(true)
    }
    fn apply_group_layouts(
        &mut self,
        mut groups: Vec<Group>,
        incoming: &[i64],
        destination: Option<GroupId>,
    ) -> Result<(), String> {
        let mut layouts = Vec::new();
        for group in &mut groups {
            if self.groups().iter().any(|old| old == group) {
                continue;
            }
            let members = group
                .members
                .iter()
                .map(|id| {
                    self.placement_for_id(*id)
                        .map(|p| (*id, p.state.geometry))
                        .ok_or_else(|| "Missing Group member".to_owned())
                })
                .collect::<Result<Vec<_>, _>>()?;
            let layout = crate::placement::group_layout::layout_group(
                group,
                &members,
                if destination == Some(group.id) {
                    incoming
                } else {
                    &[]
                },
            )
            .map_err(|e| format!("Invalid Group layout: {e:?}"))?;
            group.container = layout.container;
            layouts.extend(layout.members);
        }
        for (id, geometry) in layouts {
            if let Some(image) = self
                .document
                .placements
                .iter_mut()
                .find(|p| p.state.id == id)
            {
                image.state.geometry = geometry;
            }
            self.mark_dirty(id);
        }
        self.replace_groups(groups);
        Ok(())
    }
    pub fn edit_group(&mut self, group: Group) -> Result<(), String> {
        let mut groups = self.document.groups.clone();
        let Some(existing) = groups.iter_mut().find(|g| g.id == group.id) else {
            return Err("Group no longer exists".into());
        };
        let old_members: std::collections::HashSet<_> = existing.members.iter().copied().collect();
        let new_members: std::collections::HashSet<_> = group.members.iter().copied().collect();
        if old_members != new_members || new_members.len() != group.members.len() {
            return Err("Use Add to group to change membership".into());
        }
        *existing = group;
        self.apply_group_layouts(groups, &[], None)
    }
    pub fn ungroup_selected(&mut self) -> bool {
        let selected = self.selected_group_ids();
        let mut groups = self.document.groups.clone();
        groups.retain(|g| !selected.contains(&g.id));
        let changed = groups.len() != self.groups().len();
        self.replace_groups(groups);
        self.interaction.selected_group_ids.clear();
        self.interaction.editing_group_id = None;
        changed
    }
    pub(super) fn remove_group_members(&mut self, ids: &[i64]) {
        let mut groups = self.document.groups.clone();
        for group in &mut groups {
            group.members.retain(|id| !ids.contains(id));
        }
        let _ = self.apply_group_layouts(groups, &[], None);
    }
    pub(crate) fn reflow_dirty_group_members(&mut self) {
        if self.document.groups_dirty {
            return;
        }
        let dirty = self.document.dirty_placement_ids.clone();
        let representatives = self
            .groups()
            .iter()
            .filter_map(|group| group.members.iter().find(|id| dirty.contains(id)).copied())
            .collect::<Vec<_>>();
        for id in representatives {
            self.reflow_group_for_member(id);
        }
    }
    pub(super) fn reflow_group_for_member(&mut self, id: i64) {
        let Some(group_id) = self.group_for_member(id) else {
            return;
        };
        let group = self
            .groups()
            .iter()
            .find(|g| g.id == group_id)
            .unwrap()
            .clone();
        let members = group
            .members
            .iter()
            .filter_map(|id| self.placement_for_id(*id).map(|p| (*id, p.state.geometry)))
            .collect::<Vec<_>>();
        let Ok(layout) = crate::placement::group_layout::layout_group(&group, &members, &[]) else {
            return;
        };
        for (id, geometry) in layout.members {
            if let Some(image) = self
                .document
                .placements
                .iter_mut()
                .find(|p| p.state.id == id)
            {
                image.state.geometry = geometry;
            }
            self.mark_dirty(id);
        }
        if layout.container != group.container {
            self.document
                .groups
                .iter_mut()
                .find(|g| g.id == group_id)
                .unwrap()
                .container = layout.container;
            self.document.groups_dirty = true;
        }
    }
    pub(super) fn finish_group_member_drag(&mut self, id: i64) {
        let Some(group_id) = self.interaction.editing_group_id else {
            return;
        };
        let Some(mut group) = self.groups().iter().find(|g| g.id == group_id).cloned() else {
            return;
        };
        let Some(placement) = self.placement_for_id(id) else {
            return;
        };
        let point = placement.state.geometry.center;
        group.members.retain(|member| *member != id);
        let position = group
            .members
            .iter()
            .position(|member| {
                self.placement_for_id(*member).is_some_and(|p| {
                    let other = p.state.geometry.center;
                    match group.flow {
                        GroupFlow::Row => point.x < other.x,
                        GroupFlow::Column => point.y < other.y,
                        GroupFlow::Wrap => {
                            point.y < other.y - p.state.geometry.scaled_size().y / 2.
                                || ((point.y - other.y).abs()
                                    < p.state.geometry.scaled_size().y / 2.
                                    && point.x < other.x)
                        }
                    }
                })
            })
            .unwrap_or(group.members.len());
        group.members.insert(position, id);
        let _ = self.edit_group(group);
        self.reflow_group_for_member(id);
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
            group.spacing *= factor;
            group.padding *= factor;
            self.interaction.group_transform_changed = true;
        }
    }
}

pub(super) struct GroupPicker {
    query: String,
    highlighted: Option<GroupId>,
    focus: bool,
    members: Vec<i64>,
}

impl CanvasState {
    pub fn open_group_picker(&mut self) {
        if self.selected_placement_ids().is_empty() {
            return;
        }
        self.interaction.group_picker = Some(GroupPicker {
            query: String::new(),
            highlighted: self
                .interaction
                .last_group_destination
                .filter(|id| self.groups().iter().any(|g| g.id == *id))
                .or_else(|| self.groups().first().map(|g| g.id)),
            focus: true,
            members: self.selected_placement_ids(),
        });
    }
    pub(super) fn show_group_picker(&mut self, ctx: &egui::Context) {
        let Some(mut picker) = self.interaction.group_picker.take() else {
            return;
        };
        let mut confirmed = None;
        let mut cancel = ctx.input(|i| i.key_pressed(egui::Key::Escape));
        egui::Window::new("Add to group")
            .id(egui::Id::new("group-picker"))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.set_width(280.);
                ui.label("Find a group");
                let search = ui.text_edit_singleline(&mut picker.query);
                if picker.focus {
                    search.request_focus();
                    picker.focus = false;
                }
                let choices: Vec<_> = self
                    .groups()
                    .iter()
                    .filter(|g| g.name.to_lowercase().contains(&picker.query.to_lowercase()))
                    .collect();
                if !choices.iter().any(|g| Some(g.id) == picker.highlighted) {
                    picker.highlighted = choices.first().map(|g| g.id);
                }
                if !choices.is_empty() {
                    let current = choices
                        .iter()
                        .position(|g| Some(g.id) == picker.highlighted)
                        .unwrap_or(0);
                    if ctx.input(|i| i.key_pressed(egui::Key::ArrowDown)) {
                        picker.highlighted = Some(choices[(current + 1) % choices.len()].id);
                    }
                    if ctx.input(|i| i.key_pressed(egui::Key::ArrowUp)) {
                        picker.highlighted =
                            Some(choices[(current + choices.len() - 1) % choices.len()].id);
                    }
                }
                egui::ScrollArea::vertical()
                    .max_height(240.)
                    .show(ui, |ui| {
                        for group in choices {
                            if ui
                                .selectable_label(picker.highlighted == Some(group.id), &group.name)
                                .clicked()
                            {
                                confirmed = Some(group.id);
                            }
                        }
                    });
                if ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
                    confirmed = picker.highlighted;
                }
                if ui.button("Cancel").clicked() {
                    cancel = true;
                }
            });
        if let Some(id) = confirmed {
            if self.add_members_to_group(id, &picker.members).is_ok() {
                self.interaction.last_group_destination = Some(id);
                self.select_group(id);
            }
        } else if !cancel {
            self.interaction.group_picker = Some(picker);
        }
    }
    pub(super) fn paint_groups(&self, painter: &egui::Painter, viewport: egui::Rect) {
        use crate::ui::theme;
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
                egui::Stroke::new(1., color),
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
        {
            if self
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
        }
        let id = *self.interaction.selected_group_ids.first()?;
        if self.interaction.selected_group_ids.len() != 1 {
            return None;
        }
        let mut group = self.groups().iter().find(|g| g.id == id)?.clone();
        let origin = self.world_to_screen(group.container.unrotated_bounds().min, viewport);
        let position = egui::pos2(
            origin.x.clamp(
                viewport.left() + theme::SPACE_3,
                (viewport.right() - 460.).max(viewport.left() + theme::SPACE_3),
            ),
            (origin.y - 90.).max(viewport.top() + theme::SPACE_3),
        );
        let mut changed = false;
        let response = egui::Area::new(egui::Id::new(("group-controls", id)))
            .fixed_pos(position)
            .order(egui::Order::Foreground)
            .show(ui.ctx(), |ui| {
                egui::Frame::new()
                    .fill(theme::SURFACE)
                    .corner_radius(theme::RADIUS_PANEL)
                    .inner_margin(theme::SPACE_2)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label("Name");
                            let draft_id = egui::Id::new(("group-name-draft", id));
                            let mut name = ui
                                .ctx()
                                .data(|data| data.get_temp::<String>(draft_id))
                                .unwrap_or_else(|| group.name.clone());
                            let name_response =
                                ui.add(egui::TextEdit::singleline(&mut name).desired_width(160.));
                            if name_response.has_focus() {
                                ui.ctx()
                                    .data_mut(|data| data.insert_temp(draft_id, name.clone()));
                            }
                            if name_response.lost_focus() {
                                if !name.trim().is_empty() && group.name != name.trim() {
                                    group.name = name.trim().to_owned();
                                    changed = true;
                                }
                                ui.ctx().data_mut(|data| data.remove::<String>(draft_id));
                            }
                            for (flow, label) in [
                                (GroupFlow::Row, "Row"),
                                (GroupFlow::Column, "Column"),
                                (GroupFlow::Wrap, "Wrap"),
                            ] {
                                changed |=
                                    ui.selectable_value(&mut group.flow, flow, label).changed();
                            }
                        });
                        ui.horizontal(|ui| {
                            ui.label("Spacing");
                            changed |= ui
                                .add(
                                    egui::DragValue::new(&mut group.spacing)
                                        .range(0.0..=1000.0)
                                        .speed(1.),
                                )
                                .changed();
                            ui.label("Padding");
                            changed |= ui
                                .add(
                                    egui::DragValue::new(&mut group.padding)
                                        .range(0.0..=(group.container.size.x / 2. - 1.).max(0.))
                                        .speed(1.),
                                )
                                .changed();
                            let mut width = group.container.size.x;
                            ui.label("Width");
                            if ui
                                .add(
                                    egui::DragValue::new(&mut width)
                                        .range((group.padding * 2. + 1.)..=100000.)
                                        .speed(2.),
                                )
                                .changed()
                            {
                                group.container.center.x += (width - group.container.size.x) / 2.;
                                group.container.size.x = width;
                                changed = true;
                            }
                        });
                    });
            });
        if changed {
            let before = self.groups().iter().find(|g| g.id == id).cloned();
            if self.edit_group(group).is_ok() && ui.input(|i| i.pointer.primary_down()) {
                if self.interaction.group_controls_start.is_none() {
                    self.interaction.group_controls_start = before;
                }
                self.document.groups_dirty = false;
                if let Some(before) = &self.interaction.group_controls_start {
                    self.document
                        .dirty_placement_ids
                        .retain(|id| !before.members.contains(id));
                }
            }
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
    Resize,
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
        let pointer = ui.input(|i| i.pointer.latest_pos());
        let released = ui.input(|i| i.pointer.button_released(egui::PointerButton::Primary));
        if released && self.interaction.group_transform_changed {
            self.document.groups_dirty = true;
            self.interaction.group_transform_changed = false;
        }
        if let Some(gesture) = self.interaction.group_gesture.clone() {
            if let Some(pointer) = pointer {
                let delta = self.screen_to_world(pointer, viewport) - gesture.start;
                let mut group = gesture.group.clone();
                match gesture.kind {
                    GroupGestureKind::Resize => {
                        let width = (group.container.size.x + delta.x).max(group.padding * 2. + 1.);
                        group.container.center.x += (width - group.container.size.x) / 2.;
                        group.container.size.x = width;
                        let _ = self.edit_group(group);
                    }
                    GroupGestureKind::Move => {
                        group.container.center += delta;
                        for (id, mut geometry) in gesture.members.clone() {
                            geometry.center += delta;
                            if let Some(image) = self
                                .document
                                .placements
                                .iter_mut()
                                .find(|p| p.state.id == id)
                            {
                                image.state.geometry = geometry;
                            }
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
                            group.spacing = layout.spacing;
                            group.padding = layout.padding;
                            for (id, geometry) in layout.members {
                                if let Some(image) = self
                                    .document
                                    .placements
                                    .iter_mut()
                                    .find(|p| p.state.id == id)
                                {
                                    image.state.geometry = geometry;
                                }
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
                        self.mark_dirty(id);
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
            let resize = selected && pointer.distance(rect.right_bottom()) < 12.;
            let scale = selected
                && ui.input(|i| i.modifiers.command && i.modifiers.alt)
                && rect.contains(pointer);
            let body = !hit_member
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
                    .filter_map(|id| self.placement_for_id(*id).map(|p| (*id, p.state.geometry)))
                    .collect();
                self.select_group(group.id);
                self.interaction.group_gesture = Some(GroupGesture {
                    group,
                    members,
                    start: self.screen_to_world(pointer, viewport),
                    kind: if scale {
                        GroupGestureKind::Scale
                    } else if resize {
                        GroupGestureKind::Resize
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
