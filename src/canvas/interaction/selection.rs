use eframe::egui;

use super::super::geometry::placement_contains_screen_point;
use super::super::{ActiveGesture, CanvasContextMenuRequest, CanvasHit, CanvasState, MarqueeState};
use super::{annotation_hit_test, annotation_screen_rect, placement_screen_rect};
use crate::data::CanvasStackItem;

pub(crate) fn open_context_menu_at_pointer(
    state: &mut CanvasState,
    rect: egui::Rect,
    pointer_pos: egui::Pos2,
) {
    if let Some(hit) = hit_test_canvas_item(state, rect, pointer_pos) {
        match hit {
            CanvasHit::Annotation(annotation_id) => {
                if !state.is_annotation_selected(annotation_id) {
                    state.finish_textbox_edit();
                    state.document.selected_placement_ids.clear();
                    state.document.selected_annotation_ids = vec![annotation_id];
                }
            }
            CanvasHit::Placement(placement_id) => {
                if !state.is_selected(placement_id) {
                    state.set_single_selection(placement_id);
                }
            }
        }
    }

    state.interaction.end_gesture();
    state.interaction.pending_context_menu_request = Some(CanvasContextMenuRequest {
        screen_pos: pointer_pos,
    });
}

pub(crate) fn hit_test_canvas_item(
    state: &CanvasState,
    rect: egui::Rect,
    pointer_pos: egui::Pos2,
) -> Option<CanvasHit> {
    state
        .ordered_canvas_items_back_to_front()
        .into_iter()
        .rev()
        .find_map(|item| match item {
            CanvasStackItem::Placement(placement_id) => state
                .rendered_placement_by_id(placement_id)
                .filter(|image| {
                    placement_contains_screen_point(
                        &image.placement,
                        pointer_pos,
                        state.pan,
                        state.zoom,
                        rect.center(),
                    )
                })
                .map(|_| CanvasHit::Placement(placement_id)),
            CanvasStackItem::Annotation(annotation_id) => state
                .annotation_by_id(annotation_id)
                .filter(|annotation| annotation_hit_test(state, rect, annotation, pointer_pos))
                .map(|_| CanvasHit::Annotation(annotation_id)),
        })
}

pub(crate) fn begin_marquee_selection(
    state: &mut CanvasState,
    pointer_pos: egui::Pos2,
    shift_held: bool,
) {
    if !shift_held {
        state.clear_selection();
    }
    state.interaction.gesture = Some(ActiveGesture::Marquee(MarqueeState {
        start_screen: pointer_pos,
        current_screen: pointer_pos,
    }));
}

pub(crate) fn update_marquee_selection(
    state: &mut CanvasState,
    rect: egui::Rect,
    pointer_pos: egui::Pos2,
    shift_held: bool,
) {
    let Some(ActiveGesture::Marquee(marquee_state)) = &mut state.interaction.gesture else {
        return;
    };
    marquee_state.current_screen = pointer_pos;
    let selection_rect =
        egui::Rect::from_two_pos(marquee_state.start_screen, marquee_state.current_screen);
    let selected = state
        .document
        .rendered_placements
        .iter()
        .filter(|image| placement_screen_rect(state, rect, image).intersects(selection_rect))
        .map(|image| image.placement.state.id)
        .collect::<Vec<_>>();
    let selected_annotation_ids = state
        .document
        .annotations
        .iter()
        .filter(|annotation| {
            annotation_screen_rect(state, rect, annotation).intersects(selection_rect)
        })
        .map(|annotation| annotation.id)
        .collect::<Vec<_>>();
    state.set_marquee_selection(selected, selected_annotation_ids, shift_held);
}
