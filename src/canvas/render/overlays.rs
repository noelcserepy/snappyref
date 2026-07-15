use eframe::egui;

use super::image_pass::selection_border_visible;
use crate::canvas::geometry::{clamped_crop_screen_rect, screen_corners_for_placement};
use crate::canvas::interaction::{
    hit_test_placement, placement_screen_rect, selected_placement_rotation_handle_screen_pos,
};
use crate::canvas::{CanvasState, IMAGE_CULL_MARGIN_SCREEN, PLACEMENT_ROTATE_HANDLE_RADIUS_SCREEN};
use crate::data::{CanvasAnnotationKind, CanvasStackItem, PlacementLifecycle};

const SELECTION_BORDER_WIDTH: f32 = 1.0;
const IMAGE_SELECTION_BORDER_COLOR: egui::Color32 = crate::ui::theme::TEXT;
const GUIDE_SNAP_COLOR: egui::Color32 = crate::ui::theme::ACCENT_ORANGE;
const GUIDE_SNAP_X_RADIUS: f32 = 2.0;

/// Provisional search/import previews use the same square 1px image-selection
/// border as selected canvas Placements and selected search-result thumbnails.
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
        let image_rect = placement_screen_rect(state, rect, image);
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

    let image_rect = placement_screen_rect(state, rect, image);
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

pub(crate) fn paint_hover_hairline(
    painter: &egui::Painter,
    rect: egui::Rect,
    state: &CanvasState,
    pointer_interactions_enabled: bool,
) {
    let interaction_active = state.interaction.drag_state.is_some()
        || state.interaction.marquee_state.is_some()
        || state.interaction.crop_state.is_some()
        || state.interaction.annotation_draft_state.is_some()
        || state.interaction.placement_transform_state.is_some();
    let hovered_id = if pointer_interactions_enabled && !interaction_active {
        painter
            .ctx()
            .input(|input| input.pointer.hover_pos())
            .filter(|pos| rect.contains(*pos))
            .and_then(|pos| hit_test_placement(state, rect, pos))
    } else {
        None
    };

    let cull_rect = rect.expand(IMAGE_CULL_MARGIN_SCREEN);
    for image in &state.document.rendered_placements {
        let placement_id = image.placement.state.id;
        let image_rect = placement_screen_rect(state, rect, image);
        if !image_rect.intersects(cull_rect) {
            continue;
        }
        let hot = hovered_id == Some(placement_id)
            && !state.is_selected(placement_id)
            && image.placement.lifecycle != PlacementLifecycle::Provisional;
        let fade = crate::ui::motion::hover_dissolve(
            painter.ctx(),
            egui::Id::new(("placement_hover_hairline", placement_id)),
            hot,
        );
        if fade <= 0.0 || state.is_selected(placement_id) {
            continue;
        }
        let corners = screen_corners_for_placement(
            &image.placement,
            state.pan,
            state.zoom,
            painter.clip_rect().center(),
        );
        painter.add(egui::Shape::closed_line(
            corners.to_vec(),
            egui::Stroke::new(
                SELECTION_BORDER_WIDTH,
                IMAGE_SELECTION_BORDER_COLOR
                    .gamma_multiply(crate::ui::theme::HOVER_HAIRLINE_OPACITY * fade),
            ),
        ));
    }
}

pub(crate) fn paint_selected_placement_borders(
    painter: &egui::Painter,
    rect: egui::Rect,
    state: &CanvasState,
) {
    let now_seconds = painter.ctx().input(|input| input.time);
    for placement_id in selected_placement_border_paint_ids(state, rect, now_seconds) {
        let Some(image) = state.rendered_placement_by_id(placement_id) else {
            continue;
        };
        let corners = screen_corners_for_placement(
            &image.placement,
            state.pan,
            state.zoom,
            painter.clip_rect().center(),
        );
        painter.add(egui::Shape::closed_line(
            corners.to_vec(),
            egui::Stroke::new(SELECTION_BORDER_WIDTH, IMAGE_SELECTION_BORDER_COLOR),
        ));
    }
}

/// Selected placement borders are painted in a dedicated top overlay pass.
///
/// The selected Placement's own Stack Order still controls its image content,
/// but the selection border must remain visible above every Placement,
/// including higher-z neighbors that overlap the selected image.
pub(super) fn selected_placement_border_paint_ids(
    state: &CanvasState,
    rect: egui::Rect,
    now_seconds: f64,
) -> Vec<i64> {
    let cull_rect = rect.expand(IMAGE_CULL_MARGIN_SCREEN);
    state
        .ordered_canvas_items_back_to_front()
        .into_iter()
        .filter_map(|item| match item {
            CanvasStackItem::Placement(placement_id) => Some(placement_id),
            CanvasStackItem::Annotation(_) => None,
        })
        .filter(|placement_id| {
            let Some(image) = state.rendered_placement_by_id(*placement_id) else {
                return false;
            };
            state.is_selected(*placement_id)
                && placement_screen_rect(state, rect, image).intersects(cull_rect)
                && (image.placement.lifecycle == PlacementLifecycle::Provisional
                    || selection_border_visible(state, now_seconds))
        })
        .collect()
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
    let pulse = crate::ui::motion::pulse_decay(
        painter.ctx(),
        egui::Id::new("guide_snap_pulse"),
        state.guide_snap_pulse_started_at(),
    )
    .unwrap_or(0.0);
    let stroke = egui::Stroke::new(
        1.0 + pulse,
        GUIDE_SNAP_COLOR.lerp_to_gamma(crate::ui::theme::TEXT, 0.35 * pulse),
    );
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
    let visible = rotation_handle_visible(painter.ctx(), state);
    let appear = crate::ui::motion::acquire_amount(
        painter.ctx(),
        egui::Id::new("rotation_handle_appear"),
        visible,
    );
    if appear <= 0.01 {
        return;
    }
    let Some((line_start, handle_pos)) = selected_rotation_handle_screen_pos(state, rect) else {
        return;
    };
    let radius = PLACEMENT_ROTATE_HANDLE_RADIUS_SCREEN * appear.clamp(0.0, 1.0);
    painter.line_segment([line_start, handle_pos], crate::ui::theme::stroke_strong());
    painter.circle_filled(handle_pos, radius, crate::ui::theme::SURFACE);
    painter.circle_stroke(handle_pos, radius, crate::ui::theme::stroke_strong());
}

fn selected_rotation_handle_screen_pos(
    state: &CanvasState,
    rect: egui::Rect,
) -> Option<(egui::Pos2, egui::Pos2)> {
    selected_placement_rotation_handle_screen_pos(state, rect)
}

fn rotation_handle_visible(ctx: &egui::Context, state: &CanvasState) -> bool {
    crate::canvas::rotation_handle_key_held(ctx)
        || matches!(
            state.interaction.placement_transform_state,
            Some(crate::canvas::PlacementTransformState::Rotating { .. })
        )
}
