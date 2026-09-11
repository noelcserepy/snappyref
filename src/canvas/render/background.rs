use eframe::egui;

use crate::canvas::CanvasState;
use crate::canvas::{
    BACKGROUND_BASE_WORLD_STEP, BACKGROUND_MAX_LOD_LEVEL, BACKGROUND_MAX_SCREEN_STEP,
    BACKGROUND_MIN_LOD_LEVEL, BACKGROUND_MIN_SCREEN_STEP,
};
use crate::data::CanvasBackground;

const BACKGROUND_DOT_ALPHA: u8 = 76;
const BACKGROUND_GRID_ALPHA: u8 = 58;
const BACKGROUND_LOD_FADE_BAND_SCREEN: f32 = 24.0;

pub(crate) fn paint_background(painter: &egui::Painter, rect: egui::Rect, state: &mut CanvasState) {
    state.render.background_lod_level =
        settled_background_lod_level(state.render.background_lod_level, state.zoom);
    let visible_world_rect = visible_world_rect(state, rect);
    match state.background {
        CanvasBackground::None => {}
        CanvasBackground::Dots => {
            for layer in background_lod_layers(state.render.background_lod_level, state.zoom) {
                paint_background_dots(painter, rect, state, visible_world_rect, layer);
            }
        }
        CanvasBackground::Grid => {
            for layer in background_lod_layers(state.render.background_lod_level, state.zoom) {
                paint_background_grid(painter, rect, state, visible_world_rect, layer);
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct BackgroundLodLayer {
    pub(crate) level: i32,
    pub(crate) alpha: f32,
}

pub(crate) fn background_lod_layers(current_level: i32, zoom: f32) -> Vec<BackgroundLodLayer> {
    let level = current_level.clamp(BACKGROUND_MIN_LOD_LEVEL, BACKGROUND_MAX_LOD_LEVEL);
    let screen_step = background_screen_step(level, zoom);
    let mut primary_alpha = 1.0_f32;
    let mut layers = Vec::with_capacity(2);

    if screen_step < BACKGROUND_MIN_SCREEN_STEP + BACKGROUND_LOD_FADE_BAND_SCREEN
        && level < BACKGROUND_MAX_LOD_LEVEL
    {
        let adjacent_alpha = ((BACKGROUND_MIN_SCREEN_STEP + BACKGROUND_LOD_FADE_BAND_SCREEN
            - screen_step)
            / BACKGROUND_LOD_FADE_BAND_SCREEN)
            .clamp(0.0, 1.0);
        primary_alpha = 1.0 - adjacent_alpha;
        layers.push(BackgroundLodLayer {
            level: level + 1,
            alpha: adjacent_alpha,
        });
    } else if screen_step > BACKGROUND_MAX_SCREEN_STEP - BACKGROUND_LOD_FADE_BAND_SCREEN
        && level > BACKGROUND_MIN_LOD_LEVEL
    {
        let adjacent_alpha = ((screen_step
            - (BACKGROUND_MAX_SCREEN_STEP - BACKGROUND_LOD_FADE_BAND_SCREEN))
            / BACKGROUND_LOD_FADE_BAND_SCREEN)
            .clamp(0.0, 1.0);
        primary_alpha = 1.0 - adjacent_alpha;
        layers.push(BackgroundLodLayer {
            level: level - 1,
            alpha: adjacent_alpha,
        });
    }

    layers.push(BackgroundLodLayer {
        level,
        alpha: primary_alpha,
    });
    layers
}

fn paint_background_dots(
    painter: &egui::Painter,
    rect: egui::Rect,
    state: &CanvasState,
    visible_world_rect: egui::Rect,
    layer: BackgroundLodLayer,
) {
    let Some(alpha) = scaled_alpha(BACKGROUND_DOT_ALPHA, layer.alpha) else {
        return;
    };
    let base = crate::ui::theme::TEXT_MUTED;
    let color = egui::Color32::from_rgba_unmultiplied(base.r(), base.g(), base.b(), alpha);
    let world_step = background_world_step(layer.level);
    let mut world_x = first_lattice_at_or_before(visible_world_rect.left(), world_step);
    while world_x <= visible_world_rect.right() + world_step {
        let mut world_y = first_lattice_at_or_before(visible_world_rect.top(), world_step);
        while world_y <= visible_world_rect.bottom() + world_step {
            let screen_pos = state.world_to_screen(egui::pos2(world_x, world_y), rect);
            painter.circle_filled(snap_point(screen_pos), 0.9, color);
            world_y += world_step;
        }
        world_x += world_step;
    }
}

fn paint_background_grid(
    painter: &egui::Painter,
    rect: egui::Rect,
    state: &CanvasState,
    visible_world_rect: egui::Rect,
    layer: BackgroundLodLayer,
) {
    let Some(alpha) = scaled_alpha(BACKGROUND_GRID_ALPHA, layer.alpha) else {
        return;
    };
    let base = crate::ui::theme::TEXT_SUBTLE;
    let color = egui::Color32::from_rgba_unmultiplied(base.r(), base.g(), base.b(), alpha);
    let stroke = egui::Stroke::new(0.45_f32, color);
    let world_step = background_world_step(layer.level);
    let mut world_x = first_lattice_at_or_before(visible_world_rect.left(), world_step);
    while world_x <= visible_world_rect.right() + world_step {
        let screen_x =
            snap_line_coordinate(state.world_to_screen(egui::pos2(world_x, 0.0), rect).x);
        painter.line_segment(
            [
                egui::pos2(screen_x, rect.top()),
                egui::pos2(screen_x, rect.bottom()),
            ],
            stroke,
        );
        world_x += world_step;
    }
    let mut world_y = first_lattice_at_or_before(visible_world_rect.top(), world_step);
    while world_y <= visible_world_rect.bottom() + world_step {
        let screen_y =
            snap_line_coordinate(state.world_to_screen(egui::pos2(0.0, world_y), rect).y);
        painter.line_segment(
            [
                egui::pos2(rect.left(), screen_y),
                egui::pos2(rect.right(), screen_y),
            ],
            stroke,
        );
        world_y += world_step;
    }
}

fn scaled_alpha(base: u8, scale: f32) -> Option<u8> {
    let alpha = (f32::from(base) * scale).round().clamp(0.0, 255.0) as u8;
    (alpha > 0).then_some(alpha)
}

pub(crate) fn settled_background_lod_level(current_level: i32, zoom: f32) -> i32 {
    let mut level = current_level.clamp(BACKGROUND_MIN_LOD_LEVEL, BACKGROUND_MAX_LOD_LEVEL);

    loop {
        let screen_step = background_screen_step(level, zoom);
        if screen_step < BACKGROUND_MIN_SCREEN_STEP && level < BACKGROUND_MAX_LOD_LEVEL {
            level += 1;
            continue;
        }
        if screen_step > BACKGROUND_MAX_SCREEN_STEP && level > BACKGROUND_MIN_LOD_LEVEL {
            level -= 1;
            continue;
        }
        return level;
    }
}

pub(crate) fn background_world_step(level: i32) -> f32 {
    BACKGROUND_BASE_WORLD_STEP * 2.0_f32.powi(level)
}

pub(crate) fn background_screen_step(level: i32, zoom: f32) -> f32 {
    background_world_step(level) * zoom
}

fn visible_world_rect(state: &CanvasState, rect: egui::Rect) -> egui::Rect {
    let corners = [
        state.screen_to_world(rect.left_top(), rect),
        state.screen_to_world(rect.right_top(), rect),
        state.screen_to_world(rect.left_bottom(), rect),
        state.screen_to_world(rect.right_bottom(), rect),
    ];
    let min_x = corners
        .iter()
        .map(|corner| corner.x)
        .fold(f32::INFINITY, f32::min);
    let max_x = corners
        .iter()
        .map(|corner| corner.x)
        .fold(f32::NEG_INFINITY, f32::max);
    let min_y = corners
        .iter()
        .map(|corner| corner.y)
        .fold(f32::INFINITY, f32::min);
    let max_y = corners
        .iter()
        .map(|corner| corner.y)
        .fold(f32::NEG_INFINITY, f32::max);
    egui::Rect::from_min_max(egui::pos2(min_x, min_y), egui::pos2(max_x, max_y))
}

fn first_lattice_at_or_before(value: f32, step: f32) -> f32 {
    (value / step).floor() * step
}

fn snap_point(point: egui::Pos2) -> egui::Pos2 {
    egui::pos2(point.x.round(), point.y.round())
}

fn snap_line_coordinate(value: f32) -> f32 {
    value.round() + 0.5
}
