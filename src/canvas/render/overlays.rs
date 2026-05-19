use eframe::egui;

use super::image_pass::selection_border_visible;
use crate::canvas::geometry::clamped_crop_screen_rect;
use crate::canvas::interaction::{image_screen_rect, selected_image_rotation_handle_screen_pos};
use crate::canvas::{CanvasState, IMAGE_ROTATE_HANDLE_RADIUS_SCREEN};
use crate::data::{CanvasAnnotationKind, PlacementLifecycle};

const SELECTION_BORDER_WIDTH: f32 = 1.0;
const IMAGE_SELECTION_BORDER_COLOR: egui::Color32 = crate::ui::theme::TEXT;
const GUIDE_SNAP_COLOR: egui::Color32 = crate::ui::theme::ACCENT_ORANGE;
const GUIDE_SNAP_X_RADIUS: f32 = 2.0;

pub(crate) fn paint_provisional_placement_previews(
    painter: &egui::Painter,
    rect: egui::Rect,
    state: &CanvasState,
) {
    let stroke = egui::Stroke::new(SELECTION_BORDER_WIDTH, IMAGE_SELECTION_BORDER_COLOR);
    let mut painted = Vec::new();
    for image in state
        .document
        .rendered_placements
        .iter()
        .filter(|image| image.placement.lifecycle == PlacementLifecycle::Provisional)
    {
        if state.is_selected(image.placement.state.id) {
            continue;
        }
        let image_rect = image_screen_rect(state, rect, image);
        painted.push((image.placement.state.id, image_rect));
        painter.rect_stroke(
            image_rect,
            crate::ui::theme::RADIUS_NONE,
            stroke,
            egui::StrokeKind::Outside,
        );
    }
    if !painted.is_empty() {
        crate::diagnostics::search_drag_debug(format!(
            "paint_provisional_placement_previews count={} rects={painted:?}",
            painted.len()
        ));
    }
}

pub(crate) fn paint_marquee(painter: &egui::Painter, state: &CanvasState) {
    let Some(marquee_state) = &state.interaction.marquee_state else {
        return;
    };

    let rect = egui::Rect::from_two_pos(marquee_state.start_screen, marquee_state.current_screen);
    painter.rect_stroke(
        rect,
        0.0,
        crate::ui::theme::stroke_strong(),
        egui::StrokeKind::Outside,
    );
}

pub(crate) fn paint_crop_marquee(painter: &egui::Painter, rect: egui::Rect, state: &CanvasState) {
    let Some(crop_state) = &state.interaction.crop_state else {
        return;
    };
    let Some(image) = state.rendered_placement_by_id(crop_state.placement_id) else {
        return;
    };

    let image_rect = image_screen_rect(state, rect, image);
    let Some(crop_rect) = clamped_crop_screen_rect(
        crop_state.start_screen,
        crop_state.current_screen,
        image_rect,
    ) else {
        return;
    };

    painter.rect_stroke(
        image_rect,
        0.0,
        crate::ui::theme::stroke_strong(),
        egui::StrokeKind::Outside,
    );
    painter.rect_stroke(
        crop_rect,
        0.0,
        crate::ui::theme::stroke_strong(),
        egui::StrokeKind::Outside,
    );
}

pub(crate) fn paint_selection_bounds(
    painter: &egui::Painter,
    rect: egui::Rect,
    state: &CanvasState,
) {
    if !selection_border_visible(state, painter.ctx().input(|input| input.time)) {
        return;
    }

    if state.document.selected_annotation_ids.is_empty()
        && state.document.selected_placement_ids.len() <= 1
    {
        paint_rotation_handle(painter, rect, state);
        return;
    }

    if state.document.selected_placement_ids.is_empty()
        && let Some(annotation) = state.selected_annotation()
        && annotation.kind == CanvasAnnotationKind::Textbox
    {
        return;
    }

    let Some(selection_rect) = state.selection_screen_rect(rect) else {
        return;
    };

    painter.rect_stroke(
        selection_rect,
        crate::ui::theme::RADIUS_NONE,
        egui::Stroke::new(SELECTION_BORDER_WIDTH, IMAGE_SELECTION_BORDER_COLOR),
        egui::StrokeKind::Outside,
    );
    paint_rotation_handle(painter, rect, state);
}

pub(crate) fn paint_guide_snap_indicators(
    painter: &egui::Painter,
    rect: egui::Rect,
    state: &CanvasState,
) {
    let stroke = egui::Stroke::new(1.0, GUIDE_SNAP_COLOR);
    for indicator in &state.interaction.guide_snap_indicators {
        for (start_world, end_world) in &indicator.segments {
            let start = state.world_to_screen(*start_world, rect);
            let end = state.world_to_screen(*end_world, rect);
            painter.line_segment([start, end], stroke);
        }
        for marker_world in &indicator.marks {
            let marker = state.world_to_screen(*marker_world, rect);
            painter.line_segment(
                [
                    marker + egui::vec2(-GUIDE_SNAP_X_RADIUS, -GUIDE_SNAP_X_RADIUS),
                    marker + egui::vec2(GUIDE_SNAP_X_RADIUS, GUIDE_SNAP_X_RADIUS),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    marker + egui::vec2(-GUIDE_SNAP_X_RADIUS, GUIDE_SNAP_X_RADIUS),
                    marker + egui::vec2(GUIDE_SNAP_X_RADIUS, -GUIDE_SNAP_X_RADIUS),
                ],
                stroke,
            );
        }
    }
}

fn paint_rotation_handle(painter: &egui::Painter, rect: egui::Rect, state: &CanvasState) {
    if !rotation_handle_visible(painter.ctx(), state) {
        return;
    }
    let Some((line_start, handle_pos)) = selected_rotation_handle_screen_pos(state, rect) else {
        return;
    };
    painter.line_segment([line_start, handle_pos], crate::ui::theme::stroke_strong());
    painter.circle_filled(
        handle_pos,
        IMAGE_ROTATE_HANDLE_RADIUS_SCREEN,
        crate::ui::theme::SURFACE,
    );
    painter.circle_stroke(
        handle_pos,
        IMAGE_ROTATE_HANDLE_RADIUS_SCREEN,
        crate::ui::theme::stroke_strong(),
    );
}

fn selected_rotation_handle_screen_pos(
    state: &CanvasState,
    rect: egui::Rect,
) -> Option<(egui::Pos2, egui::Pos2)> {
    selected_image_rotation_handle_screen_pos(state, rect)
}

fn rotation_handle_visible(ctx: &egui::Context, state: &CanvasState) -> bool {
    crate::canvas::rotation_handle_key_held(ctx)
        || matches!(
            state.interaction.image_transform_state,
            Some(crate::canvas::ImageTransformState::Rotating { .. })
        )
}
