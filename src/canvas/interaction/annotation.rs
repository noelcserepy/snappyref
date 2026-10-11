use eframe::egui;

use super::super::{
    ANNOTATION_HIT_RADIUS, ActiveGesture, AnnotationDraftState, AnnotationDragState,
    AnnotationEndpoint, CanvasState, PendingAnnotation,
};
use super::{normalized_textbox_rect, textbox_screen_rect, textbox_world_rect};
use crate::canvas::render::stroke_width_value;
use crate::data::{CanvasAnnotation, CanvasAnnotationKind, CanvasRectangleFill, SegmentGeometry};

pub(crate) fn begin_annotation_drag(
    state: &mut CanvasState,
    rect: egui::Rect,
    annotation_id: i64,
    pointer_pos: egui::Pos2,
    endpoint: Option<AnnotationEndpoint>,
) {
    state.finish_textbox_edit();
    state.interaction.selected_placement_ids.clear();
    if !state.is_annotation_selected(annotation_id) {
        state.interaction.selected_annotation_ids = vec![annotation_id];
    }

    let annotation_ids = state
        .interaction
        .selected_annotation_ids
        .iter()
        .copied()
        .filter(|selected_id| {
            state
                .annotation_by_id(*selected_id)
                .is_some_and(|annotation| {
                    matches!(
                        annotation.kind,
                        CanvasAnnotationKind::Line | CanvasAnnotationKind::Arrow
                    )
                })
        })
        .collect::<Vec<_>>();
    let starting_segments = annotation_ids
        .iter()
        .filter_map(|selected_id| {
            state.annotation_by_id(*selected_id).map(|annotation| {
                // A dragged end starts where its handle shows, which is the
                // label's edge when the end sits on a label.
                let (shown_start, shown_end) = annotation_screen_points(state, rect, annotation);
                let (start, end) = match endpoint {
                    Some(AnnotationEndpoint::Start) => {
                        (state.screen_to_world(shown_start, rect), annotation.end())
                    }
                    Some(AnnotationEndpoint::End) => {
                        (annotation.start(), state.screen_to_world(shown_end, rect))
                    }
                    None => (annotation.start(), annotation.end()),
                };
                (*selected_id, SegmentGeometry::new(start, end, 0))
            })
        })
        .collect::<Vec<_>>();

    state.interaction.gesture = Some(ActiveGesture::AnnotationDrag(AnnotationDragState {
        pointer_start_world: state.screen_to_world(pointer_pos, rect),
        pointer_start_screen: pointer_pos,
        starting_segments,
        endpoint,
    }));
}

pub(crate) fn annotation_endpoint_at_pointer(
    state: &CanvasState,
    rect: egui::Rect,
    annotation: &CanvasAnnotation,
    pointer: egui::Pos2,
) -> Option<AnnotationEndpoint> {
    if annotation.kind.is_rect() {
        return None;
    }
    let (start, end) = annotation_screen_points(state, rect, annotation);
    let start_distance = pointer.distance(start);
    let end_distance = pointer.distance(end);
    if start_distance.min(end_distance) > ANNOTATION_HIT_RADIUS {
        None
    } else if start_distance <= end_distance {
        Some(AnnotationEndpoint::Start)
    } else {
        Some(AnnotationEndpoint::End)
    }
}

/// Reaches the visible pixels of an annotation. Clip hides a member's overflow.
pub(crate) fn hit_test_annotation(
    state: &CanvasState,
    rect: egui::Rect,
    pointer_pos: egui::Pos2,
) -> Option<i64> {
    let world = state.screen_to_world(pointer_pos, rect);
    state
        .document
        .annotations
        .iter()
        .rev()
        .find(|annotation| {
            super::grouped_item_reachable(state, annotation.id, world, true)
                && annotation_hit_test(state, rect, annotation, pointer_pos)
        })
        .map(|annotation| annotation.id)
}

/// Gap between a label's edge and a line or arrow that leaves or reaches it.
const LABEL_CONNECT_GAP: f32 = 3.0;

/// Where a line or arrow shows and takes the pointer. An end that sits on a
/// label stops at the label's edge, so the label reads as its source and
/// keeps its own clicks.
pub(crate) fn annotation_screen_points(
    state: &CanvasState,
    rect: egui::Rect,
    annotation: &CanvasAnnotation,
) -> (egui::Pos2, egui::Pos2) {
    let mut start = state.world_to_screen(annotation.start(), rect);
    let mut end = state.world_to_screen(annotation.end(), rect);
    for label in state
        .document
        .annotations
        .iter()
        .filter(|item| item.kind == CanvasAnnotationKind::Textbox)
    {
        let chip = textbox_screen_rect(state, rect, label).expand(LABEL_CONNECT_GAP);
        if chip.contains(start) && !chip.contains(end) {
            start = exit_point(start, end, chip);
        } else if chip.contains(end) && !chip.contains(start) {
            end = exit_point(end, start, chip);
        }
    }
    (start, end)
}

/// Where the segment from `inside` toward `outside` leaves `bounds`.
fn exit_point(inside: egui::Pos2, outside: egui::Pos2, bounds: egui::Rect) -> egui::Pos2 {
    let direction = outside - inside;
    let mut t = 1.0_f32;
    for (d, from, min, max) in [
        (direction.x, inside.x, bounds.min.x, bounds.max.x),
        (direction.y, inside.y, bounds.min.y, bounds.max.y),
    ] {
        if d > 0.0 {
            t = t.min((max - from) / d);
        } else if d < 0.0 {
            t = t.min((min - from) / d);
        }
    }
    inside + direction * t.clamp(0.0, 1.0)
}

pub(crate) fn annotation_world_rect(annotation: &CanvasAnnotation) -> egui::Rect {
    match annotation.kind {
        CanvasAnnotationKind::Textbox | CanvasAnnotationKind::Rectangle => {
            textbox_world_rect(annotation)
        }
        CanvasAnnotationKind::Line | CanvasAnnotationKind::Arrow => egui::Rect::from_two_pos(
            egui::pos2(annotation.start().x, annotation.start().y),
            egui::pos2(annotation.end().x, annotation.end().y),
        ),
    }
}

pub(crate) fn annotation_screen_rect(
    state: &CanvasState,
    rect: egui::Rect,
    annotation: &CanvasAnnotation,
) -> egui::Rect {
    match annotation.kind {
        CanvasAnnotationKind::Textbox | CanvasAnnotationKind::Rectangle => {
            textbox_screen_rect(state, rect, annotation)
        }
        CanvasAnnotationKind::Line | CanvasAnnotationKind::Arrow => {
            let (start, end) = annotation_screen_points(state, rect, annotation);
            egui::Rect::from_two_pos(start, end).expand(ANNOTATION_HIT_RADIUS)
        }
    }
}

pub(crate) fn annotation_hit_test(
    state: &CanvasState,
    rect: egui::Rect,
    annotation: &CanvasAnnotation,
    pointer_pos: egui::Pos2,
) -> bool {
    match annotation.kind {
        CanvasAnnotationKind::Textbox => {
            annotation_screen_rect(state, rect, annotation).contains(pointer_pos)
        }
        // A rectangle sits over the image it calls out, so only its edge takes
        // the press until it is selected. The image underneath stays reachable.
        // A solid one hides that image and takes the press anywhere.
        CanvasAnnotationKind::Rectangle => {
            let screen_rect = annotation_screen_rect(state, rect, annotation);
            // The edge is drawn inward, so a heavy one takes the press across
            // its whole weight.
            let edge = stroke_width_value(annotation.stroke_width) * state.zoom;
            let near_edge = screen_rect
                .expand(ANNOTATION_HIT_RADIUS)
                .contains(pointer_pos)
                && !screen_rect
                    .shrink(ANNOTATION_HIT_RADIUS.max(edge))
                    .contains(pointer_pos);
            near_edge
                || ((annotation.fill == CanvasRectangleFill::Solid
                    || state.is_annotation_selected(annotation.id))
                    && screen_rect.contains(pointer_pos))
        }
        CanvasAnnotationKind::Line | CanvasAnnotationKind::Arrow => {
            let (start, end) = annotation_screen_points(state, rect, annotation);
            let half_width = stroke_width_value(annotation.stroke_width) * state.zoom * 0.5;
            distance_to_segment(pointer_pos, start, end) <= ANNOTATION_HIT_RADIUS.max(half_width)
        }
    }
}

fn distance_to_segment(point: egui::Pos2, start: egui::Pos2, end: egui::Pos2) -> f32 {
    let segment = end - start;
    let length_sq = segment.length_sq();
    if length_sq <= f32::EPSILON {
        return point.distance(start);
    }
    let t = ((point - start).dot(segment) / length_sq).clamp(0.0, 1.0);
    let projection = start + segment * t;
    point.distance(projection)
}

pub(crate) fn pending_annotation_from_draft(
    draft_state: AnnotationDraftState,
) -> Option<PendingAnnotation> {
    match draft_state.kind {
        CanvasAnnotationKind::Line | CanvasAnnotationKind::Arrow => {
            let distance = draft_state
                .start_world()
                .distance(draft_state.current_world());
            (distance > 2.0).then_some(PendingAnnotation::new(
                draft_state.kind,
                draft_state.start_world(),
                draft_state.current_world(),
            ))
        }
        // A label sizes itself to its text, so only the press point matters.
        CanvasAnnotationKind::Textbox => {
            let start = draft_state.start_world();
            Some(PendingAnnotation::new(
                CanvasAnnotationKind::Textbox,
                start,
                start + egui::vec2(1.0, 1.0),
            ))
        }
        CanvasAnnotationKind::Rectangle => {
            let rect =
                egui::Rect::from_two_pos(draft_state.start_world(), draft_state.current_world());
            (rect.width() > 4.0 && rect.height() > 4.0).then(|| {
                let rect = normalized_textbox_rect(rect.min, rect.max);
                PendingAnnotation::new(CanvasAnnotationKind::Rectangle, rect.min, rect.max)
            })
        }
    }
}
