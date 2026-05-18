use eframe::egui;

use super::image_pass::selection_border_visible;
use crate::canvas::geometry::clamped_crop_screen_rect;
use crate::canvas::interaction::{image_screen_rect, selected_image_rotation_handle_screen_pos};
use crate::canvas::{CanvasState, IMAGE_CULL_MARGIN_SCREEN, IMAGE_ROTATE_HANDLE_RADIUS_SCREEN};
use crate::data::CanvasAnnotationKind;

const SELECTION_BORDER_WIDTH: f32 = 1.0;
const IMAGE_SELECTION_BORDER_COLOR: egui::Color32 = crate::ui::theme::TEXT;
const GUIDE_SNAP_COLOR: egui::Color32 = crate::ui::theme::ACCENT_ORANGE;
const GUIDE_SNAP_X_RADIUS: f32 = 2.0;

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
    let Some(image) = state.image_by_item_id(crop_state.item_id) else {
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

pub(crate) fn paint_import_placeholders(
    painter: &egui::Painter,
    rect: egui::Rect,
    state: &CanvasState,
) {
    const SHIMMER_PERIOD_SECS: f32 = 1.8;

    let cull_rect = rect.expand(IMAGE_CULL_MARGIN_SCREEN);
    let time = painter.ctx().input(|i| i.time) as f32;
    let phase = (time / SHIMMER_PERIOD_SECS).rem_euclid(1.0);
    let mut any_visible = false;

    for placeholder in state
        .interaction
        .import_placeholders
        .iter()
        .chain(state.interaction.hover_preview_placeholders.iter())
    {
        any_visible |=
            paint_import_placeholder(painter, rect, state, cull_rect, placeholder, phase, false);
    }

    if any_visible {
        painter.ctx().request_repaint();
    }
}

fn paint_import_placeholder(
    painter: &egui::Painter,
    rect: egui::Rect,
    state: &CanvasState,
    cull_rect: egui::Rect,
    placeholder: &crate::canvas::ImportPlaceholder,
    phase: f32,
    show_selection_border: bool,
) -> bool {
    const SHIMMER_PEAK_ALPHA: u8 = 40;
    const SHIMMER_BAND_FRACTION: f32 = 0.35;
    const SHIMMER_TILT_FRACTION: f32 = 0.25;

    let placeholder_rect = egui::Rect::from_center_size(
        state.world_to_screen(placeholder.center_world, rect),
        placeholder.size_world * state.zoom,
    );
    if !placeholder_rect.intersects(cull_rect) {
        return false;
    }

    painter.rect_filled(
        placeholder_rect.expand(2.0),
        0.0,
        egui::Color32::from_rgba_premultiplied(0, 0, 0, 120),
    );
    painter.rect_filled(placeholder_rect, 0.0, crate::ui::theme::SURFACE_ELEVATED);
    painter.rect_stroke(
        placeholder_rect,
        crate::ui::theme::RADIUS_NONE,
        egui::Stroke::new(1.0, crate::ui::theme::BORDER_STRONG),
        egui::StrokeKind::Outside,
    );
    if show_selection_border {
        painter.rect_stroke(
            placeholder_rect,
            crate::ui::theme::RADIUS_NONE,
            egui::Stroke::new(SELECTION_BORDER_WIDTH, IMAGE_SELECTION_BORDER_COLOR),
            egui::StrokeKind::Outside,
        );
    }

    if let Some(shimmer) = shimmer_mesh(
        placeholder_rect,
        phase,
        SHIMMER_BAND_FRACTION,
        SHIMMER_TILT_FRACTION,
        SHIMMER_PEAK_ALPHA,
    ) {
        painter.with_clip_rect(placeholder_rect).add(shimmer);
    }

    true
}

fn shimmer_mesh(
    card_rect: egui::Rect,
    phase: f32,
    band_fraction: f32,
    tilt_fraction: f32,
    peak_alpha: u8,
) -> Option<egui::epaint::Mesh> {
    if card_rect.width() <= 1.0 || card_rect.height() <= 1.0 {
        return None;
    }
    let band_width = (card_rect.width() * band_fraction).max(24.0);
    let tilt = card_rect.height() * tilt_fraction;
    let travel_start = card_rect.left() - band_width - tilt;
    let travel_end = card_rect.right() + band_width;
    let center_x = travel_start + phase * (travel_end - travel_start);
    let half = band_width / 2.0;

    let peak = egui::Color32::from_rgba_unmultiplied(255, 255, 255, peak_alpha);
    let edge = egui::Color32::TRANSPARENT;

    let mut mesh = egui::epaint::Mesh::default();
    let mut push_vertex = |pos: egui::Pos2, color: egui::Color32| -> u32 {
        let index = mesh.vertices.len() as u32;
        mesh.vertices.push(egui::epaint::Vertex {
            pos,
            uv: egui::epaint::WHITE_UV,
            color,
        });
        index
    };
    let top = card_rect.top();
    let bottom = card_rect.bottom();
    let left_top = push_vertex(egui::pos2(center_x - half, top), edge);
    let center_top = push_vertex(egui::pos2(center_x, top), peak);
    let right_top = push_vertex(egui::pos2(center_x + half, top), edge);
    let left_bot = push_vertex(egui::pos2(center_x - half - tilt, bottom), edge);
    let center_bot = push_vertex(egui::pos2(center_x - tilt, bottom), peak);
    let right_bot = push_vertex(egui::pos2(center_x + half - tilt, bottom), edge);
    mesh.add_triangle(left_top, center_top, center_bot);
    mesh.add_triangle(left_top, center_bot, left_bot);
    mesh.add_triangle(center_top, right_top, right_bot);
    mesh.add_triangle(center_top, right_bot, center_bot);
    Some(mesh)
}
