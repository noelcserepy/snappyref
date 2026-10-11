use eframe::egui;

use crate::canvas::interaction::{
    annotation_screen_rect, annotation_world_rect, placement_screen_rect,
};
use crate::canvas::{
    CanvasCropRequest, CanvasState, FOCUS_SELECTION_PADDING_SCREEN, MAX_CANVAS_ZOOM,
    MIN_CANVAS_ZOOM, PendingAnnotation, merge_ids,
};
use crate::data::{
    CanvasAnnotation, CanvasAnnotationKind, CanvasBackground, Placement, ViewportState,
};

impl CanvasState {
    pub fn apply_viewport_state(&mut self, viewport: &ViewportState) {
        self.interaction.mouse_wheel_zoom_animation = None;
        self.pan = egui::vec2(viewport.camera_pan_x, viewport.camera_pan_y);
        self.zoom = viewport.camera_zoom.clamp(MIN_CANVAS_ZOOM, MAX_CANVAS_ZOOM);
        self.background = viewport.canvas_background;
    }

    pub fn set_rotation_snap_degrees(&mut self, degrees: f32) {
        self.rotation_snap_degrees = degrees.max(0.0);
    }

    pub fn set_pointer_mode(&mut self, mode: crate::data::PointerMode) {
        self.pointer_mode = mode;
    }

    pub(in crate::canvas) fn pointer_mode(&self) -> crate::data::PointerMode {
        self.pointer_mode
    }

    pub fn set_placement_gap(&mut self, gap: f32) {
        self.placement_gap = gap.max(0.0);
    }

    pub fn viewport_state(&self) -> ViewportState {
        ViewportState {
            camera_pan_x: self.pan.x,
            camera_pan_y: self.pan.y,
            camera_zoom: self.zoom,
            canvas_background: self.background,
        }
    }

    pub fn has_selection(&self) -> bool {
        !self.interaction.selected_group_ids.is_empty()
            || self.has_placement_selection()
            || !self.interaction.selected_annotation_ids.is_empty()
    }

    pub fn has_placement_selection(&self) -> bool {
        !self.interaction.selected_placement_ids.is_empty()
    }

    pub fn selected_placement_ids(&self) -> Vec<i64> {
        self.interaction.selected_placement_ids.clone()
    }

    pub fn selected_annotation_ids(&self) -> Vec<i64> {
        self.interaction.selected_annotation_ids.clone()
    }

    pub fn next_annotation_style(&self) -> super::super::annotation_style::AnnotationStyle {
        self.interaction.next_annotation_style
    }

    pub fn background(&self) -> CanvasBackground {
        self.background
    }

    pub fn set_background(&mut self, background: CanvasBackground) {
        self.background = background;
    }

    pub fn focus_selection_in_view(
        &mut self,
        canvas_rect: egui::Rect,
        target_rect: egui::Rect,
    ) -> bool {
        let Some(bounds) = self.selected_world_bounds() else {
            return false;
        };

        let (pan, zoom) = focus_framing(canvas_rect, target_rect, bounds);
        self.interaction.mouse_wheel_zoom_animation = None;
        self.pan = pan;
        self.zoom = zoom;
        true
    }

    pub(crate) fn zoom_out_to_fit_world_rect(
        &mut self,
        canvas_rect: egui::Rect,
        target_rect: egui::Rect,
        bounds: egui::Rect,
        padding_screen: f32,
    ) -> bool {
        let (pan, zoom) = framing(canvas_rect, target_rect, bounds, padding_screen, self.zoom);
        if zoom >= self.zoom {
            return false;
        }

        self.interaction.mouse_wheel_zoom_animation = None;
        self.pan = pan;
        self.zoom = zoom;
        true
    }

    pub fn selected_center_world(&self) -> Option<egui::Pos2> {
        let mut min = egui::Pos2::new(f32::INFINITY, f32::INFINITY);
        let mut max = egui::Pos2::new(f32::NEG_INFINITY, f32::NEG_INFINITY);
        let mut found = false;

        for image in &self.document.placements {
            if !self.is_selected(image.state.id) {
                continue;
            }

            found = true;
            min.x = min.x.min(image.state.geometry.center.x);
            min.y = min.y.min(image.state.geometry.center.y);
            max.x = max.x.max(image.state.geometry.center.x);
            max.y = max.y.max(image.state.geometry.center.y);
        }

        found.then(|| egui::pos2((min.x + max.x) * 0.5, (min.y + max.y) * 0.5))
    }

    pub fn take_pending_crop(&mut self) -> Option<CanvasCropRequest> {
        self.interaction.pending_crop.take()
    }

    #[cfg(test)]
    pub(crate) fn set_pending_crop_for_test(&mut self, request: CanvasCropRequest) {
        self.interaction.pending_crop = Some(request);
    }

    pub fn take_pending_annotation(&mut self) -> Option<PendingAnnotation> {
        self.interaction.pending_annotation.take()
    }

    pub fn take_context_menu_request(&mut self) -> Option<crate::canvas::CanvasContextMenuRequest> {
        self.interaction.pending_context_menu_request.take()
    }

    pub fn take_focus_selection_request(&mut self) -> bool {
        let requested = self.interaction.pending_focus_selection_request;
        self.interaction.pending_focus_selection_request = false;
        requested
    }

    pub fn has_pending_crop_work(&self) -> bool {
        self.interaction.pending_crop.is_some()
            || !self.interaction.crop_pending_placement_ids.is_empty()
            || !self.render.pending_crop_previews.is_empty()
    }

    pub(crate) fn clear_guide_snap_indicators(&mut self) {
        self.interaction.guide_snap_indicators.clear();
        self.interaction.guide_snap_signature = 0;
        self.interaction.guide_snap_engaged_at = None;
    }

    pub(crate) fn set_guide_snap_indicators(
        &mut self,
        indicators: Vec<crate::canvas::GuideSnapIndicator>,
    ) {
        let signature = guide_snap_signature(&indicators);
        if signature != self.interaction.guide_snap_signature {
            self.interaction.guide_snap_signature = signature;
            self.interaction.guide_snap_engaged_at = if indicators.is_empty() {
                None
            } else {
                Some(std::time::Instant::now())
            };
        }
        self.interaction.guide_snap_indicators = indicators;
    }

    pub(crate) fn guide_snap_pulse_started_at(&self) -> Option<std::time::Instant> {
        self.interaction.guide_snap_engaged_at
    }

    #[cfg(test)]
    pub fn guide_snap_indicator_count(&self) -> usize {
        self.interaction.guide_snap_indicators.len()
    }

    #[cfg(test)]
    pub fn guide_snap_mark_count(&self) -> usize {
        self.interaction
            .guide_snap_indicators
            .iter()
            .map(|indicator| indicator.marks.len())
            .sum()
    }

    pub fn mark_crop_pending(&mut self, placement_id: i64) {
        if !self
            .interaction
            .crop_pending_placement_ids
            .contains(&placement_id)
        {
            self.interaction
                .crop_pending_placement_ids
                .push(placement_id);
        }
    }

    pub fn begin_textbox_edit(&mut self, annotation_id: i64) {
        if self.document.annotations.iter().any(|annotation| {
            annotation.id == annotation_id && annotation.kind == CanvasAnnotationKind::Textbox
        }) {
            self.interaction.selected_annotation_ids = vec![annotation_id];
            self.interaction.editing_text_annotation_id = Some(annotation_id);
            self.interaction.focus_text_editor = true;
            self.interaction.selected_placement_ids.clear();
            self.interaction.end_transform_gesture();
        }
    }

    pub fn selection_screen_rect(&self, rect: egui::Rect) -> Option<egui::Rect> {
        let mut selected_rect: Option<egui::Rect> = None;

        for image in &self.document.placements {
            if !self.is_selected(image.state.id) {
                continue;
            }

            let image_rect = placement_screen_rect(self, rect, image);
            selected_rect = Some(match selected_rect {
                Some(rect) => rect.union(image_rect),
                None => image_rect,
            });
        }

        for annotation in self.selected_annotations() {
            let annotation_rect = annotation_screen_rect(self, rect, annotation);
            selected_rect = Some(match selected_rect {
                Some(existing) => existing.union(annotation_rect),
                None => annotation_rect,
            });
        }

        selected_rect
    }

    pub fn selected_bounds_world(&self) -> Option<egui::Rect> {
        let mut selected_rect: Option<egui::Rect> = None;

        for image in &self.document.placements {
            if !self.is_selected(image.state.id) {
                continue;
            }

            let size = image.scaled_display_size();
            let image_rect = egui::Rect::from_center_size(
                egui::pos2(image.state.geometry.center.x, image.state.geometry.center.y),
                size,
            );
            selected_rect = Some(match selected_rect {
                Some(rect) => rect.union(image_rect),
                None => image_rect,
            });
        }

        selected_rect
    }

    pub fn select_placements(&mut self, placement_ids: &[i64]) {
        self.finish_textbox_edit();
        self.interaction.selected_placement_ids = placement_ids
            .iter()
            .copied()
            .filter(|placement_id| {
                self.document
                    .placements
                    .iter()
                    .any(|image| image.state.id == *placement_id)
            })
            .collect();
        self.interaction.selected_annotation_ids.clear();
        self.interaction.editing_text_annotation_id = None;
        self.interaction.end_transform_gesture();
        self.prune_cached_pixels_to_selected_items();
    }

    pub fn select_content(&mut self, placement_ids: &[i64], annotation_ids: &[i64]) {
        self.interaction.selected_group_ids.clear();
        self.interaction.editing_group_id = None;
        self.finish_textbox_edit();
        self.interaction.selected_placement_ids = placement_ids
            .iter()
            .copied()
            .filter(|placement_id| {
                self.document
                    .placements
                    .iter()
                    .any(|image| image.state.id == *placement_id)
            })
            .collect();
        self.interaction.selected_annotation_ids = annotation_ids
            .iter()
            .copied()
            .filter(|annotation_id| {
                self.document
                    .annotations
                    .iter()
                    .any(|annotation| annotation.id == *annotation_id)
            })
            .collect();
        self.interaction.editing_text_annotation_id = None;
        self.interaction.end_transform_gesture();
        self.prune_cached_pixels_to_selected_items();
    }

    pub fn select_all_content(&mut self) {
        let placement_ids = self.all_placement_ids();
        let annotation_ids = self.all_annotation_ids();
        self.select_content(&placement_ids, &annotation_ids);
        self.interaction.selected_group_ids = self.groups().iter().map(|g| g.id).collect();
    }

    pub(crate) fn selection_border_visible(&self, now_seconds: f64) -> bool {
        let Some(drag_state) = self.interaction.placement_drag() else {
            return true;
        };
        !drag_state.moved
            || now_seconds - drag_state.last_movement_seconds
                >= crate::canvas::SELECTION_BORDER_REAPPEAR_SECONDS
    }

    pub(in crate::canvas) fn is_selected(&self, placement_id: i64) -> bool {
        self.interaction
            .selected_placement_ids
            .contains(&placement_id)
    }

    pub(in crate::canvas) fn set_single_selection(&mut self, placement_id: i64) {
        self.finish_textbox_edit();
        self.interaction.selected_placement_ids.clear();
        self.interaction.selected_placement_ids.push(placement_id);
        self.interaction.selected_group_ids.clear();
        self.interaction.selected_annotation_ids.clear();
        self.expand_group_selection();
        self.interaction.editing_text_annotation_id = None;
        self.interaction.end_transform_gesture();
        self.prune_cached_pixels_to_selected_items();
    }

    /// Outside member editing a Group is selected as a whole, so removing one
    /// member drops the Group and every member with it.
    pub(in crate::canvas) fn remove_from_selection(&mut self, placement_id: i64) {
        let group = self
            .group_for_member(placement_id)
            .filter(|_| self.interaction.editing_group_id.is_none());
        match group {
            Some(group_id) => {
                let members = self
                    .groups()
                    .iter()
                    .find(|group| group.id == group_id)
                    .map(|group| group.members.clone())
                    .unwrap_or_default();
                self.interaction
                    .selected_placement_ids
                    .retain(|id| !members.contains(id));
                self.interaction
                    .selected_group_ids
                    .retain(|id| *id != group_id);
            }
            None => self
                .interaction
                .selected_placement_ids
                .retain(|id| *id != placement_id),
        }
        self.prune_cached_pixels_to_selected_items();
    }

    pub(in crate::canvas) fn add_to_selection(&mut self, placement_id: i64) {
        self.finish_textbox_edit();
        if !self
            .interaction
            .selected_placement_ids
            .contains(&placement_id)
        {
            self.interaction.selected_placement_ids.push(placement_id);
        }
        self.expand_group_selection();
        self.interaction.end_transform_gesture();
        self.prune_cached_pixels_to_selected_items();
    }

    pub(in crate::canvas) fn add_annotation_to_selection(&mut self, annotation_id: i64) {
        self.finish_textbox_edit();
        if !self
            .interaction
            .selected_annotation_ids
            .contains(&annotation_id)
        {
            self.interaction.selected_annotation_ids.push(annotation_id);
        }
        self.interaction.end_transform_gesture();
        self.prune_cached_pixels_to_selected_items();
    }

    pub(in crate::canvas) fn clear_selection(&mut self) {
        self.interaction.selected_group_ids.clear();
        self.interaction.editing_group_id = None;
        self.finish_textbox_edit();
        self.interaction.selected_placement_ids.clear();
        self.interaction.selected_annotation_ids.clear();
        self.interaction.editing_text_annotation_id = None;
        self.interaction.end_transform_gesture();
        self.prune_cached_pixels_to_selected_items();
    }

    pub(in crate::canvas) fn set_marquee_selection(
        &mut self,
        placement_ids: Vec<i64>,
        annotation_ids: Vec<i64>,
        additive: bool,
    ) {
        self.finish_textbox_edit();
        self.interaction.selected_placement_ids = if additive {
            merge_ids(&self.interaction.selected_placement_ids, &placement_ids)
        } else {
            placement_ids
        };
        self.interaction.selected_annotation_ids = if additive {
            merge_ids(&self.interaction.selected_annotation_ids, &annotation_ids)
        } else {
            annotation_ids
        };
        if !additive {
            self.interaction.selected_group_ids.clear();
        }
        self.expand_group_selection();
        self.interaction.editing_text_annotation_id = None;
        self.interaction.end_transform_gesture();
        self.prune_cached_pixels_to_selected_items();
    }

    pub(in crate::canvas) fn selected_single_placement(&self) -> Option<&Placement> {
        (self.interaction.selected_placement_ids.len() == 1)
            .then(|| self.placement_by_id(self.interaction.selected_placement_ids[0]))
            .flatten()
    }

    pub(in crate::canvas) fn selected_annotations(&self) -> Vec<&CanvasAnnotation> {
        self.document
            .annotations
            .iter()
            .filter(|annotation| self.is_annotation_selected(annotation.id))
            .collect()
    }

    pub fn selected_world_bounds(&self) -> Option<egui::Rect> {
        let mut selected_rect = self.selected_bounds_world();
        for group in self
            .groups()
            .iter()
            .filter(|g| self.interaction.selected_group_ids.contains(&g.id))
        {
            let bounds = group.container.unrotated_bounds();
            selected_rect = Some(selected_rect.map_or(bounds, |existing| existing.union(bounds)));
        }

        for annotation in self.selected_annotations() {
            let annotation_rect = annotation_world_rect(annotation);
            selected_rect = Some(match selected_rect {
                Some(existing) => existing.union(annotation_rect),
                None => annotation_rect,
            });
        }

        selected_rect
    }

    pub(in crate::canvas) fn selected_annotation(&self) -> Option<&CanvasAnnotation> {
        let selected_id = self.single_selected_annotation_id()?;
        self.document
            .annotations
            .iter()
            .find(|annotation| annotation.id == selected_id)
    }

    pub(in crate::canvas) fn single_selected_annotation_id(&self) -> Option<i64> {
        if self.interaction.selected_annotation_ids.len() == 1 {
            Some(self.interaction.selected_annotation_ids[0])
        } else {
            None
        }
    }

    pub(in crate::canvas) fn is_annotation_selected(&self, annotation_id: i64) -> bool {
        self.interaction
            .selected_annotation_ids
            .contains(&annotation_id)
    }
}

/// Pan and zoom that frame world `bounds` inside `target_rect`, as Focus Selection does.
pub(crate) fn focus_framing(
    canvas_rect: egui::Rect,
    target_rect: egui::Rect,
    bounds: egui::Rect,
) -> (egui::Vec2, f32) {
    framing(
        canvas_rect,
        target_rect,
        bounds,
        FOCUS_SELECTION_PADDING_SCREEN,
        MAX_CANVAS_ZOOM,
    )
}

fn framing(
    canvas_rect: egui::Rect,
    target_rect: egui::Rect,
    bounds: egui::Rect,
    padding_screen: f32,
    max_zoom: f32,
) -> (egui::Vec2, f32) {
    let padded_size = target_rect
        .shrink(padding_screen)
        .size()
        .max(egui::vec2(1.0, 1.0));
    let width_zoom = padded_size.x / bounds.width().max(1.0);
    let height_zoom = padded_size.y / bounds.height().max(1.0);
    let zoom = width_zoom.min(height_zoom).clamp(MIN_CANVAS_ZOOM, max_zoom);
    let pan = target_rect.center() - canvas_rect.center() - bounds.center().to_vec2() * zoom;
    (pan, zoom)
}

fn guide_snap_signature(indicators: &[crate::canvas::GuideSnapIndicator]) -> u64 {
    use std::hash::{Hash, Hasher};

    let mut hasher = std::hash::DefaultHasher::new();
    indicators.len().hash(&mut hasher);
    for indicator in indicators {
        indicator.segments.len().hash(&mut hasher);
        for (start, end) in &indicator.segments {
            let snapped_coordinate = if (start.x - end.x).abs() <= f32::EPSILON {
                start.x
            } else {
                start.y
            };
            snapped_coordinate.to_bits().hash(&mut hasher);
        }
    }
    hasher.finish()
}
