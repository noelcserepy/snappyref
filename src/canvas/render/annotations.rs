use eframe::egui;

use super::textbox_editor::{
    LABEL_RADIUS_EM, fit_label_to_text, label_colors, label_text_offset, textbox_font_size,
};
use crate::canvas::interaction::{
    annotation_screen_points, annotation_screen_rect, pending_annotation_from_draft,
    textbox_screen_rect,
};
use crate::canvas::{ANNOTATION_CULL_MARGIN_SCREEN, ActiveGesture, CanvasState, PendingAnnotation};
use crate::data::{
    CanvasAnnotation, CanvasAnnotationColor, CanvasAnnotationGeometry, CanvasAnnotationKind,
    CanvasRectangleFill, CanvasStrokeWidth,
};
use crate::ui::theme;

pub(crate) fn annotation_color(color: CanvasAnnotationColor) -> egui::Color32 {
    match color {
        CanvasAnnotationColor::Default => theme::TEXT,
        CanvasAnnotationColor::Coral => egui::Color32::from_rgb(0xE8, 0x89, 0x73),
        CanvasAnnotationColor::Gold => egui::Color32::from_rgb(0xE0, 0xB4, 0x56),
        CanvasAnnotationColor::Mint => egui::Color32::from_rgb(0x7A, 0xC6, 0xA5),
        CanvasAnnotationColor::Sky => egui::Color32::from_rgb(0x7E, 0xB8, 0xE8),
    }
}

/// Weight of a line, an arrow, or a rectangle's border in world units, so a
/// stroke zooms with the images it points at.
pub(crate) fn stroke_width_value(width: CanvasStrokeWidth) -> f32 {
    match width {
        CanvasStrokeWidth::S => 1.25,
        CanvasStrokeWidth::M => 2.0,
        CanvasStrokeWidth::L => 3.5,
        CanvasStrokeWidth::Xl => 6.0,
        CanvasStrokeWidth::Xxl => 10.0,
    }
}

/// A tinted rectangle's wash over the image it calls out.
const RECTANGLE_TINT_OPACITY: f32 = 0.12;
const RECTANGLE_EDGE_OPACITY: f32 = 0.85;
/// A soft dark edge under every stroke, so a coral arrow still reads on a
/// warm painting.
const STROKE_CASING: egui::Color32 = egui::Color32::from_black_alpha(90);
/// How far the casing shows past each side of a stroke, in world units.
const STROKE_CASING_WIDTH: f32 = 1.0;
const HANDLE_RADIUS: f32 = theme::SPACE_1;

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
    let zoom = state.zoom;
    let width = stroke_width_value(annotation.stroke_width) * zoom;

    match annotation.kind {
        CanvasAnnotationKind::Line => {
            let (start, end) = annotation_screen_points(state, rect, annotation);
            paint_line_segment(painter, start, end, zoom, width, color);
        }
        CanvasAnnotationKind::Arrow => {
            let (start, end) = annotation_screen_points(state, rect, annotation);
            paint_arrow_segment(painter, start, end, zoom, width, color);
        }
        CanvasAnnotationKind::Rectangle => {
            let rectangle = textbox_screen_rect(state, rect, annotation);
            paint_rectangle(
                painter,
                rectangle,
                zoom,
                width,
                color,
                annotation.fill,
                selected,
            );
            if state.single_selected_annotation_id() == Some(annotation.id) {
                for corner in [
                    rectangle.left_top(),
                    rectangle.right_top(),
                    rectangle.left_bottom(),
                    rectangle.right_bottom(),
                ] {
                    paint_handle(painter, corner);
                }
            }
        }
        CanvasAnnotationKind::Textbox => {
            if let Some(index) = state
                .document
                .annotations
                .iter()
                .position(|item| item.id == annotation.id)
            {
                fit_label_to_text(ui.ctx(), state, index);
            }
            let Some(annotation) = state.annotation_by_id(annotation.id).cloned() else {
                return;
            };
            paint_label(painter, ui, rect, state, &annotation, selected, editing);
        }
    }
    if state.single_selected_annotation_id() == Some(annotation.id) && !annotation.kind.is_rect() {
        // Endpoint handles keep their screen size at every zoom.
        let (start, end) = annotation_screen_points(state, rect, annotation);
        for point in [start, end] {
            paint_handle(painter, point);
        }
    }
}

fn paint_handle(painter: &egui::Painter, point: egui::Pos2) {
    painter.circle_filled(point, HANDLE_RADIUS, theme::SURFACE);
    painter.circle_stroke(point, HANDLE_RADIUS, theme::stroke_strong());
}

fn paint_rectangle(
    painter: &egui::Painter,
    rectangle: egui::Rect,
    zoom: f32,
    width: f32,
    color: egui::Color32,
    fill: CanvasRectangleFill,
    selected: bool,
) {
    // Corners scale with the image under them, so a zoomed-out region stays a box.
    let radius = (f32::from(theme::RADIUS_CONTROL) * zoom).min(rectangle.size().min_elem() * 0.25);
    let edge = if selected {
        1.0
    } else {
        RECTANGLE_EDGE_OPACITY
    };
    let fill = match fill {
        CanvasRectangleFill::None => egui::Color32::TRANSPARENT,
        CanvasRectangleFill::Tint => color.gamma_multiply(RECTANGLE_TINT_OPACITY),
        CanvasRectangleFill::Solid => color,
    };
    // Fill and border are separate shapes. As one shape, a border heavier than
    // the corner radius folds the fill's corner over itself, which shows as a
    // dotted diagonal through a tint.
    painter.rect_filled(rectangle, radius, fill);
    painter.rect_stroke(
        rectangle,
        radius,
        egui::Stroke::new(width, color.gamma_multiply(edge)),
        egui::StrokeKind::Inside,
    );
}

fn paint_label(
    painter: &egui::Painter,
    ui: &egui::Ui,
    rect: egui::Rect,
    state: &mut CanvasState,
    annotation: &CanvasAnnotation,
    selected: bool,
    editing: bool,
) {
    let chip = textbox_screen_rect(state, rect, annotation);
    let zoom = state.zoom;
    let em = textbox_font_size(annotation.text_size) * zoom;
    let radius = (LABEL_RADIUS_EM * em).min(255.0);
    painter.add(
        egui::epaint::Shadow {
            offset: [0, 1],
            blur: 6,
            spread: 0,
            color: egui::Color32::from_black_alpha(90),
        }
        .as_shape(chip, radius),
    );
    painter.rect_filled(chip, radius, label_colors(annotation.color).0);
    if selected || editing {
        let ring = if editing { theme::ACCENT } else { theme::TEXT };
        painter.rect_stroke(
            chip,
            radius,
            egui::Stroke::new(1.5, ring),
            egui::StrokeKind::Outside,
        );
    }
    let layout = state.cached_textbox_layout(
        ui,
        annotation.id,
        &annotation.text,
        annotation.text_size,
        annotation.color,
        f32::INFINITY,
        zoom,
    );
    if !editing && !annotation.text.is_empty() {
        let text_pos = chip.min + label_text_offset(annotation.text_size) * zoom;
        painter.galley(text_pos, layout.galley, theme::TEXT);
    }
}

pub(crate) fn annotation_should_paint(
    state: &CanvasState,
    rect: egui::Rect,
    cull_rect: egui::Rect,
    annotation: &CanvasAnnotation,
) -> bool {
    // Group Fill can shrink a textbox to zero, and an empty textbox paints nothing.
    let empty = matches!(
        annotation.geometry,
        CanvasAnnotationGeometry::Rect(r) if r.scaled_size().min_elem() <= 0.0
    );
    !empty && annotation_screen_rect(state, rect, annotation).intersects(cull_rect)
}

pub(crate) fn paint_annotation_draft(
    painter: &egui::Painter,
    rect: egui::Rect,
    state: &CanvasState,
) {
    let Some(ActiveGesture::AnnotationDraft(draft_state)) = &state.interaction.gesture else {
        return;
    };
    let draft = pending_annotation_from_draft(draft_state.clone()).unwrap_or_else(|| {
        PendingAnnotation::new(
            draft_state.kind,
            draft_state.start_world(),
            draft_state.current_world(),
        )
    });
    let style = state.interaction.next_annotation_style;
    let color = annotation_color(style.color);
    let zoom = state.zoom;
    let width = stroke_width_value(style.stroke_width) * zoom;
    let start = state.world_to_screen(draft.start_world(), rect);
    let end = state.world_to_screen(draft.end_world(), rect);
    match draft.kind {
        CanvasAnnotationKind::Line => paint_line_segment(painter, start, end, zoom, width, color),
        CanvasAnnotationKind::Arrow => paint_arrow_segment(painter, start, end, zoom, width, color),
        CanvasAnnotationKind::Rectangle => {
            paint_rectangle(
                painter,
                egui::Rect::from_two_pos(start, end),
                zoom,
                width,
                color,
                style.fill,
                true,
            );
        }
        // A label appears on release, already sized to its caret.
        CanvasAnnotationKind::Textbox => {}
    }
}

/// A stroke with round caps over its casing. The ink is opaque, so a cap
/// never shows where it overlaps the stroke.
fn paint_line_segment(
    painter: &egui::Painter,
    start: egui::Pos2,
    end: egui::Pos2,
    zoom: f32,
    width: f32,
    color: egui::Color32,
) {
    let cased = width + 2.0 * STROKE_CASING_WIDTH * zoom;
    for (stroke_width, stroke_color) in [(cased, STROKE_CASING), (width, color)] {
        painter.line_segment([start, end], egui::Stroke::new(stroke_width, stroke_color));
        painter.circle_filled(start, stroke_width * 0.5, stroke_color);
        painter.circle_filled(end, stroke_width * 0.5, stroke_color);
    }
}

/// A shaft with a round tail and a filled head, over its casing.
fn paint_arrow_segment(
    painter: &egui::Painter,
    start: egui::Pos2,
    end: egui::Pos2,
    zoom: f32,
    width: f32,
    color: egui::Color32,
) {
    let direction = end - start;
    let length = direction.length();
    if length <= 1.0 {
        return;
    }
    let unit = direction / length;
    let normal = egui::vec2(-unit.y, unit.x);
    let head = (6.0 * zoom + width * 2.5).min(length * 0.5);
    let casing = STROKE_CASING_WIDTH * zoom;
    let base = end - unit * head;
    let head_points = vec![
        end,
        base + normal * (head * 0.42),
        base - normal * (head * 0.42),
    ];
    let shaft_end = base + unit * 0.5;
    painter.line_segment(
        [start, shaft_end],
        egui::Stroke::new(width + 2.0 * casing, STROKE_CASING),
    );
    painter.circle_filled(start, width * 0.5 + casing, STROKE_CASING);
    painter.add(egui::Shape::convex_polygon(
        head_points.clone(),
        egui::Color32::TRANSPARENT,
        egui::Stroke::new(2.0 * casing, STROKE_CASING),
    ));
    painter.line_segment([start, shaft_end], egui::Stroke::new(width, color));
    painter.circle_filled(start, width * 0.5, color);
    painter.add(egui::Shape::convex_polygon(
        head_points,
        color,
        egui::Stroke::NONE,
    ));
}
