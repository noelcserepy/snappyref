use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui;

use super::annotations::{annotation_should_paint, paint_annotation, paint_annotations};
use crate::canvas::geometry::{image_screen_corners_for_record, image_screen_rect_for_record};
use crate::canvas::interaction::image_screen_rect;
use crate::canvas::{
    ANNOTATION_CULL_MARGIN_SCREEN, CanvasImage, CanvasImageLod, CanvasState, CanvasTexture,
    IMAGE_CULL_MARGIN_SCREEN, IMAGE_DETAIL_PROMOTION_THRESHOLD, ImagePaintStats, ImagePassBatch,
    ImagePassCallback, ImagePassInstance, ImagePassMetrics,
};
use crate::data::CanvasStackItem;

const SELECTION_BORDER_WIDTH: f32 = 1.0;
const IMAGE_SELECTION_BORDER_COLOR: egui::Color32 = crate::ui::theme::TEXT;

pub(crate) fn paint_images(
    painter: &egui::Painter,
    ui: &egui::Ui,
    rect: egui::Rect,
    state: &mut CanvasState,
    render_state: Option<&eframe::egui_wgpu::RenderState>,
    allow_lod_promotion: bool,
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
                .then_some((image.record.state.id, image_rect))
        })
        .collect::<Vec<_>>();
    metrics.visible_images = visible_images.len();

    let animation_time = state
        .render
        .animation_time_override_seconds
        .unwrap_or_else(|| painter.ctx().input(|input| input.time as f32));
    let pointer_hover_pos = painter.ctx().input(|input| input.pointer.hover_pos());
    let mut native_images = Vec::new();
    let mut svg_images = Vec::new();
    let mut fallback_images = Vec::new();
    let mut next_animation_repaint = None;

    for (item_id, image_rect) in &visible_images {
        let animation_active = state.is_selected(*item_id)
            || pointer_hover_pos.is_some_and(|pos| image_rect.contains(pos));
        if animation_active
            && let Some((rgba, width, height)) =
                state.advance_animated_image(*item_id, animation_time)
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
        if !image.record.is_loaded() || matches!(image.texture, CanvasTexture::Unloaded { .. }) {
            fallback_images.push(*item_id);
            continue;
        }
        if image.animation.is_some() {
            metrics.visible_animated_images += 1;
            if animation_active {
                metrics.active_animated_images += 1;
                next_animation_repaint = min_repaint_delay(
                    next_animation_repaint,
                    state.animated_image_repaint_delay(*item_id, animation_time),
                );
            }
        }
        if matches!(image.texture, CanvasTexture::Svg { .. }) {
            svg_images.push((*item_id, *image_rect));
            continue;
        }

        let native_eligible = matches!(image.texture, CanvasTexture::Native(_))
            && image.animation.is_none()
            && image.record.state.rotation.abs() <= 0.0001;
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
        if image.record.is_loaded() {
            paint_image_mesh(painter, state, rect, image);
        } else {
            let image_rect = image_screen_rect(state, rect, image);
            paint_unloaded_placement(painter, image, image_rect);
        }
    }

    metrics.build_time_ms = build_started_at.elapsed().as_secs_f64() * 1000.0;

    if allow_lod_promotion {
        let promotion_requests = visible_images
            .iter()
            .filter_map(|(item_id, _)| {
                desired_image_lod(state, rect, *item_id).map(|lod| (*item_id, lod))
            })
            .collect::<Vec<_>>();
        for (item_id, lod) in promotion_requests {
            state.request_image_lod_load(item_id, lod);
        }
    }

    for image in &state.document.images {
        let image_rect = image_screen_rect(state, rect, image);
        if !image_rect.intersects(cull_rect) {
            continue;
        }
        paint_image_overlay(painter, state, image, image_rect, debug_labels);
    }

    if let Some(delay) = next_animation_repaint {
        painter.ctx().request_repaint_after(delay);
    }

    metrics
}

pub(crate) fn paint_canvas_items(
    painter: &egui::Painter,
    ui: &egui::Ui,
    rect: egui::Rect,
    state: &mut CanvasState,
    render_state: Option<&eframe::egui_wgpu::RenderState>,
    allow_lod_promotion: bool,
    debug_labels: bool,
) -> ImagePassMetrics {
    if canvas_stack_matches_existing_layering(state) {
        let image_pass = paint_images(
            painter,
            ui,
            rect,
            state,
            render_state,
            allow_lod_promotion,
            debug_labels,
        );
        paint_annotations(painter, ui, rect, state);
        return image_pass;
    }

    paint_canvas_items_interleaved(painter, ui, rect, state, allow_lod_promotion, debug_labels)
}

#[cfg(test)]
pub(crate) fn unified_canvas_paint_order_for_state(state: &CanvasState) -> Vec<CanvasStackItem> {
    state.ordered_canvas_items_back_to_front()
}

fn canvas_stack_matches_existing_layering(state: &CanvasState) -> bool {
    let ordered = state.ordered_canvas_items_back_to_front();
    let existing = state
        .document
        .images
        .iter()
        .map(|image| CanvasStackItem::Placement(image.record.state.id))
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
    allow_lod_promotion: bool,
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
    let pointer_hover_pos = painter.ctx().input(|input| input.pointer.hover_pos());
    let mut next_animation_repaint = None;
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

                let animation_active = state.is_selected(item_id)
                    || pointer_hover_pos.is_some_and(|pos| image_rect.contains(pos));
                if animation_active
                    && let Some((rgba, width, height)) =
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
                if !image.record.is_loaded()
                    || matches!(image.texture, CanvasTexture::Unloaded { .. })
                {
                    paint_unloaded_placement(painter, image, image_rect);
                    paint_image_overlay(painter, state, image, image_rect, debug_labels);
                    continue;
                }
                if image.animation.is_some() {
                    metrics.visible_animated_images += 1;
                    if animation_active {
                        metrics.active_animated_images += 1;
                        next_animation_repaint = min_repaint_delay(
                            next_animation_repaint,
                            state.animated_image_repaint_delay(item_id, animation_time),
                        );
                    }
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

    if allow_lod_promotion {
        let promotion_requests = visible_item_ids
            .iter()
            .filter_map(|item_id| {
                desired_image_lod(state, rect, *item_id).map(|lod| (*item_id, lod))
            })
            .collect::<Vec<_>>();
        for (item_id, lod) in promotion_requests {
            state.request_image_lod_load(item_id, lod);
        }
    }

    if let Some(delay) = next_animation_repaint {
        painter.ctx().request_repaint_after(delay);
    }

    metrics.build_time_ms = build_started_at.elapsed().as_secs_f64() * 1000.0;
    metrics
}

fn paint_unloaded_placement(painter: &egui::Painter, image: &CanvasImage, image_rect: egui::Rect) {
    let (fill, stroke, label) = match &image.record.content {
        crate::data::PlacementContent::Loading { .. } => (
            crate::ui::theme::SURFACE,
            egui::Stroke::new(1.0, crate::ui::theme::BORDER_STRONG),
            None,
        ),
        crate::data::PlacementContent::Failed { .. } => (
            crate::ui::theme::DESTRUCTIVE_FILL,
            egui::Stroke::new(1.0, crate::ui::theme::DESTRUCTIVE_STROKE),
            Some("Failed"),
        ),
        crate::data::PlacementContent::Loaded { .. } => (
            crate::ui::theme::SURFACE,
            egui::Stroke::new(1.0, crate::ui::theme::BORDER_STRONG),
            None,
        ),
    };
    painter.rect_filled(image_rect, crate::ui::theme::RADIUS_NONE, fill);
    painter.rect_stroke(
        image_rect,
        crate::ui::theme::RADIUS_NONE,
        stroke,
        egui::StrokeKind::Inside,
    );
    if let Some(label) = label
        && image_rect.width() >= 48.0
        && image_rect.height() >= 24.0
    {
        painter.text(
            image_rect.center(),
            egui::Align2::CENTER_CENTER,
            label,
            egui::FontId::proportional(12.0),
            crate::ui::theme::TEXT_MUTED,
        );
    }
}

fn min_repaint_delay(current: Option<Duration>, candidate: Option<Duration>) -> Option<Duration> {
    match (current, candidate) {
        (Some(current), Some(candidate)) => Some(current.min(candidate)),
        (None, Some(candidate)) => Some(candidate),
        (Some(current), None) => Some(current),
        (None, None) => None,
    }
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

pub(crate) fn desired_image_lod(
    state: &CanvasState,
    rect: egui::Rect,
    item_id: i64,
) -> Option<CanvasImageLod> {
    let image = state.image_by_item_id(item_id)?;
    if !image.record.is_loaded() || matches!(image.texture, CanvasTexture::Unloaded { .. }) {
        return None;
    }
    if image.loaded_lod < CanvasImageLod::Working {
        return Some(CanvasImageLod::Working);
    }
    if image.loaded_lod >= CanvasImageLod::Detail {
        return None;
    }
    let image_rect = image_screen_rect(state, rect, image);
    let on_screen_max_side = image_rect.width().max(image_rect.height());
    let texture_max_side = image.texture.size()[0].max(image.texture.size()[1]) as f32;
    (texture_max_side > 0.0
        && on_screen_max_side >= texture_max_side * IMAGE_DETAIL_PROMOTION_THRESHOLD)
        .then_some(CanvasImageLod::Detail)
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
        .contains(&image.record.state.id)
    {
        painter.rect_stroke(
            image_rect,
            crate::ui::theme::RADIUS_NONE,
            egui::Stroke::new(SELECTION_BORDER_WIDTH, IMAGE_SELECTION_BORDER_COLOR),
            egui::StrokeKind::Outside,
        );
    }
    if state.is_selected(image.record.state.id)
        && selection_border_visible(state, painter.ctx().input(|input| input.time))
    {
        let corners = image_screen_corners_for_record(
            &image.record,
            state.pan,
            state.zoom,
            painter.clip_rect().center(),
        );
        painter.add(egui::Shape::closed_line(
            corners.to_vec(),
            egui::Stroke::new(SELECTION_BORDER_WIDTH, IMAGE_SELECTION_BORDER_COLOR),
        ));
    }

    if debug_labels {
        let label = format!(
            "#{}  ({:.0}, {:.0})",
            image.record.state.id, image.record.state.center.x, image.record.state.center.y
        );
        painter.text(
            image_rect.min + egui::vec2(crate::ui::theme::SPACE_2, crate::ui::theme::SPACE_2),
            egui::Align2::LEFT_TOP,
            label,
            egui::TextStyle::Body.resolve(&painter.ctx().global_style()),
            crate::ui::theme::TEXT,
        );
    }
}

pub(crate) fn selection_border_visible(state: &CanvasState, now_seconds: f64) -> bool {
    let Some(drag_state) = &state.interaction.drag_state else {
        return true;
    };
    !drag_state.moved
        || now_seconds - drag_state.last_movement_seconds
            >= crate::canvas::SELECTION_BORDER_REAPPEAR_SECONDS
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

pub(crate) fn canvas_texture_from_preview(
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

pub(crate) fn image_paint_stats_for_records(
    records: &[crate::data::Placement],
    rect: egui::Rect,
    pan: egui::Vec2,
    zoom: f32,
    cull_offscreen: bool,
) -> ImagePaintStats {
    let mut stats = ImagePaintStats::default();
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

pub(crate) fn image_texture_group_count_for_records(
    records: &[crate::data::Placement],
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
        if previous_asset_id != Some(record.asset.as_ref().expect("loaded placement asset").id) {
            groups += 1;
            previous_asset_id = Some(record.asset.as_ref().expect("loaded placement asset").id);
        }
    }
    groups
}
