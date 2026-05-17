use std::sync::Arc;

use eframe::egui;

use super::textbox_editor::textbox_content_rect;
use crate::canvas::interaction::{
    annotation_screen_points, annotation_screen_rect, pending_annotation_from_draft,
};
use crate::canvas::{ANNOTATION_CULL_MARGIN_SCREEN, CanvasState, PendingAnnotation};
#[cfg(test)]
use crate::canvas::{AnnotationPaintStats, CanvasDocumentState};
use crate::data::{
    CanvasAnnotation, CanvasAnnotationColor, CanvasAnnotationKind, CanvasStrokeWidth,
    CanvasTextSize,
};

pub(crate) fn annotation_color(color: CanvasAnnotationColor) -> egui::Color32 {
    match color {
        CanvasAnnotationColor::Default => crate::ui::theme::TEXT,
        CanvasAnnotationColor::Coral => egui::Color32::from_rgb(0xE8, 0x89, 0x73),
        CanvasAnnotationColor::Gold => egui::Color32::from_rgb(0xE0, 0xB4, 0x56),
        CanvasAnnotationColor::Mint => egui::Color32::from_rgb(0x7A, 0xC6, 0xA5),
        CanvasAnnotationColor::Sky => egui::Color32::from_rgb(0x7E, 0xB8, 0xE8),
    }
}

pub(crate) fn stroke_width_value(width: CanvasStrokeWidth) -> f32 {
    match width {
        CanvasStrokeWidth::S => 2.0,
        CanvasStrokeWidth::M => 4.0,
        CanvasStrokeWidth::L => 6.0,
    }
}

pub(crate) fn paint_annotations(
    painter: &egui::Painter,
    ui: &egui::Ui,
    rect: egui::Rect,
    state: &mut CanvasState,
) {
    let cull_rect = rect.expand(ANNOTATION_CULL_MARGIN_SCREEN);
    for index in 0..state.document.annotations.len() {
        let annotation = state.document.annotations[index].clone();
        if !annotation_should_paint(state, rect, cull_rect, &annotation) {
            continue;
        }
        paint_annotation(painter, ui, rect, state, &annotation);
    }
}

pub(crate) fn paint_annotation(
    painter: &egui::Painter,
    ui: &egui::Ui,
    rect: egui::Rect,
    state: &mut CanvasState,
    annotation: &CanvasAnnotation,
) {
    let selected = state.is_annotation_selected(annotation.id);
    let editing = state.interaction.editing_text_annotation_id == Some(annotation.id);
    let color = annotation_color(annotation.color);
    let stroke = if selected || editing {
        egui::Stroke::new(stroke_width_value(annotation.stroke_width), color)
    } else {
        egui::Stroke::new(
            stroke_width_value(annotation.stroke_width),
            egui::Color32::from_rgba_premultiplied(color.r(), color.g(), color.b(), 220),
        )
    };

    match annotation.kind {
        CanvasAnnotationKind::Line => {
            let (start, end) = annotation_screen_points(state, rect, annotation);
            painter.line_segment([start, end], stroke);
        }
        CanvasAnnotationKind::Arrow => {
            let (start, end) = annotation_screen_points(state, rect, annotation);
            paint_arrow_segment(painter, start, end, stroke);
        }
        CanvasAnnotationKind::Textbox => {
            let textbox_rect = annotation_screen_rect(state, rect, annotation);
            let content_rect = textbox_content_rect(textbox_rect);
            if annotation.background_enabled {
                let fill_alpha = if selected || editing { 184 } else { 112 };
                painter.rect_filled(
                    textbox_rect,
                    crate::ui::theme::RADIUS_CONTROL,
                    egui::Color32::from_rgba_premultiplied(
                        crate::ui::theme::BACKGROUND.r(),
                        crate::ui::theme::BACKGROUND.g(),
                        crate::ui::theme::BACKGROUND.b(),
                        fill_alpha,
                    ),
                );
            }
            if selected || editing {
                painter.rect_stroke(
                    textbox_rect,
                    crate::ui::theme::RADIUS_CONTROL,
                    stroke,
                    egui::StrokeKind::Outside,
                );
            }
            if !editing && !annotation.text.trim().is_empty() {
                paint_textbox_text(
                    painter,
                    &state
                        .cached_textbox_layout(
                            ui,
                            annotation.id,
                            &annotation.text,
                            annotation.text_size,
                            annotation.color,
                            content_rect.width(),
                            state.zoom,
                        )
                        .galley,
                    content_rect,
                    color,
                );
            }
        }
    }
}

pub(crate) fn annotation_should_paint(
    state: &CanvasState,
    rect: egui::Rect,
    cull_rect: egui::Rect,
    annotation: &CanvasAnnotation,
) -> bool {
    annotation_screen_rect(state, rect, annotation).intersects(cull_rect)
}

fn paint_textbox_text(
    painter: &egui::Painter,
    galley: &Arc<egui::Galley>,
    content_rect: egui::Rect,
    color: egui::Color32,
) {
    painter
        .with_clip_rect(content_rect)
        .galley(content_rect.min, galley.clone(), color);
}

pub(crate) fn paint_annotation_draft(
    painter: &egui::Painter,
    rect: egui::Rect,
    state: &CanvasState,
) {
    let Some(draft_state) = &state.interaction.annotation_draft_state else {
        return;
    };
    let draft = pending_annotation_from_draft(draft_state.clone()).unwrap_or(PendingAnnotation {
        kind: draft_state.kind,
        start_world: draft_state.start_world,
        end_world: draft_state.current_world,
        text: String::new(),
        text_size: CanvasTextSize::default(),
    });
    let stroke = crate::ui::theme::stroke_strong();
    match draft.kind {
        CanvasAnnotationKind::Line => {
            let start = state.world_to_screen(draft.start_world, rect);
            let end = state.world_to_screen(draft.end_world, rect);
            painter.line_segment([start, end], stroke);
        }
        CanvasAnnotationKind::Arrow => {
            let start = state.world_to_screen(draft.start_world, rect);
            let end = state.world_to_screen(draft.end_world, rect);
            paint_arrow_segment(painter, start, end, stroke);
        }
        CanvasAnnotationKind::Textbox => {
            let textbox_rect = egui::Rect::from_two_pos(
                state.world_to_screen(draft.start_world, rect),
                state.world_to_screen(draft.end_world, rect),
            );
            painter.rect_stroke(
                textbox_rect,
                crate::ui::theme::RADIUS_CONTROL,
                stroke,
                egui::StrokeKind::Outside,
            );
        }
    }
}

fn paint_arrow_segment(
    painter: &egui::Painter,
    start: egui::Pos2,
    end: egui::Pos2,
    stroke: egui::Stroke,
) {
    painter.line_segment([start, end], stroke);
    let direction = end - start;
    let length = direction.length();
    if length <= 1.0 {
        return;
    }
    let unit = direction / length;
    let head = 8.0 + stroke.width * 2.0;
    let left = end - unit * head + egui::vec2(-unit.y, unit.x) * (head * 0.5);
    let right = end - unit * head + egui::vec2(unit.y, -unit.x) * (head * 0.5);
    painter.line_segment([end, left], stroke);
    painter.line_segment([end, right], stroke);
}
#[cfg(test)]
pub(crate) fn annotation_paint_stats_for_annotations(
    annotations: &[CanvasAnnotation],
    rect: egui::Rect,
    pan: egui::Vec2,
    zoom: f32,
    cull_offscreen: bool,
) -> AnnotationPaintStats {
    let state = CanvasState {
        pan,
        zoom,
        document: CanvasDocumentState {
            annotations: annotations.to_vec(),
            ..Default::default()
        },
        ..CanvasState::default()
    };
    let cull_rect = rect.expand(ANNOTATION_CULL_MARGIN_SCREEN);
    let mut stats = AnnotationPaintStats::default();
    for annotation in annotations {
        stats.considered += 1;
        if !cull_offscreen || annotation_should_paint(&state, rect, cull_rect, annotation) {
            stats.drawn += 1;
        }
    }
    stats
}
