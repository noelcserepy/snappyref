use eframe::egui;

use super::super::{
    ANNOTATION_HIT_RADIUS, ActiveGesture, AnnotationDraftState, AnnotationDragState,
    AnnotationEndpoint, CanvasState, PendingAnnotation,
};
use super::{normalized_textbox_rect, textbox_screen_rect, textbox_world_rect};
use crate::data::{CanvasAnnotation, CanvasAnnotationKind, CanvasTextSize, SegmentGeometry};

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
                (
                    *selected_id,
                    SegmentGeometry::new(annotation.start(), annotation.end(), 0),
                )
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
    if annotation.kind == CanvasAnnotationKind::Textbox {
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

pub(crate) fn hit_test_annotation(
    state: &CanvasState,
    rect: egui::Rect,
    pointer_pos: egui::Pos2,
) -> Option<i64> {
    state
        .document
        .annotations
        .iter()
        .rev()
        .find(|annotation| annotation_hit_test(state, rect, annotation, pointer_pos))
        .map(|annotation| annotation.id)
}

pub(crate) fn annotation_screen_points(
    state: &CanvasState,
    rect: egui::Rect,
    annotation: &CanvasAnnotation,
) -> (egui::Pos2, egui::Pos2) {
    (
        state.world_to_screen(egui::pos2(annotation.start().x, annotation.start().y), rect),
        state.world_to_screen(egui::pos2(annotation.end().x, annotation.end().y), rect),
    )
}

pub(crate) fn annotation_world_rect(annotation: &CanvasAnnotation) -> egui::Rect {
    match annotation.kind {
        CanvasAnnotationKind::Textbox => textbox_world_rect(annotation),
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
        CanvasAnnotationKind::Textbox => textbox_screen_rect(state, rect, annotation),
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
        CanvasAnnotationKind::Line | CanvasAnnotationKind::Arrow => {
            let (start, end) = annotation_screen_points(state, rect, annotation);
            distance_to_segment(pointer_pos, start, end) <= ANNOTATION_HIT_RADIUS
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
                String::new(),
                CanvasTextSize::default(),
            ))
        }
        CanvasAnnotationKind::Textbox => {
            let rect =
                normalized_textbox_rect(draft_state.start_world(), draft_state.current_world());
            Some(PendingAnnotation::new(
                CanvasAnnotationKind::Textbox,
                rect.min,
                rect.max,
                String::new(),
                CanvasTextSize::default(),
            ))
        }
    }
}
