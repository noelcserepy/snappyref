use eframe::egui;
use std::time::Duration;

use super::geometry::{
    image_contains_screen_point, image_rotation_handle_screen_pos, image_screen_rect_for_record,
    min_image_scale,
};
use super::render::textbox_pointer_region;
use super::{
    ANNOTATION_HIT_RADIUS, AnnotationDraftState, AnnotationDragState, CanvasHit, CanvasImage,
    CanvasState, CropState, DragState, IMAGE_MOVE_SNAP_MAX_DISTANCE_SCREEN,
    IMAGE_RESIZE_HIT_RADIUS, IMAGE_ROTATE_HANDLE_HIT_RADIUS_SCREEN, ImageResizeHandle,
    ImageTransformState, MAX_CANVAS_ZOOM, MIN_CANVAS_ZOOM, MIN_TEXTBOX_WORLD_SIZE,
    PendingAnnotation, SelectedLayoutItem, TEXTBOX_DRAG_THRESHOLD_SCREEN,
    TEXTBOX_RESIZE_HIT_RADIUS, TextboxPointerRegion, TextboxResizeHandle, TextboxTransformState,
};
use crate::data::{CanvasAnnotation, CanvasAnnotationKind, CanvasStackItem, CanvasTextSize};

// Suppresses drag movement (and layout snapping) for the first 90ms after
// primary-press on a placement so click-to-select doesn't jerk the image via
// snap.
const IMAGE_MOVE_DEBOUNCE_SECONDS: f64 = 0.090;
const GUIDE_SNAP_ACQUIRE_DISTANCE_SCREEN: f32 = 8.0;
const GUIDE_SNAP_RELEASE_DISTANCE_SCREEN: f32 = 12.0;
const GUIDE_SNAP_VIEWPORT_PADDING_SCREEN: f32 = 64.0;
const GUIDE_SNAP_EPSILON: f32 = 0.001;

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
    let secondary_pressed = ui.input(|i| i.pointer.button_pressed(egui::PointerButton::Secondary));
    let primary_released = ui.input(|i| i.pointer.button_released(egui::PointerButton::Primary));
    let primary_double_clicked = ui.input(|i| {
        i.pointer
            .button_double_clicked(egui::PointerButton::Primary)
    });
    let primary_down = ui.input(|i| i.pointer.primary_down());
    let crop_key_held = ui.input(|i| i.key_down(egui::Key::C));
    let rotation_handle_key_held = super::rotation_handle_key_held(ui.ctx());
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

    // Alt+LMB pan: starts a canvas pan even over a placement, mirroring PureRef.
    if state.interaction.alt_pan_active {
        if primary_released {
            state.interaction.alt_pan_active = false;
        } else if primary_down {
            state.pan += response.drag_delta();
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
            return;
        }
    } else if pointer_in_canvas_rect && primary_pressed && alt_held {
        state.interaction.alt_pan_active = true;
        state.interaction.drag_state = None;
        state.interaction.image_transform_state = None;
        state.interaction.textbox_transform_state = None;
        state.interaction.annotation_drag_state = None;
        state.interaction.marquee_state = None;
        state.interaction.crop_state = None;
        state.interaction.annotation_draft_state = None;
        state.clear_move_preview_placeholders();
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        return;
    } else if pointer_in_canvas_rect && alt_held && !primary_down {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }

    if pointer_in_canvas_rect {
        state.interaction.last_pointer_screen_pos = pointer_pos;
    }

    if pointer_in_canvas_rect
        && secondary_pressed
        && let Some(pointer_pos) = pointer_pos
    {
        open_context_menu_at_pointer(state, rect, pointer_pos);
        return;
    }

    if matches!(
        state.interaction.image_transform_state.as_ref(),
        Some(ImageTransformState::Rotating { .. })
    ) {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    } else if let Some(pointer_pos) = pointer_pos
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
        && rotation_handle_key_held
        && hit_test_selected_image_rotation_handle(state, rect, pointer_pos)
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
            state.clear_move_preview_placeholders();
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
            state.clear_move_preview_placeholders();
        } else if let Some(image) = state.selected_single_image().map(|image| {
            (
                image.record.item.id,
                hit_test_image_resize_handle(state, rect, image, pointer_pos),
            )
        }) && let (item_id, Some(handle)) = image
        {
            begin_image_resize(state, rect, item_id, handle);
            return;
        } else if rotation_handle_key_held
            && hit_test_selected_image_rotation_handle(state, rect, pointer_pos)
        {
            begin_image_rotation(state, rect, pointer_pos);
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
        } else if let Some(hit) = hit_test_canvas_item(state, rect, pointer_pos) {
            match hit {
                CanvasHit::Annotation(annotation_id) => {
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
                }
                CanvasHit::Placement(item_id) => {
                    if shift_held {
                        state.add_to_selection(item_id);
                    } else if !state.is_selected(item_id) {
                        state.set_single_selection(item_id);
                    }

                    let pointer_world = state.screen_to_world(pointer_pos, rect);
                    let starting_items = state.selected_layout_items();

                    state.interaction.drag_state = Some(DragState {
                        pointer_start_world: pointer_world,
                        starting_items,
                        snap_preview_rect_world: None,
                        started_at_seconds: ui.input(|input| input.time),
                        last_movement_seconds: ui.input(|input| input.time),
                        last_world_delta: egui::Vec2::ZERO,
                        moved: false,
                    });
                    state.interaction.image_transform_state = None;
                    state.interaction.textbox_transform_state = None;
                    state.interaction.annotation_drag_state = None;
                    state.interaction.marquee_state = None;
                    state.interaction.crop_state = None;
                    state.interaction.annotation_draft_state = None;
                    state.clear_move_preview_placeholders();
                    return;
                }
            }
        } else {
            state.interaction.drag_state = None;
            state.interaction.image_transform_state = None;
            state.interaction.textbox_transform_state = None;
            state.interaction.annotation_drag_state = None;
            state.clear_move_preview_placeholders();
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
        && let Some(transform_state) = state.interaction.image_transform_state.clone()
    {
        match transform_state {
            ImageTransformState::Resizing {
                item_id,
                anchor_screen,
                start_handle_screen,
                start_scale,
            } => {
                if let Some(image) = state.image_by_item_id(item_id) {
                    let (center_screen, scale) = resized_image_transform(
                        &image.record.asset,
                        start_scale,
                        anchor_screen,
                        start_handle_screen,
                        pointer_pos,
                    );
                    let center_world = state.screen_to_world(center_screen, rect);
                    state.set_image_center_and_scale(item_id, center_world, scale);
                }
            }
            ImageTransformState::Rotating {
                item_ids: _,
                center_screen,
                center_world,
                start_pointer_angle,
                starting_items,
            } => {
                let rotation_delta = rotated_image_delta(
                    center_screen,
                    pointer_pos,
                    start_pointer_angle,
                    state.rotation_snap_degrees,
                    alt_held,
                );
                for (item_id, start_center_world, start_rotation) in starting_items {
                    let offset = start_center_world - center_world;
                    let rotated_offset = rotate_vec2(offset, rotation_delta);
                    state.set_image_center_and_rotation(
                        item_id,
                        center_world + rotated_offset,
                        start_rotation + rotation_delta,
                    );
                }
            }
        }
    }

    if state.interaction.crop_state.is_none()
        && state.interaction.annotation_draft_state.is_none()
        && let Some(drag_state) = state.interaction.drag_state.clone()
        && primary_down
        && let Some(pointer_pos) = pointer_pos
    {
        let elapsed_seconds = ui.input(|input| input.time) - drag_state.started_at_seconds;
        if elapsed_seconds < IMAGE_MOVE_DEBOUNCE_SECONDS {
            ui.ctx().request_repaint_after(Duration::from_secs_f64(
                IMAGE_MOVE_DEBOUNCE_SECONDS - elapsed_seconds,
            ));
            return;
        }
        let pointer_world = state.screen_to_world(pointer_pos, rect);
        let world_delta = pointer_world - drag_state.pointer_start_world;
        let guide_snap_disabled = ui.input(|input| input.modifiers.command);
        let snap_preview_rect = if guide_snap_disabled {
            state.clear_guide_snap_indicators();
            state.clear_move_preview_placeholders();
            None
        } else {
            compute_drag_snap_preview(state, rect, &drag_state, world_delta)
        };
        apply_dragged_item_positions(
            state,
            &drag_state.starting_items,
            world_delta,
            snap_preview_rect,
        );
        let now = ui.input(|input| input.time);
        if let Some(active_drag_state) = &mut state.interaction.drag_state {
            active_drag_state.snap_preview_rect_world = snap_preview_rect;
            if (world_delta - active_drag_state.last_world_delta).length_sq() > 0.01 {
                active_drag_state.last_movement_seconds = now;
                active_drag_state.last_world_delta = world_delta;
            }
            active_drag_state.moved = true;
            ui.ctx().request_repaint_after(Duration::from_secs_f64(
                super::SELECTION_BORDER_REAPPEAR_SECONDS,
            ));
        }
    }

    if primary_released {
        if let Some(crop_state) = state.interaction.crop_state.take() {
            state.interaction.pending_crop = state.build_crop_request(rect, &crop_state);
        } else if let Some(draft_state) = state.interaction.annotation_draft_state.take() {
            state.interaction.pending_annotation = pending_annotation_from_draft(draft_state);
        } else if let Some(transform_state) = state.interaction.image_transform_state.take() {
            match transform_state {
                ImageTransformState::Resizing { item_id, .. } => state.mark_dirty(item_id),
                ImageTransformState::Rotating { item_ids, .. } => {
                    for item_id in item_ids {
                        state.mark_dirty(item_id);
                    }
                }
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
            if drag_state.moved {
                for item in drag_state.starting_items {
                    state.mark_dirty(item.id);
                }
            }
            state.clear_move_preview_placeholders();
            state.clear_guide_snap_indicators();
        }
        state.interaction.marquee_state = None;
        state.clear_move_preview_placeholders();
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
        apply_canvas_navigation(ui, state, rect, pointer_pos);
        return;
    }

    apply_canvas_navigation(ui, state, rect, pointer_pos);
}

fn apply_canvas_navigation(
    ui: &egui::Ui,
    state: &mut CanvasState,
    rect: egui::Rect,
    pointer_pos: egui::Pos2,
) {
    match state.pointer_mode() {
        crate::data::PointerMode::Trackpad => {
            let zoom_delta = ui.input(|i| i.zoom_delta());
            if (zoom_delta - 1.0).abs() > f32::EPSILON {
                zoom_around_pointer(state, rect, pointer_pos, zoom_delta);
            }
            let pan_delta = ui.input(|i| i.translation_delta());
            if pan_delta != egui::Vec2::ZERO {
                state.pan += pan_delta;
            }
        }
        crate::data::PointerMode::Mouse => {
            let (zoom_factor, pan_delta) = ui.input(|i| map_mouse_mode_scroll_events(&i.events));
            if (zoom_factor - 1.0).abs() > f32::EPSILON {
                zoom_around_pointer(state, rect, pointer_pos, zoom_factor);
            }
            if pan_delta != egui::Vec2::ZERO {
                state.pan += pan_delta;
            }
        }
    }
}

/// In Mouse mode, route mouse-wheel notches (Line/Page units) to zoom around
/// the pointer; leave true trackpad pan (Point units) and pinch (`Event::Zoom`)
/// alone so they still work for users with mixed input devices.
///
/// Scroll-wheel up (negative y per egui convention) -> zoom in. Each line tick
/// scales by 1.1; sub-line deltas scale smoothly via `1.1^(-y)`.
pub(super) fn map_mouse_mode_scroll_events(events: &[egui::Event]) -> (f32, egui::Vec2) {
    let mut zoom_factor: f32 = 1.0;
    let mut pan_delta = egui::Vec2::ZERO;
    let mut wheel_zoom_y: f32 = 0.0;
    for event in events {
        match event {
            egui::Event::MouseWheel { unit, delta, .. } => match unit {
                egui::MouseWheelUnit::Line | egui::MouseWheelUnit::Page => {
                    wheel_zoom_y += delta.y;
                }
                egui::MouseWheelUnit::Point => {
                    pan_delta += *delta;
                }
            },
            egui::Event::Zoom(factor) => {
                zoom_factor *= factor;
            }
            _ => {}
        }
    }
    if wheel_zoom_y.abs() > f32::EPSILON {
        zoom_factor *= 1.1_f32.powf(-wheel_zoom_y);
    }
    (zoom_factor, pan_delta)
}

pub(super) fn open_context_menu_at_pointer(
    state: &mut CanvasState,
    rect: egui::Rect,
    pointer_pos: egui::Pos2,
) {
    if let Some(hit) = hit_test_canvas_item(state, rect, pointer_pos) {
        match hit {
            CanvasHit::Annotation(annotation_id) => {
                if !state.is_annotation_selected(annotation_id) {
                    state.finish_textbox_edit();
                    state.document.selected_item_ids.clear();
                    state.document.selected_annotation_ids = vec![annotation_id];
                }
            }
            CanvasHit::Placement(item_id) => {
                if !state.is_selected(item_id) {
                    state.set_single_selection(item_id);
                }
            }
        }
    }

    state.interaction.drag_state = None;
    state.interaction.annotation_drag_state = None;
    state.interaction.image_transform_state = None;
    state.interaction.textbox_transform_state = None;
    state.interaction.marquee_state = None;
    state.interaction.crop_state = None;
    state.interaction.annotation_draft_state = None;
    state.clear_move_preview_placeholders();
    state.interaction.pending_context_menu_request = Some(super::CanvasContextMenuRequest {
        screen_pos: pointer_pos,
    });
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

pub(super) fn hit_test_canvas_item(
    state: &CanvasState,
    rect: egui::Rect,
    pointer_pos: egui::Pos2,
) -> Option<CanvasHit> {
    state
        .ordered_canvas_items_back_to_front()
        .into_iter()
        .rev()
        .find_map(|item| match item {
            CanvasStackItem::Placement(item_id) => state
                .image_by_item_id(item_id)
                .filter(|image| {
                    image_contains_screen_point(
                        &image.record,
                        pointer_pos,
                        state.pan,
                        state.zoom,
                        rect.center(),
                    )
                })
                .map(|_| CanvasHit::Placement(item_id)),
            CanvasStackItem::Annotation(annotation_id) => state
                .annotation_by_id(annotation_id)
                .filter(|annotation| annotation_hit_test(state, rect, annotation, pointer_pos))
                .map(|_| CanvasHit::Annotation(annotation_id)),
        })
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
    let corners = super::geometry::image_screen_corners_for_record(
        &image.record,
        state.pan,
        state.zoom,
        rect.center(),
    );
    let start_handle_screen = image_resize_handle_screen_pos(corners, handle);
    let anchor_screen = image_resize_anchor_screen_pos(corners, handle);
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
    state.clear_move_preview_placeholders();
    state.interaction.image_transform_state = Some(ImageTransformState::Resizing {
        item_id,
        anchor_screen,
        start_handle_screen,
        start_scale,
    });
}

fn begin_image_rotation(state: &mut CanvasState, rect: egui::Rect, pointer_pos: egui::Pos2) {
    let item_ids = selected_existing_image_ids(state);
    if item_ids.is_empty() {
        return;
    }
    let Some(selection_rect) = selected_image_rotation_screen_rect(state, rect) else {
        return;
    };
    let center_screen = selection_rect.center();
    let center_world = state.screen_to_world(center_screen, rect);
    let start_pointer_delta = pointer_pos - center_screen;
    let start_pointer_angle = if start_pointer_delta.length_sq() <= f32::EPSILON {
        0.0
    } else {
        start_pointer_delta.angle()
    };
    let starting_items = item_ids
        .iter()
        .filter_map(|item_id| {
            state.image_by_item_id(*item_id).map(|image| {
                (
                    *item_id,
                    egui::pos2(image.record.item.x, image.record.item.y),
                    image.record.item.rotation,
                )
            })
        })
        .collect::<Vec<_>>();

    state.finish_textbox_edit();
    state.interaction.drag_state = None;
    state.interaction.annotation_drag_state = None;
    state.interaction.marquee_state = None;
    state.interaction.crop_state = None;
    state.interaction.annotation_draft_state = None;
    state.interaction.textbox_transform_state = None;
    state.clear_move_preview_placeholders();
    state.interaction.image_transform_state = Some(ImageTransformState::Rotating {
        item_ids,
        center_screen,
        center_world,
        start_pointer_angle,
        starting_items,
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
    state.clear_move_preview_placeholders();
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
    state.clear_move_preview_placeholders();
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

fn image_resize_handle_screen_pos(
    corners: [egui::Pos2; 4],
    handle: ImageResizeHandle,
) -> egui::Pos2 {
    match handle {
        ImageResizeHandle::TopLeft => corners[0],
        ImageResizeHandle::TopRight => corners[1],
        ImageResizeHandle::BottomRight => corners[2],
        ImageResizeHandle::BottomLeft => corners[3],
    }
}

fn image_resize_anchor_screen_pos(
    corners: [egui::Pos2; 4],
    handle: ImageResizeHandle,
) -> egui::Pos2 {
    match handle {
        ImageResizeHandle::TopLeft => corners[2],
        ImageResizeHandle::TopRight => corners[3],
        ImageResizeHandle::BottomRight => corners[0],
        ImageResizeHandle::BottomLeft => corners[1],
    }
}

fn hit_test_selected_image_rotation_handle(
    state: &CanvasState,
    rect: egui::Rect,
    pointer_pos: egui::Pos2,
) -> bool {
    let Some((_, handle_pos)) = selected_image_rotation_handle_screen_pos(state, rect) else {
        return false;
    };
    handle_pos.distance(pointer_pos) <= IMAGE_ROTATE_HANDLE_HIT_RADIUS_SCREEN
}

pub(super) fn selected_image_rotation_handle_screen_pos(
    state: &CanvasState,
    rect: egui::Rect,
) -> Option<(egui::Pos2, egui::Pos2)> {
    if !state.document.selected_annotation_ids.is_empty() {
        return None;
    }

    if let Some(image) = state.selected_single_image() {
        return Some(image_rotation_handle_screen_pos(
            &image.record,
            state.pan,
            state.zoom,
            rect.center(),
        ));
    }

    let rotation = selected_group_rotation_angle(state)?;
    selected_image_rotation_screen_rect(state, rect).map(|selection_rect| {
        group_rotation_handle_screen_pos(selection_rect.expand(6.0), rotation)
    })
}

fn selected_image_rotation_screen_rect(
    state: &CanvasState,
    rect: egui::Rect,
) -> Option<egui::Rect> {
    if selected_existing_image_ids(state).is_empty() {
        return None;
    }
    state.selection_screen_rect(rect)
}

fn selected_existing_image_ids(state: &CanvasState) -> Vec<i64> {
    state
        .document
        .selected_item_ids
        .iter()
        .copied()
        .filter(|item_id| state.image_by_item_id(*item_id).is_some())
        .collect()
}

fn selected_group_rotation_angle(state: &CanvasState) -> Option<f32> {
    let mut sin_sum = 0.0_f32;
    let mut cos_sum = 0.0_f32;
    let mut count = 0_usize;
    for item_id in &state.document.selected_item_ids {
        let Some(image) = state.image_by_item_id(*item_id) else {
            continue;
        };
        sin_sum += image.record.item.rotation.sin();
        cos_sum += image.record.item.rotation.cos();
        count += 1;
    }
    (count > 0).then(|| sin_sum.atan2(cos_sum))
}

pub(super) fn group_rotation_handle_screen_pos(
    selection_rect: egui::Rect,
    rotation: f32,
) -> (egui::Pos2, egui::Pos2) {
    let center = selection_rect.center();
    let outward = egui::vec2(rotation.sin(), -rotation.cos());
    let mut distance = f32::INFINITY;
    if outward.x > f32::EPSILON {
        distance = distance.min((selection_rect.right() - center.x) / outward.x);
    } else if outward.x < -f32::EPSILON {
        distance = distance.min((selection_rect.left() - center.x) / outward.x);
    }
    if outward.y > f32::EPSILON {
        distance = distance.min((selection_rect.bottom() - center.y) / outward.y);
    } else if outward.y < -f32::EPSILON {
        distance = distance.min((selection_rect.top() - center.y) / outward.y);
    }

    let line_start = center + outward * distance.max(0.0);
    let handle_pos = line_start + outward * super::IMAGE_ROTATE_HANDLE_OFFSET_SCREEN;
    (line_start, handle_pos)
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
    let corners = super::geometry::image_screen_corners_for_record(
        &image.record,
        state.pan,
        state.zoom,
        rect.center(),
    );
    [
        ImageResizeHandle::TopLeft,
        ImageResizeHandle::TopRight,
        ImageResizeHandle::BottomLeft,
        ImageResizeHandle::BottomRight,
    ]
    .into_iter()
    .find(|handle| {
        image_resize_handle_screen_pos(corners, *handle).distance(pointer_pos)
            <= IMAGE_RESIZE_HIT_RADIUS
    })
}

pub(super) fn resized_image_transform(
    asset: &crate::data::ImageAsset,
    start_scale: f32,
    anchor_screen: egui::Pos2,
    start_handle_screen: egui::Pos2,
    pointer_pos: egui::Pos2,
) -> (egui::Pos2, f32) {
    let start_delta = start_handle_screen - anchor_screen;
    let pointer_delta = pointer_pos - anchor_screen;
    let start_len_sq = start_delta.length_sq();
    let ratio = if start_len_sq <= f32::EPSILON {
        1.0
    } else {
        pointer_delta.dot(start_delta) / start_len_sq
    };
    let scale = (start_scale * ratio).max(min_image_scale(asset));
    let normalized_ratio = scale / start_scale.max(f32::EPSILON);
    let handle_screen = anchor_screen + start_delta * normalized_ratio;
    (anchor_screen.lerp(handle_screen, 0.5), scale)
}

fn rotated_image_delta(
    center_screen: egui::Pos2,
    pointer_pos: egui::Pos2,
    start_pointer_angle: f32,
    snap_degrees: f32,
    unsnapped: bool,
) -> f32 {
    let pointer_delta = pointer_pos - center_screen;
    if pointer_delta.length_sq() <= f32::EPSILON {
        return 0.0;
    }

    let rotation = pointer_delta.angle() - start_pointer_angle;
    if unsnapped || snap_degrees <= f32::EPSILON {
        rotation
    } else {
        let snap_radians = snap_degrees.to_radians();
        (rotation / snap_radians).round() * snap_radians
    }
}

fn rotate_vec2(offset: egui::Vec2, angle: f32) -> egui::Vec2 {
    let sin = angle.sin();
    let cos = angle.cos();
    egui::vec2(
        offset.x * cos - offset.y * sin,
        offset.x * sin + offset.y * cos,
    )
}

pub(super) fn compute_drag_snap_preview(
    state: &mut CanvasState,
    rect: egui::Rect,
    drag_state: &DragState,
    world_delta: egui::Vec2,
) -> Option<egui::Rect> {
    if drag_state.starting_items.len() == 1 {
        let snap_rect = compute_single_placement_guide_snap(state, rect, drag_state, world_delta);
        if snap_rect.is_none() {
            state.clear_move_preview_placeholders();
            state.clear_guide_snap_indicators();
        }
        return snap_rect;
    }

    let excluded_ids = drag_state
        .starting_items
        .iter()
        .map(|item| item.id)
        .collect::<Vec<_>>();
    let obstacles = state.board_snap_obstacles_excluding(&excluded_ids);
    let Some(bounds) = offset_layout_bounds(&drag_state.starting_items, world_delta) else {
        state.clear_move_preview_placeholders();
        return None;
    };

    let max_snap_distance_world = IMAGE_MOVE_SNAP_MAX_DISTANCE_SCREEN / state.zoom.max(0.01);
    let request = crate::layout_snap::MoveSnapRequest {
        desired_rect: bounds,
        drag_delta: world_delta,
        gap: state.layout_gap,
        max_snap_distance: max_snap_distance_world,
    };
    let candidate = apply_preview_hysteresis(
        crate::layout_snap::best_move_snap_candidate(&obstacles, &request),
        drag_state
            .snap_preview_rect_world
            .and_then(|previous_rect| {
                crate::layout_snap::validate_move_snap_candidate(
                    &obstacles,
                    &request,
                    previous_rect,
                )
            }),
        (IMAGE_MOVE_SNAP_MAX_DISTANCE_SCREEN * 0.2) / state.zoom.max(0.01),
    );
    let Some(candidate) = candidate else {
        state.clear_move_preview_placeholders();
        return None;
    };

    let candidate_screen = state.world_to_screen(candidate.rect.center(), rect);
    let desired_screen = state.world_to_screen(bounds.center(), rect);
    if candidate_screen.distance(desired_screen) >= IMAGE_MOVE_SNAP_MAX_DISTANCE_SCREEN {
        state.clear_move_preview_placeholders();
        return None;
    }

    state.clear_move_preview_placeholders();
    Some(candidate.rect)
}

fn compute_single_placement_guide_snap(
    state: &mut CanvasState,
    screen_rect: egui::Rect,
    drag_state: &DragState,
    world_delta: egui::Vec2,
) -> Option<egui::Rect> {
    let item = drag_state.starting_items.first()?;
    let desired_rect = offset_layout_bounds(&drag_state.starting_items, world_delta)?;
    let candidates = guide_snap_candidate_rects(state, screen_rect, item.id);
    if candidates.is_empty() {
        return None;
    }

    let threshold = guide_snap_threshold_world(state, drag_state);
    let mut best_x = best_axis_alignment_snap_x(desired_rect, &candidates, threshold);
    consider_horizontal_adjacency_snaps(
        desired_rect,
        &candidates,
        state.layout_gap,
        threshold,
        &mut best_x,
    );
    let mut best_y = best_axis_alignment_snap_y(desired_rect, &candidates, threshold);
    consider_vertical_adjacency_snaps(
        desired_rect,
        &candidates,
        state.layout_gap,
        threshold,
        &mut best_y,
    );

    let nudge = egui::vec2(
        best_x.map(|snap| snap.nudge).unwrap_or(0.0),
        best_y.map(|snap| snap.nudge).unwrap_or(0.0),
    );
    if nudge.length_sq() <= GUIDE_SNAP_EPSILON {
        state.clear_guide_snap_indicators();
        return None;
    }
    let snapped_rect = desired_rect.translate(nudge);
    let mut indicators = Vec::new();
    if let Some(snap) = best_x {
        indicators.push(guide_indicator_for_axis(
            GuideAxis::Vertical,
            snap.coord,
            snap.kind,
            snapped_rect,
            &candidates,
        ));
    }
    if let Some(snap) = best_y {
        indicators.push(guide_indicator_for_axis(
            GuideAxis::Horizontal,
            snap.coord,
            snap.kind,
            snapped_rect,
            &candidates,
        ));
    }
    if let (Some(x_snap), Some(y_snap)) = (best_x, best_y) {
        add_guide_intersection_marks(&mut indicators, egui::pos2(x_snap.coord, y_snap.coord));
    }
    state.set_guide_snap_indicators(indicators);
    Some(snapped_rect)
}

#[derive(Clone, Copy, Debug)]
struct GuideAxisSnap {
    nudge: f32,
    distance: f32,
    coord: f32,
    kind: GuideSnapKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GuideAxis {
    Horizontal,
    Vertical,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GuideSnapKind {
    Edge,
    Center,
    Adjacency,
}

fn guide_snap_threshold_world(state: &CanvasState, drag_state: &DragState) -> f32 {
    let threshold_screen = if drag_state.snap_preview_rect_world.is_some() {
        GUIDE_SNAP_RELEASE_DISTANCE_SCREEN
    } else {
        GUIDE_SNAP_ACQUIRE_DISTANCE_SCREEN
    };
    threshold_screen / state.zoom.max(0.01)
}

fn guide_snap_candidate_rects(
    state: &CanvasState,
    screen_rect: egui::Rect,
    excluded_item_id: i64,
) -> Vec<egui::Rect> {
    let viewport = viewport_near_world_rect(state, screen_rect);
    state
        .document
        .images
        .iter()
        .filter(|image| image.record.item.id != excluded_item_id)
        .filter_map(|image| {
            let visual_size = super::rotated_visual_size(
                image.record.asset.width as f32 * image.record.item.scale.max(0.01),
                image.record.asset.height as f32 * image.record.item.scale.max(0.01),
                image.record.item.rotation,
            );
            let rect = egui::Rect::from_center_size(
                egui::pos2(image.record.item.x, image.record.item.y),
                visual_size,
            );
            rect.intersects(viewport).then_some(rect)
        })
        .collect()
}

fn viewport_near_world_rect(state: &CanvasState, screen_rect: egui::Rect) -> egui::Rect {
    let expanded = screen_rect.expand(GUIDE_SNAP_VIEWPORT_PADDING_SCREEN);
    egui::Rect::from_two_pos(
        state.screen_to_world(expanded.min, screen_rect),
        state.screen_to_world(expanded.max, screen_rect),
    )
}

fn best_axis_alignment_snap_x(
    desired_rect: egui::Rect,
    candidates: &[egui::Rect],
    threshold: f32,
) -> Option<GuideAxisSnap> {
    let desired_coords = [
        (desired_rect.left(), false),
        (desired_rect.center().x, true),
        (desired_rect.right(), false),
    ];
    let mut best = None;
    for candidate in candidates {
        let candidate_coords = [
            (candidate.left(), false),
            (candidate.center().x, true),
            (candidate.right(), false),
        ];
        for (desired, desired_center) in desired_coords {
            for (target, target_center) in candidate_coords {
                if desired_center != target_center {
                    continue;
                }
                let kind = if desired_center {
                    GuideSnapKind::Center
                } else {
                    GuideSnapKind::Edge
                };
                best = consider_axis_snap(best, target - desired, threshold, target, kind);
            }
        }
    }
    best
}

fn best_axis_alignment_snap_y(
    desired_rect: egui::Rect,
    candidates: &[egui::Rect],
    threshold: f32,
) -> Option<GuideAxisSnap> {
    let desired_coords = [
        (desired_rect.top(), false),
        (desired_rect.center().y, true),
        (desired_rect.bottom(), false),
    ];
    let mut best = None;
    for candidate in candidates {
        let candidate_coords = [
            (candidate.top(), false),
            (candidate.center().y, true),
            (candidate.bottom(), false),
        ];
        for (desired, desired_center) in desired_coords {
            for (target, target_center) in candidate_coords {
                if desired_center != target_center {
                    continue;
                }
                let kind = if desired_center {
                    GuideSnapKind::Center
                } else {
                    GuideSnapKind::Edge
                };
                best = consider_axis_snap(best, target - desired, threshold, target, kind);
            }
        }
    }
    best
}

fn consider_horizontal_adjacency_snaps(
    desired_rect: egui::Rect,
    candidates: &[egui::Rect],
    gap: f32,
    threshold: f32,
    best: &mut Option<GuideAxisSnap>,
) {
    for candidate in candidates {
        if vertical_overlap_span(desired_rect, *candidate) <= GUIDE_SNAP_EPSILON {
            continue;
        }
        for nudge in [
            candidate.right() + gap - desired_rect.left(),
            candidate.left() - gap - desired_rect.right(),
        ] {
            let coord = if nudge > 0.0 {
                desired_rect.left() + nudge
            } else {
                desired_rect.right() + nudge
            };
            *best = consider_axis_snap(*best, nudge, threshold, coord, GuideSnapKind::Adjacency);
        }
    }
}

fn consider_vertical_adjacency_snaps(
    desired_rect: egui::Rect,
    candidates: &[egui::Rect],
    gap: f32,
    threshold: f32,
    best: &mut Option<GuideAxisSnap>,
) {
    for candidate in candidates {
        if horizontal_overlap_span(desired_rect, *candidate) <= GUIDE_SNAP_EPSILON {
            continue;
        }
        for nudge in [
            candidate.bottom() + gap - desired_rect.top(),
            candidate.top() - gap - desired_rect.bottom(),
        ] {
            let coord = if nudge > 0.0 {
                desired_rect.top() + nudge
            } else {
                desired_rect.bottom() + nudge
            };
            *best = consider_axis_snap(*best, nudge, threshold, coord, GuideSnapKind::Adjacency);
        }
    }
}

fn consider_axis_snap(
    best: Option<GuideAxisSnap>,
    nudge: f32,
    threshold: f32,
    coord: f32,
    kind: GuideSnapKind,
) -> Option<GuideAxisSnap> {
    let distance = nudge.abs();
    if distance > threshold || distance <= GUIDE_SNAP_EPSILON {
        return best;
    }
    match best {
        Some(best) if best.distance <= distance => Some(best),
        _ => Some(GuideAxisSnap {
            nudge,
            distance,
            coord,
            kind,
        }),
    }
}

fn vertical_overlap_span(left: egui::Rect, right: egui::Rect) -> f32 {
    (left.bottom().min(right.bottom()) - left.top().max(right.top())).max(0.0)
}

fn horizontal_overlap_span(left: egui::Rect, right: egui::Rect) -> f32 {
    (left.right().min(right.right()) - left.left().max(right.left())).max(0.0)
}

fn guide_indicator_for_axis(
    axis: GuideAxis,
    coord: f32,
    kind: GuideSnapKind,
    snapped_rect: egui::Rect,
    candidates: &[egui::Rect],
) -> super::GuideSnapIndicator {
    let mut marks = Vec::new();
    collect_axis_marks(axis, coord, kind, snapped_rect, &mut marks);
    for candidate in candidates {
        collect_axis_marks(axis, coord, kind, *candidate, &mut marks);
    }
    dedupe_points(&mut marks);

    if marks.is_empty() {
        return super::GuideSnapIndicator {
            segments: Vec::new(),
            marks,
        };
    }

    let segment = match axis {
        GuideAxis::Vertical => {
            let min_y = marks
                .iter()
                .map(|point| point.y)
                .fold(f32::INFINITY, f32::min);
            let max_y = marks
                .iter()
                .map(|point| point.y)
                .fold(f32::NEG_INFINITY, f32::max);
            (egui::pos2(coord, min_y), egui::pos2(coord, max_y))
        }
        GuideAxis::Horizontal => {
            let min_x = marks
                .iter()
                .map(|point| point.x)
                .fold(f32::INFINITY, f32::min);
            let max_x = marks
                .iter()
                .map(|point| point.x)
                .fold(f32::NEG_INFINITY, f32::max);
            (egui::pos2(min_x, coord), egui::pos2(max_x, coord))
        }
    };

    super::GuideSnapIndicator {
        segments: vec![segment],
        marks,
    }
}

fn collect_axis_marks(
    axis: GuideAxis,
    coord: f32,
    kind: GuideSnapKind,
    rect: egui::Rect,
    marks: &mut Vec<egui::Pos2>,
) {
    for point in rect_snap_points(rect, kind) {
        let aligned = match axis {
            GuideAxis::Vertical => (point.x - coord).abs() <= GUIDE_SNAP_EPSILON,
            GuideAxis::Horizontal => (point.y - coord).abs() <= GUIDE_SNAP_EPSILON,
        };
        if aligned {
            marks.push(point);
        }
    }
}

fn rect_snap_points(rect: egui::Rect, kind: GuideSnapKind) -> Vec<egui::Pos2> {
    match kind {
        GuideSnapKind::Edge | GuideSnapKind::Adjacency => {
            vec![
                rect.left_top(),
                rect.right_top(),
                rect.left_bottom(),
                rect.right_bottom(),
            ]
        }
        GuideSnapKind::Center => vec![
            egui::pos2(rect.center().x, rect.top()),
            egui::pos2(rect.left(), rect.center().y),
            rect.center(),
            egui::pos2(rect.right(), rect.center().y),
            egui::pos2(rect.center().x, rect.bottom()),
        ],
    }
}

fn dedupe_points(points: &mut Vec<egui::Pos2>) {
    let mut deduped = Vec::with_capacity(points.len());
    for point in points.drain(..) {
        if deduped
            .iter()
            .any(|existing: &egui::Pos2| existing.distance(point) <= GUIDE_SNAP_EPSILON)
        {
            continue;
        }
        deduped.push(point);
    }
    *points = deduped;
}

fn add_guide_intersection_marks(indicators: &mut [super::GuideSnapIndicator], point: egui::Pos2) {
    for indicator in indicators {
        indicator.marks.push(point);
        dedupe_points(&mut indicator.marks);
    }
}

fn apply_preview_hysteresis(
    best: Option<crate::layout_snap::MoveSnapCandidate>,
    previous: Option<crate::layout_snap::MoveSnapCandidate>,
    hysteresis_distance: f32,
) -> Option<crate::layout_snap::MoveSnapCandidate> {
    match (best, previous) {
        (Some(best), Some(previous))
            if best.rect.center().distance(previous.rect.center()) <= hysteresis_distance =>
        {
            Some(previous)
        }
        (Some(best), _) => Some(best),
        (None, Some(previous)) => Some(previous),
        (None, None) => None,
    }
}

pub(super) fn offset_layout_bounds(
    items: &[SelectedLayoutItem],
    offset: egui::Vec2,
) -> Option<egui::Rect> {
    let mut bounds = egui::Rect::NOTHING;
    let mut found = false;
    for item in items {
        let rect = egui::Rect::from_center_size(
            egui::pos2(item.x + offset.x, item.y + offset.y),
            egui::vec2(item.width, item.height),
        );
        if found {
            bounds = bounds.union(rect);
        } else {
            bounds = rect;
            found = true;
        }
    }
    found.then_some(bounds)
}

fn apply_dragged_item_positions(
    state: &mut CanvasState,
    items: &[SelectedLayoutItem],
    world_delta: egui::Vec2,
    snap_preview_rect: Option<egui::Rect>,
) {
    let snap_offset = snap_preview_rect
        .and_then(|snap_rect| {
            offset_layout_bounds(items, world_delta)
                .map(|bounds| snap_rect.center() - bounds.center())
        })
        .unwrap_or(egui::Vec2::ZERO);

    for item in items {
        if let Some(image) = state
            .document
            .images
            .iter_mut()
            .find(|image| image.record.item.id == item.id)
        {
            image.record.item.x = item.x + world_delta.x + snap_offset.x;
            image.record.item.y = item.y + world_delta.y + snap_offset.y;
        }
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

#[cfg(test)]
mod tests {
    use super::{
        DragState, ImageTransformState, begin_image_rotation, compute_drag_snap_preview,
        map_mouse_mode_scroll_events, offset_layout_bounds,
        selected_image_rotation_handle_screen_pos,
    };
    use crate::canvas::CanvasState;
    use crate::data::{
        BoardItem, BoardItemWithAsset, CanvasAnnotation, CanvasAnnotationColor,
        CanvasAnnotationKind, CanvasImageLod, CanvasStrokeWidth, CanvasTextSize, ImageAsset,
    };
    use crate::jobs::{ImportOrigin, ImportedPreviewTexture, LoadedCanvasItem};
    use eframe::egui;
    use std::path::PathBuf;

    #[test]
    fn multi_image_selection_uses_group_rotation_handle() {
        let ctx = egui::Context::default();
        let mut canvas = CanvasState::default();
        add_test_image(&mut canvas, &ctx, 1, egui::pos2(-120.0, 0.0), 100, 80);
        add_test_image(&mut canvas, &ctx, 2, egui::pos2(120.0, 0.0), 100, 80);
        canvas.select_items(&[1, 2]);
        let screen_rect = test_screen_rect();
        let selection_rect = canvas
            .selection_screen_rect(screen_rect)
            .unwrap()
            .expand(6.0);

        let (line_start, handle_pos) =
            selected_image_rotation_handle_screen_pos(&canvas, screen_rect).unwrap();

        assert_eq!(
            line_start,
            egui::pos2(selection_rect.center().x, selection_rect.top())
        );
        assert_eq!(
            handle_pos,
            line_start - egui::vec2(0.0, super::super::IMAGE_ROTATE_HANDLE_OFFSET_SCREEN)
        );
    }

    #[test]
    fn group_rotation_handle_follows_selected_image_rotation() {
        let ctx = egui::Context::default();
        let mut canvas = CanvasState::default();
        add_test_image(&mut canvas, &ctx, 1, egui::pos2(-120.0, 0.0), 100, 80);
        add_test_image(&mut canvas, &ctx, 2, egui::pos2(120.0, 0.0), 100, 80);
        canvas.select_items(&[1, 2]);
        let _ = canvas.set_image_rotation(1, std::f32::consts::FRAC_PI_2);
        let _ = canvas.set_image_rotation(2, std::f32::consts::FRAC_PI_2);
        let screen_rect = test_screen_rect();
        let selection_rect = canvas
            .selection_screen_rect(screen_rect)
            .unwrap()
            .expand(6.0);

        let (line_start, handle_pos) =
            selected_image_rotation_handle_screen_pos(&canvas, screen_rect).unwrap();

        assert!((line_start.x - selection_rect.right()).abs() < 0.01);
        assert!((line_start.y - selection_rect.center().y).abs() < 0.01);
        assert_eq!(
            handle_pos,
            line_start + egui::vec2(super::super::IMAGE_ROTATE_HANDLE_OFFSET_SCREEN, 0.0)
        );
    }

    #[test]
    fn image_rotation_state_captures_selected_group() {
        let ctx = egui::Context::default();
        let mut canvas = CanvasState::default();
        add_test_image(&mut canvas, &ctx, 1, egui::pos2(-120.0, 0.0), 100, 80);
        add_test_image(&mut canvas, &ctx, 2, egui::pos2(120.0, 0.0), 100, 80);
        canvas.select_items(&[1, 2]);
        let screen_rect = test_screen_rect();
        let (_, handle_pos) =
            selected_image_rotation_handle_screen_pos(&canvas, screen_rect).unwrap();

        begin_image_rotation(&mut canvas, screen_rect, handle_pos);

        assert!(matches!(
            canvas.interaction.image_transform_state,
            Some(ImageTransformState::Rotating {
                ref item_ids,
                ref starting_items,
                ..
            }) if item_ids == &vec![1, 2] && starting_items.len() == 2
        ));
    }

    #[test]
    fn drag_snap_preview_acquires_nearby_placement_target() {
        let ctx = egui::Context::default();
        let mut canvas = CanvasState::default();
        add_test_image(&mut canvas, &ctx, 1, egui::pos2(-120.0, 0.0), 180, 140);
        add_test_image(&mut canvas, &ctx, 2, egui::pos2(0.0, 0.0), 180, 140);
        canvas.select_items(&[2]);

        let drag_state = DragState {
            pointer_start_world: egui::Pos2::ZERO,
            starting_items: canvas.selected_layout_items(),
            snap_preview_rect_world: None,
            started_at_seconds: 0.0,
            last_movement_seconds: 0.0,
            last_world_delta: egui::Vec2::ZERO,
            moved: false,
        };

        let snap_rect = compute_drag_snap_preview(
            &mut canvas,
            test_screen_rect(),
            &drag_state,
            egui::vec2(82.0, 0.0),
        )
        .expect("expected snap preview");
        let current_bounds =
            offset_layout_bounds(&drag_state.starting_items, egui::vec2(82.0, 0.0))
                .expect("bounds");
        let snap_offset = snap_rect.center() - current_bounds.center();

        assert_eq!(snap_offset, egui::vec2(2.0, 0.0));
        assert_eq!(canvas.move_preview_placeholder_count(), 0);
    }

    #[test]
    fn drag_snap_preview_respects_max_snap_distance() {
        let ctx = egui::Context::default();
        let mut canvas = CanvasState::default();
        add_test_image(&mut canvas, &ctx, 1, egui::pos2(-120.0, 0.0), 180, 140);
        add_test_image(&mut canvas, &ctx, 2, egui::pos2(0.0, 0.0), 180, 140);
        canvas.select_items(&[2]);

        let drag_state = DragState {
            pointer_start_world: egui::Pos2::ZERO,
            starting_items: canvas.selected_layout_items(),
            snap_preview_rect_world: None,
            started_at_seconds: 0.0,
            last_movement_seconds: 0.0,
            last_world_delta: egui::Vec2::ZERO,
            moved: false,
        };

        let snap_rect = compute_drag_snap_preview(
            &mut canvas,
            test_screen_rect(),
            &drag_state,
            egui::vec2(220.0, 0.0),
        );

        assert!(snap_rect.is_none());
        assert_eq!(canvas.move_preview_placeholder_count(), 0);
    }

    #[test]
    fn drag_snap_preview_ignores_textbox_targets_for_guide_snapping() {
        let ctx = egui::Context::default();
        let mut canvas = CanvasState::default();
        add_test_image(&mut canvas, &ctx, 2, egui::pos2(0.0, 0.0), 180, 140);
        canvas.add_annotation(CanvasAnnotation {
            id: 10,
            kind: CanvasAnnotationKind::Textbox,
            start_x: -210.0,
            start_y: -70.0,
            end_x: -30.0,
            end_y: 70.0,
            text: "textbox".to_owned(),
            text_size: CanvasTextSize::default(),
            stroke_width: CanvasStrokeWidth::default(),
            color: CanvasAnnotationColor::Sky,
            background_enabled: false,
            archived: false,
        });
        canvas.select_items(&[2]);

        let drag_state = DragState {
            pointer_start_world: egui::Pos2::ZERO,
            starting_items: canvas.selected_layout_items(),
            snap_preview_rect_world: None,
            started_at_seconds: 0.0,
            last_movement_seconds: 0.0,
            last_world_delta: egui::Vec2::ZERO,
            moved: false,
        };

        let snap_rect = compute_drag_snap_preview(
            &mut canvas,
            test_screen_rect(),
            &drag_state,
            egui::vec2(10.0, 0.0),
        );

        assert!(snap_rect.is_none());
        assert_eq!(canvas.move_preview_placeholder_count(), 0);
    }

    #[test]
    fn drag_snap_preview_uses_larger_release_threshold_after_acquiring() {
        let ctx = egui::Context::default();
        let mut canvas = CanvasState::default();
        canvas.set_layout_gap(24.0);
        add_test_image(&mut canvas, &ctx, 1, egui::pos2(0.0, 0.0), 100, 100);
        add_test_image(&mut canvas, &ctx, 2, egui::pos2(180.0, 40.0), 100, 100);
        canvas.select_items(&[2]);

        let rect = test_screen_rect();
        let mut drag_state = DragState {
            pointer_start_world: egui::Pos2::ZERO,
            starting_items: canvas.selected_layout_items(),
            snap_preview_rect_world: None,
            started_at_seconds: 0.0,
            last_movement_seconds: 0.0,
            last_world_delta: egui::Vec2::ZERO,
            moved: false,
        };

        let first_delta = egui::vec2(-53.0, 0.0);
        let first_rect = compute_drag_snap_preview(&mut canvas, rect, &drag_state, first_delta)
            .expect("expected initial snap preview");
        drag_state.snap_preview_rect_world = Some(first_rect);

        let second_delta = egui::vec2(-46.0, 0.0);
        let second_rect = compute_drag_snap_preview(&mut canvas, rect, &drag_state, second_delta)
            .expect("expected release-threshold snap preview");

        assert_eq!(second_rect.center(), first_rect.center());
    }

    #[test]
    fn single_placement_guide_snapping_ignores_targets_outside_guide_threshold() {
        let ctx = egui::Context::default();
        let mut canvas = CanvasState::default();
        canvas.set_layout_gap(24.0);
        add_test_image(&mut canvas, &ctx, 1, egui::pos2(0.0, 0.0), 100, 100);
        add_test_image(&mut canvas, &ctx, 2, egui::pos2(180.0, 40.0), 100, 100);
        canvas.select_items(&[2]);

        let drag_state = DragState {
            pointer_start_world: egui::Pos2::ZERO,
            starting_items: canvas.selected_layout_items(),
            snap_preview_rect_world: None,
            started_at_seconds: 0.0,
            last_movement_seconds: 0.0,
            last_world_delta: egui::Vec2::ZERO,
            moved: false,
        };

        let snap_rect = compute_drag_snap_preview(
            &mut canvas,
            test_screen_rect(),
            &drag_state,
            egui::vec2(-44.0, 0.0),
        );

        assert!(snap_rect.is_none());
    }

    #[test]
    fn single_placement_guide_snapping_acquires_layout_gap_adjacency() {
        let ctx = egui::Context::default();
        let mut canvas = CanvasState::default();
        canvas.set_layout_gap(24.0);
        add_test_image(&mut canvas, &ctx, 1, egui::pos2(0.0, 0.0), 100, 100);
        add_test_image(&mut canvas, &ctx, 2, egui::pos2(180.0, 40.0), 100, 100);
        canvas.select_items(&[2]);

        let drag_state = DragState {
            pointer_start_world: egui::Pos2::ZERO,
            starting_items: canvas.selected_layout_items(),
            snap_preview_rect_world: None,
            started_at_seconds: 0.0,
            last_movement_seconds: 0.0,
            last_world_delta: egui::Vec2::ZERO,
            moved: false,
        };

        let snap_rect = compute_drag_snap_preview(
            &mut canvas,
            test_screen_rect(),
            &drag_state,
            egui::vec2(-53.0, 0.0),
        )
        .expect("expected guide snap preview");

        assert_eq!(snap_rect.center(), egui::pos2(124.0, 40.0));
    }

    #[test]
    fn single_placement_guide_snapping_records_indicator() {
        let ctx = egui::Context::default();
        let mut canvas = CanvasState::default();
        canvas.set_layout_gap(24.0);
        add_test_image(&mut canvas, &ctx, 1, egui::pos2(0.0, 0.0), 100, 100);
        add_test_image(&mut canvas, &ctx, 2, egui::pos2(180.0, 40.0), 100, 100);
        canvas.select_items(&[2]);

        let drag_state = DragState {
            pointer_start_world: egui::Pos2::ZERO,
            starting_items: canvas.selected_layout_items(),
            snap_preview_rect_world: None,
            started_at_seconds: 0.0,
            last_movement_seconds: 0.0,
            last_world_delta: egui::Vec2::ZERO,
            moved: false,
        };

        let _ = compute_drag_snap_preview(
            &mut canvas,
            test_screen_rect(),
            &drag_state,
            egui::vec2(-53.0, 0.0),
        );

        assert_eq!(canvas.guide_snap_indicator_count(), 1);
        assert_eq!(canvas.guide_snap_mark_count(), 2);
    }

    #[test]
    fn guide_snapping_marks_all_edge_points_on_the_guide_line() {
        let ctx = egui::Context::default();
        let mut canvas = CanvasState::default();
        canvas.set_layout_gap(24.0);
        add_test_image(&mut canvas, &ctx, 1, egui::pos2(0.0, 0.0), 100, 100);
        add_test_image(&mut canvas, &ctx, 2, egui::pos2(160.0, 0.0), 100, 140);
        canvas.select_items(&[2]);

        let drag_state = DragState {
            pointer_start_world: egui::Pos2::ZERO,
            starting_items: canvas.selected_layout_items(),
            snap_preview_rect_world: None,
            started_at_seconds: 0.0,
            last_movement_seconds: 0.0,
            last_world_delta: egui::Vec2::ZERO,
            moved: false,
        };

        let snap_rect = compute_drag_snap_preview(
            &mut canvas,
            test_screen_rect(),
            &drag_state,
            egui::vec2(-67.0, 0.0),
        )
        .expect("expected edge guide snap");

        assert_eq!(snap_rect.left(), 50.0);
        assert_eq!(canvas.guide_snap_indicator_count(), 1);
        assert_eq!(canvas.guide_snap_mark_count(), 4);
        assert_eq!(
            canvas.interaction.guide_snap_indicators[0].segments[0],
            (egui::pos2(50.0, -70.0), egui::pos2(50.0, 70.0))
        );
    }

    #[test]
    fn guide_snapping_marks_center_axis_only_for_center_alignment() {
        let ctx = egui::Context::default();
        let mut canvas = CanvasState::default();
        add_test_image(&mut canvas, &ctx, 1, egui::pos2(0.0, 0.0), 100, 100);
        add_test_image(&mut canvas, &ctx, 2, egui::pos2(7.0, 180.0), 60, 100);
        canvas.select_items(&[2]);

        let drag_state = DragState {
            pointer_start_world: egui::Pos2::ZERO,
            starting_items: canvas.selected_layout_items(),
            snap_preview_rect_world: None,
            started_at_seconds: 0.0,
            last_movement_seconds: 0.0,
            last_world_delta: egui::Vec2::ZERO,
            moved: false,
        };

        let snap_rect = compute_drag_snap_preview(
            &mut canvas,
            test_screen_rect(),
            &drag_state,
            egui::vec2(0.0, 0.0),
        )
        .expect("expected center guide snap");

        assert_eq!(snap_rect.center().x, 0.0);
        assert_eq!(canvas.guide_snap_indicator_count(), 1);
        assert_eq!(canvas.guide_snap_mark_count(), 6);
    }

    fn add_test_image(
        canvas: &mut CanvasState,
        ctx: &egui::Context,
        id: i64,
        center_world: egui::Pos2,
        width: u32,
        height: u32,
    ) {
        let asset = ImageAsset {
            id,
            original_path: PathBuf::from(format!("fixture-{id}.png")),
            filename: format!("fixture-{id}.png"),
            mime_type: "image/png".to_owned(),
            width,
            height,
            file_size: 0,
            imported_at: 0,
        };
        let record = BoardItemWithAsset {
            item: BoardItem {
                id,
                asset_id: id,
                x: center_world.x,
                y: center_world.y,
                scale: 1.0,
                rotation: 0.0,
                z_index: 0,
                archived: false,
            },
            asset,
        };
        let rgba = vec![255; width as usize * height as usize * 4];
        canvas.add_loaded_item(
            ctx,
            LoadedCanvasItem {
                target_board_id: 1,
                queue_id: None,
                record,
                preview_rgba: rgba,
                preview_width: width,
                preview_height: height,
                animated_preview: None,
                preview_texture: ImportedPreviewTexture::Cpu,
                loaded_lod: CanvasImageLod::Working,
                existing_load_diagnostics: None,
                origin: ImportOrigin::Existing,
                trace: None,
            },
        );
    }

    fn test_screen_rect() -> egui::Rect {
        egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(960.0, 720.0))
    }

    fn wheel_event(unit: egui::MouseWheelUnit, delta: egui::Vec2) -> egui::Event {
        egui::Event::MouseWheel {
            unit,
            delta,
            phase: egui::TouchPhase::Move,
            modifiers: egui::Modifiers::NONE,
        }
    }

    #[test]
    fn mouse_mode_wheel_up_zooms_in() {
        let events = vec![wheel_event(
            egui::MouseWheelUnit::Line,
            egui::vec2(0.0, -1.0),
        )];
        let (zoom_factor, pan_delta) = map_mouse_mode_scroll_events(&events);
        assert!(
            zoom_factor > 1.0,
            "wheel up should zoom in, got {zoom_factor}"
        );
        assert!((zoom_factor - 1.1).abs() < 1e-5);
        assert_eq!(pan_delta, egui::Vec2::ZERO);
    }

    #[test]
    fn mouse_mode_wheel_down_zooms_out() {
        let events = vec![wheel_event(
            egui::MouseWheelUnit::Line,
            egui::vec2(0.0, 1.0),
        )];
        let (zoom_factor, _) = map_mouse_mode_scroll_events(&events);
        assert!(
            zoom_factor < 1.0,
            "wheel down should zoom out, got {zoom_factor}"
        );
        assert!((zoom_factor - (1.0 / 1.1)).abs() < 1e-5);
    }

    #[test]
    fn mouse_mode_does_not_pan_from_wheel_notches() {
        let events = vec![wheel_event(
            egui::MouseWheelUnit::Line,
            egui::vec2(0.0, 3.0),
        )];
        let (_, pan_delta) = map_mouse_mode_scroll_events(&events);
        assert_eq!(pan_delta, egui::Vec2::ZERO);
    }

    #[test]
    fn mouse_mode_passes_trackpad_pan_through() {
        let events = vec![wheel_event(
            egui::MouseWheelUnit::Point,
            egui::vec2(8.0, -4.0),
        )];
        let (zoom_factor, pan_delta) = map_mouse_mode_scroll_events(&events);
        assert_eq!(zoom_factor, 1.0);
        assert_eq!(pan_delta, egui::vec2(8.0, -4.0));
    }

    #[test]
    fn mouse_mode_passes_pinch_zoom_through() {
        let events = vec![egui::Event::Zoom(1.25)];
        let (zoom_factor, pan_delta) = map_mouse_mode_scroll_events(&events);
        assert!((zoom_factor - 1.25).abs() < 1e-6);
        assert_eq!(pan_delta, egui::Vec2::ZERO);
    }

    #[test]
    fn mouse_mode_ignores_horizontal_wheel() {
        let events = vec![wheel_event(
            egui::MouseWheelUnit::Line,
            egui::vec2(2.0, 0.0),
        )];
        let (zoom_factor, pan_delta) = map_mouse_mode_scroll_events(&events);
        assert_eq!(zoom_factor, 1.0);
        assert_eq!(pan_delta, egui::Vec2::ZERO);
    }
}
