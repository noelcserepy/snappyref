use eframe::egui;

use super::super::{
    ANNOTATION_HIT_RADIUS, AnnotationDraftState, AnnotationDragState, CanvasState,
    PendingAnnotation,
};
use super::{normalized_textbox_rect, textbox_screen_rect, textbox_world_rect};
use crate::data::{CanvasAnnotation, CanvasAnnotationKind, CanvasTextSize};

pub(crate) fn begin_annotation_drag(
    state: &mut CanvasState,
    rect: egui::Rect,
    annotation_id: i64,
    pointer_pos: egui::Pos2,
) {
    state.finish_textbox_edit();
    state.document.selected_placement_ids.clear();
    if !state.is_annotation_selected(annotation_id) {
        state.document.selected_annotation_ids = vec![annotation_id];
    }

    let annotation_ids = state
        .document
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
    let starting_points = annotation_ids
        .iter()
        .filter_map(|selected_id| {
            state.annotation_by_id(*selected_id).map(|annotation| {
                (
                    *selected_id,
                    egui::pos2(annotation.start_x, annotation.start_y),
                    egui::pos2(annotation.end_x, annotation.end_y),
                )
            })
        })
        .collect::<Vec<_>>();

    state.interaction.drag_state = None;
    state.interaction.marquee_state = None;
    state.interaction.crop_state = None;
    state.interaction.annotation_draft_state = None;
    state.interaction.textbox_transform_state = None;
    state.interaction.annotation_drag_state = Some(AnnotationDragState {
        annotation_ids,
        pointer_start_world: state.screen_to_world(pointer_pos, rect),
        pointer_start_screen: pointer_pos,
        starting_points,
    });
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
        state.world_to_screen(egui::pos2(annotation.start_x, annotation.start_y), rect),
        state.world_to_screen(egui::pos2(annotation.end_x, annotation.end_y), rect),
    )
}

pub(crate) fn annotation_world_rect(annotation: &CanvasAnnotation) -> egui::Rect {
    match annotation.kind {
        CanvasAnnotationKind::Textbox => textbox_world_rect(annotation),
        CanvasAnnotationKind::Line | CanvasAnnotationKind::Arrow => egui::Rect::from_two_pos(
            egui::pos2(annotation.start_x, annotation.start_y),
            egui::pos2(annotation.end_x, annotation.end_y),
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
            let distance = draft_state.start_world.distance(draft_state.current_world);
            (distance > 2.0).then_some(PendingAnnotation {
                kind: draft_state.kind,
                start_world: draft_state.start_world,
                end_world: draft_state.current_world,
                text: String::new(),
                text_size: CanvasTextSize::default(),
            })
        }
        CanvasAnnotationKind::Textbox => {
            let rect = normalized_textbox_rect(draft_state.start_world, draft_state.current_world);
            Some(PendingAnnotation {
                kind: CanvasAnnotationKind::Textbox,
                start_world: rect.min,
                end_world: rect.max,
                text: String::new(),
                text_size: CanvasTextSize::default(),
            })
        }
    }
}
