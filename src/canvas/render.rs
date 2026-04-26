use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::Arc;
use std::time::Instant;

use eframe::egui;

use super::geometry::{
    clamped_crop_screen_rect, image_screen_corners_for_record, image_screen_rect_for_record,
};
use super::interaction::{
    annotation_screen_points, annotation_screen_rect, hit_test_textbox_resize_handle,
    image_screen_rect, normalized_textbox_rect, pending_annotation_from_draft, textbox_screen_rect,
};
use super::{
    ANNOTATION_CULL_MARGIN_SCREEN, BACKGROUND_BASE_WORLD_STEP, BACKGROUND_MAX_LOD_LEVEL,
    BACKGROUND_MAX_SCREEN_STEP, BACKGROUND_MIN_LOD_LEVEL, BACKGROUND_MIN_SCREEN_STEP, CanvasImage,
    CanvasImageCacheTier, CanvasState, CanvasTexture, IMAGE_CULL_MARGIN_SCREEN,
    IMAGE_DETAIL_PROMOTION_THRESHOLD, ImagePassBatch, ImagePassCallback, ImagePassInstance,
    ImagePassMetrics, MIN_CANVAS_ZOOM, MIN_TEXTBOX_WORLD_SIZE, PendingAnnotation,
    TEXTBOX_EDITOR_TOOLBAR_BUTTON_GAP, TEXTBOX_EDITOR_TOOLBAR_BUTTON_WIDTH,
    TEXTBOX_EDITOR_TOOLBAR_CLEARANCE, TEXTBOX_EDITOR_TOOLBAR_VIEWPORT_MARGIN,
    TEXTBOX_EMPTY_HOTSPOT_MAX_WIDTH, TEXTBOX_EMPTY_HOTSPOT_MIN_WIDTH, TextboxLayoutCacheKey,
    TextboxPointerRegion,
};
use crate::data::{
    CanvasAnnotation, CanvasAnnotationColor, CanvasAnnotationKind, CanvasBackground,
    CanvasStackItem, CanvasStrokeWidth, CanvasTextSize,
};

#[cfg(test)]
use super::{AnnotationPaintStats, CanvasDocumentState};

const BACKGROUND_DOT_ALPHA: u8 = 76;
const BACKGROUND_GRID_ALPHA: u8 = 58;
const BACKGROUND_LOD_FADE_BAND_SCREEN: f32 = 24.0;

pub(super) fn paint_images(
    painter: &egui::Painter,
    ui: &egui::Ui,
    rect: egui::Rect,
    state: &mut CanvasState,
    render_state: Option<&eframe::egui_wgpu::RenderState>,
    allow_tier_promotion: bool,
    debug_labels: bool,
) -> ImagePassMetrics {
    let build_started_at = Instant::now();
    let cull_rect = rect.expand(IMAGE_CULL_MARGIN_SCREEN);
    let mut metrics = ImagePassMetrics::default();
    let visible_images = state
        .document
        .images
        .iter()
        .filter_map(|image| {
            let image_rect = image_screen_rect(state, rect, image);
            image_rect
                .intersects(cull_rect)
                .then_some((image.record.item.id, image_rect))
        })
        .collect::<Vec<_>>();
    metrics.visible_images = visible_images.len();

    let animation_time = state
        .render
        .animation_time_override_seconds
        .unwrap_or_else(|| painter.ctx().input(|input| input.time as f32));
    let mut native_images = Vec::new();
    let mut svg_images = Vec::new();
    let mut fallback_images = Vec::new();
    let mut any_visible_animated = false;

    for (item_id, image_rect) in &visible_images {
        if let Some((rgba, width, height)) = state.advance_animated_image(*item_id, animation_time)
            && let Some(image) = state.image_by_item_id_mut(*item_id)
            && let CanvasTexture::Managed(texture) = &mut image.texture
        {
            texture.set(
                egui::ColorImage::from_rgba_unmultiplied([width as usize, height as usize], &rgba),
                egui::TextureOptions::LINEAR,
            );
        }

        let Some(image) = state.image_by_item_id(*item_id) else {
            continue;
        };
        if image.animation.is_some() {
            any_visible_animated = true;
        }
        if matches!(image.texture, CanvasTexture::Svg { .. }) {
            svg_images.push((*item_id, *image_rect));
            continue;
        }

        let native_eligible = matches!(image.texture, CanvasTexture::Native(_))
            && image.animation.is_none()
            && image.record.item.rotation.abs() <= 0.0001;
        if native_eligible {
            native_images.push((*item_id, *image_rect));
        } else {
            fallback_images.push(*item_id);
        }
    }

    if !native_images.is_empty() && render_state.is_some() {
        let mut native_instances = Vec::with_capacity(native_images.len());
        let mut native_batches = Vec::new();
        let mut current_batch: Option<(
            egui::TextureId,
            Arc<crate::jobs::NativeTextureHandle>,
            u32,
        )> = None;
        for (item_id, image_rect) in &native_images {
            let Some(image) = state.image_by_item_id(*item_id) else {
                continue;
            };
            let CanvasTexture::Native(texture) = &image.texture else {
                continue;
            };
            let instance_index = native_instances.len() as u32;
            native_instances.push(image_pass_instance(rect, *image_rect));
            match &current_batch {
                Some((texture_id, _, _)) if *texture_id == texture.texture_id() => {}
                Some((_, previous_texture, start)) => {
                    native_batches.push(ImagePassBatch {
                        texture: previous_texture.clone(),
                        instance_range: *start..instance_index,
                    });
                    current_batch = Some((texture.texture_id(), texture.clone(), instance_index));
                }
                None => {
                    current_batch = Some((texture.texture_id(), texture.clone(), instance_index));
                }
            }
        }
        if let Some((_, texture, start)) = current_batch {
            native_batches.push(ImagePassBatch {
                texture,
                instance_range: start..native_instances.len() as u32,
            });
        }
        metrics.texture_groups = native_batches.len();
        metrics.draw_calls = native_batches.len();
        if let Some(render_state) = render_state {
            painter.add(eframe::egui_wgpu::Callback::new_paint_callback(
                rect,
                ImagePassCallback::new(
                    render_state.target_format,
                    native_instances,
                    native_batches,
                ),
            ));
        }
    }

    metrics.fallback_images = fallback_images.len();
    for (item_id, image_rect) in svg_images {
        let Some(image) = state.image_by_item_id(item_id) else {
            continue;
        };
        paint_svg_image(ui, image, image_rect);
    }
    for item_id in fallback_images {
        let Some(image) = state.image_by_item_id(item_id) else {
            continue;
        };
        paint_image_mesh(painter, state, rect, image);
    }

    metrics.build_time_ms = build_started_at.elapsed().as_secs_f64() * 1000.0;

    if allow_tier_promotion {
        let promotion_requests = visible_images
            .iter()
            .filter_map(|(item_id, _)| {
                desired_image_tier(state, rect, *item_id).map(|tier| (*item_id, tier))
            })
            .collect::<Vec<_>>();
        for (item_id, tier) in promotion_requests {
            state.request_image_tier_load(item_id, tier);
        }
    }

    for image in &state.document.images {
        let image_rect = image_screen_rect(state, rect, image);
        if !image_rect.intersects(cull_rect) {
            continue;
        }
        paint_image_overlay(painter, state, image, image_rect, debug_labels);
    }

    if let Some(preview) = &state.interaction.search_drag_preview {
        for image in &preview.images {
            let image_rect = image_screen_rect(state, rect, image);
            if !image_rect.intersects(cull_rect) {
                continue;
            }
            paint_image_mesh(painter, state, rect, image);
            painter.rect_stroke(
                image_rect.expand(2.0),
                0.0,
                crate::theme::stroke_strong(),
                egui::StrokeKind::Outside,
            );
        }
    }

    if any_visible_animated {
        painter.ctx().request_repaint();
    }

    metrics
}

pub(super) fn paint_canvas_items(
    painter: &egui::Painter,
    ui: &egui::Ui,
    rect: egui::Rect,
    state: &mut CanvasState,
    render_state: Option<&eframe::egui_wgpu::RenderState>,
    allow_tier_promotion: bool,
    debug_labels: bool,
) -> ImagePassMetrics {
    if canvas_stack_matches_existing_layering(state) {
        let image_pass = paint_images(
            painter,
            ui,
            rect,
            state,
            render_state,
            allow_tier_promotion,
            debug_labels,
        );
        paint_annotations(painter, ui, rect, state);
        return image_pass;
    }

    paint_canvas_items_interleaved(painter, ui, rect, state, allow_tier_promotion, debug_labels)
}

#[cfg(test)]
pub(super) fn unified_canvas_paint_order_for_state(state: &CanvasState) -> Vec<CanvasStackItem> {
    state.ordered_canvas_items_back_to_front()
}

fn canvas_stack_matches_existing_layering(state: &CanvasState) -> bool {
    let ordered = state.ordered_canvas_items_back_to_front();
    let existing = state
        .document
        .images
        .iter()
        .map(|image| CanvasStackItem::Placement(image.record.item.id))
        .chain(
            state
                .document
                .annotations
                .iter()
                .map(|annotation| CanvasStackItem::Annotation(annotation.id)),
        )
        .collect::<Vec<_>>();
    ordered == existing
}

fn paint_canvas_items_interleaved(
    painter: &egui::Painter,
    ui: &egui::Ui,
    rect: egui::Rect,
    state: &mut CanvasState,
    allow_tier_promotion: bool,
    debug_labels: bool,
) -> ImagePassMetrics {
    let build_started_at = Instant::now();
    let image_cull_rect = rect.expand(IMAGE_CULL_MARGIN_SCREEN);
    let annotation_cull_rect = rect.expand(ANNOTATION_CULL_MARGIN_SCREEN);
    let mut metrics = ImagePassMetrics::default();

    let animation_time = state
        .render
        .animation_time_override_seconds
        .unwrap_or_else(|| painter.ctx().input(|input| input.time as f32));
    let mut any_visible_animated = false;
    let mut visible_item_ids = Vec::new();

    for item in state.ordered_canvas_items_back_to_front() {
        match item {
            CanvasStackItem::Placement(item_id) => {
                let Some(image) = state.image_by_item_id(item_id) else {
                    continue;
                };
                let image_rect = image_screen_rect(state, rect, image);
                if !image_rect.intersects(image_cull_rect) {
                    continue;
                }
                metrics.visible_images += 1;
                metrics.fallback_images += 1;
                visible_item_ids.push(item_id);

                if let Some((rgba, width, height)) =
                    state.advance_animated_image(item_id, animation_time)
                    && let Some(image) = state.image_by_item_id_mut(item_id)
                    && let CanvasTexture::Managed(texture) = &mut image.texture
                {
                    texture.set(
                        egui::ColorImage::from_rgba_unmultiplied(
                            [width as usize, height as usize],
                            &rgba,
                        ),
                        egui::TextureOptions::LINEAR,
                    );
                }

                let Some(image) = state.image_by_item_id(item_id) else {
                    continue;
                };
                if image.animation.is_some() {
                    any_visible_animated = true;
                }
                if matches!(image.texture, CanvasTexture::Svg { .. }) {
                    paint_svg_image(ui, image, image_rect);
                } else {
                    paint_image_mesh(painter, state, rect, image);
                }
                paint_image_overlay(painter, state, image, image_rect, debug_labels);
            }
            CanvasStackItem::Annotation(annotation_id) => {
                let Some(annotation) = state.annotation_by_id(annotation_id).cloned() else {
                    continue;
                };
                if annotation_should_paint(state, rect, annotation_cull_rect, &annotation) {
                    paint_annotation(painter, ui, rect, state, &annotation);
                }
            }
        }
    }

    if allow_tier_promotion {
        let promotion_requests = visible_item_ids
            .iter()
            .filter_map(|item_id| {
                desired_image_tier(state, rect, *item_id).map(|tier| (*item_id, tier))
            })
            .collect::<Vec<_>>();
        for (item_id, tier) in promotion_requests {
            state.request_image_tier_load(item_id, tier);
        }
    }

    if let Some(preview) = &state.interaction.search_drag_preview {
        for image in &preview.images {
            let image_rect = image_screen_rect(state, rect, image);
            if !image_rect.intersects(image_cull_rect) {
                continue;
            }
            paint_image_mesh(painter, state, rect, image);
            painter.rect_stroke(
                image_rect.expand(2.0),
                0.0,
                crate::theme::stroke_strong(),
                egui::StrokeKind::Outside,
            );
        }
    }

    if any_visible_animated {
        painter.ctx().request_repaint();
    }

    metrics.build_time_ms = build_started_at.elapsed().as_secs_f64() * 1000.0;
    metrics
}

fn paint_svg_image(ui: &egui::Ui, image: &CanvasImage, image_rect: egui::Rect) {
    let CanvasTexture::Svg { uri, bytes, .. } = &image.texture else {
        return;
    };
    egui::Image::from_bytes(uri.clone(), bytes.clone()).paint_at(ui, image_rect);
}

fn image_pass_instance(rect: egui::Rect, image_rect: egui::Rect) -> ImagePassInstance {
    let local_min = image_rect.min - rect.min;
    let local_max = image_rect.max - rect.min;
    let width = rect.width().max(1.0);
    let height = rect.height().max(1.0);
    let min_ndc = [
        (local_min.x / width) * 2.0 - 1.0,
        1.0 - (local_max.y / height) * 2.0,
    ];
    let max_ndc = [
        (local_max.x / width) * 2.0 - 1.0,
        1.0 - (local_min.y / height) * 2.0,
    ];
    ImagePassInstance {
        min_ndc,
        max_ndc,
        min_uv: [0.0, 1.0],
        max_uv: [1.0, 0.0],
    }
}

fn desired_image_tier(
    state: &CanvasState,
    rect: egui::Rect,
    item_id: i64,
) -> Option<CanvasImageCacheTier> {
    let image = state.image_by_item_id(item_id)?;
    if image.loaded_tier >= CanvasImageCacheTier::Detail {
        return None;
    }
    let image_rect = image_screen_rect(state, rect, image);
    let on_screen_max_side = image_rect.width().max(image_rect.height());
    let texture_max_side = image.texture.size()[0].max(image.texture.size()[1]) as f32;
    (texture_max_side > 0.0
        && on_screen_max_side >= texture_max_side * IMAGE_DETAIL_PROMOTION_THRESHOLD)
        .then_some(CanvasImageCacheTier::Detail)
}

fn paint_image_overlay(
    painter: &egui::Painter,
    state: &CanvasState,
    image: &CanvasImage,
    image_rect: egui::Rect,
    debug_labels: bool,
) {
    if state
        .interaction
        .search_match_item_ids
        .contains(&image.record.item.id)
    {
        let stroke = if state.interaction.active_search_item_id == Some(image.record.item.id) {
            egui::Stroke::new(3.0, egui::Color32::from_rgb(255, 170, 72))
        } else {
            egui::Stroke::new(2.0, egui::Color32::from_rgb(255, 215, 140))
        };
        painter.rect_stroke(
            image_rect.expand(2.0),
            0.0,
            stroke,
            egui::StrokeKind::Outside,
        );
    }
    if state.is_selected(image.record.item.id) {
        let corners = image_screen_corners_for_record(
            &image.record,
            state.pan,
            state.zoom,
            painter.clip_rect().center(),
        );
        painter.add(egui::Shape::closed_line(
            corners.to_vec(),
            crate::theme::stroke_strong(),
        ));
    }

    if debug_labels {
        let label = format!(
            "#{}  ({:.0}, {:.0})",
            image.record.item.id, image.record.item.x, image.record.item.y
        );
        painter.text(
            image_rect.min + egui::vec2(crate::theme::SPACE_2, crate::theme::SPACE_2),
            egui::Align2::LEFT_TOP,
            label,
            egui::TextStyle::Body.resolve(&painter.ctx().global_style()),
            crate::theme::TEXT,
        );
    }
}

fn paint_image_mesh(
    painter: &egui::Painter,
    state: &CanvasState,
    rect: egui::Rect,
    image: &CanvasImage,
) {
    let corners =
        image_screen_corners_for_record(&image.record, state.pan, state.zoom, rect.center());
    let mut mesh = egui::Mesh::with_texture(image.texture.id());
    let left_top = mesh.vertices.len() as u32;
    mesh.vertices.push(egui::epaint::Vertex {
        pos: corners[0],
        uv: egui::pos2(0.0, 0.0),
        color: egui::Color32::WHITE,
    });
    mesh.vertices.push(egui::epaint::Vertex {
        pos: corners[1],
        uv: egui::pos2(1.0, 0.0),
        color: egui::Color32::WHITE,
    });
    mesh.vertices.push(egui::epaint::Vertex {
        pos: corners[2],
        uv: egui::pos2(1.0, 1.0),
        color: egui::Color32::WHITE,
    });
    mesh.vertices.push(egui::epaint::Vertex {
        pos: corners[3],
        uv: egui::pos2(0.0, 1.0),
        color: egui::Color32::WHITE,
    });
    mesh.add_triangle(left_top, left_top + 1, left_top + 2);
    mesh.add_triangle(left_top, left_top + 2, left_top + 3);
    painter.add(egui::Shape::mesh(mesh));
}

pub(super) fn canvas_texture_from_preview(
    ctx: &egui::Context,
    texture_name: String,
    preview_rgba: &[u8],
    preview_width: u32,
    preview_height: u32,
    preview_texture: crate::jobs::ImportedPreviewTexture,
) -> CanvasTexture {
    match preview_texture {
        crate::jobs::ImportedPreviewTexture::Cpu => CanvasTexture::Managed(ctx.load_texture(
            texture_name,
            egui::ColorImage::from_rgba_unmultiplied(
                [preview_width as usize, preview_height as usize],
                preview_rgba,
            ),
            egui::TextureOptions::LINEAR,
        )),
        crate::jobs::ImportedPreviewTexture::Native(texture) => CanvasTexture::Native(texture),
        crate::jobs::ImportedPreviewTexture::Svg { uri, bytes, size } => {
            CanvasTexture::Svg { uri, bytes, size }
        }
    }
}

pub(super) fn paint_background(painter: &egui::Painter, rect: egui::Rect, state: &mut CanvasState) {
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
pub(super) struct BackgroundLodLayer {
    pub(super) level: i32,
    pub(super) alpha: f32,
}

pub(super) fn background_lod_layers(current_level: i32, zoom: f32) -> Vec<BackgroundLodLayer> {
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
    let base = crate::theme::TEXT_MUTED;
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
    let base = crate::theme::TEXT_SUBTLE;
    let color = egui::Color32::from_rgba_unmultiplied(base.r(), base.g(), base.b(), alpha);
    let stroke = egui::Stroke::new(0.45, color);
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

pub(super) fn settled_background_lod_level(current_level: i32, zoom: f32) -> i32 {
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

pub(super) fn background_world_step(level: i32) -> f32 {
    BACKGROUND_BASE_WORLD_STEP * 2.0_f32.powi(level)
}

pub(super) fn background_screen_step(level: i32, zoom: f32) -> f32 {
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

pub(super) fn textbox_padding() -> egui::Vec2 {
    egui::vec2(crate::theme::SPACE_2, crate::theme::SPACE_2)
}

pub(super) fn textbox_font_size(text_size: CanvasTextSize) -> f32 {
    let baseline = crate::jobs::default_import_reference_span();
    match text_size {
        CanvasTextSize::S => baseline * (16.0 / 300.0),
        CanvasTextSize::M => baseline * (20.0 / 300.0),
        CanvasTextSize::L => baseline * (40.0 / 300.0),
        CanvasTextSize::Xl => baseline * (64.0 / 300.0),
    }
}

pub(super) fn textbox_font_id(text_size: CanvasTextSize, zoom: f32) -> egui::FontId {
    egui::FontId::proportional(textbox_font_size(text_size) * zoom.max(MIN_CANVAS_ZOOM))
}

pub(super) fn textbox_content_rect(textbox_rect: egui::Rect) -> egui::Rect {
    let padding = textbox_padding();
    let min = egui::pos2(
        (textbox_rect.min.x + padding.x).min(textbox_rect.max.x),
        (textbox_rect.min.y + padding.y).min(textbox_rect.max.y),
    );
    let available_size = (textbox_rect.max - min).max(egui::Vec2::ZERO);
    let desired_size = (textbox_rect.size() - padding * 2.0).max(egui::Vec2::ZERO);
    egui::Rect::from_min_size(
        min,
        egui::vec2(
            desired_size.x.min(available_size.x),
            desired_size.y.min(available_size.y),
        ),
    )
}

fn textbox_row_height(ui: &egui::Ui, text_size: CanvasTextSize, zoom: f32) -> f32 {
    let font_id = textbox_font_id(text_size, zoom);
    ui.fonts_mut(|fonts| fonts.row_height(&font_id))
}

pub(super) fn textbox_empty_hotspot_rect(
    ui: &egui::Ui,
    content_rect: egui::Rect,
    text_size: CanvasTextSize,
    zoom: f32,
) -> egui::Rect {
    let width = content_rect.width().clamp(
        TEXTBOX_EMPTY_HOTSPOT_MIN_WIDTH,
        TEXTBOX_EMPTY_HOTSPOT_MAX_WIDTH,
    );
    let size = egui::vec2(
        width,
        textbox_row_height(ui, text_size, zoom).min(content_rect.height()),
    );
    egui::Rect::from_min_size(content_rect.min, size.min(content_rect.size()))
}

pub(super) fn textbox_text_interactive_rect_from_galley(
    ui: &egui::Ui,
    content_rect: egui::Rect,
    galley: &egui::Galley,
    text_size: CanvasTextSize,
    zoom: f32,
    is_empty: bool,
) -> egui::Rect {
    if is_empty {
        return textbox_empty_hotspot_rect(ui, content_rect, text_size, zoom);
    }

    egui::Rect::from_min_size(content_rect.min, galley.size()).intersect(content_rect)
}

pub(super) fn textbox_pointer_region(
    ui: &egui::Ui,
    rect: egui::Rect,
    state: &mut CanvasState,
    annotation: &CanvasAnnotation,
    pointer_pos: egui::Pos2,
) -> Option<TextboxPointerRegion> {
    debug_assert_eq!(annotation.kind, CanvasAnnotationKind::Textbox);

    let editing = state.interaction.editing_text_annotation_id == Some(annotation.id);
    let textbox_rect = textbox_screen_rect(state, rect, annotation);
    if !textbox_rect.contains(pointer_pos) {
        return hit_test_textbox_resize_handle(state, rect, annotation, pointer_pos)
            .map(TextboxPointerRegion::Resize);
    }

    if editing {
        let content_rect = textbox_content_rect(textbox_rect);
        let layout = state.cached_textbox_layout(
            ui,
            annotation.id,
            &annotation.text,
            annotation.text_size,
            annotation.color,
            content_rect.width(),
            state.zoom,
        );
        let text_rect = textbox_text_interactive_rect_from_galley(
            ui,
            content_rect,
            &layout.galley,
            annotation.text_size,
            state.zoom,
            annotation.text.is_empty(),
        );
        if text_rect.contains(pointer_pos) {
            return Some(TextboxPointerRegion::Text);
        }
    }

    if let Some(handle) = hit_test_textbox_resize_handle(state, rect, annotation, pointer_pos) {
        return Some(TextboxPointerRegion::Resize(handle));
    }

    Some(TextboxPointerRegion::Body)
}

pub(super) fn textbox_layout_cache_key(
    text: &str,
    text_size: CanvasTextSize,
    color: CanvasAnnotationColor,
    wrap_width: f32,
    zoom: f32,
) -> TextboxLayoutCacheKey {
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    TextboxLayoutCacheKey {
        text_hash: hasher.finish(),
        text_size,
        color,
        wrap_width_bits: wrap_width.max(24.0).to_bits(),
        zoom_bits: zoom.to_bits(),
    }
}

pub(super) fn layout_textbox_galley(
    ui: &egui::Ui,
    text: &str,
    text_size: CanvasTextSize,
    color: CanvasAnnotationColor,
    wrap_width: f32,
    zoom: f32,
) -> Arc<egui::Galley> {
    let job = egui::text::LayoutJob::simple(
        if text.is_empty() {
            " ".to_owned()
        } else {
            text.to_owned()
        },
        textbox_font_id(text_size, zoom),
        annotation_color(color),
        wrap_width.max(24.0),
    );
    ui.fonts_mut(|fonts| fonts.layout_job(job))
}

pub(super) fn textbox_required_world_height(galley_height_screen: f32, zoom: f32) -> f32 {
    let padding = textbox_padding();
    let screen_height = galley_height_screen + padding.y * 2.0;
    (screen_height / zoom.max(MIN_CANVAS_ZOOM)).max(MIN_TEXTBOX_WORLD_SIZE.y)
}

fn annotation_color(color: CanvasAnnotationColor) -> egui::Color32 {
    match color {
        CanvasAnnotationColor::Default => crate::theme::TEXT,
        CanvasAnnotationColor::Coral => egui::Color32::from_rgb(0xE8, 0x89, 0x73),
        CanvasAnnotationColor::Gold => egui::Color32::from_rgb(0xE0, 0xB4, 0x56),
        CanvasAnnotationColor::Mint => egui::Color32::from_rgb(0x7A, 0xC6, 0xA5),
        CanvasAnnotationColor::Sky => egui::Color32::from_rgb(0x7E, 0xB8, 0xE8),
    }
}

fn stroke_width_value(width: CanvasStrokeWidth) -> f32 {
    match width {
        CanvasStrokeWidth::S => 2.0,
        CanvasStrokeWidth::M => 4.0,
        CanvasStrokeWidth::L => 6.0,
    }
}

#[derive(Clone, Copy)]
struct TextboxEditorToolbarMetrics {
    size: egui::Vec2,
}

const ANNOTATION_COLORS: [CanvasAnnotationColor; 5] = [
    CanvasAnnotationColor::Default,
    CanvasAnnotationColor::Coral,
    CanvasAnnotationColor::Gold,
    CanvasAnnotationColor::Mint,
    CanvasAnnotationColor::Sky,
];
const TOOLBAR_SWATCH_BUTTON_WIDTH: f32 = 28.0;
const TOOLBAR_BACKGROUND_BUTTON_WIDTH: f32 = 36.0;

fn expand_textbox_to_height(state: &mut CanvasState, index: usize, required_height: f32) -> bool {
    let annotation = &state.document.annotations[index];
    if annotation.kind != CanvasAnnotationKind::Textbox {
        return false;
    }

    let world_rect = normalized_textbox_rect(
        egui::pos2(annotation.start_x, annotation.start_y),
        egui::pos2(annotation.end_x, annotation.end_y),
    );
    if required_height <= world_rect.height() + 0.5 {
        return false;
    }

    let annotation_id = annotation.id;
    let new_rect = egui::Rect::from_min_size(
        world_rect.min,
        egui::vec2(world_rect.width(), required_height),
    );
    let annotation = &mut state.document.annotations[index];
    annotation.start_x = new_rect.min.x;
    annotation.start_y = new_rect.min.y;
    annotation.end_x = new_rect.max.x;
    annotation.end_y = new_rect.max.y;
    state.mark_annotation_dirty(annotation_id);
    true
}

pub(super) fn paint_annotations(
    painter: &egui::Painter,
    ui: &egui::Ui,
    rect: egui::Rect,
    state: &mut CanvasState,
) {
    let cull_rect = rect.expand(ANNOTATION_CULL_MARGIN_SCREEN);
    for index in 0..state.document.annotations.len() {
        let annotation = state.document.annotations[index].clone();
        if !annotation_should_paint(state, rect, cull_rect, &annotation) {
            continue;
        }
        paint_annotation(painter, ui, rect, state, &annotation);
    }
}

fn paint_annotation(
    painter: &egui::Painter,
    ui: &egui::Ui,
    rect: egui::Rect,
    state: &mut CanvasState,
    annotation: &CanvasAnnotation,
) {
    let selected = state.is_annotation_selected(annotation.id);
    let editing = state.interaction.editing_text_annotation_id == Some(annotation.id);
    let color = annotation_color(annotation.color);
    let stroke = if selected || editing {
        egui::Stroke::new(stroke_width_value(annotation.stroke_width), color)
    } else {
        egui::Stroke::new(
            stroke_width_value(annotation.stroke_width),
            egui::Color32::from_rgba_premultiplied(color.r(), color.g(), color.b(), 220),
        )
    };

    match annotation.kind {
        CanvasAnnotationKind::Line => {
            let (start, end) = annotation_screen_points(state, rect, annotation);
            painter.line_segment([start, end], stroke);
        }
        CanvasAnnotationKind::Arrow => {
            let (start, end) = annotation_screen_points(state, rect, annotation);
            paint_arrow_segment(painter, start, end, stroke);
        }
        CanvasAnnotationKind::Textbox => {
            let textbox_rect = annotation_screen_rect(state, rect, annotation);
            let content_rect = textbox_content_rect(textbox_rect);
            if annotation.background_enabled {
                let fill_alpha = if selected || editing { 184 } else { 112 };
                painter.rect_filled(
                    textbox_rect,
                    crate::theme::CONTROL_RADIUS,
                    egui::Color32::from_rgba_premultiplied(
                        crate::theme::BACKGROUND.r(),
                        crate::theme::BACKGROUND.g(),
                        crate::theme::BACKGROUND.b(),
                        fill_alpha,
                    ),
                );
            }
            if selected || editing {
                painter.rect_stroke(
                    textbox_rect,
                    crate::theme::CONTROL_RADIUS,
                    stroke,
                    egui::StrokeKind::Outside,
                );
            }
            if !editing && !annotation.text.trim().is_empty() {
                paint_textbox_text(
                    painter,
                    &state
                        .cached_textbox_layout(
                            ui,
                            annotation.id,
                            &annotation.text,
                            annotation.text_size,
                            annotation.color,
                            content_rect.width(),
                            state.zoom,
                        )
                        .galley,
                    content_rect,
                    color,
                );
            }
        }
    }
}

fn annotation_should_paint(
    state: &CanvasState,
    rect: egui::Rect,
    cull_rect: egui::Rect,
    annotation: &CanvasAnnotation,
) -> bool {
    annotation_screen_rect(state, rect, annotation).intersects(cull_rect)
}

fn paint_textbox_text(
    painter: &egui::Painter,
    galley: &Arc<egui::Galley>,
    content_rect: egui::Rect,
    color: egui::Color32,
) {
    painter
        .with_clip_rect(content_rect)
        .galley(content_rect.min, galley.clone(), color);
}

pub(super) fn paint_annotation_draft(
    painter: &egui::Painter,
    rect: egui::Rect,
    state: &CanvasState,
) {
    let Some(draft_state) = &state.interaction.annotation_draft_state else {
        return;
    };
    let draft = pending_annotation_from_draft(draft_state.clone()).unwrap_or(PendingAnnotation {
        kind: draft_state.kind,
        start_world: draft_state.start_world,
        end_world: draft_state.current_world,
        text: String::new(),
        text_size: CanvasTextSize::default(),
    });
    let stroke = crate::theme::stroke_strong();
    match draft.kind {
        CanvasAnnotationKind::Line => {
            let start = state.world_to_screen(draft.start_world, rect);
            let end = state.world_to_screen(draft.end_world, rect);
            painter.line_segment([start, end], stroke);
        }
        CanvasAnnotationKind::Arrow => {
            let start = state.world_to_screen(draft.start_world, rect);
            let end = state.world_to_screen(draft.end_world, rect);
            paint_arrow_segment(painter, start, end, stroke);
        }
        CanvasAnnotationKind::Textbox => {
            let textbox_rect = egui::Rect::from_two_pos(
                state.world_to_screen(draft.start_world, rect),
                state.world_to_screen(draft.end_world, rect),
            );
            painter.rect_stroke(
                textbox_rect,
                crate::theme::CONTROL_RADIUS,
                stroke,
                egui::StrokeKind::Outside,
            );
        }
    }
}

fn paint_arrow_segment(
    painter: &egui::Painter,
    start: egui::Pos2,
    end: egui::Pos2,
    stroke: egui::Stroke,
) {
    painter.line_segment([start, end], stroke);
    let direction = end - start;
    let length = direction.length();
    if length <= 1.0 {
        return;
    }
    let unit = direction / length;
    let head = 8.0 + stroke.width * 2.0;
    let left = end - unit * head + egui::vec2(-unit.y, unit.x) * (head * 0.5);
    let right = end - unit * head + egui::vec2(unit.y, -unit.x) * (head * 0.5);
    painter.line_segment([end, left], stroke);
    painter.line_segment([end, right], stroke);
}

pub(super) fn show_textbox_editor(ui: &egui::Ui, rect: egui::Rect, state: &mut CanvasState) {
    let Some(annotation_id) = state.interaction.editing_text_annotation_id else {
        return;
    };
    let Some(index) = state
        .document
        .annotations
        .iter()
        .position(|annotation| annotation.id == annotation_id)
    else {
        state.interaction.editing_text_annotation_id = None;
        state.interaction.focus_text_editor = false;
        return;
    };
    let textbox_rect = annotation_screen_rect(state, rect, &state.document.annotations[index]);
    let toolbar_rect = editing_textbox_toolbar_rect(ui, state, rect);
    let content_rect = textbox_content_rect(textbox_rect);
    let text = state.document.annotations[index].text.clone();
    let layout = state.cached_textbox_layout(
        ui,
        annotation_id,
        &text,
        state.document.annotations[index].text_size,
        state.document.annotations[index].color,
        content_rect.width(),
        state.zoom,
    );
    let text_rect = textbox_text_interactive_rect_from_galley(
        ui,
        content_rect,
        &layout.galley,
        state.document.annotations[index].text_size,
        state.zoom,
        text.is_empty(),
    );
    let toolbar_interacting = toolbar_rect
        .map(|toolbar_rect| show_textbox_editor_toolbar(ui, toolbar_rect, state))
        .unwrap_or(false);

    egui::Area::new(egui::Id::new(("textbox-editor", annotation_id)))
        .order(egui::Order::Foreground)
        .fixed_pos(text_rect.min)
        .show(ui.ctx(), |ui| {
            ui.set_clip_rect(content_rect);
            let zoom = state.zoom;
            let text_size = state.document.annotations[index].text_size;
            let text_color = state.document.annotations[index].color;
            let mut layouter = |ui: &egui::Ui, text: &dyn egui::TextBuffer, width: f32| {
                let wrap_width = if content_rect.width() > 0.0 {
                    content_rect.width()
                } else {
                    width
                };
                layout_textbox_galley(ui, text.as_str(), text_size, text_color, wrap_width, zoom)
            };
            let output = ui
                .allocate_ui(text_rect.size(), |ui| {
                    ui.set_min_size(text_rect.size());
                    ui.visuals_mut().override_text_color = Some(annotation_color(text_color));
                    egui::TextEdit::multiline(&mut state.document.annotations[index].text)
                        .desired_width(content_rect.width())
                        .font(textbox_font_id(text_size, state.zoom))
                        .layouter(&mut layouter)
                        .frame(egui::Frame::NONE)
                        .show(ui)
                })
                .inner;
            let text = state.document.annotations[index].text.clone();
            let entry = state.cache_textbox_layout(
                annotation_id,
                &text,
                text_size,
                text_color,
                content_rect.width(),
                state.zoom,
                output.galley.clone(),
            );
            let required_height = entry.required_world_height;
            expand_textbox_to_height(state, index, required_height);
            if state.interaction.focus_text_editor {
                output.response.request_focus();
                state.interaction.focus_text_editor = false;
            }
            if output.response.changed() {
                state.mark_annotation_dirty(annotation_id);
            }
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) && output.response.has_focus() {
                state.finish_textbox_edit();
            } else if output.response.lost_focus() && !toolbar_interacting {
                state.finish_textbox_edit();
            }
        });
}

fn textbox_editor_toolbar_metrics(_ui: &egui::Ui) -> TextboxEditorToolbarMetrics {
    let button_span = TEXTBOX_EDITOR_TOOLBAR_BUTTON_WIDTH * 4.0
        + TOOLBAR_SWATCH_BUTTON_WIDTH * 5.0
        + TOOLBAR_BACKGROUND_BUTTON_WIDTH
        + TEXTBOX_EDITOR_TOOLBAR_BUTTON_GAP * 9.0;
    TextboxEditorToolbarMetrics {
        size: egui::vec2(button_span, crate::theme::CONTROL_HEIGHT),
    }
}

fn annotation_style_toolbar_metrics(_ui: &egui::Ui) -> TextboxEditorToolbarMetrics {
    let button_span = TEXTBOX_EDITOR_TOOLBAR_BUTTON_WIDTH * 3.0
        + TOOLBAR_SWATCH_BUTTON_WIDTH * 5.0
        + TEXTBOX_EDITOR_TOOLBAR_BUTTON_GAP * 7.0;
    TextboxEditorToolbarMetrics {
        size: egui::vec2(button_span, crate::theme::CONTROL_HEIGHT),
    }
}

pub(super) fn editing_textbox_toolbar_rect(
    ui: &egui::Ui,
    state: &CanvasState,
    rect: egui::Rect,
) -> Option<egui::Rect> {
    let annotation_id = state.interaction.editing_text_annotation_id?;
    let annotation = state.annotation_by_id(annotation_id)?;
    if annotation.kind != CanvasAnnotationKind::Textbox {
        return None;
    }

    let textbox_rect = annotation_screen_rect(state, rect, annotation);
    let metrics = textbox_editor_toolbar_metrics(ui);
    let min_x = rect.left() + TEXTBOX_EDITOR_TOOLBAR_VIEWPORT_MARGIN;
    let max_x = (rect.right() - TEXTBOX_EDITOR_TOOLBAR_VIEWPORT_MARGIN - metrics.size.x).max(min_x);
    let preferred_x = textbox_rect.left();
    let x = preferred_x.clamp(min_x, max_x);

    let min_y = rect.top() + TEXTBOX_EDITOR_TOOLBAR_VIEWPORT_MARGIN;
    let above_y = textbox_rect.top() - TEXTBOX_EDITOR_TOOLBAR_CLEARANCE - metrics.size.y;
    let y = above_y.max(min_y);

    Some(egui::Rect::from_min_size(egui::pos2(x, y), metrics.size))
}

fn show_textbox_editor_toolbar(
    ui: &egui::Ui,
    toolbar_rect: egui::Rect,
    state: &mut CanvasState,
) -> bool {
    let selected = state.editing_textbox_size();
    let selected_color = state.editing_textbox_color();
    let background_enabled = state.editing_textbox_background_enabled().unwrap_or(false);
    let mut next_size = None;
    let mut next_color = None;
    let mut toggle_background = false;
    let pointer_interacting = ui.input(|input| {
        input.pointer.latest_pos().is_some_and(|pointer_pos| {
            toolbar_rect.contains(pointer_pos)
                && (input.pointer.any_pressed()
                    || input.pointer.any_down()
                    || input.pointer.any_released())
        })
    });

    egui::Area::new(egui::Id::new("textbox-editor-toolbar"))
        .order(egui::Order::Foreground)
        .fixed_pos(toolbar_rect.min)
        .show(ui.ctx(), |ui| {
            ui.set_min_size(toolbar_rect.size());
            ui.spacing_mut().item_spacing.x = TEXTBOX_EDITOR_TOOLBAR_BUTTON_GAP;
            ui.horizontal(|ui| {
                for size in [
                    CanvasTextSize::S,
                    CanvasTextSize::M,
                    CanvasTextSize::L,
                    CanvasTextSize::Xl,
                ] {
                    let button = egui::Button::new(
                        egui::RichText::new(size.label())
                            .text_style(egui::TextStyle::Button)
                            .color(crate::theme::TEXT),
                    )
                    .fill(if selected == Some(size) {
                        crate::theme::SURFACE_ELEVATED
                    } else {
                        crate::theme::SURFACE
                    })
                    .stroke(if selected == Some(size) {
                        crate::theme::stroke_strong()
                    } else {
                        crate::theme::stroke_subtle()
                    })
                    .corner_radius(crate::theme::CONTROL_RADIUS)
                    .min_size(egui::vec2(
                        TEXTBOX_EDITOR_TOOLBAR_BUTTON_WIDTH,
                        crate::theme::CONTROL_HEIGHT,
                    ));
                    if ui.add(button).clicked() {
                        next_size = Some(size);
                    }
                }
                for color in ANNOTATION_COLORS {
                    if paint_color_swatch_button(ui, color, selected_color == Some(color)).clicked()
                    {
                        next_color = Some(color);
                    }
                }
                let background_button = egui::Button::new(
                    egui::RichText::new("BG").text_style(egui::TextStyle::Button),
                )
                .fill(if background_enabled {
                    crate::theme::SURFACE_ELEVATED
                } else {
                    crate::theme::SURFACE
                })
                .stroke(if background_enabled {
                    crate::theme::stroke_strong()
                } else {
                    crate::theme::stroke_subtle()
                })
                .corner_radius(crate::theme::CONTROL_RADIUS)
                .min_size(egui::vec2(
                    TOOLBAR_BACKGROUND_BUTTON_WIDTH,
                    crate::theme::CONTROL_HEIGHT,
                ));
                if ui.add(background_button).clicked() {
                    toggle_background = true;
                }
            });
        });

    let changed = next_size
        .map(|text_size| state.apply_text_size_to_editing_textbox(text_size))
        .unwrap_or(false)
        || next_color
            .map(|color| state.apply_color_to_editing_textbox(color))
            .unwrap_or(false)
        || (toggle_background && state.toggle_editing_textbox_background());
    pointer_interacting || changed
}

pub(super) fn annotation_style_toolbar_rect(
    ui: &egui::Ui,
    state: &CanvasState,
    rect: egui::Rect,
) -> Option<egui::Rect> {
    let annotation = state.selected_annotation()?;
    if !matches!(
        annotation.kind,
        CanvasAnnotationKind::Line | CanvasAnnotationKind::Arrow
    ) {
        return None;
    }

    let annotation_rect = annotation_screen_rect(state, rect, annotation);
    let metrics = annotation_style_toolbar_metrics(ui);
    let min_x = rect.left() + TEXTBOX_EDITOR_TOOLBAR_VIEWPORT_MARGIN;
    let max_x = (rect.right() - TEXTBOX_EDITOR_TOOLBAR_VIEWPORT_MARGIN - metrics.size.x).max(min_x);
    let x = annotation_rect.left().clamp(min_x, max_x);
    let min_y = rect.top() + TEXTBOX_EDITOR_TOOLBAR_VIEWPORT_MARGIN;
    let y = (annotation_rect.top() - TEXTBOX_EDITOR_TOOLBAR_CLEARANCE - metrics.size.y).max(min_y);
    Some(egui::Rect::from_min_size(egui::pos2(x, y), metrics.size))
}

pub(super) fn show_annotation_style_toolbar(
    ui: &egui::Ui,
    rect: egui::Rect,
    state: &mut CanvasState,
) {
    let Some(toolbar_rect) = annotation_style_toolbar_rect(ui, state, rect) else {
        return;
    };
    let selected_width = state.selected_stroke_width();
    let selected_color = state.selected_annotation_color();
    let mut next_width = None;
    let mut next_color = None;

    egui::Area::new(egui::Id::new("annotation-style-toolbar"))
        .order(egui::Order::Foreground)
        .fixed_pos(toolbar_rect.min)
        .show(ui.ctx(), |ui| {
            ui.set_min_size(toolbar_rect.size());
            ui.spacing_mut().item_spacing.x = TEXTBOX_EDITOR_TOOLBAR_BUTTON_GAP;
            ui.horizontal(|ui| {
                for width in [
                    CanvasStrokeWidth::S,
                    CanvasStrokeWidth::M,
                    CanvasStrokeWidth::L,
                ] {
                    let button = egui::Button::new(width.label())
                        .fill(if selected_width == Some(width) {
                            crate::theme::SURFACE_ELEVATED
                        } else {
                            crate::theme::SURFACE
                        })
                        .stroke(if selected_width == Some(width) {
                            crate::theme::stroke_strong()
                        } else {
                            crate::theme::stroke_subtle()
                        })
                        .corner_radius(crate::theme::CONTROL_RADIUS)
                        .min_size(egui::vec2(
                            TEXTBOX_EDITOR_TOOLBAR_BUTTON_WIDTH,
                            crate::theme::CONTROL_HEIGHT,
                        ));
                    if ui.add(button).clicked() {
                        next_width = Some(width);
                    }
                }
                for color in ANNOTATION_COLORS {
                    if paint_color_swatch_button(ui, color, selected_color == Some(color)).clicked()
                    {
                        next_color = Some(color);
                    }
                }
            });
        });

    if let Some(width) = next_width {
        state.apply_stroke_width_to_selected_annotation(width);
    }
    if let Some(color) = next_color {
        state.apply_color_to_selected_annotation(color);
    }
}

fn paint_color_swatch_button(
    ui: &mut egui::Ui,
    color: CanvasAnnotationColor,
    selected: bool,
) -> egui::Response {
    ui.add(
        egui::Button::new("")
            .fill(annotation_color(color))
            .stroke(if selected {
                crate::theme::stroke_strong()
            } else {
                crate::theme::stroke_subtle()
            })
            .corner_radius(crate::theme::CONTROL_RADIUS)
            .min_size(egui::vec2(
                TOOLBAR_SWATCH_BUTTON_WIDTH,
                crate::theme::CONTROL_HEIGHT,
            )),
    )
}

pub(super) fn paint_marquee(painter: &egui::Painter, state: &CanvasState) {
    let Some(marquee_state) = &state.interaction.marquee_state else {
        return;
    };

    let rect = egui::Rect::from_two_pos(marquee_state.start_screen, marquee_state.current_screen);
    painter.rect_stroke(
        rect,
        0.0,
        crate::theme::stroke_strong(),
        egui::StrokeKind::Outside,
    );
}

pub(super) fn paint_crop_marquee(painter: &egui::Painter, rect: egui::Rect, state: &CanvasState) {
    let Some(crop_state) = &state.interaction.crop_state else {
        return;
    };
    let Some(image) = state.image_by_item_id(crop_state.item_id) else {
        return;
    };

    let image_rect = image_screen_rect(state, rect, image);
    let Some(crop_rect) = clamped_crop_screen_rect(
        crop_state.start_screen,
        crop_state.current_screen,
        image_rect,
    ) else {
        return;
    };

    painter.rect_stroke(
        image_rect,
        0.0,
        crate::theme::stroke_strong(),
        egui::StrokeKind::Outside,
    );
    painter.rect_stroke(
        crop_rect,
        0.0,
        crate::theme::stroke_strong(),
        egui::StrokeKind::Outside,
    );
}

pub(super) fn paint_selection_bounds(
    painter: &egui::Painter,
    rect: egui::Rect,
    state: &CanvasState,
) {
    if state.document.selected_annotation_ids.is_empty()
        && state.document.selected_item_ids.len() <= 1
    {
        return;
    }

    if state.document.selected_item_ids.is_empty()
        && let Some(annotation) = state.selected_annotation()
        && annotation.kind == CanvasAnnotationKind::Textbox
    {
        return;
    }

    let Some(selection_rect) = state.selection_screen_rect(rect) else {
        return;
    };

    painter.rect_stroke(
        selection_rect.expand(6.0),
        0.0,
        crate::theme::stroke_strong(),
        egui::StrokeKind::Outside,
    );
}

pub(super) fn paint_import_placeholders(
    painter: &egui::Painter,
    rect: egui::Rect,
    state: &CanvasState,
) {
    const SHIMMER_PERIOD_SECS: f32 = 1.8;
    const SHIMMER_PEAK_ALPHA: u8 = 40;
    const SHIMMER_BAND_FRACTION: f32 = 0.35;
    const SHIMMER_TILT_FRACTION: f32 = 0.25;

    let cull_rect = rect.expand(IMAGE_CULL_MARGIN_SCREEN);
    let time = painter.ctx().input(|i| i.time) as f32;
    let phase = (time / SHIMMER_PERIOD_SECS).rem_euclid(1.0);
    let mut any_visible = false;

    for placeholder in state
        .interaction
        .import_placeholders
        .iter()
        .chain(state.interaction.hover_preview_placeholders.iter())
        .chain(state.interaction.move_preview_placeholders.iter())
    {
        let placeholder_rect = egui::Rect::from_center_size(
            state.world_to_screen(placeholder.center_world, rect),
            placeholder.size_world * state.zoom,
        );
        if !placeholder_rect.intersects(cull_rect) {
            continue;
        }
        any_visible = true;

        painter.rect_filled(
            placeholder_rect.expand(2.0),
            0.0,
            egui::Color32::from_rgba_premultiplied(0, 0, 0, 120),
        );
        painter.rect_filled(placeholder_rect, 0.0, crate::theme::SURFACE_ELEVATED);
        painter.rect_stroke(
            placeholder_rect,
            0.0,
            egui::Stroke::new(1.0, crate::theme::BORDER_STRONG),
            egui::StrokeKind::Inside,
        );

        if let Some(shimmer) = shimmer_mesh(
            placeholder_rect,
            phase,
            SHIMMER_BAND_FRACTION,
            SHIMMER_TILT_FRACTION,
            SHIMMER_PEAK_ALPHA,
        ) {
            painter.with_clip_rect(placeholder_rect).add(shimmer);
        }
    }

    if any_visible {
        painter.ctx().request_repaint();
    }
}

fn shimmer_mesh(
    card_rect: egui::Rect,
    phase: f32,
    band_fraction: f32,
    tilt_fraction: f32,
    peak_alpha: u8,
) -> Option<egui::epaint::Mesh> {
    if card_rect.width() <= 1.0 || card_rect.height() <= 1.0 {
        return None;
    }
    let band_width = (card_rect.width() * band_fraction).max(24.0);
    let tilt = card_rect.height() * tilt_fraction;
    let travel_start = card_rect.left() - band_width - tilt;
    let travel_end = card_rect.right() + band_width;
    let center_x = travel_start + phase * (travel_end - travel_start);
    let half = band_width / 2.0;

    let peak = egui::Color32::from_rgba_unmultiplied(255, 255, 255, peak_alpha);
    let edge = egui::Color32::TRANSPARENT;

    let mut mesh = egui::epaint::Mesh::default();
    let mut push_vertex = |pos: egui::Pos2, color: egui::Color32| -> u32 {
        let index = mesh.vertices.len() as u32;
        mesh.vertices.push(egui::epaint::Vertex {
            pos,
            uv: egui::epaint::WHITE_UV,
            color,
        });
        index
    };
    let top = card_rect.top();
    let bottom = card_rect.bottom();
    let left_top = push_vertex(egui::pos2(center_x - half, top), edge);
    let center_top = push_vertex(egui::pos2(center_x, top), peak);
    let right_top = push_vertex(egui::pos2(center_x + half, top), edge);
    let left_bot = push_vertex(egui::pos2(center_x - half - tilt, bottom), edge);
    let center_bot = push_vertex(egui::pos2(center_x - tilt, bottom), peak);
    let right_bot = push_vertex(egui::pos2(center_x + half - tilt, bottom), edge);
    mesh.add_triangle(left_top, center_top, center_bot);
    mesh.add_triangle(left_top, center_bot, left_bot);
    mesh.add_triangle(center_top, right_top, right_bot);
    mesh.add_triangle(center_top, right_bot, center_bot);
    Some(mesh)
}

pub(super) fn image_paint_stats_for_records(
    records: &[crate::data::BoardItemWithAsset],
    rect: egui::Rect,
    pan: egui::Vec2,
    zoom: f32,
    cull_offscreen: bool,
) -> super::ImagePaintStats {
    let mut stats = super::ImagePaintStats::default();
    let cull_rect = rect.expand(IMAGE_CULL_MARGIN_SCREEN);
    for record in records {
        stats.considered += 1;
        let image_rect = image_screen_rect_for_record(record, rect, pan, zoom, rect.center());
        if !cull_offscreen || image_rect.intersects(cull_rect) {
            stats.drawn += 1;
        }
    }
    stats
}

pub(super) fn image_texture_group_count_for_records(
    records: &[crate::data::BoardItemWithAsset],
    rect: egui::Rect,
    pan: egui::Vec2,
    zoom: f32,
) -> usize {
    let cull_rect = rect.expand(IMAGE_CULL_MARGIN_SCREEN);
    let mut groups = 0;
    let mut previous_asset_id = None;
    for record in records {
        let image_rect = image_screen_rect_for_record(record, rect, pan, zoom, rect.center());
        if !image_rect.intersects(cull_rect) {
            continue;
        }
        if previous_asset_id != Some(record.asset.id) {
            groups += 1;
            previous_asset_id = Some(record.asset.id);
        }
    }
    groups
}

#[cfg(test)]
pub(super) fn annotation_paint_stats_for_annotations(
    annotations: &[CanvasAnnotation],
    rect: egui::Rect,
    pan: egui::Vec2,
    zoom: f32,
    cull_offscreen: bool,
) -> AnnotationPaintStats {
    let state = CanvasState {
        pan,
        zoom,
        document: CanvasDocumentState {
            annotations: annotations.to_vec(),
            ..Default::default()
        },
        ..CanvasState::default()
    };
    let cull_rect = rect.expand(ANNOTATION_CULL_MARGIN_SCREEN);
    let mut stats = AnnotationPaintStats::default();
    for annotation in annotations {
        stats.considered += 1;
        if !cull_offscreen || annotation_should_paint(&state, rect, cull_rect, annotation) {
            stats.drawn += 1;
        }
    }
    stats
}
