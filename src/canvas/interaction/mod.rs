use eframe::egui;

mod annotation;
mod crop;
mod placement_transform;
mod selection;
mod textbox;
mod viewport;

pub(super) use annotation::{
    annotation_hit_test, annotation_screen_points, annotation_screen_rect, annotation_world_rect,
    begin_annotation_drag, hit_test_annotation, pending_annotation_from_draft,
};
pub(super) use crop::{begin_crop, finish_crop, update_crop};
pub(super) use placement_transform::{
    begin_image_drag_scale, begin_image_resize, begin_image_rotation, compute_resize_snap_preview,
    compute_uniform_scale_snap_preview, hit_test_image_resize_handle,
    hit_test_selected_image_rotation_handle, image_drag_scale_factor, image_resize_cursor_icon,
    image_screen_rect, resized_image_transform, rotate_vec2, rotated_image_delta,
    scaled_image_transforms, selected_image_rotation_handle_screen_pos,
};
pub(super) use selection::{
    begin_marquee_selection, hit_test_canvas_item, open_context_menu_at_pointer,
    update_marquee_selection,
};
pub(super) use textbox::{
    handle_textbox_primary_press, hit_test_textbox_resize_handle, moved_textbox_rect,
    normalized_textbox_rect, resized_textbox_rect, textbox_resize_cursor_icon, textbox_screen_rect,
    textbox_world_rect,
};

use std::time::Duration;
#[cfg(test)]
use viewport::{animate_mouse_wheel_zoom_around_pointer, map_mouse_mode_scroll_events};
pub(super) use viewport::{
    apply_canvas_navigation, handle_viewport_navigation_input, step_mouse_wheel_zoom_animation,
};

#[cfg(test)]
use super::ImageScaleStart;
use super::geometry::{image_contains_screen_point, min_image_scale};
use super::render::textbox_pointer_region;
use super::{
    AnnotationDraftState, CanvasHit, CanvasState, ImageTransformState,
    TEXTBOX_DRAG_THRESHOLD_SCREEN, TextboxPointerRegion, TextboxTransformState,
};
use crate::data::CanvasAnnotationKind;

// Suppresses drag movement (and guide snapping) for the first 90ms after
// primary-press on a placement so click-to-select doesn't jerk the image via
// snap.
const IMAGE_MOVE_DEBOUNCE_SECONDS: f64 = 0.090;

fn scale_drag_active(modifiers: egui::Modifiers) -> bool {
    modifiers.command && modifiers.alt
}

fn guide_snap_disabled(modifiers: egui::Modifiers) -> bool {
    modifiers.command && !modifiers.alt
}

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
    let image_drag_scale_modifier_held = ui.input(|i| scale_drag_active(i.modifiers));
    let pointer_in_canvas_rect = pointer_pos
        .map(|pointer_pos| {
            input_rect.contains(pointer_pos)
                && !overlay_rect.is_some_and(|overlay_rect| overlay_rect.contains(pointer_pos))
        })
        .unwrap_or(false);

    if pointer_in_canvas_rect && response.dragged_by(egui::PointerButton::Middle) {
        state.interaction.mouse_wheel_zoom_animation = None;
        state.pan += response.drag_delta();
    }

    if pointer_in_canvas_rect
        && primary_pressed
        && image_drag_scale_modifier_held
        && let Some(pointer_pos) = pointer_pos
        && let Some(placement_id) = hit_test_image(state, rect, pointer_pos)
    {
        begin_image_drag_scale(state, rect, placement_id, pointer_pos);
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        return;
    }

    // Alt+LMB pan: starts a canvas pan even over a placement, mirroring PureRef.
    if state.interaction.alt_pan_active {
        if primary_released {
            state.interaction.alt_pan_active = false;
        } else if primary_down {
            state.interaction.mouse_wheel_zoom_animation = None;
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
        Some(ImageTransformState::Rotating { .. } | ImageTransformState::Scaling { .. })
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
        && let Some(image) = state.selected_single_rendered_placement()
        && let Some(handle) = hit_test_image_resize_handle(state, rect, image, pointer_pos)
    {
        ui.ctx().set_cursor_icon(image_resize_cursor_icon(handle));
    } else if pointer_in_canvas_rect
        && image_drag_scale_modifier_held
        && let Some(pointer_pos) = pointer_pos
        && hit_test_image(state, rect, pointer_pos).is_some()
    {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
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
            state.interaction.annotation_draft_state =
                Some(AnnotationDraftState::new(kind, pointer_world));
        } else if crop_key_held && let Some(placement_id) = hit_test_image(state, rect, pointer_pos)
        {
            if !begin_crop(state, placement_id, pointer_pos) {
                return;
            }
        } else if let Some(image) = state.selected_single_rendered_placement().map(|image| {
            (
                image.placement.state.id,
                hit_test_image_resize_handle(state, rect, image, pointer_pos),
            )
        }) && let (placement_id, Some(handle)) = image
        {
            begin_image_resize(state, rect, placement_id, handle);
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
                        state.document.selected_placement_ids.clear();
                        state.document.selected_annotation_ids = vec![annotation_id];
                        begin_annotation_drag(state, rect, annotation_id, pointer_pos);
                        state.interaction.image_transform_state = None;
                        state.interaction.marquee_state = None;
                        state.interaction.crop_state = None;
                        state.interaction.annotation_draft_state = None;
                        return;
                    }
                }
                CanvasHit::Placement(placement_id) => {
                    if shift_held {
                        state.add_to_selection(placement_id);
                    } else if !state.is_selected(placement_id) {
                        state.set_single_selection(placement_id);
                    }

                    let pointer_world = state.screen_to_world(pointer_pos, rect);
                    let now = ui.input(|input| input.time);
                    crate::placement::movement::begin_selected_placement_drag(
                        state,
                        pointer_world,
                        now,
                    );
                    state.interaction.image_transform_state = None;
                    state.interaction.textbox_transform_state = None;
                    state.interaction.annotation_drag_state = None;
                    state.interaction.marquee_state = None;
                    state.interaction.crop_state = None;
                    state.interaction.annotation_draft_state = None;
                    return;
                }
            }
        } else {
            begin_marquee_selection(state, pointer_pos, shift_held);
        }
    }

    if state.interaction.crop_state.is_some()
        && primary_down
        && let Some(pointer_pos) = pointer_pos
    {
        update_crop(state, pointer_pos);
    }

    if primary_down
        && let Some(pointer_pos) = pointer_pos
        && let Some(draft_state) = &mut state.interaction.annotation_draft_state
    {
        let world = (pointer_pos - rect.center() - state.pan) / state.zoom;
        let pointer_world = egui::pos2(world.x, world.y);
        draft_state.set_current_world(pointer_world);
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
            for (annotation_id, segment) in drag_state.starting_segments {
                state.set_annotation_world_points(
                    annotation_id,
                    segment.start + world_delta,
                    segment.end + world_delta,
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
                for (annotation_id, start_geometry) in start_rects_world {
                    let world_rect = moved_textbox_rect(
                        start_geometry.unrotated_bounds(),
                        pointer_start_world,
                        pointer_world,
                    );
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
                    start_rect_world.unrotated_bounds(),
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
                placement_id,
                anchor_screen,
                start_handle_screen,
                start_scale,
            } => {
                if let Some(image) = state.rendered_placement_by_id(placement_id).cloned() {
                    let (center_screen, scale) = resized_image_transform(
                        image
                            .placement
                            .asset
                            .as_ref()
                            .expect("loaded placement asset"),
                        start_scale,
                        anchor_screen,
                        start_handle_screen,
                        pointer_pos,
                    );
                    let center_world = state.screen_to_world(center_screen, rect);
                    let anchor_world = state.screen_to_world(anchor_screen, rect);
                    let base_size = image.placement.base_display_size();
                    let base_display_size = super::rotated_display_size(
                        base_size.x,
                        base_size.y,
                        image.placement.state.geometry.rotation,
                    );
                    let desired_rect = egui::Rect::from_center_size(
                        center_world,
                        base_display_size * scale.max(0.01),
                    );
                    let guide_snap_disabled =
                        ui.input(|input| guide_snap_disabled(input.modifiers));
                    let snap_result = if guide_snap_disabled {
                        state.clear_guide_snap_indicators();
                        None
                    } else {
                        compute_resize_snap_preview(
                            state,
                            rect,
                            placement_id,
                            desired_rect,
                            anchor_world,
                            base_display_size,
                            min_image_scale(
                                image
                                    .placement
                                    .asset
                                    .as_ref()
                                    .expect("loaded placement asset"),
                            ),
                        )
                    };
                    let (center_world, scale) = snap_result
                        .map(|snap_rect| {
                            (
                                snap_rect.center(),
                                (snap_rect.width() / base_display_size.x.max(f32::EPSILON)).max(
                                    min_image_scale(
                                        image
                                            .placement
                                            .asset
                                            .as_ref()
                                            .expect("loaded placement asset"),
                                    ),
                                ),
                            )
                        })
                        .unwrap_or((center_world, scale));
                    state.set_image_center_and_scale(placement_id, center_world, scale);
                }
            }
            ImageTransformState::Scaling {
                pointer_start_screen,
                group_center_world,
                starting_items,
                placement_ids,
                ..
            } => {
                let raw_factor = image_drag_scale_factor(pointer_start_screen, pointer_pos);
                let factor = compute_uniform_scale_snap_preview(
                    state,
                    rect,
                    &placement_ids,
                    group_center_world,
                    &starting_items,
                    raw_factor,
                )
                .unwrap_or(raw_factor);
                for transform in
                    scaled_image_transforms(&starting_items, group_center_world, factor)
                {
                    state.set_image_center_and_scale(
                        transform.placement_id,
                        transform.center_world,
                        transform.scale,
                    );
                }
            }
            ImageTransformState::Rotating {
                placement_ids: _,
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
                for (placement_id, start_center_world, start_rotation) in starting_items {
                    let offset = start_center_world - center_world;
                    let rotated_offset = rotate_vec2(offset, rotation_delta);
                    state.set_image_center_and_rotation(
                        placement_id,
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
        let guide_snap_disabled = ui.input(|input| guide_snap_disabled(input.modifiers));
        let now = ui.input(|input| input.time);
        crate::placement::movement::update_selected_placement_drag(
            state,
            rect,
            pointer_world,
            guide_snap_disabled,
            now,
        );
        ui.ctx().request_repaint_after(Duration::from_secs_f64(
            super::SELECTION_BORDER_REAPPEAR_SECONDS,
        ));
    }

    if primary_released {
        if finish_crop(state, rect) {
        } else if let Some(draft_state) = state.interaction.annotation_draft_state.take() {
            state.interaction.pending_annotation = pending_annotation_from_draft(draft_state);
        } else if let Some(transform_state) = state.interaction.image_transform_state.take() {
            match transform_state {
                ImageTransformState::Resizing { placement_id, .. } => {
                    state.mark_dirty(placement_id)
                }
                ImageTransformState::Scaling { placement_ids, .. } => {
                    for placement_id in placement_ids {
                        state.mark_dirty(placement_id);
                    }
                }
                ImageTransformState::Rotating { placement_ids, .. } => {
                    for placement_id in placement_ids {
                        state.mark_dirty(placement_id);
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
            state.clear_guide_snap_indicators();
        }
        state.interaction.marquee_state = None;
    }

    if primary_released
        && primary_double_clicked
        && pointer_in_canvas_rect
        && let Some(pointer_pos) = pointer_pos
        && let Some(placement_id) = hit_test_image(state, rect, pointer_pos)
    {
        if !state.is_selected(placement_id) {
            state.set_single_selection(placement_id);
        }
        state.interaction.pending_focus_selection_request = true;
        return;
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
        && state.interaction.marquee_state.is_some()
        && ui.input(|i| i.pointer.primary_down())
        && let Some(pointer_pos) = pointer_pos
    {
        update_marquee_selection(state, rect, pointer_pos, shift_held);
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
        .rendered_placements
        .iter()
        .rev()
        .find(|image| {
            image_contains_screen_point(
                &image.placement,
                pointer_pos,
                state.pan,
                state.zoom,
                rect.center(),
            )
        })
        .map(|image| image.placement.state.id)
}

#[cfg(test)]
mod tests {
    use super::{
        ImageTransformState, animate_mouse_wheel_zoom_around_pointer, begin_image_drag_scale,
        begin_image_rotation, compute_uniform_scale_snap_preview, image_drag_scale_factor,
        map_mouse_mode_scroll_events, scaled_image_transforms,
        selected_image_rotation_handle_screen_pos, step_mouse_wheel_zoom_animation,
    };
    use crate::canvas::{CanvasState, DragState};
    use crate::data::{
        CanvasAnnotation, CanvasAnnotationColor, CanvasAnnotationKind, CanvasImageLod,
        CanvasStrokeWidth, CanvasTextSize, ImageAsset, Placement, PlacementState,
    };
    use crate::jobs::{ImportOrigin, ImportedPreviewTexture, PreparedPlacementImage};
    use crate::placement::movement::{compute_drag_snap_preview, layout_bounds_with_offset};
    use eframe::egui;
    use std::path::PathBuf;

    #[test]
    fn multi_image_selection_uses_group_rotation_handle() {
        let ctx = egui::Context::default();
        let mut canvas = CanvasState::default();
        add_test_image(&mut canvas, &ctx, 1, egui::pos2(-120.0, 0.0), 100, 80);
        add_test_image(&mut canvas, &ctx, 2, egui::pos2(120.0, 0.0), 100, 80);
        canvas.select_placements(&[1, 2]);
        let screen_rect = test_screen_rect();
        let selection_rect = canvas.selection_screen_rect(screen_rect).unwrap();

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
    fn cmd_alt_scale_drag_does_not_disable_guide_snapping() {
        let modifiers = egui::Modifiers {
            command: true,
            alt: true,
            ..Default::default()
        };

        assert!(super::scale_drag_active(modifiers));
        assert!(!super::guide_snap_disabled(modifiers));
    }

    #[test]
    fn plain_cmd_disables_guide_snapping() {
        let modifiers = egui::Modifiers {
            command: true,
            ..Default::default()
        };

        assert!(!super::scale_drag_active(modifiers));
        assert!(super::guide_snap_disabled(modifiers));
    }

    #[test]
    fn group_rotation_handle_follows_selected_image_rotation() {
        let ctx = egui::Context::default();
        let mut canvas = CanvasState::default();
        add_test_image(&mut canvas, &ctx, 1, egui::pos2(-120.0, 0.0), 100, 80);
        add_test_image(&mut canvas, &ctx, 2, egui::pos2(120.0, 0.0), 100, 80);
        canvas.select_placements(&[1, 2]);
        let _ = canvas.set_image_rotation(1, std::f32::consts::FRAC_PI_2);
        let _ = canvas.set_image_rotation(2, std::f32::consts::FRAC_PI_2);
        let screen_rect = test_screen_rect();
        let selection_rect = canvas.selection_screen_rect(screen_rect).unwrap();

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
        canvas.select_placements(&[1, 2]);
        let screen_rect = test_screen_rect();
        let (_, handle_pos) =
            selected_image_rotation_handle_screen_pos(&canvas, screen_rect).unwrap();

        begin_image_rotation(&mut canvas, screen_rect, handle_pos);

        assert!(matches!(
            canvas.interaction.image_transform_state,
            Some(ImageTransformState::Rotating {
                ref placement_ids,
                ref starting_items,
                ..
            }) if placement_ids == &vec![1, 2] && starting_items.len() == 2
        ));
    }

    #[test]
    fn horizontal_image_drag_scale_grows_to_the_right_and_shrinks_to_the_left() {
        let start = egui::pos2(200.0, 120.0);

        let grow = image_drag_scale_factor(start, egui::pos2(440.0, 120.0));
        let shrink = image_drag_scale_factor(start, egui::pos2(-40.0, 120.0));

        assert!((grow - 2.0).abs() < 0.01);
        assert!((shrink - 0.5).abs() < 0.01);
    }

    #[test]
    fn image_drag_scale_preserves_multi_image_layout_around_group_center() {
        let starting_items = vec![
            super::ImageScaleStart::new(
                1,
                egui::pos2(-100.0, 0.0),
                egui::vec2(100.0, 80.0),
                1.0,
                0.1,
            ),
            super::ImageScaleStart::new(
                2,
                egui::pos2(100.0, 0.0),
                egui::vec2(100.0, 80.0),
                0.5,
                0.1,
            ),
        ];

        let transforms = scaled_image_transforms(&starting_items, egui::pos2(0.0, 0.0), 2.0);

        assert_eq!(transforms[0].placement_id, 1);
        assert_eq!(transforms[0].center_world, egui::pos2(-200.0, 0.0));
        assert!((transforms[0].scale - 2.0).abs() < 0.01);
        assert_eq!(transforms[1].placement_id, 2);
        assert_eq!(transforms[1].center_world, egui::pos2(200.0, 0.0));
        assert!((transforms[1].scale - 1.0).abs() < 0.01);
    }

    #[test]
    fn image_drag_scale_state_captures_selected_group() {
        let ctx = egui::Context::default();
        let mut canvas = CanvasState::default();
        add_test_image(&mut canvas, &ctx, 1, egui::pos2(-100.0, 0.0), 100, 80);
        add_test_image(&mut canvas, &ctx, 2, egui::pos2(100.0, 0.0), 100, 80);
        canvas.select_placements(&[1, 2]);

        begin_image_drag_scale(&mut canvas, test_screen_rect(), 1, egui::pos2(380.0, 360.0));

        assert!(matches!(
            canvas.interaction.image_transform_state,
            Some(ImageTransformState::Scaling {
                ref placement_ids,
                group_center_world,
                ref starting_items,
                ..
            }) if placement_ids == &vec![1, 2]
                && group_center_world == egui::pos2(0.0, 0.0)
                && starting_items.len() == 2
        ));
    }

    #[test]
    fn image_drag_scale_snap_preview_returns_snapped_factor() {
        let ctx = egui::Context::default();
        let mut canvas = CanvasState::default();
        canvas.set_placement_gap(24.0);
        add_test_image(&mut canvas, &ctx, 1, egui::pos2(0.0, 0.0), 100, 100);
        add_test_image(&mut canvas, &ctx, 2, egui::pos2(174.0, 0.0), 100, 100);
        canvas.select_placements(&[1]);

        let starting_items = vec![super::ImageScaleStart::new(
            1,
            egui::Pos2::ZERO,
            egui::vec2(100.0, 100.0),
            1.0,
            0.1,
        )];

        let factor = compute_uniform_scale_snap_preview(
            &mut canvas,
            test_screen_rect(),
            &[1],
            egui::Pos2::ZERO,
            &starting_items,
            1.96,
        )
        .expect("expected scale snap");

        assert_eq!(factor, 2.0);
        assert_eq!(canvas.guide_snap_indicator_count(), 1);
    }

    #[test]
    fn drag_snap_preview_acquires_nearby_placement_target() {
        let ctx = egui::Context::default();
        let mut canvas = CanvasState::default();
        canvas.set_placement_gap(24.0);
        add_test_image(&mut canvas, &ctx, 1, egui::pos2(-120.0, 0.0), 180, 140);
        add_test_image(&mut canvas, &ctx, 2, egui::pos2(0.0, 0.0), 180, 140);
        canvas.select_placements(&[2]);

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
            layout_bounds_with_offset(&drag_state.starting_items, egui::vec2(82.0, 0.0))
                .expect("bounds");
        let snap_offset = snap_rect.center() - current_bounds.center();

        assert_eq!(snap_offset, egui::vec2(2.0, 0.0));
    }

    #[test]
    fn drag_snap_preview_respects_max_snap_distance() {
        let ctx = egui::Context::default();
        let mut canvas = CanvasState::default();
        add_test_image(&mut canvas, &ctx, 1, egui::pos2(-120.0, 0.0), 180, 140);
        add_test_image(&mut canvas, &ctx, 2, egui::pos2(0.0, 0.0), 180, 140);
        canvas.select_placements(&[2]);

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
    }

    #[test]
    fn drag_snap_preview_ignores_textbox_targets_for_guide_snapping() {
        let ctx = egui::Context::default();
        let mut canvas = CanvasState::default();
        add_test_image(&mut canvas, &ctx, 2, egui::pos2(0.0, 0.0), 180, 140);
        canvas.add_annotation(CanvasAnnotation::new(
            10,
            CanvasAnnotationKind::Textbox,
            eframe::egui::pos2(-210.0, -70.0),
            eframe::egui::pos2(-30.0, 70.0),
            "textbox".to_owned(),
            CanvasTextSize::default(),
            CanvasStrokeWidth::default(),
            CanvasAnnotationColor::Sky,
            false,
            false,
        ));
        canvas.select_placements(&[2]);

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
    }

    #[test]
    fn drag_snap_preview_uses_larger_release_threshold_after_acquiring() {
        let ctx = egui::Context::default();
        let mut canvas = CanvasState::default();
        canvas.set_placement_gap(24.0);
        add_test_image(&mut canvas, &ctx, 1, egui::pos2(0.0, 0.0), 100, 100);
        add_test_image(&mut canvas, &ctx, 2, egui::pos2(180.0, 40.0), 100, 100);
        canvas.select_placements(&[2]);

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
        canvas.set_placement_gap(24.0);
        add_test_image(&mut canvas, &ctx, 1, egui::pos2(0.0, 0.0), 100, 100);
        add_test_image(&mut canvas, &ctx, 2, egui::pos2(180.0, 40.0), 100, 100);
        canvas.select_placements(&[2]);

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
    fn single_placement_guide_snapping_acquires_placement_gap_adjacency() {
        let ctx = egui::Context::default();
        let mut canvas = CanvasState::default();
        canvas.set_placement_gap(24.0);
        add_test_image(&mut canvas, &ctx, 1, egui::pos2(0.0, 0.0), 100, 100);
        add_test_image(&mut canvas, &ctx, 2, egui::pos2(180.0, 40.0), 100, 100);
        canvas.select_placements(&[2]);

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
        canvas.set_placement_gap(24.0);
        add_test_image(&mut canvas, &ctx, 1, egui::pos2(0.0, 0.0), 100, 100);
        add_test_image(&mut canvas, &ctx, 2, egui::pos2(180.0, 40.0), 100, 100);
        canvas.select_placements(&[2]);

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
        assert_eq!(canvas.guide_snap_mark_count(), 4);
        assert_eq!(
            canvas.interaction.guide_snap_indicators[0].segments[0],
            (egui::pos2(74.0, -50.0), egui::pos2(74.0, 90.0))
        );
    }

    #[test]
    fn nonzero_gap_guide_snapping_does_not_acquire_target_image_border() {
        let ctx = egui::Context::default();
        let mut canvas = CanvasState::default();
        canvas.set_placement_gap(24.0);
        add_test_image(&mut canvas, &ctx, 1, egui::pos2(0.0, 0.0), 100, 100);
        add_test_image(&mut canvas, &ctx, 2, egui::pos2(160.0, 200.0), 100, 140);
        canvas.select_placements(&[2]);

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
        );

        assert!(snap_rect.is_none());
    }

    #[test]
    fn distant_edge_alignment_does_not_snap_without_contact() {
        let ctx = egui::Context::default();
        let mut canvas = CanvasState::default();
        canvas.set_placement_gap(24.0);
        add_test_image(&mut canvas, &ctx, 1, egui::pos2(0.0, 0.0), 100, 100);
        add_test_image(&mut canvas, &ctx, 2, egui::pos2(200.0, 300.0), 100, 100);
        canvas.select_placements(&[2]);

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
            egui::vec2(-200.0, 7.0),
        );

        assert!(snap_rect.is_none());
    }

    #[test]
    fn vertically_distant_candidate_does_not_contribute_horizontal_gap_snap() {
        let ctx = egui::Context::default();
        let mut canvas = CanvasState::default();
        canvas.set_placement_gap(24.0);
        add_test_image(&mut canvas, &ctx, 1, egui::pos2(0.0, 0.0), 100, 100);
        add_test_image(&mut canvas, &ctx, 2, egui::pos2(124.0, 0.0), 100, 100);
        add_test_image(&mut canvas, &ctx, 3, egui::pos2(0.0, 260.0), 100, 100);
        canvas.select_placements(&[2]);

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
            egui::vec2(-2.0, 0.0),
        )
        .expect("expected top-left contact-local snap");

        assert_eq!(snap_rect.left(), 74.0);
        assert_eq!(canvas.guide_snap_indicator_count(), 1);
        assert_eq!(
            canvas.interaction.guide_snap_indicators[0].segments[0],
            (egui::pos2(74.0, -50.0), egui::pos2(74.0, 50.0))
        );
    }

    #[test]
    fn horizontally_distant_candidate_does_not_contribute_vertical_gap_snap() {
        let ctx = egui::Context::default();
        let mut canvas = CanvasState::default();
        canvas.set_placement_gap(24.0);
        add_test_image(&mut canvas, &ctx, 1, egui::pos2(0.0, 0.0), 100, 100);
        add_test_image(&mut canvas, &ctx, 2, egui::pos2(0.0, 124.0), 100, 100);
        add_test_image(&mut canvas, &ctx, 3, egui::pos2(260.0, 0.0), 100, 100);
        canvas.select_placements(&[2]);

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
            egui::vec2(0.0, -2.0),
        )
        .expect("expected top target contact-local snap");

        assert_eq!(snap_rect.top(), 74.0);
        assert_eq!(canvas.guide_snap_indicator_count(), 1);
        assert_eq!(
            canvas.interaction.guide_snap_indicators[0].segments[0],
            (egui::pos2(-50.0, 74.0), egui::pos2(50.0, 74.0))
        );
    }

    #[test]
    fn gap_spacing_can_combine_with_real_edge_alignment() {
        let ctx = egui::Context::default();
        let mut canvas = CanvasState::default();
        canvas.set_placement_gap(24.0);
        add_test_image(&mut canvas, &ctx, 1, egui::pos2(0.0, 0.0), 100, 100);
        add_test_image(&mut canvas, &ctx, 2, egui::pos2(180.0, 8.0), 100, 100);
        canvas.select_placements(&[2]);

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
            egui::vec2(-53.0, -5.0),
        )
        .expect("expected gap spacing plus top-edge alignment");

        assert_eq!(snap_rect.left(), 74.0);
        assert_eq!(snap_rect.top(), -50.0);
        assert_eq!(canvas.guide_snap_indicator_count(), 3);
    }

    #[test]
    fn gap_spacing_can_snap_into_corner_from_different_targets() {
        let ctx = egui::Context::default();
        let mut canvas = CanvasState::default();
        canvas.set_placement_gap(24.0);
        add_test_image(&mut canvas, &ctx, 1, egui::pos2(0.0, 124.0), 100, 100);
        add_test_image(&mut canvas, &ctx, 2, egui::pos2(124.0, 0.0), 100, 100);
        add_test_image(&mut canvas, &ctx, 3, egui::pos2(130.0, 130.0), 100, 100);
        canvas.select_placements(&[3]);

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
            egui::vec2(-3.0, -3.0),
        )
        .expect("expected corner snap");

        assert_eq!(snap_rect.left(), 74.0);
        assert_eq!(snap_rect.top(), 74.0);
        assert_eq!(canvas.guide_snap_indicator_count(), 3);
    }

    #[test]
    fn guide_snapping_marks_all_edge_points_on_the_guide_line() {
        let ctx = egui::Context::default();
        let mut canvas = CanvasState::default();
        canvas.set_placement_gap(0.0);
        add_test_image(&mut canvas, &ctx, 1, egui::pos2(0.0, 0.0), 100, 100);
        add_test_image(&mut canvas, &ctx, 2, egui::pos2(160.0, 0.0), 100, 140);
        canvas.select_placements(&[2]);

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
        add_test_image(&mut canvas, &ctx, 2, egui::pos2(7.0, 100.0), 60, 100);
        canvas.select_placements(&[2]);

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
        assert_eq!(canvas.guide_snap_mark_count(), 5);
    }

    #[test]
    fn multi_placement_guide_snapping_uses_selection_bounding_box_edge() {
        let ctx = egui::Context::default();
        let mut canvas = CanvasState::default();
        add_test_image(&mut canvas, &ctx, 1, egui::pos2(0.0, 0.0), 100, 100);
        add_test_image(&mut canvas, &ctx, 2, egui::pos2(210.0, -40.0), 80, 80);
        add_test_image(&mut canvas, &ctx, 3, egui::pos2(320.0, 60.0), 60, 100);
        canvas.select_placements(&[2, 3]);

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
            egui::vec2(-119.0, 0.0),
        )
        .expect("expected multi-selection guide snap");

        assert_eq!(snap_rect.left(), 50.0);
        assert_eq!(canvas.guide_snap_indicator_count(), 1);
        assert_eq!(
            canvas.interaction.guide_snap_indicators[0].segments[0],
            (egui::pos2(50.0, -80.0), egui::pos2(50.0, 110.0))
        );
    }

    #[test]
    fn multi_placement_guide_snapping_uses_selection_bounding_box_center() {
        let ctx = egui::Context::default();
        let mut canvas = CanvasState::default();
        add_test_image(&mut canvas, &ctx, 1, egui::pos2(0.0, 0.0), 100, 100);
        add_test_image(&mut canvas, &ctx, 2, egui::pos2(170.0, 90.0), 80, 80);
        add_test_image(&mut canvas, &ctx, 3, egui::pos2(280.0, 190.0), 60, 100);
        canvas.select_placements(&[2, 3]);

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
            egui::vec2(-213.0, 0.0),
        )
        .expect("expected multi-selection center guide snap");

        assert_eq!(snap_rect.center().x, 0.0);
        assert_eq!(canvas.guide_snap_indicator_count(), 1);
        assert_eq!(canvas.guide_snap_mark_count(), 5);
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
        let placement = Placement::loaded(
            PlacementState::new(
                id,
                Some(id),
                eframe::egui::pos2(center_world.x, center_world.y),
                1.0,
                0.0,
                0,
                false,
            ),
            asset,
        );
        let rgba = vec![255; width as usize * height as usize * 4];
        canvas.add_prepared_placement_image(
            ctx,
            PreparedPlacementImage {
                target_board_id: 1,
                queue_id: None,
                placement,
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
            egui::vec2(0.0, 1.0),
        )];
        let navigation = map_mouse_mode_scroll_events(&events);
        assert!(
            navigation.wheel_zoom_factor > 1.0,
            "wheel up should zoom in, got {}",
            navigation.wheel_zoom_factor
        );
        assert!((navigation.wheel_zoom_factor - 1.3).abs() < 1e-5);
        assert_eq!(navigation.pinch_zoom_factor, 1.0);
        assert_eq!(navigation.pan_delta, egui::Vec2::ZERO);
    }

    #[test]
    fn mouse_mode_wheel_down_zooms_out() {
        let events = vec![wheel_event(
            egui::MouseWheelUnit::Line,
            egui::vec2(0.0, -1.0),
        )];
        let navigation = map_mouse_mode_scroll_events(&events);
        assert!(
            navigation.wheel_zoom_factor < 1.0,
            "wheel down should zoom out, got {}",
            navigation.wheel_zoom_factor
        );
        assert!((navigation.wheel_zoom_factor - (1.0 / 1.3)).abs() < 1e-5);
    }

    #[test]
    fn mouse_mode_does_not_pan_from_wheel_notches() {
        let events = vec![wheel_event(
            egui::MouseWheelUnit::Line,
            egui::vec2(0.0, 3.0),
        )];
        let navigation = map_mouse_mode_scroll_events(&events);
        assert_eq!(navigation.pan_delta, egui::Vec2::ZERO);
    }

    #[test]
    fn mouse_mode_passes_trackpad_pan_through() {
        let events = vec![wheel_event(
            egui::MouseWheelUnit::Point,
            egui::vec2(8.0, -4.0),
        )];
        let navigation = map_mouse_mode_scroll_events(&events);
        assert_eq!(navigation.wheel_zoom_factor, 1.0);
        assert_eq!(navigation.pinch_zoom_factor, 1.0);
        assert_eq!(navigation.pan_delta, egui::vec2(8.0, -4.0));
    }

    #[test]
    fn mouse_mode_passes_pinch_zoom_through() {
        let events = vec![egui::Event::Zoom(1.25)];
        let navigation = map_mouse_mode_scroll_events(&events);
        assert_eq!(navigation.wheel_zoom_factor, 1.0);
        assert!((navigation.pinch_zoom_factor - 1.25).abs() < 1e-6);
        assert_eq!(navigation.pan_delta, egui::Vec2::ZERO);
    }

    #[test]
    fn mouse_mode_ignores_horizontal_wheel() {
        let events = vec![wheel_event(
            egui::MouseWheelUnit::Line,
            egui::vec2(2.0, 0.0),
        )];
        let navigation = map_mouse_mode_scroll_events(&events);
        assert_eq!(navigation.wheel_zoom_factor, 1.0);
        assert_eq!(navigation.pinch_zoom_factor, 1.0);
        assert_eq!(navigation.pan_delta, egui::Vec2::ZERO);
    }

    #[test]
    fn mouse_wheel_zoom_animation_settles_at_target_after_80ms() {
        let mut state = CanvasState::default();
        let rect = test_screen_rect();
        let pointer = egui::pos2(640.0, 420.0);
        let world_before = state.screen_to_world(pointer, rect);

        animate_mouse_wheel_zoom_around_pointer(&mut state, rect, pointer, 1.3, 10.0);

        assert_eq!(state.zoom, 1.0);
        assert!(state.interaction.mouse_wheel_zoom_animation.is_some());

        assert!(step_mouse_wheel_zoom_animation(&mut state, 10.04));
        assert!(state.zoom > 1.0);
        assert!(state.zoom < 1.3);

        assert!(!step_mouse_wheel_zoom_animation(&mut state, 10.08));
        assert!((state.zoom - 1.3).abs() < 1e-6);
        assert_eq!(state.interaction.mouse_wheel_zoom_animation, None);

        let world_after = state.screen_to_world(pointer, rect);
        assert!((world_after.x - world_before.x).abs() < 1e-4);
        assert!((world_after.y - world_before.y).abs() < 1e-4);
    }
}
