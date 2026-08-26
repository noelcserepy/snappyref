use std::sync::Arc;

use eframe::egui;

#[cfg(test)]
use super::CanvasImagePreview;
use super::geometry::{
    clamped_crop_screen_rect, crop_center_offset_local, merge_cropped_item_fields,
    min_loaded_placement_scale, pixel_crop_rect_from_local, screen_rect_to_local_crop_rect,
};
use super::interaction::{
    annotation_screen_rect, annotation_world_rect, normalized_textbox_rect, placement_screen_rect,
    textbox_world_rect,
};
use super::render::{
    canvas_texture_from_preview, layout_textbox_galley, textbox_layout_cache_key,
    textbox_required_world_height,
};
use super::{
    AnimatedCanvasFrame, AnimatedCanvasImage, CanvasCropRequest, CanvasDocumentChanges,
    CanvasState, CanvasTexture, CropState, FOCUS_SELECTION_PADDING_SCREEN, MAX_CANVAS_ZOOM,
    MIN_CANVAS_ZOOM, PendingAnnotation, RenderedPlacement, SharedRgba, TextboxLayoutCacheEntry,
    merge_ids,
};
use crate::data::{
    CanvasAnnotation, CanvasAnnotationColor, CanvasAnnotationKind, CanvasBackground,
    CanvasImageLod, CanvasStackItem, CanvasStrokeWidth, CanvasTextSize, Placement,
    PlacementLifecycle, ViewportState,
};
use crate::jobs::{PreparedCropPreview, PreparedPlacementImage};
use crate::placement::arrangement::PlacementArrangementInput;

#[derive(Clone, Copy)]
enum StackOrderMove {
    Front,
    Back,
}

struct CachedCropPreview {
    rgba: Vec<u8>,
    width: u32,
    height: u32,
}

fn crop_cached_preview(
    rgba: &[u8],
    preview_width: u32,
    preview_height: u32,
    asset_width: u32,
    asset_height: u32,
    crop_rect: super::PixelCropRect,
) -> Result<CachedCropPreview, String> {
    if preview_width == 0 || preview_height == 0 || asset_width == 0 || asset_height == 0 {
        return Err("cannot crop zero-sized preview".to_owned());
    }
    let expected_len = preview_width as usize * preview_height as usize * 4;
    if rgba.len() != expected_len {
        return Err("cached preview pixels did not match preview dimensions".to_owned());
    }

    let x0 = scale_floor(crop_rect.x, preview_width, asset_width).min(preview_width - 1);
    let y0 = scale_floor(crop_rect.y, preview_height, asset_height).min(preview_height - 1);
    let x1 = scale_ceil(
        crop_rect.x.saturating_add(crop_rect.width),
        preview_width,
        asset_width,
    )
    .clamp(x0 + 1, preview_width);
    let y1 = scale_ceil(
        crop_rect.y.saturating_add(crop_rect.height),
        preview_height,
        asset_height,
    )
    .clamp(y0 + 1, preview_height);
    let width = x1 - x0;
    let height = y1 - y0;
    let mut cropped = Vec::with_capacity(width as usize * height as usize * 4);
    for y in y0..y1 {
        let row_start = (y as usize * preview_width as usize + x0 as usize) * 4;
        let row_end = row_start + width as usize * 4;
        cropped.extend_from_slice(&rgba[row_start..row_end]);
    }

    Ok(CachedCropPreview {
        rgba: cropped,
        width,
        height,
    })
}

fn scale_floor(value: u32, target: u32, source: u32) -> u32 {
    ((value as u64 * target as u64) / source as u64) as u32
}

fn scale_ceil(value: u32, target: u32, source: u32) -> u32 {
    let numerator = value as u64 * target as u64;
    let denominator = source as u64;
    numerator.div_ceil(denominator) as u32
}

impl CanvasState {
    pub fn add_prepared_placement_image(
        &mut self,
        ctx: &egui::Context,
        loaded: PreparedPlacementImage,
    ) {
        let animation = loaded.animated_preview.map(|animated| AnimatedCanvasImage {
            frames: animated
                .frames
                .into_iter()
                .map(|frame| AnimatedCanvasFrame {
                    rgba: SharedRgba::from(frame.rgba),
                    delay_seconds: frame.delay_seconds,
                })
                .collect(),
            width: animated.width,
            height: animated.height,
            current_frame: 0,
        });
        let texture = canvas_texture_from_preview(
            ctx,
            format!(
                "asset-{}-item-{}",
                loaded.placement.require_loaded_asset().id,
                loaded.placement.state.id
            ),
            &loaded.preview_rgba,
            loaded.preview_width,
            loaded.preview_height,
            loaded.preview_texture,
        );

        self.document
            .rendered_placements
            .retain(|image| image.placement.state.id != loaded.placement.state.id);
        self.render
            .pending_image_lod_loads
            .remove(&loaded.placement.state.id);
        self.document.rendered_placements.push(RenderedPlacement {
            placement: loaded.placement,
            texture,
            loaded_lod: loaded.loaded_lod,
            animation,
        });
        self.append_missing_stack_item(CanvasStackItem::Placement(
            self.document
                .rendered_placements
                .last()
                .unwrap()
                .placement
                .state
                .id,
        ));
        self.document
            .rendered_placements
            .sort_by_key(|image| image.placement.state.geometry.z_index);
    }

    pub fn add_unresolved_placement(&mut self, placement: Placement) {
        self.document
            .rendered_placements
            .retain(|image| image.placement.state.id != placement.state.id);
        self.render
            .pending_image_lod_loads
            .remove(&placement.state.id);
        self.document.rendered_placements.push(RenderedPlacement {
            texture: CanvasTexture::Unloaded {
                size: [
                    placement.state.geometry.size.x.max(1.0).round() as usize,
                    placement.state.geometry.size.y.max(1.0).round() as usize,
                ],
            },
            loaded_lod: crate::data::CanvasImageLod::Overview,
            animation: None,
            placement,
        });
        self.append_missing_stack_item(CanvasStackItem::Placement(
            self.document
                .rendered_placements
                .last()
                .unwrap()
                .placement
                .state
                .id,
        ));
        self.document
            .rendered_placements
            .sort_by_key(|image| image.placement.state.geometry.z_index);
    }

    pub fn add_provisional_unresolved_placement(&mut self, mut placement: Placement) {
        placement.lifecycle = PlacementLifecycle::Provisional;
        self.add_unresolved_placement(placement);
    }

    pub fn next_provisional_placement_id(&self) -> i64 {
        self.document
            .rendered_placements
            .iter()
            .filter_map(|image| (image.placement.state.id < 0).then_some(image.placement.state.id))
            .min()
            .unwrap_or(0)
            - 1
    }

    pub fn remove_provisional_placements(&mut self) -> Vec<i64> {
        let ids = self
            .document
            .rendered_placements
            .iter()
            .filter_map(|image| {
                (image.placement.lifecycle == PlacementLifecycle::Provisional)
                    .then_some(image.placement.state.id)
            })
            .collect::<Vec<_>>();
        self.delete_placements(&ids)
    }

    pub fn commit_provisional_placements_in_place(
        &mut self,
        committed: Vec<(i64, Placement)>,
    ) -> Vec<Placement> {
        let mut committed_placements = Vec::with_capacity(committed.len());
        for (provisional_id, mut placement) in committed {
            placement.lifecycle = PlacementLifecycle::Committed;
            let committed_id = placement.state.id;
            if let Some(image) = self
                .document
                .rendered_placements
                .iter_mut()
                .find(|image| image.placement.state.id == provisional_id)
            {
                image.placement = placement.clone();
            }
            if let Some(load) = self.render.pending_image_lod_loads.remove(&provisional_id) {
                self.render
                    .pending_image_lod_loads
                    .insert(committed_id, load);
            }
            for selected_id in &mut self.document.selected_placement_ids {
                if *selected_id == provisional_id {
                    *selected_id = committed_id;
                }
            }
            for item in &mut self.document.stack_order {
                if *item == CanvasStackItem::Placement(provisional_id) {
                    *item = CanvasStackItem::Placement(committed_id);
                }
            }
            committed_placements.push(placement);
        }
        committed_placements
    }

    pub fn provisional_placements_in_selection_order(&self) -> Vec<Placement> {
        self.document
            .selected_placement_ids
            .iter()
            .filter_map(|placement_id| self.placement_for_id(*placement_id))
            .filter(|placement| placement.lifecycle == PlacementLifecycle::Provisional)
            .collect()
    }

    pub fn add_loaded_reference_placement(&mut self, placement: Placement) -> Result<(), String> {
        let source = self
            .document
            .rendered_placements
            .iter()
            .find(|image| {
                image.placement.require_loaded_asset().id == placement.require_loaded_asset().id
            })
            .ok_or_else(|| {
                format!(
                    "missing source texture for asset {}",
                    placement.require_loaded_asset().id
                )
            })?;
        let texture = source.texture.clone();
        let animation = source.animation.clone();

        self.document.rendered_placements.push(RenderedPlacement {
            placement,
            texture,
            loaded_lod: CanvasImageLod::Working,
            animation,
        });
        self.append_missing_stack_item(CanvasStackItem::Placement(
            self.document
                .rendered_placements
                .last()
                .unwrap()
                .placement
                .state
                .id,
        ));
        self.document
            .rendered_placements
            .sort_by_key(|image| image.placement.state.geometry.z_index);
        Ok(())
    }

    pub fn warm_cached_pixels_for_item(
        &mut self,
        library: &crate::library::storage::Library,
        placement_id: i64,
    ) -> Result<(), String> {
        let image = self
            .rendered_placement_by_id(placement_id)
            .ok_or_else(|| format!("missing canvas item {placement_id}"))?;
        let [width, height] = image.texture.size();
        let expected_len = width
            .checked_mul(height)
            .and_then(|pixels| pixels.checked_mul(4))
            .ok_or_else(|| "canvas pixel cache dimensions overflowed".to_owned())?;
        if self
            .render
            .asset_rgba_cache
            .get(&image.placement.require_loaded_asset().id)
            .is_some_and(|rgba| rgba.len() == expected_len)
        {
            return Ok(());
        }

        let placement = image.placement.clone();
        let (rgba, loaded_width, loaded_height) = crate::jobs::load_canvas_snapshot_pixels(
            library,
            &placement,
            width as u32,
            height as u32,
        )?;
        if loaded_width as usize != width || loaded_height as usize != height {
            return Err("cached canvas pixels did not match texture size".to_owned());
        }
        self.render
            .asset_rgba_cache
            .insert(placement.require_loaded_asset().id, SharedRgba::from(rgba));
        Ok(())
    }

    pub fn snapshot_item_image(
        &self,
        placement_id: i64,
    ) -> Result<(Placement, SharedRgba, u32, u32), String> {
        if let Some(preview) = self.render.pending_crop_previews.get(&placement_id) {
            let [width, height] = preview.image.texture.size();
            return Ok((
                preview.image.placement.clone(),
                preview.rgba.clone(),
                width as u32,
                height as u32,
            ));
        }

        let image = self
            .rendered_placement_by_id(placement_id)
            .ok_or_else(|| format!("missing canvas item {}", placement_id))?;
        let rgba = self
            .render
            .asset_rgba_cache
            .get(&image.placement.require_loaded_asset().id)
            .cloned()
            .ok_or_else(|| {
                format!(
                    "missing cached pixels for asset {}",
                    image.placement.require_loaded_asset().id
                )
            })?;
        Ok((
            image.placement.clone(),
            rgba,
            image.texture.size()[0] as u32,
            image.texture.size()[1] as u32,
        ))
    }

    pub fn projected_cropped_item(
        &self,
        cropped: &PreparedCropPreview,
    ) -> Result<crate::data::PlacementState, String> {
        let source_placement_state =
            if let Some(preview) = self.render.pending_crop_previews.get(&cropped.placement_id) {
                preview.image.placement.state.clone()
            } else {
                let Some(image) = self.rendered_placement_by_id(cropped.placement_id) else {
                    return Err(format!("missing canvas item {}", cropped.placement_id));
                };
                image.placement.state.clone()
            };

        let mut updated_placement_state = source_placement_state;
        let asset_id = updated_placement_state
            .asset_id
            .ok_or_else(|| format!("placement {} has no asset", updated_placement_state.id))?;
        merge_cropped_item_fields(
            &mut updated_placement_state,
            asset_id,
            cropped.crop_offset_local,
        );
        Ok(updated_placement_state)
    }

    pub fn apply_pending_crop_preview(
        &mut self,
        ctx: &egui::Context,
        request: &CanvasCropRequest,
    ) -> Result<(), String> {
        let image_index = self
            .document
            .rendered_placements
            .iter()
            .position(|image| image.placement.state.id == request.placement_id)
            .ok_or_else(|| format!("missing canvas item {}", request.placement_id))?;
        let original_image = self.document.rendered_placements[image_index].clone();
        let source_rgba = self
            .render
            .asset_rgba_cache
            .get(&request.source_asset.id)
            .cloned()
            .ok_or_else(|| {
                format!(
                    "missing cached pixels for asset {}",
                    request.source_asset.id
                )
            })?;
        self.render
            .pending_crop_previews
            .entry(request.placement_id)
            .or_insert_with(|| super::PendingCropPreview {
                image: original_image.clone(),
                rgba: source_rgba.clone(),
            });

        let [source_width, source_height] = original_image.texture.size();
        let preview = crop_cached_preview(
            &source_rgba,
            source_width as u32,
            source_height as u32,
            request.source_asset.width,
            request.source_asset.height,
            request.crop_rect,
        )?;
        let texture = canvas_texture_from_preview(
            ctx,
            format!(
                "asset-{}-item-{}-pending-crop",
                request.source_asset.id, request.placement_id
            ),
            &preview.rgba,
            preview.width,
            preview.height,
            crate::jobs::ImagePrepPreviewTexture::Cpu,
        );
        let mut asset = request.source_asset.clone();
        asset.width = request.crop_rect.width;
        asset.height = request.crop_rect.height;
        let mut updated_placement_state = original_image.placement.state.clone();
        let asset_id = updated_placement_state
            .asset_id
            .ok_or_else(|| format!("placement {} has no asset", updated_placement_state.id))?;
        merge_cropped_item_fields(
            &mut updated_placement_state,
            asset_id,
            request.crop_offset_local,
        );

        let image = &mut self.document.rendered_placements[image_index];
        image.placement.state = updated_placement_state;
        image.placement.asset = Some(asset);
        image.texture = texture;
        image.loaded_lod = CanvasImageLod::Working;
        image.animation = None;

        Ok(())
    }

    pub fn apply_cropped_item(
        &mut self,
        ctx: &egui::Context,
        cropped: PreparedCropPreview,
        asset: crate::data::Asset,
        updated_item: crate::data::PlacementState,
    ) -> Result<(), String> {
        let placement_id = cropped.placement_id;
        let loaded_lod = cropped.loaded_lod;
        let should_cache_pixels = self.document.selected_placement_ids.contains(&placement_id);
        let texture = canvas_texture_from_preview(
            ctx,
            format!("asset-{}-item-{}", asset.id, placement_id),
            &cropped.preview_rgba,
            cropped.preview_width,
            cropped.preview_height,
            cropped.preview_texture,
        );
        if should_cache_pixels {
            self.render
                .asset_rgba_cache
                .insert(asset.id, SharedRgba::from(cropped.preview_rgba));
        }

        let Some(image) = self
            .document
            .rendered_placements
            .iter_mut()
            .find(|image| image.placement.state.id == placement_id)
        else {
            return Err(format!("missing canvas item {}", placement_id));
        };

        image.placement.state = updated_item;
        image.placement.asset = Some(asset);
        image.texture = texture;
        image.loaded_lod = loaded_lod;
        image.animation = None;
        self.render.pending_image_lod_loads.remove(&placement_id);
        self.render.pending_crop_previews.remove(&placement_id);
        Ok(())
    }

    pub fn apply_viewport_state(&mut self, viewport: &ViewportState) {
        self.interaction.mouse_wheel_zoom_animation = None;
        self.pan = egui::vec2(viewport.camera_pan_x, viewport.camera_pan_y);
        self.zoom = viewport.camera_zoom.clamp(MIN_CANVAS_ZOOM, MAX_CANVAS_ZOOM);
        self.background = viewport.canvas_background;
    }

    pub fn set_rotation_snap_degrees(&mut self, degrees: f32) {
        self.rotation_snap_degrees = degrees.max(0.0);
    }

    pub fn set_pointer_mode(&mut self, mode: crate::data::PointerMode) {
        self.pointer_mode = mode;
    }

    pub(super) fn pointer_mode(&self) -> crate::data::PointerMode {
        self.pointer_mode
    }

    pub fn set_placement_gap(&mut self, gap: f32) {
        self.placement_gap = gap.max(0.0);
    }

    pub fn viewport_state(&self) -> ViewportState {
        ViewportState {
            camera_pan_x: self.pan.x,
            camera_pan_y: self.pan.y,
            camera_zoom: self.zoom,
            canvas_background: self.background,
        }
    }

    pub fn has_selection(&self) -> bool {
        self.has_placement_selection() || !self.document.selected_annotation_ids.is_empty()
    }

    pub fn has_placement_selection(&self) -> bool {
        !self.document.selected_placement_ids.is_empty()
    }

    pub fn selected_placement_ids(&self) -> Vec<i64> {
        self.document.selected_placement_ids.clone()
    }

    pub fn selected_annotation_ids(&self) -> Vec<i64> {
        self.document.selected_annotation_ids.clone()
    }

    pub fn all_placement_ids(&self) -> Vec<i64> {
        self.document
            .rendered_placements
            .iter()
            .map(|image| image.placement.state.id)
            .collect()
    }

    pub fn all_annotation_ids(&self) -> Vec<i64> {
        self.document
            .annotations
            .iter()
            .map(|annotation| annotation.id)
            .collect()
    }

    pub fn annotation_for_id(&self, annotation_id: i64) -> Option<CanvasAnnotation> {
        self.document
            .annotations
            .iter()
            .find(|annotation| annotation.id == annotation_id)
            .cloned()
    }

    pub fn placement_for_id(&self, placement_id: i64) -> Option<Placement> {
        self.document
            .rendered_placements
            .iter()
            .find(|image| image.placement.state.id == placement_id)
            .map(|image| image.placement.clone())
    }

    #[cfg(test)]
    pub fn preview_for_placement_id(&self, placement_id: i64) -> Option<CanvasImagePreview> {
        let image = self.rendered_placement_by_id(placement_id)?;
        let [width, height] = image.texture.size();
        Some(CanvasImagePreview {
            texture_size: egui::vec2(width as f32, height as f32),
        })
    }

    pub fn background(&self) -> CanvasBackground {
        self.background
    }

    pub fn set_background(&mut self, background: CanvasBackground) {
        self.background = background;
    }

    pub fn focus_selection_in_view(
        &mut self,
        canvas_rect: egui::Rect,
        target_rect: egui::Rect,
    ) -> bool {
        let Some(bounds) = self.selected_world_bounds() else {
            return false;
        };

        let padded_size = target_rect
            .shrink(FOCUS_SELECTION_PADDING_SCREEN)
            .size()
            .max(egui::vec2(1.0, 1.0));
        let width_zoom = padded_size.x / bounds.width().max(1.0);
        let height_zoom = padded_size.y / bounds.height().max(1.0);
        self.zoom = width_zoom
            .min(height_zoom)
            .clamp(MIN_CANVAS_ZOOM, MAX_CANVAS_ZOOM);

        let center_world = bounds.center().to_vec2();
        self.interaction.mouse_wheel_zoom_animation = None;
        self.pan = target_rect.center() - canvas_rect.center() - center_world * self.zoom;
        true
    }

    pub(crate) fn zoom_out_to_fit_world_rect(
        &mut self,
        canvas_rect: egui::Rect,
        target_rect: egui::Rect,
        bounds: egui::Rect,
        padding_screen: f32,
    ) -> bool {
        let padded_size = target_rect
            .shrink(padding_screen)
            .size()
            .max(egui::vec2(1.0, 1.0));
        let width_zoom = padded_size.x / bounds.width().max(1.0);
        let height_zoom = padded_size.y / bounds.height().max(1.0);
        let target_zoom = width_zoom
            .min(height_zoom)
            .clamp(MIN_CANVAS_ZOOM, self.zoom);
        if target_zoom >= self.zoom {
            return false;
        }

        let center_world = bounds.center().to_vec2();
        self.interaction.mouse_wheel_zoom_animation = None;
        self.zoom = target_zoom;
        self.pan = target_rect.center() - canvas_rect.center() - center_world * self.zoom;
        true
    }

    pub fn selected_center_world(&self) -> Option<egui::Pos2> {
        let mut min = egui::Pos2::new(f32::INFINITY, f32::INFINITY);
        let mut max = egui::Pos2::new(f32::NEG_INFINITY, f32::NEG_INFINITY);
        let mut found = false;

        for image in &self.document.rendered_placements {
            if !self.is_selected(image.placement.state.id) {
                continue;
            }

            found = true;
            min.x = min.x.min(image.placement.state.geometry.center.x);
            min.y = min.y.min(image.placement.state.geometry.center.y);
            max.x = max.x.max(image.placement.state.geometry.center.x);
            max.y = max.y.max(image.placement.state.geometry.center.y);
        }

        found.then(|| egui::pos2((min.x + max.x) * 0.5, (min.y + max.y) * 0.5))
    }

    pub fn take_pending_crop(&mut self) -> Option<CanvasCropRequest> {
        self.interaction.pending_crop.take()
    }

    #[cfg(test)]
    pub(crate) fn set_pending_crop_for_test(&mut self, request: CanvasCropRequest) {
        self.interaction.pending_crop = Some(request);
    }

    pub fn take_pending_annotation(&mut self) -> Option<PendingAnnotation> {
        self.interaction.pending_annotation.take()
    }

    pub fn take_context_menu_request(&mut self) -> Option<super::CanvasContextMenuRequest> {
        self.interaction.pending_context_menu_request.take()
    }

    pub fn take_focus_selection_request(&mut self) -> bool {
        let requested = self.interaction.pending_focus_selection_request;
        self.interaction.pending_focus_selection_request = false;
        requested
    }

    pub fn has_pending_crop_work(&self) -> bool {
        self.interaction.pending_crop.is_some()
            || !self.interaction.crop_pending_placement_ids.is_empty()
            || !self.render.pending_crop_previews.is_empty()
    }

    pub(crate) fn clear_guide_snap_indicators(&mut self) {
        self.interaction.guide_snap_indicators.clear();
        self.interaction.guide_snap_signature = 0;
        self.interaction.guide_snap_engaged_at = None;
    }

    pub(crate) fn set_guide_snap_indicators(&mut self, indicators: Vec<super::GuideSnapIndicator>) {
        let signature = guide_snap_signature(&indicators);
        if signature != self.interaction.guide_snap_signature {
            self.interaction.guide_snap_signature = signature;
            self.interaction.guide_snap_engaged_at = if indicators.is_empty() {
                None
            } else {
                Some(std::time::Instant::now())
            };
        }
        self.interaction.guide_snap_indicators = indicators;
    }

    pub(crate) fn guide_snap_pulse_started_at(&self) -> Option<std::time::Instant> {
        self.interaction.guide_snap_engaged_at
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn guide_snap_indicator_count(&self) -> usize {
        self.interaction.guide_snap_indicators.len()
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn guide_snap_mark_count(&self) -> usize {
        self.interaction
            .guide_snap_indicators
            .iter()
            .map(|indicator| indicator.marks.len())
            .sum()
    }

    pub fn compact_guided_layout_for_sizes(
        &mut self,
        sizes: &[egui::Vec2],
        center_world: egui::Pos2,
        screen_rect: egui::Rect,
        gap: f32,
    ) -> Vec<(egui::Pos2, egui::Vec2)> {
        let mut arrangement_inputs =
            crate::placement::arrangement::compact_layout_for_sizes(sizes, gap, center_world);
        self.guide_snap_arrangement_inputs(&mut arrangement_inputs, screen_rect);
        arrangement_inputs
    }

    pub fn balanced_guided_layout_for_sizes(
        &mut self,
        sizes: &[egui::Vec2],
        center_world: egui::Pos2,
        screen_rect: egui::Rect,
        gap: f32,
    ) -> Vec<(egui::Pos2, egui::Vec2)> {
        let mut arrangement_inputs =
            crate::placement::arrangement::balanced_grid_layout_for_sizes(sizes, gap, center_world);
        self.guide_snap_arrangement_inputs(&mut arrangement_inputs, screen_rect);
        arrangement_inputs
    }

    fn guide_snap_arrangement_inputs(
        &mut self,
        arrangement_inputs: &mut [(egui::Pos2, egui::Vec2)],
        screen_rect: egui::Rect,
    ) {
        let items = arrangement_inputs
            .iter()
            .enumerate()
            .map(|(index, (center, size))| {
                PlacementArrangementInput::new(
                    -((index as i64) + 1),
                    egui::pos2(center.x, center.y),
                    egui::vec2(size.x, size.y),
                )
            })
            .collect::<Vec<_>>();
        let Some(bounds) =
            crate::placement::movement::layout_bounds_with_offset(&items, egui::Vec2::ZERO)
        else {
            self.clear_guide_snap_indicators();
            return;
        };
        let Some(snap_result) = crate::placement::movement::compute_guide_snap_for_rect(
            self,
            screen_rect,
            bounds,
            &[],
            false,
        ) else {
            self.clear_guide_snap_indicators();
            return;
        };
        let snap_offset = snap_result.rect.center() - bounds.center();
        if snap_offset == egui::Vec2::ZERO {
            self.clear_guide_snap_indicators();
            return;
        }
        self.set_guide_snap_indicators(snap_result.indicators);
        for (center, _) in arrangement_inputs {
            *center += snap_offset;
        }
    }

    pub fn take_pending_image_lod_loads(&mut self) -> Vec<(i64, CanvasImageLod)> {
        self.render.pending_image_lod_loads.drain().collect()
    }

    pub fn mark_crop_pending(&mut self, placement_id: i64) {
        if !self
            .interaction
            .crop_pending_placement_ids
            .contains(&placement_id)
        {
            self.interaction
                .crop_pending_placement_ids
                .push(placement_id);
        }
    }

    pub fn clear_crop_pending(&mut self, placement_id: i64) {
        if let Some(preview) = self.render.pending_crop_previews.remove(&placement_id)
            && let Some(image) = self
                .document
                .rendered_placements
                .iter_mut()
                .find(|image| image.placement.state.id == placement_id)
        {
            *image = preview.image;
        }
        self.interaction
            .crop_pending_placement_ids
            .retain(|pending_id| *pending_id != placement_id);
    }

    pub fn clear_pending_image_lod_load(&mut self, placement_id: i64) {
        self.render.pending_image_lod_loads.remove(&placement_id);
    }

    pub fn evict_cached_asset(&mut self, asset_id: i64) {
        self.render.asset_rgba_cache.remove(&asset_id);
    }

    pub fn prune_cached_pixels_to_selected_items(&mut self) {
        let selected_asset_ids = self
            .document
            .rendered_placements
            .iter()
            .filter(|image| {
                self.document
                    .selected_placement_ids
                    .contains(&image.placement.state.id)
            })
            .filter_map(|image| image.placement.asset.as_ref().map(|asset| asset.id))
            .collect::<Vec<_>>();
        self.render
            .asset_rgba_cache
            .retain(|asset_id, _| selected_asset_ids.contains(asset_id));
    }

    pub fn add_annotation(&mut self, annotation: CanvasAnnotation) {
        self.invalidate_textbox_layout_cache(annotation.id);
        self.document
            .annotations
            .retain(|item| item.id != annotation.id);
        self.document.annotations.push(annotation.clone());
        self.append_missing_stack_item(CanvasStackItem::Annotation(annotation.id));
        if annotation.kind == CanvasAnnotationKind::Textbox
            && self.interaction.editing_text_annotation_id.is_none()
        {
            self.document.selected_annotation_ids = vec![annotation.id];
        }
    }

    pub fn begin_textbox_edit(&mut self, annotation_id: i64) {
        if self.document.annotations.iter().any(|annotation| {
            annotation.id == annotation_id && annotation.kind == CanvasAnnotationKind::Textbox
        }) {
            self.document.selected_annotation_ids = vec![annotation_id];
            self.interaction.editing_text_annotation_id = Some(annotation_id);
            self.interaction.focus_text_editor = true;
            self.document.selected_placement_ids.clear();
            self.interaction.placement_transform_state = None;
            self.interaction.textbox_transform_state = None;
            self.interaction.annotation_drag_state = None;
        }
    }

    pub fn add_annotations(&mut self, annotations: Vec<CanvasAnnotation>) {
        self.document.annotations = annotations;
        for annotation in self.document.annotations.clone() {
            self.append_missing_stack_item(CanvasStackItem::Annotation(annotation.id));
        }
        self.render.textbox_layout_cache.clear();
    }

    pub fn apply_annotation_history_state(&mut self, annotation: Option<CanvasAnnotation>) {
        let Some(annotation) = annotation else {
            return;
        };
        if annotation.archived {
            self.remove_annotation_locally(annotation.id);
        } else {
            self.add_annotation(annotation);
        }
    }

    pub fn apply_placement_history_state(
        &mut self,
        placement: Option<Placement>,
    ) -> Option<Placement> {
        let Some(placement) = placement else {
            return None;
        };
        if placement.state.archived {
            self.delete_placements(&[placement.state.id]);
            return None;
        }
        if let Some(image) = self.rendered_placement_by_id_mut(placement.state.id) {
            image.placement = placement;
            self.document
                .rendered_placements
                .sort_by_key(|image| image.placement.state.geometry.z_index);
            return None;
        }
        if self
            .add_loaded_reference_placement(placement.clone())
            .is_ok()
        {
            return None;
        }
        self.add_unresolved_placement(placement.clone());
        Some(placement)
    }

    pub fn set_canvas_stack_order(&mut self, stack_order: Vec<CanvasStackItem>) {
        if self.document.stack_order != stack_order {
            self.document.stack_order = stack_order;
            self.document.stack_order_dirty = true;
        }
    }

    pub fn restore_canvas_stack_order(&mut self, stack_order: Vec<CanvasStackItem>) {
        self.document.stack_order = stack_order;
        self.document.stack_order_dirty = false;
    }

    pub fn canvas_stack_order(&self) -> Vec<CanvasStackItem> {
        self.ordered_canvas_items_back_to_front()
    }

    pub fn clear_for_board_switch(&mut self) {
        self.finish_textbox_edit();
        self.document.rendered_placements.clear();
        self.document.annotations.clear();
        self.document.stack_order.clear();
        self.document.selected_placement_ids.clear();
        self.document.selected_annotation_ids.clear();
        self.document.dirty_placement_ids.clear();
        self.document.dirty_annotation_ids.clear();
        self.document.pending_archived_annotation_ids.clear();
        self.document.stack_order_dirty = false;
        self.interaction = Default::default();
        self.render.asset_rgba_cache.clear();
        self.render.textbox_layout_cache.clear();
        self.render.pending_image_lod_loads.clear();
    }

    pub fn take_document_changes(&mut self) -> CanvasDocumentChanges {
        let dirty_ids = std::mem::take(&mut self.document.dirty_placement_ids);
        let updated_placements = dirty_ids
            .into_iter()
            .filter_map(|id| {
                self.document
                    .rendered_placements
                    .iter()
                    .find(|image| {
                        image.placement.state.id == id
                            && image.placement.lifecycle == PlacementLifecycle::Committed
                    })
                    .map(|image| image.placement.state.clone())
            })
            .collect();

        let dirty_ids = std::mem::take(&mut self.document.dirty_annotation_ids);
        let updated_annotations = dirty_ids
            .into_iter()
            .filter_map(|id| {
                self.document
                    .annotations
                    .iter()
                    .find(|annotation| annotation.id == id)
                    .cloned()
            })
            .collect();

        let stack_order = self
            .document
            .stack_order_dirty
            .then(|| self.committed_canvas_items_back_to_front());
        self.document.stack_order_dirty = false;

        CanvasDocumentChanges {
            updated_placements,
            updated_annotations,
            archived_annotation_ids: std::mem::take(
                &mut self.document.pending_archived_annotation_ids,
            ),
            stack_order,
        }
    }

    pub fn pending_document_changes_preview(&self) -> CanvasDocumentChanges {
        let updated_placements = self
            .document
            .dirty_placement_ids
            .iter()
            .filter_map(|id| {
                self.document
                    .rendered_placements
                    .iter()
                    .find(|image| {
                        image.placement.state.id == *id
                            && image.placement.lifecycle == PlacementLifecycle::Committed
                    })
                    .map(|image| image.placement.state.clone())
            })
            .collect();

        let updated_annotations = self
            .document
            .dirty_annotation_ids
            .iter()
            .filter_map(|id| {
                self.document
                    .annotations
                    .iter()
                    .find(|annotation| annotation.id == *id)
                    .cloned()
            })
            .collect();

        CanvasDocumentChanges {
            updated_placements,
            updated_annotations,
            archived_annotation_ids: self.document.pending_archived_annotation_ids.clone(),
            stack_order: self
                .document
                .stack_order_dirty
                .then(|| self.committed_canvas_items_back_to_front()),
        }
    }

    pub fn selection_screen_rect(&self, rect: egui::Rect) -> Option<egui::Rect> {
        let mut selected_rect: Option<egui::Rect> = None;

        for image in &self.document.rendered_placements {
            if !self.is_selected(image.placement.state.id) {
                continue;
            }

            let image_rect = placement_screen_rect(self, rect, image);
            selected_rect = Some(match selected_rect {
                Some(rect) => rect.union(image_rect),
                None => image_rect,
            });
        }

        for annotation in self.selected_annotations() {
            let annotation_rect = annotation_screen_rect(self, rect, annotation);
            selected_rect = Some(match selected_rect {
                Some(existing) => existing.union(annotation_rect),
                None => annotation_rect,
            });
        }

        selected_rect
    }

    pub fn selected_bounds_world(&self) -> Option<egui::Rect> {
        let mut selected_rect: Option<egui::Rect> = None;

        for image in &self.document.rendered_placements {
            if !self.is_selected(image.placement.state.id) {
                continue;
            }

            let size = image.placement.scaled_display_size();
            let image_rect = egui::Rect::from_center_size(
                egui::pos2(
                    image.placement.state.geometry.center.x,
                    image.placement.state.geometry.center.y,
                ),
                size,
            );
            selected_rect = Some(match selected_rect {
                Some(rect) => rect.union(image_rect),
                None => image_rect,
            });
        }

        selected_rect
    }

    pub fn editing_textbox_size(&self) -> Option<CanvasTextSize> {
        let annotation_id = self.interaction.editing_text_annotation_id?;
        self.annotation_by_id(annotation_id)
            .filter(|annotation| annotation.kind == CanvasAnnotationKind::Textbox)
            .map(|annotation| annotation.text_size)
    }

    pub fn editing_textbox_color(&self) -> Option<CanvasAnnotationColor> {
        let annotation_id = self.interaction.editing_text_annotation_id?;
        self.annotation_by_id(annotation_id)
            .filter(|annotation| annotation.kind == CanvasAnnotationKind::Textbox)
            .map(|annotation| annotation.color)
    }

    pub fn editing_textbox_background_enabled(&self) -> Option<bool> {
        let annotation_id = self.interaction.editing_text_annotation_id?;
        self.annotation_by_id(annotation_id)
            .filter(|annotation| annotation.kind == CanvasAnnotationKind::Textbox)
            .map(|annotation| annotation.background_enabled)
    }

    pub fn selected_stroke_width(&self) -> Option<CanvasStrokeWidth> {
        let annotation = self.selected_annotation()?;
        matches!(
            annotation.kind,
            CanvasAnnotationKind::Line | CanvasAnnotationKind::Arrow
        )
        .then_some(annotation.stroke_width)
    }

    pub fn selected_annotation_color(&self) -> Option<CanvasAnnotationColor> {
        self.selected_annotation()
            .map(|annotation| annotation.color)
    }

    pub fn apply_text_size_to_editing_textbox(&mut self, text_size: CanvasTextSize) -> bool {
        let Some(annotation_id) = self.interaction.editing_text_annotation_id else {
            return false;
        };
        let Some(annotation) = self.annotation_by_id_mut(annotation_id) else {
            return false;
        };
        if annotation.kind != CanvasAnnotationKind::Textbox || annotation.text_size == text_size {
            return false;
        }

        annotation.text_size = text_size;
        self.invalidate_textbox_layout_cache(annotation_id);
        self.mark_annotation_dirty(annotation_id);
        self.interaction.focus_text_editor = true;
        true
    }

    pub fn apply_color_to_editing_textbox(&mut self, color: CanvasAnnotationColor) -> bool {
        let Some(annotation_id) = self.interaction.editing_text_annotation_id else {
            return false;
        };
        let Some(annotation) = self.annotation_by_id_mut(annotation_id) else {
            return false;
        };
        if annotation.kind != CanvasAnnotationKind::Textbox || annotation.color == color {
            return false;
        }

        annotation.color = color;
        self.invalidate_textbox_layout_cache(annotation_id);
        self.mark_annotation_dirty(annotation_id);
        self.interaction.focus_text_editor = true;
        true
    }

    pub fn toggle_editing_textbox_background(&mut self) -> bool {
        let Some(annotation_id) = self.interaction.editing_text_annotation_id else {
            return false;
        };
        let Some(annotation) = self.annotation_by_id_mut(annotation_id) else {
            return false;
        };
        if annotation.kind != CanvasAnnotationKind::Textbox {
            return false;
        }

        annotation.background_enabled = !annotation.background_enabled;
        self.mark_annotation_dirty(annotation_id);
        self.interaction.focus_text_editor = true;
        true
    }

    pub fn apply_stroke_width_to_selected_annotation(
        &mut self,
        stroke_width: CanvasStrokeWidth,
    ) -> bool {
        let Some(annotation_id) = self.single_selected_annotation_id() else {
            return false;
        };
        let Some(annotation) = self.annotation_by_id_mut(annotation_id) else {
            return false;
        };
        if !matches!(
            annotation.kind,
            CanvasAnnotationKind::Line | CanvasAnnotationKind::Arrow
        ) || annotation.stroke_width == stroke_width
        {
            return false;
        }

        annotation.stroke_width = stroke_width;
        self.mark_annotation_dirty(annotation_id);
        true
    }

    pub fn apply_color_to_selected_annotation(&mut self, color: CanvasAnnotationColor) -> bool {
        let Some(annotation_id) = self.single_selected_annotation_id() else {
            return false;
        };
        let Some(annotation) = self.annotation_by_id_mut(annotation_id) else {
            return false;
        };
        if annotation.color == color {
            return false;
        }

        annotation.color = color;
        if annotation.kind == CanvasAnnotationKind::Textbox {
            self.invalidate_textbox_layout_cache(annotation_id);
        }
        self.mark_annotation_dirty(annotation_id);
        true
    }

    #[cfg_attr(not(debug_assertions), allow(dead_code))]
    pub fn select_placements(&mut self, placement_ids: &[i64]) {
        self.finish_textbox_edit();
        self.document.selected_placement_ids = placement_ids
            .iter()
            .copied()
            .filter(|placement_id| {
                self.document
                    .rendered_placements
                    .iter()
                    .any(|image| image.placement.state.id == *placement_id)
            })
            .collect();
        self.document.selected_annotation_ids.clear();
        self.interaction.editing_text_annotation_id = None;
        self.interaction.placement_transform_state = None;
        self.interaction.textbox_transform_state = None;
        self.interaction.annotation_drag_state = None;
        self.prune_cached_pixels_to_selected_items();
    }

    pub fn select_content(&mut self, placement_ids: &[i64], annotation_ids: &[i64]) {
        self.finish_textbox_edit();
        self.document.selected_placement_ids = placement_ids
            .iter()
            .copied()
            .filter(|placement_id| {
                self.document
                    .rendered_placements
                    .iter()
                    .any(|image| image.placement.state.id == *placement_id)
            })
            .collect();
        self.document.selected_annotation_ids = annotation_ids
            .iter()
            .copied()
            .filter(|annotation_id| {
                self.document
                    .annotations
                    .iter()
                    .any(|annotation| annotation.id == *annotation_id)
            })
            .collect();
        self.interaction.editing_text_annotation_id = None;
        self.interaction.placement_transform_state = None;
        self.interaction.textbox_transform_state = None;
        self.interaction.annotation_drag_state = None;
        self.prune_cached_pixels_to_selected_items();
    }

    pub fn select_all_content(&mut self) {
        let placement_ids = self.all_placement_ids();
        let annotation_ids = self.all_annotation_ids();
        self.select_content(&placement_ids, &annotation_ids);
    }

    pub fn arrange_selected_horizontal(&mut self, gap: f32) -> usize {
        let mut items = self.selected_arrangement_inputs();
        if items.len() <= 1 {
            return items.len();
        }

        items.sort_by(|left, right| left.center().x.total_cmp(&right.center().x));
        let current_center = self.selected_center_world().unwrap_or(egui::Pos2::ZERO);
        let Some((placements, total_width, total_height)) =
            crate::placement::arrangement::horizontal_layout(&items, gap)
        else {
            return 0;
        };
        let local_center = egui::pos2(total_width * 0.5, total_height * 0.5);
        self.apply_arrangement(
            placements,
            current_center.to_vec2() - local_center.to_vec2(),
        )
    }

    pub fn arrange_selected_vertical(&mut self, gap: f32) -> usize {
        let mut items = self.selected_arrangement_inputs();
        if items.len() <= 1 {
            return items.len();
        }

        items.sort_by(|left, right| left.center().y.total_cmp(&right.center().y));
        let current_center = self.selected_center_world().unwrap_or(egui::Pos2::ZERO);
        let Some((placements, total_width, total_height)) =
            crate::placement::arrangement::vertical_layout(&items, gap)
        else {
            return 0;
        };
        let local_center = egui::pos2(total_width * 0.5, total_height * 0.5);
        self.apply_arrangement(
            placements,
            current_center.to_vec2() - local_center.to_vec2(),
        )
    }

    pub fn arrange_selected_compact(&mut self, gap: f32) -> usize {
        let items = self.selected_compact_input_items();
        if items.len() <= 1 {
            return items.len();
        }

        let current_center = self
            .source_center_for_arrangement_inputs(&items)
            .unwrap_or(egui::Pos2::ZERO);

        let Some(layout) = crate::placement::arrangement::best_user_compact_layout(&items, gap)
        else {
            return 0;
        };
        let local_center = egui::pos2(layout.width * 0.5, layout.height * 0.5);
        self.apply_compact_arrangement(
            layout.placements,
            layout.scale_factors,
            current_center.to_vec2() - local_center.to_vec2(),
        )
    }

    pub fn arrange_selected_compact_preserving_sizes(&mut self, gap: f32) -> usize {
        let items = self.selected_arrangement_inputs();
        if items.len() <= 1 {
            return items.len();
        }

        let current_center = self
            .source_center_for_arrangement_inputs(&items)
            .unwrap_or(egui::Pos2::ZERO);
        let Some((placements, width, height)) =
            crate::placement::arrangement::best_compact_layout(&items, gap)
        else {
            return 0;
        };
        let local_center = egui::pos2(width * 0.5, height * 0.5);
        self.apply_arrangement_if_changed(
            placements,
            current_center.to_vec2() - local_center.to_vec2(),
        )
    }

    pub fn settle_selected(
        &mut self,
        direction: crate::placement::arrangement::SettleDirection,
        gap: f32,
    ) -> usize {
        let items = self.selected_arrangement_inputs();
        if items.len() <= 1 {
            return items.len();
        }

        let cross_axis_grace = crate::placement::arrangement::SETTLE_BLOCK_GRACE_SCREEN
            / self.zoom.max(super::MIN_CANVAS_ZOOM);
        let Some(placements) = crate::placement::arrangement::directional_settle_layout(
            &items,
            direction,
            gap,
            cross_axis_grace,
        ) else {
            return 0;
        };
        self.apply_arrangement_if_changed(placements, egui::Vec2::ZERO)
    }

    pub fn move_selected_placement_group_to_center(
        &mut self,
        center_world: egui::Pos2,
        screen_rect: egui::Rect,
        now_seconds: f64,
    ) -> usize {
        crate::placement::movement::move_selected_placement_group_to_center(
            self,
            center_world,
            screen_rect,
            now_seconds,
        )
    }

    #[cfg(test)]
    pub fn selection_border_visible_for_test(&self, now_seconds: f64) -> bool {
        let Some(drag_state) = &self.interaction.drag_state else {
            return true;
        };
        !drag_state.moved
            || now_seconds - drag_state.last_movement_seconds
                >= super::SELECTION_BORDER_REAPPEAR_SECONDS
    }

    pub fn move_selected_canvas_items_to_front(&mut self) -> bool {
        self.move_selected_canvas_items(StackOrderMove::Front)
    }

    pub fn move_selected_canvas_items_to_back(&mut self) -> bool {
        self.move_selected_canvas_items(StackOrderMove::Back)
    }

    #[cfg_attr(not(debug_assertions), allow(dead_code))]
    pub fn delete_selected_placements(&mut self) -> Vec<i64> {
        let selected_ids = self.document.selected_placement_ids.clone();
        self.delete_placements(&selected_ids)
    }

    pub fn delete_selected_annotations(&mut self) -> Vec<i64> {
        let selected_ids = self.document.selected_annotation_ids.clone();
        if selected_ids.is_empty() {
            return Vec::new();
        }
        for annotation_id in &selected_ids {
            self.remove_annotation_locally(*annotation_id);
        }
        selected_ids
    }

    #[cfg_attr(not(debug_assertions), allow(dead_code))]
    pub fn delete_placements(&mut self, placement_ids: &[i64]) -> Vec<i64> {
        if placement_ids.is_empty() {
            return Vec::new();
        }

        let mut removed_ids = Vec::new();
        self.document.rendered_placements.retain(|image| {
            let keep = !placement_ids.contains(&image.placement.state.id);
            if !keep {
                removed_ids.push(image.placement.state.id);
            }
            keep
        });
        self.document
            .selected_placement_ids
            .retain(|placement_id| !removed_ids.contains(placement_id));
        self.document.stack_order.retain(|item| {
            !matches!(item, CanvasStackItem::Placement(placement_id) if removed_ids.contains(placement_id))
        });
        self.document
            .dirty_placement_ids
            .retain(|placement_id| !removed_ids.contains(placement_id));
        self.interaction.drag_state = None;
        self.interaction.placement_transform_state = None;
        self.interaction.textbox_transform_state = None;
        self.interaction.annotation_drag_state = None;
        self.interaction.marquee_state = None;
        self.interaction.crop_state = None;
        self.interaction.annotation_draft_state = None;
        self.interaction
            .crop_pending_placement_ids
            .retain(|placement_id| !removed_ids.contains(placement_id));
        self.render
            .pending_image_lod_loads
            .retain(|placement_id, _| !removed_ids.contains(placement_id));
        self.interaction.pending_crop = None;
        self.prune_cached_pixels_to_selected_items();
        removed_ids
    }

    pub fn remove_assets(&mut self, asset_ids: &[i64]) -> Vec<i64> {
        if asset_ids.is_empty() {
            return Vec::new();
        }

        let placement_ids = self
            .document
            .rendered_placements
            .iter()
            .filter(|image| asset_ids.contains(&image.placement.require_loaded_asset().id))
            .map(|image| image.placement.state.id)
            .collect::<Vec<_>>();
        self.delete_placements(&placement_ids)
    }

    pub(super) fn is_selected(&self, placement_id: i64) -> bool {
        self.document.selected_placement_ids.contains(&placement_id)
    }

    pub(super) fn request_image_lod_load(&mut self, placement_id: i64, lod: CanvasImageLod) {
        let Some(image) = self.rendered_placement_by_id(placement_id) else {
            return;
        };
        if lod <= image.loaded_lod {
            return;
        }
        match self
            .render
            .pending_image_lod_loads
            .get(&placement_id)
            .copied()
        {
            Some(pending) if pending >= lod => {}
            _ => {
                self.render
                    .pending_image_lod_loads
                    .insert(placement_id, lod);
            }
        }
    }

    pub(super) fn set_single_selection(&mut self, placement_id: i64) {
        self.finish_textbox_edit();
        self.document.selected_placement_ids.clear();
        self.document.selected_placement_ids.push(placement_id);
        self.document.selected_annotation_ids.clear();
        self.interaction.editing_text_annotation_id = None;
        self.interaction.placement_transform_state = None;
        self.interaction.textbox_transform_state = None;
        self.interaction.annotation_drag_state = None;
        self.prune_cached_pixels_to_selected_items();
    }

    pub(super) fn add_to_selection(&mut self, placement_id: i64) {
        self.finish_textbox_edit();
        if !self.document.selected_placement_ids.contains(&placement_id) {
            self.document.selected_placement_ids.push(placement_id);
        }
        self.interaction.placement_transform_state = None;
        self.interaction.textbox_transform_state = None;
        self.interaction.annotation_drag_state = None;
        self.prune_cached_pixels_to_selected_items();
    }

    pub(super) fn add_annotation_to_selection(&mut self, annotation_id: i64) {
        self.finish_textbox_edit();
        if !self
            .document
            .selected_annotation_ids
            .contains(&annotation_id)
        {
            self.document.selected_annotation_ids.push(annotation_id);
        }
        self.interaction.placement_transform_state = None;
        self.interaction.textbox_transform_state = None;
        self.interaction.annotation_drag_state = None;
        self.prune_cached_pixels_to_selected_items();
    }

    pub(super) fn clear_selection(&mut self) {
        self.finish_textbox_edit();
        self.document.selected_placement_ids.clear();
        self.document.selected_annotation_ids.clear();
        self.interaction.editing_text_annotation_id = None;
        self.interaction.placement_transform_state = None;
        self.interaction.textbox_transform_state = None;
        self.interaction.annotation_drag_state = None;
        self.prune_cached_pixels_to_selected_items();
    }

    pub(super) fn set_marquee_selection(
        &mut self,
        placement_ids: Vec<i64>,
        annotation_ids: Vec<i64>,
        additive: bool,
    ) {
        self.finish_textbox_edit();
        self.document.selected_placement_ids = if additive {
            merge_ids(&self.document.selected_placement_ids, &placement_ids)
        } else {
            placement_ids
        };
        self.document.selected_annotation_ids = if additive {
            merge_ids(&self.document.selected_annotation_ids, &annotation_ids)
        } else {
            annotation_ids
        };
        self.interaction.editing_text_annotation_id = None;
        self.interaction.placement_transform_state = None;
        self.interaction.textbox_transform_state = None;
        self.interaction.annotation_drag_state = None;
        self.prune_cached_pixels_to_selected_items();
    }

    pub(super) fn mark_dirty(&mut self, placement_id: i64) {
        if placement_id > 0 && !self.document.dirty_placement_ids.contains(&placement_id) {
            self.document.dirty_placement_ids.push(placement_id);
        }
    }

    pub(super) fn mark_annotation_dirty(&mut self, annotation_id: i64) {
        if annotation_id > 0 && !self.document.dirty_annotation_ids.contains(&annotation_id) {
            self.document.dirty_annotation_ids.push(annotation_id);
        }
    }

    pub(super) fn remove_annotation_locally(&mut self, annotation_id: i64) {
        self.document
            .annotations
            .retain(|annotation| annotation.id != annotation_id);
        self.document
            .stack_order
            .retain(|item| *item != CanvasStackItem::Annotation(annotation_id));
        self.invalidate_textbox_layout_cache(annotation_id);
        self.document
            .selected_annotation_ids
            .retain(|selected_id| *selected_id != annotation_id);
        if self.interaction.editing_text_annotation_id == Some(annotation_id) {
            self.interaction.editing_text_annotation_id = None;
        }
        self.interaction.focus_text_editor = false;
        self.interaction.placement_transform_state = None;
        if self.interaction.textbox_transform_state.is_some() {
            self.interaction.textbox_transform_state = None;
            self.interaction.annotation_drag_state = None;
        }
        self.document
            .dirty_annotation_ids
            .retain(|dirty_id| *dirty_id != annotation_id);
    }

    pub(super) fn finish_textbox_edit(&mut self) {
        let Some(annotation_id) = self.interaction.editing_text_annotation_id else {
            self.interaction.focus_text_editor = false;
            return;
        };

        self.interaction.editing_text_annotation_id = None;
        self.interaction.focus_text_editor = false;
        self.interaction.placement_transform_state = None;
        self.interaction.textbox_transform_state = None;
        self.interaction.annotation_drag_state = None;

        let Some(annotation) = self
            .document
            .annotations
            .iter()
            .find(|annotation| annotation.id == annotation_id)
        else {
            self.document
                .selected_annotation_ids
                .retain(|selected_id| *selected_id != annotation_id);
            return;
        };

        if annotation.kind == CanvasAnnotationKind::Textbox && annotation.text.trim().is_empty() {
            self.remove_annotation_locally(annotation_id);
            if annotation_id > 0
                && !self
                    .document
                    .pending_archived_annotation_ids
                    .contains(&annotation_id)
            {
                self.document
                    .pending_archived_annotation_ids
                    .push(annotation_id);
            }
        }
    }

    pub(super) fn stop_textbox_edit_without_cleanup(&mut self) {
        self.interaction.editing_text_annotation_id = None;
        self.interaction.focus_text_editor = false;
        self.interaction.placement_transform_state = None;
        self.interaction.textbox_transform_state = None;
        self.interaction.annotation_drag_state = None;
    }

    pub(super) fn annotation_by_id(&self, annotation_id: i64) -> Option<&CanvasAnnotation> {
        self.document
            .annotations
            .iter()
            .find(|annotation| annotation.id == annotation_id)
    }

    pub(super) fn ordered_canvas_items_back_to_front(&self) -> Vec<CanvasStackItem> {
        let mut ordered = Vec::new();
        for item in &self.document.stack_order {
            if self.stack_item_exists(*item) && !ordered.contains(item) {
                ordered.push(*item);
            }
        }
        for image in &self.document.rendered_placements {
            let item = CanvasStackItem::Placement(image.placement.state.id);
            if !ordered.contains(&item) {
                ordered.push(item);
            }
        }
        for annotation in &self.document.annotations {
            let item = CanvasStackItem::Annotation(annotation.id);
            if !ordered.contains(&item) {
                ordered.push(item);
            }
        }
        ordered
    }

    fn committed_canvas_items_back_to_front(&self) -> Vec<CanvasStackItem> {
        self.ordered_canvas_items_back_to_front()
            .into_iter()
            .filter(|item| match item {
                CanvasStackItem::Placement(placement_id) => self
                    .rendered_placement_by_id(*placement_id)
                    .is_some_and(|image| {
                        image.placement.lifecycle == PlacementLifecycle::Committed
                    }),
                CanvasStackItem::Annotation(_) => true,
            })
            .collect()
    }

    fn append_missing_stack_item(&mut self, item: CanvasStackItem) {
        if !self.document.stack_order.contains(&item) {
            self.document.stack_order.push(item);
        }
    }

    fn stack_item_exists(&self, item: CanvasStackItem) -> bool {
        match item {
            CanvasStackItem::Placement(placement_id) => self
                .document
                .rendered_placements
                .iter()
                .any(|image| image.placement.state.id == placement_id),
            CanvasStackItem::Annotation(annotation_id) => self
                .document
                .annotations
                .iter()
                .any(|annotation| annotation.id == annotation_id),
        }
    }

    fn move_selected_canvas_items(&mut self, direction: StackOrderMove) -> bool {
        let selected = self.selected_canvas_stack_items();
        if selected.is_empty() {
            return false;
        }

        let ordered = self.ordered_canvas_items_back_to_front();
        let mut unmoved = Vec::with_capacity(ordered.len().saturating_sub(selected.len()));
        let mut moved = Vec::with_capacity(selected.len());
        for item in ordered {
            if selected.contains(&item) {
                moved.push(item);
            } else {
                unmoved.push(item);
            }
        }

        let next_order = match direction {
            StackOrderMove::Front => {
                unmoved.extend(moved);
                unmoved
            }
            StackOrderMove::Back => {
                moved.extend(unmoved);
                moved
            }
        };

        if next_order == self.document.stack_order {
            return false;
        }
        self.document.stack_order = next_order;
        self.document.stack_order_dirty = true;
        true
    }

    fn selected_canvas_stack_items(&self) -> Vec<CanvasStackItem> {
        self.document
            .selected_placement_ids
            .iter()
            .copied()
            .map(CanvasStackItem::Placement)
            .chain(
                self.document
                    .selected_annotation_ids
                    .iter()
                    .copied()
                    .map(CanvasStackItem::Annotation),
            )
            .filter(|item| self.stack_item_exists(*item))
            .collect()
    }

    pub(super) fn annotation_by_id_mut(
        &mut self,
        annotation_id: i64,
    ) -> Option<&mut CanvasAnnotation> {
        self.document
            .annotations
            .iter_mut()
            .find(|annotation| annotation.id == annotation_id)
    }

    pub(super) fn set_textbox_world_rect(
        &mut self,
        annotation_id: i64,
        world_rect: egui::Rect,
    ) -> bool {
        let normalized = normalized_textbox_rect(world_rect.min, world_rect.max);
        let Some(annotation) = self.annotation_by_id_mut(annotation_id) else {
            return false;
        };
        if annotation.kind != CanvasAnnotationKind::Textbox {
            return false;
        }

        let changed = annotation.start().x != normalized.min.x
            || annotation.start().y != normalized.min.y
            || annotation.end().x != normalized.max.x
            || annotation.end().y != normalized.max.y;
        if changed {
            annotation.geometry = crate::data::CanvasAnnotationGeometry::from_kind(
                annotation.kind,
                normalized.min,
                normalized.max,
            );
            self.invalidate_textbox_layout_cache(annotation_id);
            self.mark_annotation_dirty(annotation_id);
        }
        changed
    }

    pub(super) fn set_annotation_world_points(
        &mut self,
        annotation_id: i64,
        start_world: egui::Pos2,
        end_world: egui::Pos2,
    ) -> bool {
        let Some(annotation) = self.annotation_by_id_mut(annotation_id) else {
            return false;
        };
        let changed = annotation.start().x != start_world.x
            || annotation.start().y != start_world.y
            || annotation.end().x != end_world.x
            || annotation.end().y != end_world.y;
        if changed {
            annotation.geometry = crate::data::CanvasAnnotationGeometry::from_kind(
                annotation.kind,
                start_world,
                end_world,
            );
            if annotation.kind == CanvasAnnotationKind::Textbox {
                self.invalidate_textbox_layout_cache(annotation_id);
            }
        }
        changed
    }

    pub(super) fn selected_textbox_start_rects(&self) -> Vec<(i64, crate::data::RectGeometry)> {
        self.document
            .selected_annotation_ids
            .iter()
            .filter_map(|annotation_id| {
                self.annotation_by_id(*annotation_id)
                    .and_then(|annotation| {
                        (annotation.kind == CanvasAnnotationKind::Textbox).then_some((
                            *annotation_id,
                            crate::data::RectGeometry::from_bounds(
                                textbox_world_rect(annotation),
                                0,
                            ),
                        ))
                    })
            })
            .collect()
    }

    pub(super) fn invalidate_textbox_layout_cache(&mut self, annotation_id: i64) {
        self.render.textbox_layout_cache.remove(&annotation_id);
    }

    pub(super) fn cache_textbox_layout(
        &mut self,
        annotation_id: i64,
        text: &str,
        text_size: CanvasTextSize,
        color: CanvasAnnotationColor,
        wrap_width: f32,
        zoom: f32,
        galley: Arc<egui::Galley>,
    ) -> TextboxLayoutCacheEntry {
        let key = textbox_layout_cache_key(text, text_size, color, wrap_width, zoom);
        let entry = TextboxLayoutCacheEntry {
            key,
            required_world_height: textbox_required_world_height(galley.size().y, zoom),
            galley,
        };
        self.render
            .textbox_layout_cache
            .insert(annotation_id, entry.clone());
        entry
    }

    pub(super) fn cached_textbox_layout(
        &mut self,
        ui: &egui::Ui,
        annotation_id: i64,
        text: &str,
        text_size: CanvasTextSize,
        color: CanvasAnnotationColor,
        wrap_width: f32,
        zoom: f32,
    ) -> TextboxLayoutCacheEntry {
        let key = textbox_layout_cache_key(text, text_size, color, wrap_width, zoom);
        if let Some(entry) = self.render.textbox_layout_cache.get(&annotation_id)
            && entry.key == key
        {
            return entry.clone();
        }

        let galley = layout_textbox_galley(ui, text, text_size, color, wrap_width, zoom);
        self.cache_textbox_layout(
            annotation_id,
            text,
            text_size,
            color,
            wrap_width,
            zoom,
            galley,
        )
    }

    pub(crate) fn selected_arrangement_inputs(&self) -> Vec<PlacementArrangementInput> {
        self.document
            .rendered_placements
            .iter()
            .filter(|image| self.is_selected(image.placement.state.id))
            .map(|image| {
                let display_size = image.placement.state.geometry.scaled_size();
                let rotated_display_size = crate::data::rotated_display_size(
                    display_size,
                    image.placement.state.geometry.rotation,
                );
                PlacementArrangementInput::new(
                    image.placement.state.id,
                    egui::pos2(
                        image.placement.state.geometry.center.x,
                        image.placement.state.geometry.center.y,
                    ),
                    rotated_display_size,
                )
            })
            .collect()
    }

    fn selected_compact_input_items(&self) -> Vec<PlacementArrangementInput> {
        self.document
            .rendered_placements
            .iter()
            .filter(|image| self.is_selected(image.placement.state.id))
            .map(|image| image.placement.compact_arrangement_input())
            .collect()
    }

    fn source_center_for_arrangement_inputs(
        &self,
        items: &[PlacementArrangementInput],
    ) -> Option<egui::Pos2> {
        layout_bounds_with_offset(items, egui::Vec2::ZERO).map(|bounds| bounds.center())
    }

    pub(super) fn apply_arrangement(
        &mut self,
        placements: Vec<(i64, egui::Pos2)>,
        offset: egui::Vec2,
    ) -> usize {
        let mut arranged_ids = Vec::with_capacity(placements.len());

        for (placement_id, local_center) in placements {
            if let Some(image) = self
                .document
                .rendered_placements
                .iter_mut()
                .find(|image| image.placement.state.id == placement_id)
            {
                let world_center = local_center + offset;
                image.placement.state.geometry.center.x = world_center.x;
                image.placement.state.geometry.center.y = world_center.y;
                arranged_ids.push(placement_id);
            }
        }

        for placement_id in &arranged_ids {
            self.mark_dirty(*placement_id);
        }

        arranged_ids.len()
    }

    fn apply_arrangement_if_changed(
        &mut self,
        placements: Vec<(i64, egui::Pos2)>,
        offset: egui::Vec2,
    ) -> usize {
        let mut arranged_ids = Vec::with_capacity(placements.len());

        for (placement_id, local_center) in placements {
            if let Some(image) = self
                .document
                .rendered_placements
                .iter_mut()
                .find(|image| image.placement.state.id == placement_id)
            {
                let world_center = local_center + offset;
                if (image.placement.state.geometry.center.x - world_center.x).abs() <= 0.001
                    && (image.placement.state.geometry.center.y - world_center.y).abs() <= 0.001
                {
                    continue;
                }
                image.placement.state.geometry.center.x = world_center.x;
                image.placement.state.geometry.center.y = world_center.y;
                arranged_ids.push(placement_id);
            }
        }

        for placement_id in &arranged_ids {
            self.mark_dirty(*placement_id);
        }

        arranged_ids.len()
    }

    fn apply_compact_arrangement(
        &mut self,
        placements: Vec<(i64, egui::Pos2)>,
        scale_factors: Vec<(i64, f32)>,
        offset: egui::Vec2,
    ) -> usize {
        let mut changed_ids = Vec::with_capacity(placements.len());

        for (placement_id, local_center) in placements {
            if let Some(image) = self
                .document
                .rendered_placements
                .iter_mut()
                .find(|image| image.placement.state.id == placement_id)
            {
                let world_center = local_center + offset;
                let scale_factor = scale_factors
                    .iter()
                    .find(|(scale_item_id, _)| *scale_item_id == placement_id)
                    .map(|(_, factor)| *factor)
                    .unwrap_or(1.0);
                let next_scale = (image.placement.state.geometry.scale * scale_factor).max(0.01);
                let position_changed =
                    (image.placement.state.geometry.center.x - world_center.x).abs() > 0.001
                        || (image.placement.state.geometry.center.y - world_center.y).abs() > 0.001;
                let scale_changed =
                    (image.placement.state.geometry.scale - next_scale).abs() > 0.001;
                if !position_changed && !scale_changed {
                    continue;
                }

                image.placement.apply_compact_output(
                    crate::placement::arrangement::CompactOutput {
                        placement_id: placement_id,
                        center: world_center,
                    },
                );
                image.placement.state.geometry.scale = next_scale;
                changed_ids.push(placement_id);
            }
        }

        for placement_id in &changed_ids {
            self.mark_dirty(*placement_id);
        }

        changed_ids.len()
    }

    pub fn apply_target_area_to_selected_placements(&mut self, target_area: f32) -> usize {
        if target_area <= 0.0 {
            return 0;
        }

        let selected_ids = self.document.selected_placement_ids.clone();
        let mut changed = 0;
        for placement_id in selected_ids {
            let Some(image) = self.rendered_placement_by_id(placement_id) else {
                continue;
            };
            let base_size = image.placement.base_display_size();
            let source_area = base_size.x * base_size.y;
            if source_area <= f32::EPSILON {
                continue;
            }

            let target_scale = (target_area / source_area).sqrt();
            if self.set_placement_scale(placement_id, target_scale) {
                self.mark_dirty(placement_id);
                changed += 1;
            }
        }

        changed
    }

    pub fn average_selected_placement_area(&self) -> Option<f32> {
        let items = self.selected_arrangement_inputs();
        if items.is_empty() {
            return None;
        }

        let total_area = items
            .iter()
            .map(|item| item.width().max(0.0) * item.height().max(0.0))
            .sum::<f32>();
        Some(total_area / items.len() as f32)
    }

    pub(super) fn rendered_placement_by_id(&self, placement_id: i64) -> Option<&RenderedPlacement> {
        self.document
            .rendered_placements
            .iter()
            .find(|image| image.placement.state.id == placement_id)
    }

    pub(super) fn rendered_placement_by_id_mut(
        &mut self,
        placement_id: i64,
    ) -> Option<&mut RenderedPlacement> {
        self.document
            .rendered_placements
            .iter_mut()
            .find(|image| image.placement.state.id == placement_id)
    }

    pub(super) fn selected_single_rendered_placement(&self) -> Option<&RenderedPlacement> {
        (self.document.selected_placement_ids.len() == 1)
            .then(|| self.rendered_placement_by_id(self.document.selected_placement_ids[0]))
            .flatten()
    }

    pub(super) fn selected_annotations(&self) -> Vec<&CanvasAnnotation> {
        self.document
            .annotations
            .iter()
            .filter(|annotation| self.is_annotation_selected(annotation.id))
            .collect()
    }

    pub fn selected_world_bounds(&self) -> Option<egui::Rect> {
        let mut selected_rect = self.selected_bounds_world();

        for annotation in self.selected_annotations() {
            let annotation_rect = annotation_world_rect(annotation);
            selected_rect = Some(match selected_rect {
                Some(existing) => existing.union(annotation_rect),
                None => annotation_rect,
            });
        }

        selected_rect
    }

    pub(super) fn selected_annotation(&self) -> Option<&CanvasAnnotation> {
        let selected_id = self.single_selected_annotation_id()?;
        self.document
            .annotations
            .iter()
            .find(|annotation| annotation.id == selected_id)
    }

    pub(super) fn single_selected_annotation_id(&self) -> Option<i64> {
        if self.document.selected_annotation_ids.len() == 1 {
            Some(self.document.selected_annotation_ids[0])
        } else {
            None
        }
    }

    pub(super) fn is_annotation_selected(&self, annotation_id: i64) -> bool {
        self.document
            .selected_annotation_ids
            .contains(&annotation_id)
    }

    pub(super) fn build_crop_request(
        &self,
        rect: egui::Rect,
        crop_state: &CropState,
    ) -> Option<CanvasCropRequest> {
        let image = self.rendered_placement_by_id(crop_state.placement_id)?;
        let image_rect = placement_screen_rect(self, rect, image);
        let clamped_screen_rect = clamped_crop_screen_rect(
            crop_state.start_screen,
            crop_state.current_screen,
            image_rect,
        )?;
        let local_rect = screen_rect_to_local_crop_rect(
            clamped_screen_rect,
            image_rect,
            image.placement.require_loaded_asset().width,
            image.placement.require_loaded_asset().height,
        )?;
        let crop_rect = pixel_crop_rect_from_local(
            local_rect,
            image.placement.require_loaded_asset().width,
            image.placement.require_loaded_asset().height,
        )?;
        let crop_offset_local =
            crop_center_offset_local(image.placement.require_loaded_asset(), local_rect);

        Some(CanvasCropRequest {
            placement_id: image.placement.state.id,
            source_asset: image.placement.require_loaded_asset().clone(),
            crop_rect,
            crop_offset_local,
        })
    }

    pub(super) fn can_start_crop_on_placement(&self, placement_id: i64) -> bool {
        !self
            .interaction
            .crop_pending_placement_ids
            .contains(&placement_id)
            && self
                .rendered_placement_by_id(placement_id)
                .is_some_and(|image| {
                    !asset_mime_type_is_svg(&image.placement.require_loaded_asset().mime_type)
                })
    }

    pub(super) fn set_placement_scale(&mut self, placement_id: i64, scale: f32) -> bool {
        let Some(image) = self.rendered_placement_by_id_mut(placement_id) else {
            return false;
        };
        let min_scale = min_loaded_placement_scale(image.placement.require_loaded_asset());
        let normalized = scale.max(min_scale);
        if (image.placement.state.geometry.scale - normalized).abs() <= f32::EPSILON {
            return false;
        }
        image.placement.state.geometry.scale = normalized;
        true
    }

    pub(super) fn set_placement_center_and_scale(
        &mut self,
        placement_id: i64,
        center: egui::Pos2,
        scale: f32,
    ) -> bool {
        let Some(image) = self.rendered_placement_by_id_mut(placement_id) else {
            return false;
        };
        let min_scale = min_loaded_placement_scale(image.placement.require_loaded_asset());
        let normalized_scale = scale.max(min_scale);
        let position_changed = (image.placement.state.geometry.center.x - center.x).abs()
            > f32::EPSILON
            || (image.placement.state.geometry.center.y - center.y).abs() > f32::EPSILON;
        let scale_changed =
            (image.placement.state.geometry.scale - normalized_scale).abs() > f32::EPSILON;
        if !position_changed && !scale_changed {
            return false;
        }

        image.placement.state.geometry.center.x = center.x;
        image.placement.state.geometry.center.y = center.y;
        image.placement.state.geometry.scale = normalized_scale;
        true
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn set_placement_rotation(&mut self, placement_id: i64, rotation: f32) -> bool {
        let Some(image) = self.rendered_placement_by_id_mut(placement_id) else {
            return false;
        };
        if (image.placement.state.geometry.rotation - rotation).abs() <= f32::EPSILON {
            return false;
        }
        image.placement.state.geometry.rotation = rotation;
        true
    }

    pub(super) fn set_placement_center_and_rotation(
        &mut self,
        placement_id: i64,
        center: egui::Pos2,
        rotation: f32,
    ) -> bool {
        let Some(image) = self.rendered_placement_by_id_mut(placement_id) else {
            return false;
        };
        let position_changed = (image.placement.state.geometry.center.x - center.x).abs()
            > f32::EPSILON
            || (image.placement.state.geometry.center.y - center.y).abs() > f32::EPSILON;
        let rotation_changed =
            (image.placement.state.geometry.rotation - rotation).abs() > f32::EPSILON;
        if !position_changed && !rotation_changed {
            return false;
        }

        image.placement.state.geometry.center.x = center.x;
        image.placement.state.geometry.center.y = center.y;
        image.placement.state.geometry.rotation = rotation;
        true
    }

    pub(super) fn advance_animated_image(
        &mut self,
        placement_id: i64,
        time_seconds: f32,
    ) -> Option<(SharedRgba, u32, u32)> {
        let image = self.rendered_placement_by_id_mut(placement_id)?;
        let animation = image.animation.as_mut()?;
        if animation.frames.is_empty() {
            return None;
        }

        let total_duration = animation
            .frames
            .iter()
            .map(|frame| frame.delay_seconds.max(0.01))
            .sum::<f32>()
            .max(0.01);
        let mut remaining = time_seconds.rem_euclid(total_duration);
        let mut frame_index = 0;
        for (index, frame) in animation.frames.iter().enumerate() {
            let delay = frame.delay_seconds.max(0.01);
            if remaining <= delay {
                frame_index = index;
                break;
            }
            remaining -= delay;
            frame_index = index;
        }

        if animation.current_frame == frame_index {
            return None;
        }

        animation.current_frame = frame_index;
        let frame = &animation.frames[frame_index];
        Some((frame.rgba.clone(), animation.width, animation.height))
    }

    pub(super) fn animated_image_repaint_delay(
        &self,
        placement_id: i64,
        time_seconds: f32,
    ) -> Option<std::time::Duration> {
        let image = self.rendered_placement_by_id(placement_id)?;
        let animation = image.animation.as_ref()?;
        if animation.frames.is_empty() {
            return None;
        }

        let total_duration = animation
            .frames
            .iter()
            .map(|frame| frame.delay_seconds.max(0.01))
            .sum::<f32>()
            .max(0.01);
        let mut remaining = time_seconds.rem_euclid(total_duration);
        for frame in &animation.frames {
            let delay = frame.delay_seconds.max(0.01);
            if remaining <= delay {
                return Some(std::time::Duration::from_secs_f32(
                    (delay - remaining).max(0.01),
                ));
            }
            remaining -= delay;
        }

        Some(std::time::Duration::from_secs_f32(0.01))
    }
}

fn asset_mime_type_is_svg(mime_type: &str) -> bool {
    mime_type
        .split(';')
        .next()
        .unwrap_or(mime_type)
        .trim()
        .eq_ignore_ascii_case("image/svg+xml")
}

fn layout_bounds_with_offset(
    items: &[PlacementArrangementInput],
    offset: egui::Vec2,
) -> Option<egui::Rect> {
    let mut bounds = egui::Rect::NOTHING;
    let mut found = false;
    for item in items {
        let rect = egui::Rect::from_center_size(
            egui::pos2(item.center().x + offset.x, item.center().y + offset.y),
            egui::vec2(item.width(), item.height()),
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

fn guide_snap_signature(indicators: &[super::GuideSnapIndicator]) -> u64 {
    use std::hash::{Hash, Hasher};

    let mut hasher = std::hash::DefaultHasher::new();
    indicators.len().hash(&mut hasher);
    for indicator in indicators {
        indicator.segments.len().hash(&mut hasher);
        for (start, end) in &indicator.segments {
            let snapped_coordinate = if (start.x - end.x).abs() <= f32::EPSILON {
                start.x
            } else {
                start.y
            };
            snapped_coordinate.to_bits().hash(&mut hasher);
        }
    }
    hasher.finish()
}
