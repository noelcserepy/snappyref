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
                    state.interaction.selected_placement_ids.clear();
                    state.interaction.selected_annotation_ids = vec![annotation_id];
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
    hit_test_items(state, rect, pointer_pos, false)
}

/// Double-click and primary-modifier click also reach member overflow that Clip leaves visible.
pub(crate) fn hit_test_member(
    state: &CanvasState,
    rect: egui::Rect,
    pointer_pos: egui::Pos2,
) -> Option<CanvasHit> {
    hit_test_items(state, rect, pointer_pos, true)
}

/// Plain clicks reach grouped items only inside the container, so the container
/// alone is the Group hit area. Clip hides overflow from member editing too.
pub(crate) fn grouped_item_reachable(
    state: &CanvasState,
    id: i64,
    world: egui::Pos2,
    member_editing: bool,
) -> bool {
    state.group_for_member(id).is_none_or(|g| {
        state
            .groups()
            .iter()
            .find(|group| group.id == g)
            .is_some_and(|group| {
                (member_editing && !group.layout.clip)
                    || group.container.unrotated_bounds().contains(world)
            })
    })
}

fn hit_test_items(
    state: &CanvasState,
    rect: egui::Rect,
    pointer_pos: egui::Pos2,
    member_editing: bool,
) -> Option<CanvasHit> {
    let world = state.screen_to_world(pointer_pos, rect);
    state
        .ordered_canvas_items_back_to_front()
        .into_iter()
        .rev()
        .filter(|item| {
            let id = match *item {
                CanvasStackItem::Placement(id) | CanvasStackItem::Annotation(id) => id,
            };
            state
                .group_member_geometry(id)
                .is_none_or(|r| r.scaled_size().min_elem() > 0.0)
                && grouped_item_reachable(state, id, world, member_editing)
        })
        .find_map(|item| match item {
            CanvasStackItem::Placement(placement_id) => state
                .placement_by_id(placement_id)
                .filter(|image| {
                    placement_contains_screen_point(
                        image,
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
        .placements
        .iter()
        .filter(|image| {
            visible_screen_rect(
                state,
                rect,
                image.state.id,
                placement_screen_rect(state, rect, image),
            )
            .is_some_and(|r| r.intersects(selection_rect))
        })
        .map(|image| image.state.id)
        .collect::<Vec<_>>();
    let selected_annotation_ids = state
        .document
        .annotations
        .iter()
        .filter(|annotation| {
            visible_screen_rect(
                state,
                rect,
                annotation.id,
                annotation_screen_rect(state, rect, annotation),
            )
            .is_some_and(|r| r.intersects(selection_rect))
        })
        .map(|annotation| annotation.id)
        .collect::<Vec<_>>();
    state.set_marquee_selection(selected, selected_annotation_ids, shift_held);
}

/// Clip hides a member's pixels outside its container, so marquee can't reach them.
fn visible_screen_rect(
    state: &CanvasState,
    rect: egui::Rect,
    id: i64,
    item: egui::Rect,
) -> Option<egui::Rect> {
    let Some(group) = state
        .groups()
        .iter()
        .find(|group| group.layout.clip && group.members.contains(&id))
    else {
        return Some(item);
    };
    let bounds = group.container.unrotated_bounds();
    let container = egui::Rect::from_min_max(
        state.world_to_screen(bounds.min, rect),
        state.world_to_screen(bounds.max, rect),
    );
    let visible = item.intersect(container);
    visible.is_positive().then_some(visible)
}
