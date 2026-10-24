use std::sync::Arc;

use eframe::egui;

use super::geometry::{
    clamped_crop_screen_rect, crop_center_offset_local, min_loaded_placement_scale,
    pixel_crop_rect_from_local, screen_rect_to_local_crop_rect,
};
use super::interaction::{normalized_textbox_rect, placement_screen_rect, textbox_world_rect};
use super::render::{layout_textbox_galley, textbox_layout_cache_key};
use super::{
    CanvasCropRequest, CanvasDocumentChanges, CanvasState, CropState, TextboxLayoutCacheEntry,
};
use crate::data::{
    CanvasAnnotation, CanvasAnnotationColor, CanvasAnnotationKind, CanvasRectangleFill,
    CanvasStackItem, CanvasStrokeWidth, CanvasTextSize, GroupId, Placement, PlacementLifecycle,
};
use crate::placement::arrangement::PlacementArrangementInput;

#[derive(Clone, Copy)]
enum StackEnd {
    Front,
    Back,
}

impl CanvasState {
    pub fn add_provisional_unresolved_placement(&mut self, mut placement: Placement) {
        placement.lifecycle = PlacementLifecycle::Provisional;
        self.add_unresolved_placement(placement);
    }

    pub fn next_provisional_placement_id(&self) -> i64 {
        self.document
            .placements
            .iter()
            .filter_map(|image| (image.state.id < 0).then_some(image.state.id))
            .min()
            .unwrap_or(0)
            - 1
    }

    pub(crate) fn has_provisional_placements(&self) -> bool {
        self.document
            .placements
            .iter()
            .any(|image| image.lifecycle == PlacementLifecycle::Provisional)
    }

    pub fn remove_provisional_placements(&mut self) -> Vec<i64> {
        let ids = self
            .document
            .placements
            .iter()
            .filter_map(|image| {
                (image.lifecycle == PlacementLifecycle::Provisional).then_some(image.state.id)
            })
            .collect::<Vec<_>>();
        self.delete_placements(&ids)
    }

    pub fn commit_provisional_placements_in_place(
        &mut self,
        committed: Vec<(i64, Placement)>,
    ) -> Vec<Placement> {
        let mut committed_placements = Vec::with_capacity(committed.len());
        for (provisional_id, mut placement) in committed {
            placement.lifecycle = PlacementLifecycle::Committed;
            let committed_id = placement.state.id;
            if let Some(image) = self
                .document
                .placements
                .iter_mut()
                .find(|image| image.state.id == provisional_id)
            {
                *image = placement.clone();
            }
            if let Some(image) = self.render.placement_images.remove(&provisional_id) {
                self.render.placement_images.insert(committed_id, image);
            }
            if let Some(load) = self.render.image_lod_loads.remove(&provisional_id) {
                self.render.image_lod_loads.insert(committed_id, load);
            }
            self.render
                .failed_image_lod_loads
                .retain(|(id, _)| *id != provisional_id);
            for selected_id in &mut self.interaction.selected_placement_ids {
                if *selected_id == provisional_id {
                    *selected_id = committed_id;
                }
            }
            for item in &mut self.document.stack_order {
                if *item == CanvasStackItem::Placement(provisional_id) {
                    *item = CanvasStackItem::Placement(committed_id);
                }
            }
            committed_placements.push(placement);
        }
        committed_placements
    }

    pub fn provisional_placements_in_selection_order(&self) -> Vec<Placement> {
        self.interaction
            .selected_placement_ids
            .iter()
            .filter_map(|placement_id| self.placement_for_id(*placement_id))
            .filter(|placement| placement.lifecycle == PlacementLifecycle::Provisional)
            .collect()
    }

    pub fn all_placement_ids(&self) -> Vec<i64> {
        self.document
            .placements
            .iter()
            .map(|image| image.state.id)
            .collect()
    }

    pub fn all_annotation_ids(&self) -> Vec<i64> {
        self.document
            .annotations
            .iter()
            .map(|annotation| annotation.id)
            .collect()
    }

    pub(crate) fn committed_placements(&self) -> Vec<Placement> {
        self.document
            .placements
            .iter()
            .filter(|image| image.lifecycle == PlacementLifecycle::Committed)
            .cloned()
            .collect()
    }

    pub(crate) fn annotations(&self) -> Vec<CanvasAnnotation> {
        self.document.annotations.clone()
    }

    pub fn annotation_for_id(&self, annotation_id: i64) -> Option<CanvasAnnotation> {
        self.document
            .annotations
            .iter()
            .find(|annotation| annotation.id == annotation_id)
            .cloned()
    }

    pub fn placement_for_id(&self, placement_id: i64) -> Option<Placement> {
        self.document
            .placements
            .iter()
            .find(|image| image.state.id == placement_id)
            .cloned()
    }

    pub fn compact_guided_layout_for_sizes(
        &mut self,
        sizes: &[egui::Vec2],
        center_world: egui::Pos2,
        screen_rect: egui::Rect,
        gap: f32,
    ) -> Vec<(egui::Pos2, egui::Vec2)> {
        let mut arrangement_inputs =
            crate::placement::arrangement::compact_layout_for_sizes(sizes, gap, center_world);
        self.guide_snap_arrangement_inputs(&mut arrangement_inputs, screen_rect);
        arrangement_inputs
    }

    pub fn balanced_guided_layout_for_sizes(
        &mut self,
        sizes: &[egui::Vec2],
        center_world: egui::Pos2,
        screen_rect: egui::Rect,
        gap: f32,
    ) -> Vec<(egui::Pos2, egui::Vec2)> {
        let mut arrangement_inputs =
            crate::placement::arrangement::balanced_grid_layout_for_sizes(sizes, gap, center_world);
        self.guide_snap_arrangement_inputs(&mut arrangement_inputs, screen_rect);
        arrangement_inputs
    }

    fn guide_snap_arrangement_inputs(
        &mut self,
        arrangement_inputs: &mut [(egui::Pos2, egui::Vec2)],
        screen_rect: egui::Rect,
    ) {
        let items = arrangement_inputs
            .iter()
            .enumerate()
            .map(|(index, (center, size))| {
                PlacementArrangementInput::new(
                    -((index as i64) + 1),
                    egui::pos2(center.x, center.y),
                    egui::vec2(size.x, size.y),
                )
            })
            .collect::<Vec<_>>();
        let Some(bounds) = crate::canvas::interaction::movement::layout_bounds_with_offset(
            &items,
            egui::Vec2::ZERO,
        ) else {
            self.clear_guide_snap_indicators();
            return;
        };
        let Some(snap_result) = crate::canvas::interaction::movement::compute_guide_snap_for_rect(
            self,
            screen_rect,
            bounds,
            &[],
            false,
        ) else {
            self.clear_guide_snap_indicators();
            return;
        };
        let snap_offset = snap_result.rect.center() - bounds.center();
        if snap_offset == egui::Vec2::ZERO {
            self.clear_guide_snap_indicators();
            return;
        }
        self.set_guide_snap_indicators(snap_result.indicators);
        for (center, _) in arrangement_inputs {
            *center += snap_offset;
        }
    }

    pub fn add_annotation(&mut self, annotation: CanvasAnnotation) {
        self.invalidate_textbox_layout_cache(annotation.id);
        self.document
            .annotations
            .retain(|item| item.id != annotation.id);
        self.document.annotations.push(annotation.clone());
        self.append_missing_stack_item(CanvasStackItem::Annotation(annotation.id));
        if annotation.kind == CanvasAnnotationKind::Textbox
            && self.interaction.editing_text_annotation_id.is_none()
        {
            self.interaction.selected_annotation_ids = vec![annotation.id];
        }
    }

    pub(crate) fn mark_placement_for_persistence(&mut self, placement_id: i64) {
        if !self.document.dirty_placement_ids.contains(&placement_id) {
            self.document.dirty_placement_ids.push(placement_id);
        }
    }

    pub(crate) fn mark_annotation_for_persistence(&mut self, annotation_id: i64) {
        if !self.document.dirty_annotation_ids.contains(&annotation_id) {
            self.document.dirty_annotation_ids.push(annotation_id);
        }
    }

    pub(crate) fn mark_stack_order_for_persistence(&mut self) {
        self.document.stack_order_dirty = true;
    }

    pub fn apply_annotation_history_state(&mut self, annotation: Option<CanvasAnnotation>) {
        let Some(annotation) = annotation else {
            return;
        };
        if annotation.archived {
            self.remove_annotation_locally(annotation.id);
        } else {
            self.add_annotation(annotation);
        }
    }

    pub fn apply_placement_history_state(
        &mut self,
        placement: Option<Placement>,
    ) -> Option<Placement> {
        let placement = placement?;
        if placement.state.archived {
            self.delete_placements(&[placement.state.id]);
            return None;
        }
        if let Some(image) = self.placement_by_id_mut(placement.state.id) {
            let content_changed = image.asset_id() != placement.asset_id();
            if content_changed {
                self.add_unresolved_placement(placement.clone());
                return Some(placement);
            }
            *image = placement;
            self.document
                .placements
                .sort_by_key(|image| image.state.geometry.z_index);
            return None;
        }
        if placement.is_loaded() {
            if self
                .add_loaded_reference_placement(placement.clone())
                .is_ok()
            {
                return None;
            }
            self.add_unresolved_placement(placement.clone());
            return Some(placement);
        }
        self.add_unresolved_placement(placement);
        None
    }

    pub fn set_canvas_stack_order(&mut self, stack_order: Vec<CanvasStackItem>) {
        if self.document.stack_order != stack_order {
            self.document.stack_order = stack_order;
            self.document.stack_order_dirty = true;
        }
    }

    pub fn restore_canvas_stack_order(&mut self, stack_order: Vec<CanvasStackItem>) {
        self.document.stack_order = stack_order;
        self.document.stack_order_dirty = false;
    }

    pub fn canvas_stack_order(&self) -> Vec<CanvasStackItem> {
        self.ordered_canvas_items_back_to_front()
    }

    pub fn clear_for_board_switch(&mut self) {
        self.finish_textbox_edit();
        self.document.placements.clear();
        self.render.placement_images.clear();
        self.document.annotations.clear();
        self.document.stack_order.clear();
        self.restore_groups(Vec::new());
        self.interaction.selected_placement_ids.clear();
        self.interaction.selected_annotation_ids.clear();
        self.document.dirty_placement_ids.clear();
        self.document.pending_archived_placement_ids.clear();
        self.document.dirty_annotation_ids.clear();
        self.document.pending_archived_annotation_ids.clear();
        self.document.stack_order_dirty = false;
        self.interaction = Default::default();
        self.render.asset_rgba_cache.clear();
        self.render.textbox_layout_cache.clear();
        self.render.image_lod_loads.clear();
        self.render.failed_image_lod_loads.clear();
    }

    pub fn take_document_changes(&mut self) -> CanvasDocumentChanges {
        let dirty_ids = std::mem::take(&mut self.document.dirty_placement_ids);
        let updated_placements = dirty_ids
            .into_iter()
            .filter_map(|id| {
                self.document
                    .placements
                    .iter()
                    .find(|image| {
                        image.state.id == id && image.lifecycle == PlacementLifecycle::Committed
                    })
                    .cloned()
            })
            .collect();

        let dirty_ids = std::mem::take(&mut self.document.dirty_annotation_ids);
        let updated_annotations = dirty_ids
            .into_iter()
            .filter_map(|id| {
                self.document
                    .annotations
                    .iter()
                    .find(|annotation| annotation.id == id)
                    .cloned()
            })
            .collect();

        let stack_order = self
            .document
            .stack_order_dirty
            .then(|| self.committed_canvas_items_back_to_front());
        self.document.stack_order_dirty = false;
        let groups = self
            .document
            .groups_dirty
            .then(|| self.document.groups.clone());
        self.document.groups_dirty = false;

        CanvasDocumentChanges {
            groups,
            updated_placements,
            archived_placement_ids: std::mem::take(
                &mut self.document.pending_archived_placement_ids,
            ),
            updated_annotations,
            archived_annotation_ids: std::mem::take(
                &mut self.document.pending_archived_annotation_ids,
            ),
            stack_order,
        }
    }

    pub fn pending_document_changes_preview(&self) -> CanvasDocumentChanges {
        let updated_placements = self
            .document
            .dirty_placement_ids
            .iter()
            .filter_map(|id| {
                self.document
                    .placements
                    .iter()
                    .find(|image| {
                        image.state.id == *id && image.lifecycle == PlacementLifecycle::Committed
                    })
                    .cloned()
            })
            .collect();

        let updated_annotations = self
            .document
            .dirty_annotation_ids
            .iter()
            .filter_map(|id| {
                self.document
                    .annotations
                    .iter()
                    .find(|annotation| annotation.id == *id)
                    .cloned()
            })
            .collect();

        CanvasDocumentChanges {
            groups: self
                .document
                .groups_dirty
                .then(|| self.document.groups.clone()),
            updated_placements,
            archived_placement_ids: self.document.pending_archived_placement_ids.clone(),
            updated_annotations,
            archived_annotation_ids: self.document.pending_archived_annotation_ids.clone(),
            stack_order: self
                .document
                .stack_order_dirty
                .then(|| self.committed_canvas_items_back_to_front()),
        }
    }

    /// Applies `edit` to every selected annotation and marks the ones it
    /// changed. Returns whether any changed.
    fn edit_selected_annotations(&mut self, edit: impl Fn(&mut CanvasAnnotation) -> bool) -> bool {
        let mut changed = false;
        for annotation_id in self.interaction.selected_annotation_ids.clone() {
            let Some(annotation) = self.annotation_by_id_mut(annotation_id) else {
                continue;
            };
            if edit(annotation) {
                self.invalidate_textbox_layout_cache(annotation_id);
                self.mark_annotation_dirty(annotation_id);
                changed = true;
            }
        }
        changed
    }

    pub fn apply_color_to_selected_annotations(&mut self, color: CanvasAnnotationColor) -> bool {
        self.edit_selected_annotations(|annotation| {
            std::mem::replace(&mut annotation.color, color) != color
        })
    }

    /// Sets each selected annotation's size on its own scale: a label's type
    /// size, or the weight of a stroke or of a rectangle's border.
    pub fn set_selected_annotation_sizes(
        &mut self,
        text_size: CanvasTextSize,
        stroke_width: CanvasStrokeWidth,
    ) -> bool {
        self.edit_selected_annotations(|annotation| match annotation.kind {
            CanvasAnnotationKind::Textbox => {
                std::mem::replace(&mut annotation.text_size, text_size) != text_size
            }
            CanvasAnnotationKind::Line
            | CanvasAnnotationKind::Arrow
            | CanvasAnnotationKind::Rectangle => {
                std::mem::replace(&mut annotation.stroke_width, stroke_width) != stroke_width
            }
        })
    }

    /// Steps every selected rectangle to the fill after the first one's.
    /// Returns the new fill, or none when no rectangle is selected.
    pub fn cycle_selected_rectangle_fills(&mut self) -> Option<CanvasRectangleFill> {
        let fill = self
            .selected_annotations()
            .into_iter()
            .find(|annotation| annotation.kind == CanvasAnnotationKind::Rectangle)?
            .fill
            .next();
        self.edit_selected_annotations(|annotation| {
            annotation.kind == CanvasAnnotationKind::Rectangle
                && std::mem::replace(&mut annotation.fill, fill) != fill
        });
        Some(fill)
    }

    /// A line is an arrow with its head off. Heads go on when any selected
    /// stroke lacks one, and off otherwise. Returns the new state, or none
    /// when no stroke is selected.
    pub fn toggle_selected_arrowheads(&mut self) -> Option<bool> {
        let mut strokes = self
            .selected_annotations()
            .into_iter()
            .filter(|annotation| {
                matches!(
                    annotation.kind,
                    CanvasAnnotationKind::Line | CanvasAnnotationKind::Arrow
                )
            });
        let first = strokes.next()?;
        let on = first.kind == CanvasAnnotationKind::Line
            || strokes.any(|annotation| annotation.kind == CanvasAnnotationKind::Line);
        let kind = if on {
            CanvasAnnotationKind::Arrow
        } else {
            CanvasAnnotationKind::Line
        };
        self.edit_selected_annotations(|annotation| {
            matches!(
                annotation.kind,
                CanvasAnnotationKind::Line | CanvasAnnotationKind::Arrow
            ) && std::mem::replace(&mut annotation.kind, kind) != kind
        });
        Some(on)
    }

    pub fn arrange_selected_horizontal(&mut self, gap: f32) -> usize {
        let mut items = self.selected_arrangement_blocks(self.selected_arrangement_inputs());
        if items.len() <= 1 {
            return items.len();
        }

        items.sort_by(|left, right| left.center().x.total_cmp(&right.center().x));
        let current_center = self.selected_center_world().unwrap_or(egui::Pos2::ZERO);
        let Some((placements, total_width, total_height)) =
            crate::placement::arrangement::horizontal_layout(&items, gap)
        else {
            return 0;
        };
        let local_center = egui::pos2(total_width * 0.5, total_height * 0.5);
        self.apply_arrangement(
            placements,
            current_center.to_vec2() - local_center.to_vec2(),
        )
    }

    pub fn arrange_selected_vertical(&mut self, gap: f32) -> usize {
        let mut items = self.selected_arrangement_blocks(self.selected_arrangement_inputs());
        if items.len() <= 1 {
            return items.len();
        }

        items.sort_by(|left, right| left.center().y.total_cmp(&right.center().y));
        let current_center = self.selected_center_world().unwrap_or(egui::Pos2::ZERO);
        let Some((placements, total_width, total_height)) =
            crate::placement::arrangement::vertical_layout(&items, gap)
        else {
            return 0;
        };
        let local_center = egui::pos2(total_width * 0.5, total_height * 0.5);
        self.apply_arrangement(
            placements,
            current_center.to_vec2() - local_center.to_vec2(),
        )
    }

    pub fn arrange_selected_compact(&mut self, gap: f32) -> usize {
        let items = self.selected_arrangement_blocks(self.selected_compact_input_items());
        if items.len() <= 1 {
            return items.len();
        }

        let current_center = self
            .source_center_for_arrangement_inputs(&items)
            .unwrap_or(egui::Pos2::ZERO);

        let Some(layout) = crate::placement::arrangement::best_user_compact_layout(&items, gap)
        else {
            return 0;
        };
        let local_center = egui::pos2(layout.width * 0.5, layout.height * 0.5);
        self.apply_compact_arrangement(
            layout.placements,
            layout.scale_factors,
            current_center.to_vec2() - local_center.to_vec2(),
        )
    }

    pub fn arrange_selected_compact_preserving_sizes(&mut self, gap: f32) -> usize {
        let items = self.selected_arrangement_inputs();
        if items.len() <= 1 {
            return items.len();
        }

        let current_center = self
            .source_center_for_arrangement_inputs(&items)
            .unwrap_or(egui::Pos2::ZERO);
        let Some((placements, width, height)) =
            crate::placement::arrangement::best_compact_layout(&items, gap)
        else {
            return 0;
        };
        let local_center = egui::pos2(width * 0.5, height * 0.5);
        self.apply_arrangement_if_changed(
            placements,
            current_center.to_vec2() - local_center.to_vec2(),
        )
    }

    pub fn settle_selected(
        &mut self,
        direction: crate::placement::arrangement::SettleDirection,
        gap: f32,
    ) -> usize {
        let items = self.selected_arrangement_blocks(self.selected_arrangement_inputs());
        if items.len() <= 1 {
            return items.len();
        }

        let cross_axis_grace = crate::placement::arrangement::SETTLE_BLOCK_GRACE_SCREEN
            / self.zoom.max(super::MIN_CANVAS_ZOOM);
        let Some(placements) = crate::placement::arrangement::directional_settle_layout(
            &items,
            direction,
            gap,
            cross_axis_grace,
        ) else {
            return 0;
        };
        self.apply_arrangement_if_changed(placements, egui::Vec2::ZERO)
    }

    pub fn move_selected_placement_group_to_center(
        &mut self,
        center_world: egui::Pos2,
        screen_rect: egui::Rect,
        now_seconds: f64,
    ) -> usize {
        crate::canvas::interaction::movement::move_selected_placement_group_to_center(
            self,
            center_world,
            screen_rect,
            now_seconds,
        )
    }

    pub fn bring_selection_to_front(&mut self) -> bool {
        self.restack_selection(StackEnd::Front)
    }

    pub fn send_selection_to_back(&mut self) -> bool {
        self.restack_selection(StackEnd::Back)
    }

    pub fn delete_selected_annotations(&mut self) -> Vec<i64> {
        let selected_ids = self.interaction.selected_annotation_ids.clone();
        if selected_ids.is_empty() {
            return Vec::new();
        }
        for annotation_id in &selected_ids {
            self.remove_annotation_locally(*annotation_id);
        }
        selected_ids
    }

    pub fn delete_placements(&mut self, placement_ids: &[i64]) -> Vec<i64> {
        if placement_ids.is_empty() {
            return Vec::new();
        }

        self.remove_group_members(placement_ids);
        let mut removed_ids = Vec::new();
        let mut removed_committed = false;
        self.document.placements.retain(|image| {
            let keep = !placement_ids.contains(&image.state.id);
            if !keep {
                removed_ids.push(image.state.id);
                if image.lifecycle == PlacementLifecycle::Committed {
                    removed_committed = true;
                    if !self
                        .document
                        .pending_archived_placement_ids
                        .contains(&image.state.id)
                    {
                        self.document
                            .pending_archived_placement_ids
                            .push(image.state.id);
                    }
                }
            }
            keep
        });
        // Provisional Placements never reached the durable stack order, so
        // removing them (a cancelled hover) must not queue a write.
        if removed_committed {
            self.document.stack_order_dirty = true;
        }
        self.interaction
            .selected_placement_ids
            .retain(|placement_id| !removed_ids.contains(placement_id));
        self.render
            .placement_images
            .retain(|id, _| !removed_ids.contains(id));
        self.document.stack_order.retain(|item| {
            !matches!(item, CanvasStackItem::Placement(placement_id) if removed_ids.contains(placement_id))
        });
        self.document
            .dirty_placement_ids
            .retain(|placement_id| !removed_ids.contains(placement_id));
        self.interaction.end_gesture();
        self.interaction
            .crop_pending_placement_ids
            .retain(|placement_id| !removed_ids.contains(placement_id));
        self.render
            .image_lod_loads
            .retain(|placement_id, _| !removed_ids.contains(placement_id));
        self.render
            .failed_image_lod_loads
            .retain(|(placement_id, _)| !removed_ids.contains(placement_id));
        self.interaction.pending_crop = None;
        self.prune_cached_pixels_to_selected_items();
        removed_ids
    }

    pub fn remove_assets(&mut self, asset_ids: &[i64]) -> Vec<i64> {
        if asset_ids.is_empty() {
            return Vec::new();
        }

        let placement_ids = self
            .document
            .placements
            .iter()
            .filter(|image| image.asset_id().is_some_and(|id| asset_ids.contains(&id)))
            .map(|image| image.state.id)
            .collect::<Vec<_>>();
        self.delete_placements(&placement_ids)
    }

    pub(super) fn mark_dirty(&mut self, placement_id: i64) {
        if placement_id > 0 && !self.document.dirty_placement_ids.contains(&placement_id) {
            self.document.dirty_placement_ids.push(placement_id);
        }
    }

    pub(super) fn mark_annotation_dirty(&mut self, annotation_id: i64) {
        if annotation_id > 0 && !self.document.dirty_annotation_ids.contains(&annotation_id) {
            self.document.dirty_annotation_ids.push(annotation_id);
        }
    }

    pub(super) fn remove_annotation_locally(&mut self, annotation_id: i64) {
        self.remove_group_members(&[annotation_id]);
        let existed = self
            .document
            .annotations
            .iter()
            .any(|annotation| annotation.id == annotation_id);
        self.document
            .annotations
            .retain(|annotation| annotation.id != annotation_id);
        self.document
            .stack_order
            .retain(|item| *item != CanvasStackItem::Annotation(annotation_id));
        self.invalidate_textbox_layout_cache(annotation_id);
        self.interaction
            .selected_annotation_ids
            .retain(|selected_id| *selected_id != annotation_id);
        if self.interaction.editing_text_annotation_id == Some(annotation_id) {
            self.interaction.editing_text_annotation_id = None;
        }
        self.interaction.focus_text_editor = false;
        self.interaction.end_transform_gesture();
        self.document
            .dirty_annotation_ids
            .retain(|dirty_id| *dirty_id != annotation_id);
        if existed {
            if !self
                .document
                .pending_archived_annotation_ids
                .contains(&annotation_id)
            {
                self.document
                    .pending_archived_annotation_ids
                    .push(annotation_id);
            }
            self.document.stack_order_dirty = true;
        }
    }

    pub(crate) fn finish_textbox_edit(&mut self) {
        let Some(annotation_id) = self.interaction.editing_text_annotation_id else {
            self.interaction.focus_text_editor = false;
            return;
        };

        self.interaction.editing_text_annotation_id = None;
        self.interaction.focus_text_editor = false;
        self.interaction.end_transform_gesture();

        let Some(annotation) = self
            .document
            .annotations
            .iter()
            .find(|annotation| annotation.id == annotation_id)
        else {
            self.interaction
                .selected_annotation_ids
                .retain(|selected_id| *selected_id != annotation_id);
            return;
        };

        if annotation.kind == CanvasAnnotationKind::Textbox && annotation.text.trim().is_empty() {
            self.remove_annotation_locally(annotation_id);
            if annotation_id > 0
                && !self
                    .document
                    .pending_archived_annotation_ids
                    .contains(&annotation_id)
            {
                self.document
                    .pending_archived_annotation_ids
                    .push(annotation_id);
            }
        }
    }

    pub(super) fn stop_textbox_edit_without_cleanup(&mut self) {
        self.interaction.editing_text_annotation_id = None;
        self.interaction.focus_text_editor = false;
        self.interaction.end_transform_gesture();
    }

    pub(super) fn annotation_by_id(&self, annotation_id: i64) -> Option<&CanvasAnnotation> {
        self.document
            .annotations
            .iter()
            .find(|annotation| annotation.id == annotation_id)
    }

    pub(super) fn ordered_canvas_items_back_to_front(&self) -> Vec<CanvasStackItem> {
        let mut ordered = Vec::new();
        for item in &self.document.stack_order {
            if self.stack_item_exists(*item) && !ordered.contains(item) {
                ordered.push(*item);
            }
        }
        for image in &self.document.placements {
            let item = CanvasStackItem::Placement(image.state.id);
            if !ordered.contains(&item) {
                ordered.push(item);
            }
        }
        for annotation in &self.document.annotations {
            let item = CanvasStackItem::Annotation(annotation.id);
            if !ordered.contains(&item) {
                ordered.push(item);
            }
        }
        for group in self.groups() {
            let item_id = |item: &CanvasStackItem| match *item {
                CanvasStackItem::Placement(id) | CanvasStackItem::Annotation(id) => id,
            };
            let Some(slot) = ordered
                .iter()
                .position(|item| group.members.contains(&item_id(item)))
            else {
                continue;
            };
            let members = ordered
                .iter()
                .filter(|item| group.members.contains(&item_id(item)))
                .copied()
                .collect::<Vec<_>>();
            ordered.retain(|item| !group.members.contains(&item_id(item)));
            let paint = group
                .members
                .iter()
                .filter(|m| {
                    !group
                        .layout
                        .members
                        .get(m)
                        .is_some_and(|mode| mode.absolute)
                })
                .chain(group.members.iter().filter(|m| {
                    group
                        .layout
                        .members
                        .get(m)
                        .is_some_and(|mode| mode.absolute)
                }))
                .filter_map(|id| members.iter().find(|item| item_id(item) == *id).copied())
                .collect::<Vec<_>>();
            ordered.splice(slot..slot, paint);
        }
        ordered
    }

    fn committed_canvas_items_back_to_front(&self) -> Vec<CanvasStackItem> {
        self.ordered_canvas_items_back_to_front()
            .into_iter()
            .filter(|item| match item {
                CanvasStackItem::Placement(placement_id) => self
                    .placement_by_id(*placement_id)
                    .is_some_and(|image| image.lifecycle == PlacementLifecycle::Committed),
                CanvasStackItem::Annotation(_) => true,
            })
            .collect()
    }

    pub(in crate::canvas) fn append_missing_stack_item(&mut self, item: CanvasStackItem) {
        if !self.document.stack_order.contains(&item) {
            self.document.stack_order.push(item);
        }
    }

    fn stack_item_exists(&self, item: CanvasStackItem) -> bool {
        match item {
            CanvasStackItem::Placement(placement_id) => self
                .document
                .placements
                .iter()
                .any(|image| image.state.id == placement_id),
            CanvasStackItem::Annotation(annotation_id) => self
                .document
                .annotations
                .iter()
                .any(|annotation| annotation.id == annotation_id),
        }
    }

    fn restack_selection(&mut self, end: StackEnd) -> bool {
        let selected = self.selected_canvas_stack_items();
        if selected.is_empty() {
            return false;
        }

        let ordered = self.ordered_canvas_items_back_to_front();
        let mut rest = Vec::with_capacity(ordered.len().saturating_sub(selected.len()));
        let mut picked = Vec::with_capacity(selected.len());
        for item in ordered {
            if selected.contains(&item) {
                picked.push(item);
            } else {
                rest.push(item);
            }
        }

        let next_order = match end {
            StackEnd::Front => {
                rest.extend(picked);
                rest
            }
            StackEnd::Back => {
                picked.extend(rest);
                picked
            }
        };

        if next_order == self.document.stack_order {
            return false;
        }
        self.document.stack_order = next_order;
        self.document.stack_order_dirty = true;
        true
    }

    fn selected_canvas_stack_items(&self) -> Vec<CanvasStackItem> {
        self.interaction
            .selected_placement_ids
            .iter()
            .copied()
            .map(CanvasStackItem::Placement)
            .chain(
                self.interaction
                    .selected_annotation_ids
                    .iter()
                    .copied()
                    .map(CanvasStackItem::Annotation),
            )
            .filter(|item| self.stack_item_exists(*item))
            .collect()
    }

    pub(super) fn annotation_by_id_mut(
        &mut self,
        annotation_id: i64,
    ) -> Option<&mut CanvasAnnotation> {
        self.document
            .annotations
            .iter_mut()
            .find(|annotation| annotation.id == annotation_id)
    }

    pub(super) fn set_textbox_world_rect(
        &mut self,
        annotation_id: i64,
        world_rect: egui::Rect,
    ) -> bool {
        let normalized = normalized_textbox_rect(world_rect.min, world_rect.max);
        let Some(annotation) = self.annotation_by_id_mut(annotation_id) else {
            return false;
        };
        if !annotation.kind.is_rect() {
            return false;
        }
        // A label's size belongs to its text; only a rectangle has a minimum.
        let normalized = if annotation.kind == CanvasAnnotationKind::Textbox {
            world_rect
        } else {
            normalized
        };

        let changed = annotation.start().x != normalized.min.x
            || annotation.start().y != normalized.min.y
            || annotation.end().x != normalized.max.x
            || annotation.end().y != normalized.max.y;
        if changed {
            annotation.geometry = crate::data::CanvasAnnotationGeometry::from_kind(
                annotation.kind,
                normalized.min,
                normalized.max,
            );
            self.invalidate_textbox_layout_cache(annotation_id);
            self.mark_annotation_dirty(annotation_id);
        }
        changed
    }

    pub(super) fn set_annotation_world_points(
        &mut self,
        annotation_id: i64,
        start_world: egui::Pos2,
        end_world: egui::Pos2,
    ) -> bool {
        let Some(annotation) = self.annotation_by_id_mut(annotation_id) else {
            return false;
        };
        let changed = annotation.start().x != start_world.x
            || annotation.start().y != start_world.y
            || annotation.end().x != end_world.x
            || annotation.end().y != end_world.y;
        if changed {
            annotation.geometry = crate::data::CanvasAnnotationGeometry::from_kind(
                annotation.kind,
                start_world,
                end_world,
            );
            if annotation.kind == CanvasAnnotationKind::Textbox {
                self.invalidate_textbox_layout_cache(annotation_id);
            }
        }
        changed
    }

    pub(super) fn selected_textbox_start_rects(&self) -> Vec<(i64, crate::data::RectGeometry)> {
        self.interaction
            .selected_annotation_ids
            .iter()
            .filter_map(|annotation_id| {
                self.annotation_by_id(*annotation_id)
                    .and_then(|annotation| {
                        annotation.kind.is_rect().then_some((
                            *annotation_id,
                            crate::data::RectGeometry::from_bounds(
                                textbox_world_rect(annotation),
                                0,
                            ),
                        ))
                    })
            })
            .collect()
    }

    pub(super) fn invalidate_textbox_layout_cache(&mut self, annotation_id: i64) {
        self.render.textbox_layout_cache.remove(&annotation_id);
    }

    pub(super) fn cache_textbox_layout(
        &mut self,
        annotation_id: i64,
        text: &str,
        text_size: CanvasTextSize,
        color: CanvasAnnotationColor,
        wrap_width: f32,
        zoom: f32,
        galley: Arc<egui::Galley>,
    ) -> TextboxLayoutCacheEntry {
        let key = textbox_layout_cache_key(text, text_size, color, wrap_width, zoom);
        let entry = TextboxLayoutCacheEntry { key, galley };
        self.render
            .textbox_layout_cache
            .insert(annotation_id, entry.clone());
        entry
    }

    pub(super) fn cached_textbox_layout(
        &mut self,
        ui: &egui::Ui,
        annotation_id: i64,
        text: &str,
        text_size: CanvasTextSize,
        color: CanvasAnnotationColor,
        wrap_width: f32,
        zoom: f32,
    ) -> TextboxLayoutCacheEntry {
        let key = textbox_layout_cache_key(text, text_size, color, wrap_width, zoom);
        if let Some(entry) = self.render.textbox_layout_cache.get(&annotation_id)
            && entry.key == key
        {
            return entry.clone();
        }

        let galley = layout_textbox_galley(ui, text, text_size, color, wrap_width, zoom);
        self.cache_textbox_layout(
            annotation_id,
            text,
            text_size,
            color,
            wrap_width,
            zoom,
            galley,
        )
    }

    pub(crate) fn selected_arrangement_inputs(&self) -> Vec<PlacementArrangementInput> {
        self.document
            .placements
            .iter()
            .filter(|image| self.is_selected(image.state.id))
            .map(|image| {
                let display_size = image.state.geometry.scaled_size();
                let rotated_display_size =
                    crate::data::rotated_display_size(display_size, image.state.geometry.rotation);
                PlacementArrangementInput::new(
                    image.state.id,
                    egui::pos2(image.state.geometry.center.x, image.state.geometry.center.y),
                    rotated_display_size,
                )
            })
            .collect()
    }

    /// Each selected Group becomes one block the size of its frame, and its
    /// members leave the list, so arranging moves the Group as a whole and
    /// keeps its own layout.
    fn selected_arrangement_blocks(
        &self,
        placements: Vec<PlacementArrangementInput>,
    ) -> Vec<PlacementArrangementInput> {
        let groups = self
            .groups()
            .iter()
            .filter(|group| self.interaction.selected_group_ids.contains(&group.id))
            .collect::<Vec<_>>();
        placements
            .into_iter()
            .filter(|item| !groups.iter().any(|group| group.members.contains(&item.id)))
            .chain(groups.iter().map(|group| PlacementArrangementInput {
                id: group.id,
                geometry: crate::data::RectGeometry::from_bounds(
                    group.container.unrotated_bounds(),
                    0,
                ),
            }))
            .collect()
    }

    /// Scales a Group block by `factor`, then moves its frame and members so
    /// the frame centers on `center`. Returns how many members changed, or
    /// `None` when `id` is not a Group.
    fn place_group_block(&mut self, id: GroupId, center: egui::Pos2, factor: f32) -> Option<usize> {
        let mut group = self.groups().iter().find(|group| group.id == id)?.clone();
        let mut changed = false;
        if (factor - 1.0).abs() > 0.001 {
            let members = group
                .members
                .iter()
                .filter_map(|&member| Some((member, self.group_member_geometry(member)?)))
                .collect::<Vec<_>>();
            if let Ok(layout) =
                crate::placement::group_layout::scale_group(&group, &members, factor)
            {
                group.container = layout.container;
                group.gap = layout.gap;
                group.layout.row_gap *= factor;
                group.padding = layout.padding;
                let mut groups = self.groups().to_vec();
                if let Some(target) = groups.iter_mut().find(|target| target.id == id) {
                    *target = group.clone();
                }
                self.replace_groups(groups);
                for (member, geometry) in layout.members {
                    self.set_group_member_geometry(member, geometry);
                }
                changed = true;
            }
        }
        let delta = center - group.container.unrotated_bounds().center();
        if delta.length() > 0.001 {
            self.move_group(id, delta);
            changed = true;
        }
        Some(if changed { group.members.len() } else { 0 })
    }

    fn selected_compact_input_items(&self) -> Vec<PlacementArrangementInput> {
        self.document
            .placements
            .iter()
            .filter(|image| self.is_selected(image.state.id))
            .map(|image| image.compact_arrangement_input())
            .collect()
    }

    fn source_center_for_arrangement_inputs(
        &self,
        items: &[PlacementArrangementInput],
    ) -> Option<egui::Pos2> {
        layout_bounds_with_offset(items, egui::Vec2::ZERO).map(|bounds| bounds.center())
    }

    pub(super) fn apply_arrangement(
        &mut self,
        placements: Vec<(i64, egui::Pos2)>,
        offset: egui::Vec2,
    ) -> usize {
        let mut arranged_ids = Vec::with_capacity(placements.len());
        let mut group_members = 0;

        for (placement_id, local_center) in placements {
            let world_center = local_center + offset;
            if let Some(moved) = self.place_group_block(placement_id, world_center, 1.0) {
                group_members += moved;
                continue;
            }
            if let Some(image) = self
                .document
                .placements
                .iter_mut()
                .find(|image| image.state.id == placement_id)
            {
                image.state.geometry.center.x = world_center.x;
                image.state.geometry.center.y = world_center.y;
                arranged_ids.push(placement_id);
            }
        }

        for placement_id in &arranged_ids {
            self.mark_dirty(*placement_id);
        }

        arranged_ids.len() + group_members
    }

    fn apply_arrangement_if_changed(
        &mut self,
        placements: Vec<(i64, egui::Pos2)>,
        offset: egui::Vec2,
    ) -> usize {
        let mut arranged_ids = Vec::with_capacity(placements.len());
        let mut group_members = 0;

        for (placement_id, local_center) in placements {
            let world_center = local_center + offset;
            if let Some(moved) = self.place_group_block(placement_id, world_center, 1.0) {
                group_members += moved;
                continue;
            }
            if let Some(image) = self
                .document
                .placements
                .iter_mut()
                .find(|image| image.state.id == placement_id)
            {
                if (image.state.geometry.center.x - world_center.x).abs() <= 0.001
                    && (image.state.geometry.center.y - world_center.y).abs() <= 0.001
                {
                    continue;
                }
                image.state.geometry.center.x = world_center.x;
                image.state.geometry.center.y = world_center.y;
                arranged_ids.push(placement_id);
            }
        }

        for placement_id in &arranged_ids {
            self.mark_dirty(*placement_id);
        }

        arranged_ids.len() + group_members
    }

    fn apply_compact_arrangement(
        &mut self,
        placements: Vec<(i64, egui::Pos2)>,
        scale_factors: Vec<(i64, f32)>,
        offset: egui::Vec2,
    ) -> usize {
        let mut changed_ids = Vec::with_capacity(placements.len());
        let mut group_members = 0;

        for (placement_id, local_center) in placements {
            let world_center = local_center + offset;
            let scale_factor = scale_factors
                .iter()
                .find(|(scale_item_id, _)| *scale_item_id == placement_id)
                .map(|(_, factor)| *factor)
                .unwrap_or(1.0);
            if let Some(moved) = self.place_group_block(placement_id, world_center, scale_factor) {
                group_members += moved;
                continue;
            }
            if let Some(image) = self
                .document
                .placements
                .iter_mut()
                .find(|image| image.state.id == placement_id)
            {
                let next_scale = (image.state.geometry.scale * scale_factor).max(0.01);
                let position_changed = (image.state.geometry.center.x - world_center.x).abs()
                    > 0.001
                    || (image.state.geometry.center.y - world_center.y).abs() > 0.001;
                let scale_changed = (image.state.geometry.scale - next_scale).abs() > 0.001;
                if !position_changed && !scale_changed {
                    continue;
                }

                image.apply_compact_output(crate::placement::arrangement::CompactOutput {
                    placement_id,
                    center: world_center,
                });
                image.state.geometry.scale = next_scale;
                changed_ids.push(placement_id);
            }
        }

        for placement_id in &changed_ids {
            self.mark_dirty(*placement_id);
        }

        changed_ids.len() + group_members
    }

    pub fn apply_target_area_to_selected_placements(&mut self, target_area: f32) -> usize {
        if target_area <= 0.0 {
            return 0;
        }

        let selected_ids = self.interaction.selected_placement_ids.clone();
        let mut changed = 0;
        for placement_id in selected_ids {
            let Some(image) = self.placement_by_id(placement_id) else {
                continue;
            };
            let base_size = image.base_display_size();
            let source_area = base_size.x * base_size.y;
            if source_area <= f32::EPSILON {
                continue;
            }

            let target_scale = (target_area / source_area).sqrt();
            if self.set_placement_scale(placement_id, target_scale) {
                self.mark_dirty(placement_id);
                changed += 1;
            }
        }

        changed
    }

    pub fn average_selected_placement_area(&self) -> Option<f32> {
        let items = self.selected_arrangement_inputs();
        if items.is_empty() {
            return None;
        }

        let total_area = items
            .iter()
            .map(|item| item.width().max(0.0) * item.height().max(0.0))
            .sum::<f32>();
        Some(total_area / items.len() as f32)
    }

    pub(super) fn placement_by_id(&self, placement_id: i64) -> Option<&Placement> {
        self.document
            .placements
            .iter()
            .find(|image| image.state.id == placement_id)
    }

    pub(super) fn placement_by_id_mut(&mut self, placement_id: i64) -> Option<&mut Placement> {
        self.document
            .placements
            .iter_mut()
            .find(|image| image.state.id == placement_id)
    }

    pub(super) fn build_crop_request(
        &self,
        rect: egui::Rect,
        crop_state: &CropState,
    ) -> Option<CanvasCropRequest> {
        let image = self.placement_by_id(crop_state.placement_id)?;
        let center = placement_screen_rect(self, rect, image).center();
        let size = image.state.geometry.size * image.state.geometry.scale * self.zoom;
        let image_rect = egui::Rect::from_center_size(center, size);
        let unrotate = |point| {
            center + super::geometry::rotate_vec2(point - center, -image.state.geometry.rotation)
        };
        let clamped_screen_rect = clamped_crop_screen_rect(
            unrotate(crop_state.start_screen),
            unrotate(crop_state.current_screen),
            image_rect,
        )?;
        let local_rect = screen_rect_to_local_crop_rect(
            clamped_screen_rect,
            image_rect,
            image.require_loaded_asset().width,
            image.require_loaded_asset().height,
        )?;
        let crop_rect = pixel_crop_rect_from_local(
            local_rect,
            image.require_loaded_asset().width,
            image.require_loaded_asset().height,
        )?;
        let crop_offset_local = crop_center_offset_local(image.require_loaded_asset(), local_rect);

        Some(CanvasCropRequest {
            placement_id: image.state.id,
            source_asset: image.require_loaded_asset().clone(),
            crop_rect,
            crop_offset_local,
        })
    }

    pub(super) fn can_start_crop_on_placement(&self, placement_id: i64) -> bool {
        !self
            .interaction
            .crop_pending_placement_ids
            .contains(&placement_id)
            && self.placement_by_id(placement_id).is_some_and(|image| {
                !asset_mime_type_is_svg(&image.require_loaded_asset().mime_type)
            })
    }

    pub(super) fn set_placement_scale(&mut self, placement_id: i64, scale: f32) -> bool {
        let Some(image) = self.placement_by_id_mut(placement_id) else {
            return false;
        };
        let min_scale = min_loaded_placement_scale(image.require_loaded_asset());
        let normalized = scale.max(min_scale);
        if (image.state.geometry.scale - normalized).abs() <= f32::EPSILON {
            return false;
        }
        image.state.geometry.scale = normalized;
        true
    }

    pub(super) fn set_placement_center_and_scale(
        &mut self,
        placement_id: i64,
        center: egui::Pos2,
        scale: f32,
    ) -> bool {
        let Some(image) = self.placement_by_id_mut(placement_id) else {
            return false;
        };
        let min_scale = min_loaded_placement_scale(image.require_loaded_asset());
        let normalized_scale = scale.max(min_scale);
        let position_changed = (image.state.geometry.center.x - center.x).abs() > f32::EPSILON
            || (image.state.geometry.center.y - center.y).abs() > f32::EPSILON;
        let scale_changed = (image.state.geometry.scale - normalized_scale).abs() > f32::EPSILON;
        if !position_changed && !scale_changed {
            return false;
        }

        image.state.geometry.center.x = center.x;
        image.state.geometry.center.y = center.y;
        image.state.geometry.scale = normalized_scale;
        true
    }

    #[cfg(test)]
    pub fn set_placement_rotation(&mut self, placement_id: i64, rotation: f32) -> bool {
        let Some(image) = self.placement_by_id_mut(placement_id) else {
            return false;
        };
        if (image.state.geometry.rotation - rotation).abs() <= f32::EPSILON {
            return false;
        }
        image.state.geometry.rotation = rotation;
        true
    }

    pub(super) fn set_placement_center_and_rotation(
        &mut self,
        placement_id: i64,
        center: egui::Pos2,
        rotation: f32,
    ) -> bool {
        let Some(image) = self.placement_by_id_mut(placement_id) else {
            return false;
        };
        let position_changed = (image.state.geometry.center.x - center.x).abs() > f32::EPSILON
            || (image.state.geometry.center.y - center.y).abs() > f32::EPSILON;
        let rotation_changed = (image.state.geometry.rotation - rotation).abs() > f32::EPSILON;
        if !position_changed && !rotation_changed {
            return false;
        }

        image.state.geometry.center.x = center.x;
        image.state.geometry.center.y = center.y;
        image.state.geometry.rotation = rotation;
        true
    }
}

fn asset_mime_type_is_svg(mime_type: &str) -> bool {
    mime_type
        .split(';')
        .next()
        .unwrap_or(mime_type)
        .trim()
        .eq_ignore_ascii_case("image/svg+xml")
}

fn layout_bounds_with_offset(
    items: &[PlacementArrangementInput],
    offset: egui::Vec2,
) -> Option<egui::Rect> {
    let mut bounds = egui::Rect::NOTHING;
    let mut found = false;
    for item in items {
        let rect = egui::Rect::from_center_size(
            egui::pos2(item.center().x + offset.x, item.center().y + offset.y),
            egui::vec2(item.width(), item.height()),
        );
        if found {
            bounds = bounds.union(rect);
        } else {
            bounds = rect;
            found = true;
        }
    }
    found.then_some(bounds)
}
