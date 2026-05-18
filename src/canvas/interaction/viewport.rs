use std::time::Duration;

use eframe::egui;

use crate::canvas::{CanvasState, MAX_CANVAS_ZOOM, MIN_CANVAS_ZOOM};

const MOUSE_WHEEL_ZOOM_FACTOR_PER_LINE: f32 = 1.3;
const MOUSE_WHEEL_ZOOM_SETTLE_SECONDS: f64 = 0.080;

pub(crate) fn handle_viewport_navigation_input(
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
    let primary_down = ui.input(|i| i.pointer.primary_down());
    let alt_held = ui.input(|i| i.modifiers.alt);
    let pointer_in_canvas_rect = pointer_pos
        .map(|pointer_pos| {
            input_rect.contains(pointer_pos)
                && !overlay_rect.is_some_and(|overlay_rect| overlay_rect.contains(pointer_pos))
        })
        .unwrap_or(false);

    if pointer_in_canvas_rect && response.dragged_by(egui::PointerButton::Middle) {
        state.interaction.mouse_wheel_zoom_animation = None;
        state.pan += response.drag_delta();
        return;
    }

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
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        return;
    } else if pointer_in_canvas_rect && alt_held && !primary_down {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }

    if pointer_in_canvas_rect {
        state.interaction.last_pointer_screen_pos = pointer_pos;
        if let Some(pointer_pos) = pointer_pos {
            apply_canvas_navigation(ui, state, rect, pointer_pos);
        }
    }
}

pub(crate) fn apply_canvas_navigation(
    ui: &egui::Ui,
    state: &mut CanvasState,
    rect: egui::Rect,
    pointer_pos: egui::Pos2,
) {
    match state.pointer_mode() {
        crate::data::PointerMode::Trackpad => {
            let zoom_delta = ui.input(|i| i.zoom_delta());
            if (zoom_delta - 1.0).abs() > f32::EPSILON {
                state.interaction.mouse_wheel_zoom_animation = None;
                zoom_around_pointer(state, rect, pointer_pos, zoom_delta);
            }
            let pan_delta = ui.input(|i| i.translation_delta());
            if pan_delta != egui::Vec2::ZERO {
                state.interaction.mouse_wheel_zoom_animation = None;
                state.pan += pan_delta;
            }
        }
        crate::data::PointerMode::Mouse => {
            let navigation = ui.input(|i| map_mouse_mode_scroll_events(&i.events));
            if (navigation.pinch_zoom_factor - 1.0).abs() > f32::EPSILON {
                state.interaction.mouse_wheel_zoom_animation = None;
                zoom_around_pointer(state, rect, pointer_pos, navigation.pinch_zoom_factor);
            }
            if (navigation.wheel_zoom_factor - 1.0).abs() > f32::EPSILON {
                let now = ui.input(|i| i.time);
                animate_mouse_wheel_zoom_around_pointer(
                    state,
                    rect,
                    pointer_pos,
                    navigation.wheel_zoom_factor,
                    now,
                );
                ui.ctx().request_repaint_after(Duration::from_millis(8));
            }
            if navigation.pan_delta != egui::Vec2::ZERO {
                state.interaction.mouse_wheel_zoom_animation = None;
                state.pan += navigation.pan_delta;
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct MouseModeNavigation {
    pub wheel_zoom_factor: f32,
    pub pinch_zoom_factor: f32,
    pub pan_delta: egui::Vec2,
}

/// In Mouse mode, route mouse-wheel notches (Line/Page units) to zoom around
/// the pointer; leave true trackpad pan (Point units) and pinch (`Event::Zoom`)
/// alone so they still work for users with mixed input devices.
///
/// Scroll-wheel up (positive y from winit line-wheel events) -> zoom in. Each
/// line tick scales by 1.3; sub-line deltas scale smoothly via `1.3^y`.
pub(crate) fn map_mouse_mode_scroll_events(events: &[egui::Event]) -> MouseModeNavigation {
    let mut pinch_zoom_factor: f32 = 1.0;
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
                pinch_zoom_factor *= factor;
            }
            _ => {}
        }
    }
    let mut wheel_zoom_factor: f32 = 1.0;
    if wheel_zoom_y.abs() > f32::EPSILON {
        wheel_zoom_factor = MOUSE_WHEEL_ZOOM_FACTOR_PER_LINE.powf(wheel_zoom_y);
    }
    MouseModeNavigation {
        wheel_zoom_factor,
        pinch_zoom_factor,
        pan_delta,
    }
}
fn zoom_around_pointer(
    state: &mut CanvasState,
    rect: egui::Rect,
    pointer_pos: egui::Pos2,
    zoom_factor: f32,
) {
    state.interaction.mouse_wheel_zoom_animation = None;
    let world_before = state.screen_to_world(pointer_pos, rect);
    state.zoom = (state.zoom * zoom_factor).clamp(MIN_CANVAS_ZOOM, MAX_CANVAS_ZOOM);
    let screen_after = state.world_to_screen(world_before, rect);
    state.pan += pointer_pos - screen_after;
}

pub(crate) fn step_mouse_wheel_zoom_animation(state: &mut CanvasState, now_seconds: f64) -> bool {
    let Some(animation) = state.interaction.mouse_wheel_zoom_animation else {
        return false;
    };
    let progress = ((now_seconds - animation.started_at_seconds) / MOUSE_WHEEL_ZOOM_SETTLE_SECONDS)
        .clamp(0.0, 1.0) as f32;
    if progress >= 1.0 {
        state.pan = animation.target_pan;
        state.zoom = animation.target_zoom;
        state.interaction.mouse_wheel_zoom_animation = None;
        return false;
    }

    let eased = 1.0 - (1.0 - progress).powi(3);
    state.pan = animation.start_pan + (animation.target_pan - animation.start_pan) * eased;
    state.zoom = egui::lerp(animation.start_zoom..=animation.target_zoom, eased);
    true
}

pub(crate) fn animate_mouse_wheel_zoom_around_pointer(
    state: &mut CanvasState,
    rect: egui::Rect,
    pointer_pos: egui::Pos2,
    zoom_factor: f32,
    now_seconds: f64,
) {
    let (base_pan, base_zoom) = state
        .interaction
        .mouse_wheel_zoom_animation
        .map(|animation| (animation.target_pan, animation.target_zoom))
        .unwrap_or((state.pan, state.zoom));
    let world_before = screen_to_world_with_camera(pointer_pos, rect, base_pan, base_zoom);
    let target_zoom = (base_zoom * zoom_factor).clamp(MIN_CANVAS_ZOOM, MAX_CANVAS_ZOOM);
    let screen_after = world_to_screen_with_camera(world_before, rect, base_pan, target_zoom);
    let target_pan = base_pan + (pointer_pos - screen_after);
    state.interaction.mouse_wheel_zoom_animation =
        Some(crate::canvas::state::MouseWheelZoomAnimation {
            start_pan: state.pan,
            start_zoom: state.zoom,
            target_pan,
            target_zoom,
            started_at_seconds: now_seconds,
        });
}

fn screen_to_world_with_camera(
    screen: egui::Pos2,
    rect: egui::Rect,
    pan: egui::Vec2,
    zoom: f32,
) -> egui::Pos2 {
    let world = (screen - rect.center() - pan) / zoom;
    egui::pos2(world.x, world.y)
}

fn world_to_screen_with_camera(
    world: egui::Pos2,
    rect: egui::Rect,
    pan: egui::Vec2,
    zoom: f32,
) -> egui::Pos2 {
    rect.center() + pan + world.to_vec2() * zoom
}
