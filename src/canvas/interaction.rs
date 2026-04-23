use eframe::egui;

use super::geometry::{
    image_contains_screen_point, image_screen_center_for_record, image_screen_corners_for_record,
    image_screen_rect_for_record, min_image_scale,
};
use super::render::textbox_pointer_region;
use super::{
    ANNOTATION_HIT_RADIUS, AnnotationDraftState, AnnotationDragState, CanvasImage, CanvasState,
    CropState, DragState, IMAGE_RESIZE_HIT_RADIUS, IMAGE_ROTATION_HANDLE_DISTANCE,
    IMAGE_ROTATION_HANDLE_RADIUS, ImageResizeHandle, ImageTransformState, MAX_CANVAS_ZOOM,
    MIN_CANVAS_ZOOM, MIN_TEXTBOX_WORLD_SIZE, PendingAnnotation, TEXTBOX_DRAG_THRESHOLD_SCREEN,
    TEXTBOX_RESIZE_HIT_RADIUS, TextboxPointerRegion, TextboxResizeHandle, TextboxTransformState,
};
use crate::data::{CanvasAnnotation, CanvasAnnotationKind, CanvasTextSize};

pub(super) fn handle_input(
    ui: &egui::Ui,
    rect: egui::Rect,
    input_rect: egui::Rect,
    overlay_rect: Option<egui::Rect>,
    state: &mut CanvasState,
    response: &egui::Response,
) {
    let pointer_pos = ui.input(|i| i.pointer.latest_pos());
    let primary_pressed = ui.input(|i| i.pointer.button_pressed(egui::PointerButton::Primary));
    let primary_released = ui.input(|i| i.pointer.button_released(egui::PointerButton::Primary));
    let primary_double_clicked = ui.input(|i| {
        i.pointer
            .button_double_clicked(egui::PointerButton::Primary)
    });
    let primary_down = ui.input(|i| i.pointer.primary_down());
    let crop_key_held = ui.input(|i| i.key_down(egui::Key::C));
    let draw_kind = drawing_annotation_kind(ui);
    let shift_held = ui.input(|i| i.modifiers.shift);
    let alt_held = ui.input(|i| i.modifiers.alt);
    let pointer_in_canvas_rect = pointer_pos
        .map(|pointer_pos| {
            input_rect.contains(pointer_pos)
                && !overlay_rect.is_some_and(|overlay_rect| overlay_rect.contains(pointer_pos))
        })
        .unwrap_or(false);

    if pointer_in_canvas_rect && response.dragged_by(egui::PointerButton::Middle) {
        state.pan += response.drag_delta();
    }

    if pointer_in_canvas_rect {
        state.interaction.last_pointer_screen_pos = pointer_pos;
    }

    if let Some(pointer_pos) = pointer_pos
        && let Some(annotation_id) = state.single_selected_annotation_id()
        && let Some(annotation) = state.annotation_by_id(annotation_id).cloned()
        && annotation.kind == CanvasAnnotationKind::Textbox
        && let Some(region) = textbox_pointer_region(ui, rect, state, &annotation, pointer_pos)
    {
        match region {
            TextboxPointerRegion::Resize(handle) => {
                ui.ctx().set_cursor_icon(textbox_resize_cursor_icon(handle));
            }
            TextboxPointerRegion::Text => {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Text);
            }
            TextboxPointerRegion::Body => {}
        }
    }

    if let Some(pointer_pos) = pointer_pos
        && let Some(image) = state.selected_single_image()
        && let Some(handle) = hit_test_image_resize_handle(state, rect, image, pointer_pos)
    {
        ui.ctx().set_cursor_icon(image_resize_cursor_icon(handle));
    } else if let Some(pointer_pos) = pointer_pos
        && let Some(image) = state.selected_single_image()
        && hit_test_image_rotation_handle(state, rect, image, pointer_pos)
    {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }

    if pointer_in_canvas_rect
        && primary_pressed
        && let Some(pointer_pos) = pointer_pos
    {
        if let Some(kind) = draw_kind {
            let pointer_world = state.screen_to_world(pointer_pos, rect);
            state.clear_selection();
            state.interaction.drag_state = None;
            state.interaction.image_transform_state = None;
            state.interaction.textbox_transform_state = None;
            state.interaction.marquee_state = None;
            state.interaction.crop_state = None;
            state.interaction.annotation_draft_state = Some(AnnotationDraftState {
                kind,
                start_world: pointer_world,
                current_world: pointer_world,
            });
        } else if crop_key_held && let Some(item_id) = hit_test_image(state, rect, pointer_pos) {
            if !state.can_start_crop_on_item(item_id) {
                return;
            }
            state.set_single_selection(item_id);
            state.interaction.drag_state = None;
            state.interaction.textbox_transform_state = None;
            state.interaction.marquee_state = None;
            state.interaction.annotation_draft_state = None;
            state.interaction.crop_state = Some(CropState {
                item_id,
                start_screen: pointer_pos,
                current_screen: pointer_pos,
            });
        } else if let Some(image) = state.selected_single_image()
            && hit_test_image_rotation_handle(state, rect, image, pointer_pos)
        {
            begin_image_rotation(state, rect, image.record.item.id);
            return;
        } else if let Some(image) = state.selected_single_image().map(|image| {
            (
                image.record.item.id,
                hit_test_image_resize_handle(state, rect, image, pointer_pos),
            )
        }) && let (item_id, Some(handle)) = image
        {
            begin_image_resize(state, rect, item_id, handle);
            return;
        } else if let Some(annotation_id) = state.single_selected_annotation_id()
            && let Some(annotation) = state.annotation_by_id(annotation_id).cloned()
            && annotation.kind == CanvasAnnotationKind::Textbox
            && matches!(
                textbox_pointer_region(ui, rect, state, &annotation, pointer_pos),
                Some(TextboxPointerRegion::Resize(_))
            )
        {
            handle_textbox_primary_press(ui, rect, state, &annotation, pointer_pos);
            return;
        } else if let Some(annotation_id) = hit_test_annotation(state, rect, pointer_pos) {
            let annotation = state.annotation_by_id(annotation_id).cloned();
            if shift_held {
                if state.interaction.editing_text_annotation_id.is_some() {
                    state.finish_textbox_edit();
                }
                state.add_annotation_to_selection(annotation_id);
                state.interaction.drag_state = None;
                state.interaction.marquee_state = None;
                state.interaction.crop_state = None;
                state.interaction.annotation_draft_state = None;
                return;
            }
            if let Some(annotation) = annotation
                && annotation.kind == CanvasAnnotationKind::Textbox
            {
                if handle_textbox_primary_press(ui, rect, state, &annotation, pointer_pos) {
                    return;
                }
            } else {
                if state.interaction.editing_text_annotation_id.is_some() {
                    state.finish_textbox_edit();
                }
                state.document.selected_item_ids.clear();
                state.document.selected_annotation_ids = vec![annotation_id];
                begin_annotation_drag(state, rect, annotation_id, pointer_pos);
                state.interaction.image_transform_state = None;
                state.interaction.marquee_state = None;
                state.interaction.crop_state = None;
                state.interaction.annotation_draft_state = None;
                return;
            }
        } else if let Some(item_id) = hit_test_image(state, rect, pointer_pos) {
            if shift_held {
                state.add_to_selection(item_id);
            } else if !state.is_selected(item_id) {
                state.set_single_selection(item_id);
            }

            let pointer_world = state.screen_to_world(pointer_pos, rect);
            let starting_positions = state
                .document
                .images
                .iter()
                .filter(|image| state.is_selected(image.record.item.id))
                .map(|image| {
                    (
                        image.record.item.id,
                        egui::pos2(image.record.item.x, image.record.item.y),
                    )
                })
                .collect();

            state.interaction.drag_state = Some(DragState {
                pointer_start_world: pointer_world,
                starting_positions,
            });
            state.interaction.image_transform_state = None;
            state.interaction.textbox_transform_state = None;
            state.interaction.annotation_drag_state = None;
            state.interaction.marquee_state = None;
            state.interaction.crop_state = None;
            state.interaction.annotation_draft_state = None;
        } else {
            state.interaction.drag_state = None;
            state.interaction.image_transform_state = None;
            state.interaction.textbox_transform_state = None;
            state.interaction.annotation_drag_state = None;
            if !shift_held {
                state.clear_selection();
            }
            state.interaction.marquee_state = Some(super::MarqueeState {
                start_screen: pointer_pos,
                current_screen: pointer_pos,
            });
            state.interaction.crop_state = None;
            state.interaction.annotation_draft_state = None;
        }
    }

    if let Some(crop_state) = &mut state.interaction.crop_state
        && primary_down
        && let Some(pointer_pos) = pointer_pos
    {
        crop_state.current_screen = pointer_pos;
    }

    if primary_down
        && let Some(pointer_pos) = pointer_pos
        && let Some(draft_state) = &mut state.interaction.annotation_draft_state
    {
        let world = (pointer_pos - rect.center() - state.pan) / state.zoom;
        let pointer_world = egui::pos2(world.x, world.y);
        draft_state.current_world = pointer_world;
    }

    if state.interaction.crop_state.is_none()
        && state.interaction.annotation_draft_state.is_none()
        && primary_down
        && let Some(pointer_pos) = pointer_pos
        && let Some(drag_state) = state.interaction.annotation_drag_state.clone()
    {
        if pointer_pos.distance(drag_state.pointer_start_screen) >= TEXTBOX_DRAG_THRESHOLD_SCREEN {
            let pointer_world = state.screen_to_world(pointer_pos, rect);
            let world_delta = pointer_world - drag_state.pointer_start_world;
            for (annotation_id, start_world, end_world) in drag_state.starting_points {
                state.set_annotation_world_points(
                    annotation_id,
                    start_world + world_delta,
                    end_world + world_delta,
                );
            }
        }
    }

    if state.interaction.crop_state.is_none()
        && state.interaction.annotation_draft_state.is_none()
        && primary_down
        && let Some(pointer_pos) = pointer_pos
        && let Some(transform_state) = state.interaction.textbox_transform_state.clone()
    {
        match transform_state {
            TextboxTransformState::PendingMove {
                annotation_ids,
                pointer_start_world,
                pointer_start_screen,
                start_rects_world,
            } => {
                if pointer_pos.distance(pointer_start_screen) >= TEXTBOX_DRAG_THRESHOLD_SCREEN {
                    state.interaction.textbox_transform_state =
                        Some(TextboxTransformState::Moving {
                            annotation_ids: annotation_ids.clone(),
                            pointer_start_world,
                            start_rects_world: start_rects_world.clone(),
                        });
                }
            }
            TextboxTransformState::Moving {
                annotation_ids: _,
                pointer_start_world,
                start_rects_world,
            } => {
                let pointer_world = state.screen_to_world(pointer_pos, rect);
                for (annotation_id, start_rect_world) in start_rects_world {
                    let world_rect =
                        moved_textbox_rect(start_rect_world, pointer_start_world, pointer_world);
                    state.set_textbox_world_rect(annotation_id, world_rect);
                }
            }
            TextboxTransformState::Resizing {
                annotation_id,
                dragged_handle,
                anchor_world,
                start_rect_world,
            } => {
                let pointer_world = state.screen_to_world(pointer_pos, rect);
                let world_rect = resized_textbox_rect(
                    dragged_handle,
                    anchor_world,
                    start_rect_world,
                    pointer_world,
                );
                state.set_textbox_world_rect(annotation_id, world_rect);
            }
        }
    }

    if state.interaction.crop_state.is_none()
        && state.interaction.annotation_draft_state.is_none()
        && primary_down
        && let Some(pointer_pos) = pointer_pos
        && let Some(transform_state) = state.interaction.image_transform_state
    {
        match transform_state {
            ImageTransformState::Resizing {
                item_id,
                center_screen,
                start_scale,
                start_radius_screen,
            } => {
                if let Some(image) = state.image_by_item_id(item_id) {
                    let scale = resized_image_scale(
                        &image.record.asset,
                        start_scale,
                        start_radius_screen,
                        center_screen,
                        pointer_pos,
                    );
                    state.set_image_scale(item_id, scale);
                }
            }
            ImageTransformState::Rotating {
                item_id,
                center_screen,
                start_rotation,
            } => {
                let rotation = rotated_image_angle(
                    center_screen,
                    pointer_pos,
                    start_rotation,
                    state.rotation_snap_degrees,
                    alt_held,
                );
                state.set_image_rotation(item_id, rotation);
            }
        }
    }

    if state.interaction.crop_state.is_none()
        && state.interaction.annotation_draft_state.is_none()
        && let Some(drag_state) = &state.interaction.drag_state
        && primary_down
        && let Some(pointer_pos) = pointer_pos
    {
        let pointer_world = state.screen_to_world(pointer_pos, rect);
        let world_delta = pointer_world - drag_state.pointer_start_world;

        for (item_id, start_pos) in &drag_state.starting_positions {
            if let Some(image) = state
                .document
                .images
                .iter_mut()
                .find(|image| image.record.item.id == *item_id)
            {
                image.record.item.x = start_pos.x + world_delta.x;
                image.record.item.y = start_pos.y + world_delta.y;
            }
        }
    }

    if primary_released {
        if let Some(crop_state) = state.interaction.crop_state.take() {
            state.interaction.pending_crop = state.build_crop_request(rect, &crop_state);
        } else if let Some(draft_state) = state.interaction.annotation_draft_state.take() {
            state.interaction.pending_annotation = pending_annotation_from_draft(draft_state);
        } else if let Some(transform_state) = state.interaction.image_transform_state.take() {
            match transform_state {
                ImageTransformState::Resizing { item_id, .. }
                | ImageTransformState::Rotating { item_id, .. } => state.mark_dirty(item_id),
            }
        } else if let Some(transform_state) = state.interaction.textbox_transform_state.take() {
            match transform_state {
                TextboxTransformState::Moving { annotation_ids, .. } => {
                    for annotation_id in annotation_ids {
                        state.mark_annotation_dirty(annotation_id);
                    }
                }
                TextboxTransformState::Resizing { annotation_id, .. } => {
                    state.mark_annotation_dirty(annotation_id);
                }
                TextboxTransformState::PendingMove { .. } => {}
            }
        } else if let Some(drag_state) = state.interaction.annotation_drag_state.take() {
            for annotation_id in drag_state.annotation_ids {
                state.mark_annotation_dirty(annotation_id);
            }
        } else if let Some(drag_state) = state.interaction.drag_state.take() {
            for (item_id, _) in drag_state.starting_positions {
                state.mark_dirty(item_id);
            }
        }
        state.interaction.marquee_state = None;
    }

    if primary_released
        && primary_double_clicked
        && pointer_in_canvas_rect
        && let Some(pointer_pos) = pointer_pos
        && let Some(annotation_id) = hit_test_annotation(state, rect, pointer_pos)
        && state
            .annotation_by_id(annotation_id)
            .is_some_and(|annotation| annotation.kind == CanvasAnnotationKind::Textbox)
    {
        state.begin_textbox_edit(annotation_id);
        return;
    }

    if state.interaction.crop_state.is_none()
        && state.interaction.annotation_draft_state.is_none()
        && let Some(marquee_state) = &mut state.interaction.marquee_state
        && ui.input(|i| i.pointer.primary_down())
        && let Some(pointer_pos) = pointer_pos
    {
        marquee_state.current_screen = pointer_pos;
        let selection_rect =
            egui::Rect::from_two_pos(marquee_state.start_screen, marquee_state.current_screen);
        let selected = state
            .document
            .images
            .iter()
            .filter(|image| image_screen_rect(state, rect, image).intersects(selection_rect))
            .map(|image| image.record.item.id)
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

    if !pointer_in_canvas_rect {
        return;
    }

    let Some(pointer_pos) = pointer_pos else {
        return;
    };

    if state.interaction.crop_state.is_some() || state.interaction.annotation_draft_state.is_some()
    {
        return;
    }

    if !state.document.selected_annotation_ids.is_empty()
        && ui.input(|i| i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace))
    {
        return;
    }

    if let Some(annotation_id) = state.single_selected_annotation_id()
        && ui.input(|i| i.key_pressed(egui::Key::Enter))
    {
        state.begin_textbox_edit(annotation_id);
        return;
    }

    if state.interaction.editing_text_annotation_id.is_some() {
        let zoom_delta = ui.input(|i| i.zoom_delta());
        if (zoom_delta - 1.0).abs() > f32::EPSILON {
            zoom_around_pointer(state, rect, pointer_pos, zoom_delta);
        }

        let pan_delta = ui.input(|i| i.translation_delta());
        if pan_delta != egui::Vec2::ZERO {
            state.pan += pan_delta;
        }
        return;
    }

    let zoom_delta = ui.input(|i| i.zoom_delta());
    if (zoom_delta - 1.0).abs() > f32::EPSILON {
        zoom_around_pointer(state, rect, pointer_pos, zoom_delta);
    }

    let pan_delta = ui.input(|i| i.translation_delta());
    if pan_delta != egui::Vec2::ZERO {
        state.pan += pan_delta;
    }
}

fn drawing_annotation_kind(ui: &egui::Ui) -> Option<CanvasAnnotationKind> {
    if ui.ctx().egui_wants_keyboard_input() {
        return None;
    }

    if ui.input(|i| i.key_down(egui::Key::A)) {
        Some(CanvasAnnotationKind::Arrow)
    } else if ui.input(|i| i.key_down(egui::Key::L)) {
        Some(CanvasAnnotationKind::Line)
    } else if ui.input(|i| i.key_down(egui::Key::T)) {
        Some(CanvasAnnotationKind::Textbox)
    } else {
        None
    }
}

fn hit_test_image(state: &CanvasState, rect: egui::Rect, pointer_pos: egui::Pos2) -> Option<i64> {
    state
        .document
        .images
        .iter()
        .rev()
        .find(|image| {
            image_contains_screen_point(
                &image.record,
                pointer_pos,
                state.pan,
                state.zoom,
                rect.center(),
            )
        })
        .map(|image| image.record.item.id)
}

fn begin_image_resize(
    state: &mut CanvasState,
    rect: egui::Rect,
    item_id: i64,
    handle: ImageResizeHandle,
) {
    let Some(image) = state.image_by_item_id(item_id) else {
        return;
    };
    let image_rect = image_screen_rect(state, rect, image);
    let center_screen = image_rect.center();
    let start_radius_screen =
        center_screen.distance(image_resize_handle_screen_pos(image_rect, handle));
    let start_scale = image.record.item.scale;

    state.finish_textbox_edit();
    state.document.selected_item_ids.clear();
    state.document.selected_item_ids.push(item_id);
    state.document.selected_annotation_ids.clear();
    state.interaction.drag_state = None;
    state.interaction.annotation_drag_state = None;
    state.interaction.marquee_state = None;
    state.interaction.crop_state = None;
    state.interaction.annotation_draft_state = None;
    state.interaction.textbox_transform_state = None;
    state.interaction.image_transform_state = Some(ImageTransformState::Resizing {
        item_id,
        center_screen,
        start_scale,
        start_radius_screen,
    });
}

fn begin_image_rotation(state: &mut CanvasState, rect: egui::Rect, item_id: i64) {
    let Some(image) = state.image_by_item_id(item_id) else {
        return;
    };
    let center_screen =
        image_screen_center_for_record(&image.record, state.pan, state.zoom, rect.center());
    let start_rotation = image.record.item.rotation;

    state.finish_textbox_edit();
    state.document.selected_item_ids.clear();
    state.document.selected_item_ids.push(item_id);
    state.document.selected_annotation_ids.clear();
    state.interaction.drag_state = None;
    state.interaction.annotation_drag_state = None;
    state.interaction.marquee_state = None;
    state.interaction.crop_state = None;
    state.interaction.annotation_draft_state = None;
    state.interaction.textbox_transform_state = None;
    state.interaction.image_transform_state = Some(ImageTransformState::Rotating {
        item_id,
        center_screen,
        start_rotation,
    });
}

fn begin_annotation_drag(
    state: &mut CanvasState,
    rect: egui::Rect,
    annotation_id: i64,
    pointer_pos: egui::Pos2,
) {
    state.finish_textbox_edit();
    state.document.selected_item_ids.clear();
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

fn begin_textbox_pending_move(
    state: &mut CanvasState,
    rect: egui::Rect,
    annotation: &CanvasAnnotation,
    pointer_pos: egui::Pos2,
) {
    state.document.selected_item_ids.clear();
    if !state.is_annotation_selected(annotation.id) {
        state.document.selected_annotation_ids = vec![annotation.id];
    }
    let annotation_ids = state
        .document
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
    state.interaction.drag_state = None;
    state.interaction.marquee_state = None;
    state.interaction.crop_state = None;
    state.interaction.annotation_draft_state = None;
    state.interaction.textbox_transform_state = Some(TextboxTransformState::PendingMove {
        annotation_ids,
        pointer_start_world: state.screen_to_world(pointer_pos, rect),
        pointer_start_screen: pointer_pos,
        start_rects_world,
    });
}

pub(super) fn handle_textbox_primary_press(
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
                state.interaction.image_transform_state = None;
                begin_textbox_pending_move(state, rect, annotation, pointer_pos);
                return true;
            }
            let world_rect = textbox_world_rect(annotation);
            state.document.selected_item_ids.clear();
            state.document.selected_annotation_ids = vec![annotation.id];
            state.interaction.drag_state = None;
            state.interaction.image_transform_state = None;
            state.interaction.marquee_state = None;
            state.interaction.crop_state = None;
            state.interaction.annotation_draft_state = None;
            state.interaction.textbox_transform_state = Some(TextboxTransformState::Resizing {
                annotation_id: annotation.id,
                dragged_handle: handle,
                anchor_world: textbox_resize_handle_anchor_world_pos(world_rect, handle),
                start_rect_world: world_rect,
            });
            true
        }
        TextboxPointerRegion::Text => true,
        TextboxPointerRegion::Body => {
            if state.interaction.editing_text_annotation_id == Some(annotation.id) {
                state.stop_textbox_edit_without_cleanup();
            } else if state.interaction.editing_text_annotation_id.is_some() {
                state.finish_textbox_edit();
            }
            state.interaction.image_transform_state = None;
            begin_textbox_pending_move(state, rect, annotation, pointer_pos);
            true
        }
    }
}

fn hit_test_annotation(
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

pub(super) fn image_screen_rect(
    state: &CanvasState,
    rect: egui::Rect,
    image: &CanvasImage,
) -> egui::Rect {
    image_screen_rect_for_record(&image.record, rect, state.pan, state.zoom, rect.center())
}

pub(super) fn image_rotation_handle_screen_pos(
    state: &CanvasState,
    rect: egui::Rect,
    image: &CanvasImage,
) -> egui::Pos2 {
    let corners =
        image_screen_corners_for_record(&image.record, state.pan, state.zoom, rect.center());
    let top_center = egui::pos2(
        (corners[0].x + corners[1].x) * 0.5,
        (corners[0].y + corners[1].y) * 0.5,
    );
    let center =
        image_screen_center_for_record(&image.record, state.pan, state.zoom, rect.center());
    let direction = (top_center - center).normalized();
    top_center + direction * IMAGE_ROTATION_HANDLE_DISTANCE
}

fn image_resize_handle_screen_pos(image_rect: egui::Rect, handle: ImageResizeHandle) -> egui::Pos2 {
    match handle {
        ImageResizeHandle::TopLeft => image_rect.left_top(),
        ImageResizeHandle::TopRight => image_rect.right_top(),
        ImageResizeHandle::BottomLeft => image_rect.left_bottom(),
        ImageResizeHandle::BottomRight => image_rect.right_bottom(),
    }
}

fn hit_test_image_rotation_handle(
    state: &CanvasState,
    rect: egui::Rect,
    image: &CanvasImage,
    pointer_pos: egui::Pos2,
) -> bool {
    image_rotation_handle_screen_pos(state, rect, image).distance(pointer_pos)
        <= IMAGE_ROTATION_HANDLE_RADIUS + 4.0
}

fn image_resize_cursor_icon(handle: ImageResizeHandle) -> egui::CursorIcon {
    match handle {
        ImageResizeHandle::TopLeft | ImageResizeHandle::BottomRight => egui::CursorIcon::ResizeNwSe,
        ImageResizeHandle::TopRight | ImageResizeHandle::BottomLeft => egui::CursorIcon::ResizeNeSw,
    }
}

pub(super) fn hit_test_image_resize_handle(
    state: &CanvasState,
    rect: egui::Rect,
    image: &CanvasImage,
    pointer_pos: egui::Pos2,
) -> Option<ImageResizeHandle> {
    let image_rect = image_screen_rect(state, rect, image);
    [
        ImageResizeHandle::TopLeft,
        ImageResizeHandle::TopRight,
        ImageResizeHandle::BottomLeft,
        ImageResizeHandle::BottomRight,
    ]
    .into_iter()
    .find(|handle| {
        image_resize_handle_screen_pos(image_rect, *handle).distance(pointer_pos)
            <= IMAGE_RESIZE_HIT_RADIUS
    })
}

pub(super) fn resized_image_scale(
    asset: &crate::data::ImageAsset,
    start_scale: f32,
    start_radius_screen: f32,
    center_screen: egui::Pos2,
    pointer_pos: egui::Pos2,
) -> f32 {
    let current_radius_screen = center_screen.distance(pointer_pos);
    let scale_ratio = if start_radius_screen <= f32::EPSILON {
        1.0
    } else {
        current_radius_screen / start_radius_screen
    };
    (start_scale * scale_ratio).max(min_image_scale(asset))
}

fn rotated_image_angle(
    center_screen: egui::Pos2,
    pointer_pos: egui::Pos2,
    start_rotation: f32,
    snap_degrees: f32,
    unsnapped: bool,
) -> f32 {
    let pointer_delta = pointer_pos - center_screen;
    if pointer_delta.length_sq() <= f32::EPSILON {
        return start_rotation;
    }

    let rotation = pointer_delta.angle() + std::f32::consts::FRAC_PI_2;
    if unsnapped || snap_degrees <= f32::EPSILON {
        rotation
    } else {
        let snap_radians = snap_degrees.to_radians();
        (rotation / snap_radians).round() * snap_radians
    }
}

fn zoom_around_pointer(
    state: &mut CanvasState,
    rect: egui::Rect,
    pointer_pos: egui::Pos2,
    zoom_factor: f32,
) {
    let world_before = state.screen_to_world(pointer_pos, rect);
    state.zoom = (state.zoom * zoom_factor).clamp(MIN_CANVAS_ZOOM, MAX_CANVAS_ZOOM);
    let screen_after = state.world_to_screen(world_before, rect);
    state.pan += pointer_pos - screen_after;
}

pub(super) fn annotation_screen_points(
    state: &CanvasState,
    rect: egui::Rect,
    annotation: &CanvasAnnotation,
) -> (egui::Pos2, egui::Pos2) {
    (
        state.world_to_screen(egui::pos2(annotation.start_x, annotation.start_y), rect),
        state.world_to_screen(egui::pos2(annotation.end_x, annotation.end_y), rect),
    )
}

pub(super) fn textbox_world_rect(annotation: &CanvasAnnotation) -> egui::Rect {
    normalized_textbox_rect(
        egui::pos2(annotation.start_x, annotation.start_y),
        egui::pos2(annotation.end_x, annotation.end_y),
    )
}

pub(super) fn annotation_world_rect(annotation: &CanvasAnnotation) -> egui::Rect {
    match annotation.kind {
        CanvasAnnotationKind::Textbox => textbox_world_rect(annotation),
        CanvasAnnotationKind::Line | CanvasAnnotationKind::Arrow => egui::Rect::from_two_pos(
            egui::pos2(annotation.start_x, annotation.start_y),
            egui::pos2(annotation.end_x, annotation.end_y),
        ),
    }
}

pub(super) fn textbox_screen_rect(
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

fn textbox_resize_cursor_icon(handle: TextboxResizeHandle) -> egui::CursorIcon {
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

pub(super) fn hit_test_textbox_resize_handle(
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

pub(super) fn annotation_screen_rect(
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

fn annotation_hit_test(
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

pub(super) fn normalized_textbox_rect(start: egui::Pos2, end: egui::Pos2) -> egui::Rect {
    let mut rect = egui::Rect::from_two_pos(start, end);
    if rect.width() < MIN_TEXTBOX_WORLD_SIZE.x {
        rect.max.x = rect.min.x + MIN_TEXTBOX_WORLD_SIZE.x;
    }
    if rect.height() < MIN_TEXTBOX_WORLD_SIZE.y {
        rect.max.y = rect.min.y + MIN_TEXTBOX_WORLD_SIZE.y;
    }
    rect
}

pub(super) fn moved_textbox_rect(
    start_rect: egui::Rect,
    pointer_start_world: egui::Pos2,
    pointer_world: egui::Pos2,
) -> egui::Rect {
    let delta = pointer_world - pointer_start_world;
    start_rect.translate(delta)
}

pub(super) fn resized_textbox_rect(
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

pub(super) fn pending_annotation_from_draft(
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
