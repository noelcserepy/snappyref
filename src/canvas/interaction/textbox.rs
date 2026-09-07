use eframe::egui;

use super::super::render::textbox_pointer_region;
use super::super::{
    ActiveGesture, CanvasState, MIN_TEXTBOX_WORLD_SIZE, TEXTBOX_RESIZE_HIT_RADIUS,
    TextboxPointerRegion, TextboxResizeHandle, TextboxTransformState,
};
use crate::data::{CanvasAnnotation, CanvasAnnotationKind};

fn begin_textbox_pending_move(
    state: &mut CanvasState,
    rect: egui::Rect,
    annotation: &CanvasAnnotation,
    pointer_pos: egui::Pos2,
) {
    state.interaction.selected_placement_ids.clear();
    if !state.is_annotation_selected(annotation.id) {
        state.interaction.selected_annotation_ids = vec![annotation.id];
    }
    let annotation_ids = state
        .interaction
        .selected_annotation_ids
        .iter()
        .copied()
        .filter(|annotation_id| {
            state
                .annotation_by_id(*annotation_id)
                .is_some_and(|annotation| annotation.kind == CanvasAnnotationKind::Textbox)
        })
        .collect::<Vec<_>>();
    let start_rects_world = state.selected_textbox_start_rects();
    state.interaction.gesture = Some(ActiveGesture::TextboxTransform(
        TextboxTransformState::PendingMove {
            annotation_ids,
            pointer_start_world: state.screen_to_world(pointer_pos, rect),
            pointer_start_screen: pointer_pos,
            start_rects_world,
        },
    ));
}

pub(crate) fn handle_textbox_primary_press(
    ui: &egui::Ui,
    rect: egui::Rect,
    state: &mut CanvasState,
    annotation: &CanvasAnnotation,
    pointer_pos: egui::Pos2,
) -> bool {
    let Some(region) = textbox_pointer_region(ui, rect, state, annotation, pointer_pos) else {
        return false;
    };

    match region {
        TextboxPointerRegion::Resize(handle) => {
            if state.single_selected_annotation_id() != Some(annotation.id) {
                if state.interaction.editing_text_annotation_id == Some(annotation.id) {
                    state.stop_textbox_edit_without_cleanup();
                } else if state.interaction.editing_text_annotation_id.is_some() {
                    state.finish_textbox_edit();
                }
                begin_textbox_pending_move(state, rect, annotation, pointer_pos);
                return true;
            }
            let world_rect = textbox_world_rect(annotation);
            state.interaction.selected_placement_ids.clear();
            state.interaction.selected_annotation_ids = vec![annotation.id];
            state.interaction.gesture = Some(ActiveGesture::TextboxTransform(
                TextboxTransformState::Resizing {
                    annotation_id: annotation.id,
                    dragged_handle: handle,
                    anchor_world: textbox_resize_handle_anchor_world_pos(world_rect, handle),
                    start_rect_world: crate::data::RectGeometry::from_bounds(world_rect, 0),
                },
            ));
            true
        }
        TextboxPointerRegion::Text => true,
        TextboxPointerRegion::Body => {
            if state.interaction.editing_text_annotation_id == Some(annotation.id) {
                state.stop_textbox_edit_without_cleanup();
            } else if state.interaction.editing_text_annotation_id.is_some() {
                state.finish_textbox_edit();
            }
            begin_textbox_pending_move(state, rect, annotation, pointer_pos);
            true
        }
    }
}

pub(crate) fn textbox_world_rect(annotation: &CanvasAnnotation) -> egui::Rect {
    normalized_textbox_rect(
        egui::pos2(annotation.start().x, annotation.start().y),
        egui::pos2(annotation.end().x, annotation.end().y),
    )
}

pub(crate) fn textbox_screen_rect(
    state: &CanvasState,
    rect: egui::Rect,
    annotation: &CanvasAnnotation,
) -> egui::Rect {
    let world_rect = textbox_world_rect(annotation);
    egui::Rect::from_min_max(
        state.world_to_screen(world_rect.min, rect),
        state.world_to_screen(world_rect.max, rect),
    )
}

fn textbox_resize_handle_screen_pos(
    textbox_rect: egui::Rect,
    handle: TextboxResizeHandle,
) -> egui::Pos2 {
    match handle {
        TextboxResizeHandle::TopLeft => textbox_rect.left_top(),
        TextboxResizeHandle::TopRight => textbox_rect.right_top(),
        TextboxResizeHandle::BottomLeft => textbox_rect.left_bottom(),
        TextboxResizeHandle::BottomRight => textbox_rect.right_bottom(),
        TextboxResizeHandle::Top => textbox_rect.center_top(),
        TextboxResizeHandle::Right => textbox_rect.right_center(),
        TextboxResizeHandle::Bottom => textbox_rect.center_bottom(),
        TextboxResizeHandle::Left => textbox_rect.left_center(),
    }
}

fn textbox_resize_handle_anchor_world_pos(
    textbox_rect: egui::Rect,
    handle: TextboxResizeHandle,
) -> egui::Pos2 {
    match handle {
        TextboxResizeHandle::TopLeft => textbox_rect.right_bottom(),
        TextboxResizeHandle::TopRight => textbox_rect.left_bottom(),
        TextboxResizeHandle::BottomLeft => textbox_rect.right_top(),
        TextboxResizeHandle::BottomRight => textbox_rect.left_top(),
        TextboxResizeHandle::Top => textbox_rect.center_bottom(),
        TextboxResizeHandle::Right => textbox_rect.left_center(),
        TextboxResizeHandle::Bottom => textbox_rect.center_top(),
        TextboxResizeHandle::Left => textbox_rect.right_center(),
    }
}

pub(crate) fn textbox_resize_cursor_icon(handle: TextboxResizeHandle) -> egui::CursorIcon {
    match handle {
        TextboxResizeHandle::TopLeft | TextboxResizeHandle::BottomRight => {
            egui::CursorIcon::ResizeNwSe
        }
        TextboxResizeHandle::TopRight | TextboxResizeHandle::BottomLeft => {
            egui::CursorIcon::ResizeNeSw
        }
        TextboxResizeHandle::Top | TextboxResizeHandle::Bottom => egui::CursorIcon::ResizeVertical,
        TextboxResizeHandle::Left | TextboxResizeHandle::Right => {
            egui::CursorIcon::ResizeHorizontal
        }
    }
}

pub(crate) fn hit_test_textbox_resize_handle(
    state: &CanvasState,
    rect: egui::Rect,
    annotation: &CanvasAnnotation,
    pointer_pos: egui::Pos2,
) -> Option<TextboxResizeHandle> {
    let textbox_rect = textbox_screen_rect(state, rect, annotation);
    [
        TextboxResizeHandle::TopLeft,
        TextboxResizeHandle::TopRight,
        TextboxResizeHandle::BottomLeft,
        TextboxResizeHandle::BottomRight,
    ]
    .into_iter()
    .find(|corner| {
        textbox_resize_handle_screen_pos(textbox_rect, *corner).distance(pointer_pos)
            <= TEXTBOX_RESIZE_HIT_RADIUS
    })
    .or_else(|| {
        let top = egui::Rect::from_min_max(
            egui::pos2(
                textbox_rect.left() + TEXTBOX_RESIZE_HIT_RADIUS,
                textbox_rect.top() - TEXTBOX_RESIZE_HIT_RADIUS,
            ),
            egui::pos2(
                textbox_rect.right() - TEXTBOX_RESIZE_HIT_RADIUS,
                textbox_rect.top() + TEXTBOX_RESIZE_HIT_RADIUS,
            ),
        );
        let right = egui::Rect::from_min_max(
            egui::pos2(
                textbox_rect.right() - TEXTBOX_RESIZE_HIT_RADIUS,
                textbox_rect.top() + TEXTBOX_RESIZE_HIT_RADIUS,
            ),
            egui::pos2(
                textbox_rect.right() + TEXTBOX_RESIZE_HIT_RADIUS,
                textbox_rect.bottom() - TEXTBOX_RESIZE_HIT_RADIUS,
            ),
        );
        let bottom = egui::Rect::from_min_max(
            egui::pos2(
                textbox_rect.left() + TEXTBOX_RESIZE_HIT_RADIUS,
                textbox_rect.bottom() - TEXTBOX_RESIZE_HIT_RADIUS,
            ),
            egui::pos2(
                textbox_rect.right() - TEXTBOX_RESIZE_HIT_RADIUS,
                textbox_rect.bottom() + TEXTBOX_RESIZE_HIT_RADIUS,
            ),
        );
        let left = egui::Rect::from_min_max(
            egui::pos2(
                textbox_rect.left() - TEXTBOX_RESIZE_HIT_RADIUS,
                textbox_rect.top() + TEXTBOX_RESIZE_HIT_RADIUS,
            ),
            egui::pos2(
                textbox_rect.left() + TEXTBOX_RESIZE_HIT_RADIUS,
                textbox_rect.bottom() - TEXTBOX_RESIZE_HIT_RADIUS,
            ),
        );

        if top.contains(pointer_pos) {
            Some(TextboxResizeHandle::Top)
        } else if right.contains(pointer_pos) {
            Some(TextboxResizeHandle::Right)
        } else if bottom.contains(pointer_pos) {
            Some(TextboxResizeHandle::Bottom)
        } else if left.contains(pointer_pos) {
            Some(TextboxResizeHandle::Left)
        } else {
            None
        }
    })
}

pub(crate) fn normalized_textbox_rect(start: egui::Pos2, end: egui::Pos2) -> egui::Rect {
    let mut rect = egui::Rect::from_two_pos(start, end);
    if rect.width() < MIN_TEXTBOX_WORLD_SIZE.x {
        rect.max.x = rect.min.x + MIN_TEXTBOX_WORLD_SIZE.x;
    }
    if rect.height() < MIN_TEXTBOX_WORLD_SIZE.y {
        rect.max.y = rect.min.y + MIN_TEXTBOX_WORLD_SIZE.y;
    }
    rect
}

pub(crate) fn moved_textbox_rect(
    start_rect: egui::Rect,
    pointer_start_world: egui::Pos2,
    pointer_world: egui::Pos2,
) -> egui::Rect {
    let delta = pointer_world - pointer_start_world;
    start_rect.translate(delta)
}

pub(crate) fn resized_textbox_rect(
    dragged_handle: TextboxResizeHandle,
    anchor_world: egui::Pos2,
    start_rect_world: egui::Rect,
    pointer_world: egui::Pos2,
) -> egui::Rect {
    match dragged_handle {
        TextboxResizeHandle::TopLeft
        | TextboxResizeHandle::TopRight
        | TextboxResizeHandle::BottomLeft
        | TextboxResizeHandle::BottomRight => normalized_textbox_rect(anchor_world, pointer_world),
        TextboxResizeHandle::Top => normalized_textbox_rect(
            egui::pos2(start_rect_world.left(), pointer_world.y),
            egui::pos2(start_rect_world.right(), anchor_world.y),
        ),
        TextboxResizeHandle::Right => normalized_textbox_rect(
            egui::pos2(anchor_world.x, start_rect_world.top()),
            egui::pos2(pointer_world.x, start_rect_world.bottom()),
        ),
        TextboxResizeHandle::Bottom => normalized_textbox_rect(
            egui::pos2(start_rect_world.left(), anchor_world.y),
            egui::pos2(start_rect_world.right(), pointer_world.y),
        ),
        TextboxResizeHandle::Left => normalized_textbox_rect(
            egui::pos2(pointer_world.x, start_rect_world.top()),
            egui::pos2(anchor_world.x, start_rect_world.bottom()),
        ),
    }
}
