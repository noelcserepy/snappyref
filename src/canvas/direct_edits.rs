//! Edits addressed by id, for callers that hold no selection, such as app
//! control. Each one marks what it changed, so the caller submits one
//! completed edit when it is done.
use super::CanvasState;
use crate::data::{CanvasAnnotation, GroupId, RectGeometry};
use eframe::egui;

pub(crate) struct Selection {
    placements: Vec<i64>,
    annotations: Vec<i64>,
    groups: Vec<GroupId>,
    editing_group: Option<GroupId>,
}

impl CanvasState {
    /// Geometry of an image, label or rectangle.
    pub(crate) fn rect_item_geometry(&self, id: i64) -> Option<RectGeometry> {
        self.group_member_geometry(id)
    }

    /// A Group that flows its members puts this one back in the flow.
    pub(crate) fn set_rect_item_geometry(&mut self, id: i64, geometry: RectGeometry) {
        self.set_group_member_geometry(id, geometry);
        self.invalidate_textbox_layout_cache(id);
        self.reflow_group_for_member(id);
    }

    pub(crate) fn set_segment_points(&mut self, id: i64, start: egui::Pos2, end: egui::Pos2) {
        if self.set_annotation_world_points(id, start, end) {
            self.mark_annotation_dirty(id);
        }
    }

    /// Returns false when no annotation has this id.
    pub(crate) fn edit_annotation(
        &mut self,
        ctx: &egui::Context,
        id: i64,
        edit: impl FnOnce(&mut CanvasAnnotation),
    ) -> bool {
        let Some(index) = self.document.annotations.iter().position(|a| a.id == id) else {
            return false;
        };
        let before = self.document.annotations[index].clone();
        edit(&mut self.document.annotations[index]);
        if self.document.annotations[index] != before {
            self.invalidate_textbox_layout_cache(id);
            self.mark_annotation_dirty(id);
            // Paint would fit a label to its new text a frame later, as a second edit.
            super::render::fit_label_to_text(ctx, self, index);
            self.reflow_group_for_member(id);
        }
        true
    }

    pub(crate) fn delete_items(&mut self, ids: &[i64]) {
        self.delete_placements(ids);
        for &id in ids {
            if self.annotation_by_id(id).is_some() {
                self.remove_annotation_locally(id);
            }
        }
    }

    /// Moves a Group's frame and its members together.
    pub(crate) fn move_group(&mut self, id: GroupId, delta: egui::Vec2) {
        let mut groups = self.groups().to_vec();
        let Some(group) = groups.iter_mut().find(|group| group.id == id) else {
            return;
        };
        group.container.center += delta;
        let members = group.members.clone();
        self.replace_groups(groups);
        for member in members {
            if let Some(mut geometry) = self.group_member_geometry(member) {
                geometry.center += delta;
                self.set_group_member_geometry(member, geometry);
            }
        }
    }

    pub(crate) fn dissolve_group(&mut self, id: GroupId) -> Result<(), String> {
        self.group_edit(|state| state.ungroup(id))?;
        self.interaction.selected_group_ids.retain(|g| *g != id);
        if self.interaction.editing_group_id == Some(id) {
            self.interaction.editing_group_id = None;
        }
        Ok(())
    }

    pub(crate) fn remove_from_group(&mut self, member: i64) -> Result<(), String> {
        let group = self.group_for_member(member).ok_or("not_in_group")?;
        self.group_edit(|state| state.detach(group, member))
    }

    /// Whether the user is dragging, drawing, cropping or typing. An edit by
    /// id would cut that short, so its caller waits.
    pub(crate) fn user_edit_in_progress(&self) -> bool {
        self.interaction.gesture.is_some()
            || self.interaction.group_gesture.is_some()
            || self.interaction.editing_text_annotation_id.is_some()
    }

    /// What the user has selected, to put back after edits that borrow the
    /// selection.
    pub(crate) fn selection(&self) -> Selection {
        Selection {
            placements: self.interaction.selected_placement_ids.clone(),
            annotations: self.interaction.selected_annotation_ids.clone(),
            groups: self.interaction.selected_group_ids.clone(),
            editing_group: self.interaction.editing_group_id,
        }
    }

    /// Whatever the edits removed drops out of the selection.
    pub(crate) fn restore_selection(&mut self, selection: Selection) {
        let exists = |id: &GroupId| self.groups().iter().any(|group| group.id == *id);
        let groups = selection.groups.into_iter().filter(exists).collect();
        let editing_group = selection.editing_group.filter(exists);
        self.select_content(&selection.placements, &selection.annotations);
        self.interaction.selected_group_ids = groups;
        self.interaction.editing_group_id = editing_group;
    }

    /// Selects items and Groups by id. Ids that name nothing are ignored.
    pub(crate) fn select_ids(&mut self, ids: &[i64]) {
        self.select_content(ids, ids);
        self.interaction.selected_group_ids = ids
            .iter()
            .copied()
            .filter(|id| self.groups().iter().any(|group| group.id == *id))
            .collect();
    }
}
