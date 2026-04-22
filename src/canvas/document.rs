use std::sync::Arc;

use eframe::egui;

use super::{
    canvas_texture_from_preview,
    clamped_crop_screen_rect, crop_center_offset_local, layout_textbox_galley,
    merge_cropped_item_fields, merge_ids, min_image_scale, pixel_crop_rect_from_local,
    screen_rect_to_local_crop_rect, textbox_layout_cache_key, textbox_required_world_height,
    CanvasCropRequest, CanvasDocumentChanges, CanvasImage, CanvasImagePreview, CanvasState,
    CropState, ImportPlaceholder, PendingAnnotation, SelectedLayoutItem, TextboxLayoutCacheEntry,
    FOCUS_SELECTION_PADDING_SCREEN, MAX_CANVAS_ZOOM, MIN_CANVAS_ZOOM,
};
use super::interaction::{
    annotation_screen_rect, annotation_world_rect, image_screen_rect, normalized_textbox_rect,
    textbox_world_rect,
};
use crate::data::{
    BoardItemWithAsset, CanvasAnnotation, CanvasAnnotationKind, CanvasBackground,
    CanvasImageCacheTier, CanvasTextSize, ViewportState,
};
use crate::jobs::{CroppedCanvasItem, LoadedCanvasItem};

impl CanvasState {
    pub fn add_loaded_item(&mut self, ctx: &egui::Context, loaded: LoadedCanvasItem) {
        self.render.asset_rgba_cache
            .insert(loaded.record.asset.id, loaded.preview_rgba.clone());
        let texture = canvas_texture_from_preview(
            ctx,
            format!(
                "asset-{}-item-{}",
                loaded.record.asset.id, loaded.record.item.id
            ),
            &loaded.preview_rgba,
            loaded.preview_width,
            loaded.preview_height,
            loaded.preview_texture,
        );

        self.document.images
            .retain(|image| image.record.item.id != loaded.record.item.id);
        self.render.pending_image_tier_loads.remove(&loaded.record.item.id);
        self.document.images.push(CanvasImage {
            record: loaded.record,
            texture,
            loaded_tier: loaded.loaded_tier,
        });
        self.document.images.sort_by_key(|image| image.record.item.z_index);
    }

    pub fn add_reference_item(&mut self, record: BoardItemWithAsset) -> Result<(), String> {
        let texture = self
            .document.images
            .iter()
            .find(|image| image.record.asset.id == record.asset.id)
            .map(|image| image.texture.clone())
            .ok_or_else(|| format!("missing source texture for asset {}", record.asset.id))?;

        self.document.images.push(CanvasImage {
            record,
            texture,
            loaded_tier: CanvasImageCacheTier::Working,
        });
        self.document.images.sort_by_key(|image| image.record.item.z_index);
        Ok(())
    }

    pub fn snapshot_item_image(
        &self,
        item_id: i64,
    ) -> Result<(BoardItemWithAsset, Vec<u8>, u32, u32), String> {
        let image = self
            .image_by_item_id(item_id)
            .ok_or_else(|| format!("missing canvas item {}", item_id))?;
        let rgba = self
            .render
            .asset_rgba_cache
            .get(&image.record.asset.id)
            .cloned()
            .ok_or_else(|| format!("missing cached pixels for asset {}", image.record.asset.id))?;
        Ok((
            image.record.clone(),
            rgba,
            image.texture.size()[0] as u32,
            image.texture.size()[1] as u32,
        ))
    }

    pub fn projected_cropped_item(
        &self,
        cropped: &CroppedCanvasItem,
    ) -> Result<crate::data::BoardItem, String> {
        let Some(image) = self.image_by_item_id(cropped.item_id) else {
            return Err(format!("missing canvas item {}", cropped.item_id));
        };

        let mut item = image.record.item.clone();
        let asset_id = item.asset_id;
        merge_cropped_item_fields(&mut item, asset_id, cropped.crop_offset_local);
        Ok(item)
    }

    pub fn apply_cropped_item(
        &mut self,
        ctx: &egui::Context,
        cropped: CroppedCanvasItem,
        asset: crate::data::ImageAsset,
        updated_item: crate::data::BoardItem,
    ) -> Result<(), String> {
        self.render.asset_rgba_cache
            .insert(asset.id, cropped.preview_rgba.clone());
        let texture = canvas_texture_from_preview(
            ctx,
            format!("asset-{}-item-{}", asset.id, cropped.item_id),
            &cropped.preview_rgba,
            cropped.preview_width,
            cropped.preview_height,
            cropped.preview_texture,
        );

        let Some(image) = self
            .document.images
            .iter_mut()
            .find(|image| image.record.item.id == cropped.item_id)
        else {
            return Err(format!("missing canvas item {}", cropped.item_id));
        };

        image.record.item = updated_item;
        image.record.asset = asset;
        image.texture = texture;
        image.loaded_tier = cropped.loaded_tier;
        self.render.pending_image_tier_loads.remove(&cropped.item_id);
        Ok(())
    }

    pub fn apply_viewport_state(&mut self, viewport: &ViewportState) {
        self.pan = egui::vec2(viewport.camera_pan_x, viewport.camera_pan_y);
        self.zoom = viewport.camera_zoom.clamp(MIN_CANVAS_ZOOM, MAX_CANVAS_ZOOM);
        self.background = viewport.canvas_background;
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
        self.has_image_selection() || !self.document.selected_annotation_ids.is_empty()
    }

    pub fn has_image_selection(&self) -> bool {
        !self.document.selected_item_ids.is_empty()
    }

    pub fn selected_item_ids(&self) -> Vec<i64> {
        self.document.selected_item_ids.clone()
    }

    pub fn record_for_item_id(&self, item_id: i64) -> Option<BoardItemWithAsset> {
        self.document.images
            .iter()
            .find(|image| image.record.item.id == item_id)
            .map(|image| image.record.clone())
    }

    #[cfg_attr(not(feature = "semantic-search"), allow(dead_code))]
    pub fn preview_for_item_id(&self, item_id: i64) -> Option<CanvasImagePreview> {
        let image = self.image_by_item_id(item_id)?;
        let [width, height] = image.texture.size();
        Some(CanvasImagePreview {
            texture_id: image.texture.id(),
            texture_size: egui::vec2(width as f32, height as f32),
        })
    }

    pub fn background(&self) -> CanvasBackground {
        self.background
    }

    pub fn set_background(&mut self, background: CanvasBackground) {
        self.background = background;
    }

    #[cfg_attr(not(feature = "semantic-search"), allow(dead_code))]
    pub fn set_search_matches(&mut self, item_ids: Vec<i64>, active_item_id: Option<i64>) {
        self.interaction.search_match_item_ids = item_ids;
        self.interaction.active_search_item_id = active_item_id;
    }

    #[cfg_attr(not(feature = "semantic-search"), allow(dead_code))]
    pub fn clear_search_matches(&mut self) {
        self.interaction.search_match_item_ids.clear();
        self.interaction.active_search_item_id = None;
    }

    #[cfg_attr(not(feature = "semantic-search"), allow(dead_code))]
    pub fn focus_item(&mut self, item_id: i64) {
        let Some(image) = self.image_by_item_id(item_id) else {
            return;
        };
        let center = egui::vec2(image.record.item.x, image.record.item.y);
        self.pan = -center * self.zoom;
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
        self.pan = target_rect.center() - canvas_rect.center() - center_world * self.zoom;
        true
    }

    pub fn selected_center_world(&self) -> Option<egui::Pos2> {
        let mut min = egui::Pos2::new(f32::INFINITY, f32::INFINITY);
        let mut max = egui::Pos2::new(f32::NEG_INFINITY, f32::NEG_INFINITY);
        let mut found = false;

        for image in &self.document.images {
            if !self.is_selected(image.record.item.id) {
                continue;
            }

            found = true;
            min.x = min.x.min(image.record.item.x);
            min.y = min.y.min(image.record.item.y);
            max.x = max.x.max(image.record.item.x);
            max.y = max.y.max(image.record.item.y);
        }

        found.then(|| egui::pos2((min.x + max.x) * 0.5, (min.y + max.y) * 0.5))
    }

    pub fn take_pending_crop(&mut self) -> Option<CanvasCropRequest> {
        self.interaction.pending_crop.take()
    }

    pub fn take_pending_annotation(&mut self) -> Option<PendingAnnotation> {
        self.interaction.pending_annotation.take()
    }

    pub fn add_import_placeholder(
        &mut self,
        queue_id: u64,
        center_world: egui::Pos2,
        size_world: egui::Vec2,
        label: String,
    ) {
        self.remove_import_placeholder(queue_id);
        self.interaction.import_placeholders.push(ImportPlaceholder {
            queue_id,
            center_world,
            size_world,
            label,
        });
    }

    pub fn remove_import_placeholder(&mut self, queue_id: u64) {
        self.interaction.import_placeholders
            .retain(|placeholder| placeholder.queue_id != queue_id);
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn import_placeholder_count(&self) -> usize {
        self.interaction.import_placeholders.len()
    }

    pub fn take_pending_image_tier_loads(&mut self) -> Vec<(i64, CanvasImageCacheTier)> {
        self.render.pending_image_tier_loads.drain().collect()
    }

    pub fn mark_crop_pending(&mut self, item_id: i64) {
        if !self.interaction.crop_pending_item_ids.contains(&item_id) {
            self.interaction.crop_pending_item_ids.push(item_id);
        }
    }

    pub fn clear_crop_pending(&mut self, item_id: i64) {
        self.interaction.crop_pending_item_ids
            .retain(|pending_id| *pending_id != item_id);
    }

    pub fn clear_pending_image_tier_load(&mut self, item_id: i64) {
        self.render.pending_image_tier_loads.remove(&item_id);
    }

    pub fn evict_cached_asset(&mut self, asset_id: i64) {
        self.render.asset_rgba_cache.remove(&asset_id);
    }

    pub fn add_annotation(&mut self, annotation: CanvasAnnotation) {
        self.invalidate_textbox_layout_cache(annotation.id);
        self.document.annotations.retain(|item| item.id != annotation.id);
        self.document.annotations.push(annotation.clone());
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
            self.document.selected_item_ids.clear();
            self.interaction.image_transform_state = None;
            self.interaction.textbox_transform_state = None;
        }
    }

    pub fn add_annotations(&mut self, annotations: Vec<CanvasAnnotation>) {
        self.document.annotations = annotations;
        self.render.textbox_layout_cache.clear();
    }

    pub fn take_document_changes(&mut self) -> CanvasDocumentChanges {
        let dirty_ids = std::mem::take(&mut self.document.dirty_item_ids);
        let updated_items = dirty_ids
            .into_iter()
            .filter_map(|id| {
                self.document.images
                    .iter()
                    .find(|image| image.record.item.id == id)
                    .map(|image| image.record.item.clone())
            })
            .collect();

        let dirty_ids = std::mem::take(&mut self.document.dirty_annotation_ids);
        let updated_annotations = dirty_ids
            .into_iter()
            .filter_map(|id| {
                self.document.annotations
                    .iter()
                    .find(|annotation| annotation.id == id)
                    .cloned()
            })
            .collect();

        CanvasDocumentChanges {
            updated_items,
            updated_annotations,
            archived_annotation_ids: std::mem::take(
                &mut self.document.pending_archived_annotation_ids,
            ),
        }
    }

    pub fn selection_screen_rect(&self, rect: egui::Rect) -> Option<egui::Rect> {
        let mut selected_rect: Option<egui::Rect> = None;

        for image in &self.document.images {
            if !self.is_selected(image.record.item.id) {
                continue;
            }

            let image_rect = image_screen_rect(self, rect, image);
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

        for image in &self.document.images {
            if !self.is_selected(image.record.item.id) {
                continue;
            }

            let size = egui::vec2(
                image.record.asset.width as f32 * image.record.item.scale.max(0.01),
                image.record.asset.height as f32 * image.record.item.scale.max(0.01),
            );
            let image_rect = egui::Rect::from_center_size(
                egui::pos2(image.record.item.x, image.record.item.y),
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

    #[cfg_attr(not(debug_assertions), allow(dead_code))]
    pub fn select_items(&mut self, item_ids: &[i64]) {
        self.finish_textbox_edit();
        self.document.selected_item_ids = item_ids
            .iter()
            .copied()
            .filter(|item_id| {
                self.document.images
                    .iter()
                    .any(|image| image.record.item.id == *item_id)
            })
            .collect();
        self.document.selected_annotation_ids.clear();
        self.interaction.editing_text_annotation_id = None;
        self.interaction.image_transform_state = None;
        self.interaction.textbox_transform_state = None;
    }

    pub fn arrange_selected_horizontal(&mut self, gap: f32) -> usize {
        let mut items = self.selected_layout_items();
        if items.len() <= 1 {
            return items.len();
        }

        items.sort_by(|left, right| left.x.total_cmp(&right.x));
        let current_center = self.selected_center_world().unwrap_or(egui::Pos2::ZERO);
        let Some((placements, total_width, total_height)) = super::horizontal_layout(&items, gap)
        else {
            return 0;
        };
        let local_center = egui::pos2(total_width * 0.5, total_height * 0.5);
        self.apply_arrangement(placements, current_center.to_vec2() - local_center.to_vec2())
    }

    pub fn arrange_selected_vertical(&mut self, gap: f32) -> usize {
        let mut items = self.selected_layout_items();
        if items.len() <= 1 {
            return items.len();
        }

        items.sort_by(|left, right| left.y.total_cmp(&right.y));
        let current_center = self.selected_center_world().unwrap_or(egui::Pos2::ZERO);
        let Some((placements, total_width, total_height)) = super::vertical_layout(&items, gap)
        else {
            return 0;
        };
        let local_center = egui::pos2(total_width * 0.5, total_height * 0.5);
        self.apply_arrangement(placements, current_center.to_vec2() - local_center.to_vec2())
    }

    pub fn arrange_selected_compact(&mut self, gap: f32) -> usize {
        let mut items = self.selected_layout_items();
        if items.len() <= 1 {
            return items.len();
        }

        items.sort_by(|left, right| {
            right
                .width
                .max(right.height)
                .total_cmp(&left.width.max(left.height))
                .then_with(|| (right.width * right.height).total_cmp(&(left.width * left.height)))
                .then_with(|| left.id.cmp(&right.id))
        });
        let current_center = self.selected_center_world().unwrap_or(egui::Pos2::ZERO);

        let Some((placements, width, height)) = super::best_compact_layout(&items, gap) else {
            return 0;
        };
        let local_center = egui::pos2(width * 0.5, height * 0.5);
        self.apply_arrangement(placements, current_center.to_vec2() - local_center.to_vec2())
    }

    #[cfg_attr(not(debug_assertions), allow(dead_code))]
    pub fn delete_selected_images(&mut self) -> Vec<i64> {
        let selected_ids = self.document.selected_item_ids.clone();
        self.delete_items(&selected_ids)
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
    pub fn delete_items(&mut self, item_ids: &[i64]) -> Vec<i64> {
        if item_ids.is_empty() {
            return Vec::new();
        }

        let mut removed_ids = Vec::new();
        self.document.images.retain(|image| {
            let keep = !item_ids.contains(&image.record.item.id);
            if !keep {
                removed_ids.push(image.record.item.id);
            }
            keep
        });
        self.document.selected_item_ids
            .retain(|item_id| !removed_ids.contains(item_id));
        self.document.dirty_item_ids
            .retain(|item_id| !removed_ids.contains(item_id));
        self.interaction.drag_state = None;
        self.interaction.image_transform_state = None;
        self.interaction.textbox_transform_state = None;
        self.interaction.marquee_state = None;
        self.interaction.crop_state = None;
        self.interaction.annotation_draft_state = None;
        self.interaction.crop_pending_item_ids
            .retain(|item_id| !removed_ids.contains(item_id));
        self.render.pending_image_tier_loads
            .retain(|item_id, _| !removed_ids.contains(item_id));
        self.interaction.pending_crop = None;
        removed_ids
    }

    pub(super) fn is_selected(&self, item_id: i64) -> bool {
        self.document.selected_item_ids.contains(&item_id)
    }

    pub(super) fn request_image_tier_load(
        &mut self,
        item_id: i64,
        tier: CanvasImageCacheTier,
    ) {
        let Some(image) = self.image_by_item_id(item_id) else {
            return;
        };
        if tier <= image.loaded_tier {
            return;
        }
        match self.render.pending_image_tier_loads.get(&item_id).copied() {
            Some(pending) if pending >= tier => {}
            _ => {
                self.render.pending_image_tier_loads.insert(item_id, tier);
            }
        }
    }

    pub(super) fn set_single_selection(&mut self, item_id: i64) {
        self.finish_textbox_edit();
        self.document.selected_item_ids.clear();
        self.document.selected_item_ids.push(item_id);
        self.document.selected_annotation_ids.clear();
        self.interaction.editing_text_annotation_id = None;
        self.interaction.image_transform_state = None;
        self.interaction.textbox_transform_state = None;
    }

    pub(super) fn add_to_selection(&mut self, item_id: i64) {
        self.finish_textbox_edit();
        if !self.document.selected_item_ids.contains(&item_id) {
            self.document.selected_item_ids.push(item_id);
        }
        self.interaction.image_transform_state = None;
        self.interaction.textbox_transform_state = None;
    }

    pub(super) fn add_annotation_to_selection(&mut self, annotation_id: i64) {
        self.finish_textbox_edit();
        if !self.document.selected_annotation_ids.contains(&annotation_id) {
            self.document.selected_annotation_ids.push(annotation_id);
        }
        self.interaction.image_transform_state = None;
        self.interaction.textbox_transform_state = None;
    }

    pub(super) fn clear_selection(&mut self) {
        self.finish_textbox_edit();
        self.document.selected_item_ids.clear();
        self.document.selected_annotation_ids.clear();
        self.interaction.editing_text_annotation_id = None;
        self.interaction.image_transform_state = None;
        self.interaction.textbox_transform_state = None;
    }

    pub(super) fn set_marquee_selection(
        &mut self,
        item_ids: Vec<i64>,
        annotation_ids: Vec<i64>,
        additive: bool,
    ) {
        self.finish_textbox_edit();
        self.document.selected_item_ids = if additive {
            merge_ids(&self.document.selected_item_ids, &item_ids)
        } else {
            item_ids
        };
        self.document.selected_annotation_ids = if additive {
            merge_ids(&self.document.selected_annotation_ids, &annotation_ids)
        } else {
            annotation_ids
        };
        self.interaction.editing_text_annotation_id = None;
        self.interaction.image_transform_state = None;
        self.interaction.textbox_transform_state = None;
    }

    pub(super) fn mark_dirty(&mut self, item_id: i64) {
        if item_id > 0 && !self.document.dirty_item_ids.contains(&item_id) {
            self.document.dirty_item_ids.push(item_id);
        }
    }

    pub(super) fn mark_annotation_dirty(&mut self, annotation_id: i64) {
        if annotation_id > 0 && !self.document.dirty_annotation_ids.contains(&annotation_id) {
            self.document.dirty_annotation_ids.push(annotation_id);
        }
    }

    pub(super) fn remove_annotation_locally(&mut self, annotation_id: i64) {
        self.document.annotations
            .retain(|annotation| annotation.id != annotation_id);
        self.invalidate_textbox_layout_cache(annotation_id);
        self.document.selected_annotation_ids
            .retain(|selected_id| *selected_id != annotation_id);
        if self.interaction.editing_text_annotation_id == Some(annotation_id) {
            self.interaction.editing_text_annotation_id = None;
        }
        self.interaction.focus_text_editor = false;
        self.interaction.image_transform_state = None;
        if self.interaction.textbox_transform_state.is_some() {
            self.interaction.textbox_transform_state = None;
        }
        self.document.dirty_annotation_ids
            .retain(|dirty_id| *dirty_id != annotation_id);
    }

    pub(super) fn finish_textbox_edit(&mut self) {
        let Some(annotation_id) = self.interaction.editing_text_annotation_id else {
            self.interaction.focus_text_editor = false;
            return;
        };

        self.interaction.editing_text_annotation_id = None;
        self.interaction.focus_text_editor = false;
        self.interaction.image_transform_state = None;
        self.interaction.textbox_transform_state = None;

        let Some(annotation) = self
            .document.annotations
            .iter()
            .find(|annotation| annotation.id == annotation_id)
        else {
            self.document.selected_annotation_ids
                .retain(|selected_id| *selected_id != annotation_id);
            return;
        };

        if annotation.kind == CanvasAnnotationKind::Textbox && annotation.text.trim().is_empty() {
            self.remove_annotation_locally(annotation_id);
            if annotation_id > 0
                && !self
                    .document.pending_archived_annotation_ids
                    .contains(&annotation_id)
            {
                self.document.pending_archived_annotation_ids.push(annotation_id);
            }
        }
    }

    pub(super) fn stop_textbox_edit_without_cleanup(&mut self) {
        self.interaction.editing_text_annotation_id = None;
        self.interaction.focus_text_editor = false;
        self.interaction.image_transform_state = None;
        self.interaction.textbox_transform_state = None;
    }

    pub(super) fn annotation_by_id(&self, annotation_id: i64) -> Option<&CanvasAnnotation> {
        self.document.annotations
            .iter()
            .find(|annotation| annotation.id == annotation_id)
    }

    pub(super) fn annotation_by_id_mut(
        &mut self,
        annotation_id: i64,
    ) -> Option<&mut CanvasAnnotation> {
        self.document.annotations
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

        let changed = annotation.start_x != normalized.min.x
            || annotation.start_y != normalized.min.y
            || annotation.end_x != normalized.max.x
            || annotation.end_y != normalized.max.y;
        if changed {
            annotation.start_x = normalized.min.x;
            annotation.start_y = normalized.min.y;
            annotation.end_x = normalized.max.x;
            annotation.end_y = normalized.max.y;
            self.invalidate_textbox_layout_cache(annotation_id);
            self.mark_annotation_dirty(annotation_id);
        }
        changed
    }

    pub(super) fn selected_textbox_start_rects(&self) -> Vec<(i64, egui::Rect)> {
        self.document.selected_annotation_ids
            .iter()
            .filter_map(|annotation_id| {
                self.annotation_by_id(*annotation_id)
                    .and_then(|annotation| {
                        (annotation.kind == CanvasAnnotationKind::Textbox)
                            .then_some((*annotation_id, textbox_world_rect(annotation)))
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
        wrap_width: f32,
        zoom: f32,
        galley: Arc<egui::Galley>,
    ) -> TextboxLayoutCacheEntry {
        let key = textbox_layout_cache_key(text, text_size, wrap_width, zoom);
        let entry = TextboxLayoutCacheEntry {
            key,
            required_world_height: textbox_required_world_height(galley.size().y, zoom),
            galley,
        };
        self.render.textbox_layout_cache
            .insert(annotation_id, entry.clone());
        entry
    }

    pub(super) fn cached_textbox_layout(
        &mut self,
        ui: &egui::Ui,
        annotation_id: i64,
        text: &str,
        text_size: CanvasTextSize,
        wrap_width: f32,
        zoom: f32,
    ) -> TextboxLayoutCacheEntry {
        let key = textbox_layout_cache_key(text, text_size, wrap_width, zoom);
        if let Some(entry) = self.render.textbox_layout_cache.get(&annotation_id)
            && entry.key == key
        {
            return entry.clone();
        }

        let galley = layout_textbox_galley(ui, text, text_size, wrap_width, zoom);
        self.cache_textbox_layout(annotation_id, text, text_size, wrap_width, zoom, galley)
    }

    pub(super) fn selected_layout_items(&self) -> Vec<SelectedLayoutItem> {
        self.document.images
            .iter()
            .filter(|image| self.is_selected(image.record.item.id))
            .map(|image| SelectedLayoutItem {
                id: image.record.item.id,
                x: image.record.item.x,
                y: image.record.item.y,
                width: image.record.asset.width as f32 * image.record.item.scale.max(0.01),
                height: image.record.asset.height as f32 * image.record.item.scale.max(0.01),
            })
            .collect()
    }

    pub(super) fn apply_arrangement(
        &mut self,
        placements: Vec<(i64, egui::Pos2)>,
        offset: egui::Vec2,
    ) -> usize {
        let mut arranged_ids = Vec::with_capacity(placements.len());

        for (item_id, local_center) in placements {
            if let Some(image) = self
                .document.images
                .iter_mut()
                .find(|image| image.record.item.id == item_id)
            {
                let world_center = local_center + offset;
                image.record.item.x = world_center.x;
                image.record.item.y = world_center.y;
                arranged_ids.push(item_id);
            }
        }

        for item_id in &arranged_ids {
            self.mark_dirty(*item_id);
        }

        arranged_ids.len()
    }

    pub fn apply_target_area_to_selected_images(&mut self, target_area: f32) -> usize {
        if target_area <= 0.0 {
            return 0;
        }

        let selected_ids = self.document.selected_item_ids.clone();
        let mut changed = 0;
        for item_id in selected_ids {
            let Some(image) = self.image_by_item_id(item_id) else {
                continue;
            };
            let source_area = image.record.asset.width as f32 * image.record.asset.height as f32;
            if source_area <= f32::EPSILON {
                continue;
            }

            let target_scale = (target_area / source_area).sqrt();
            if self.set_image_scale(item_id, target_scale) {
                self.mark_dirty(item_id);
                changed += 1;
            }
        }

        changed
    }

    pub fn average_selected_image_area(&self) -> Option<f32> {
        let items = self.selected_layout_items();
        if items.is_empty() {
            return None;
        }

        let total_area = items
            .iter()
            .map(|item| item.width.max(0.0) * item.height.max(0.0))
            .sum::<f32>();
        Some(total_area / items.len() as f32)
    }

    pub(super) fn image_by_item_id(&self, item_id: i64) -> Option<&CanvasImage> {
        self.document.images
            .iter()
            .find(|image| image.record.item.id == item_id)
    }

    pub(super) fn image_by_item_id_mut(&mut self, item_id: i64) -> Option<&mut CanvasImage> {
        self.document.images
            .iter_mut()
            .find(|image| image.record.item.id == item_id)
    }

    pub(super) fn selected_single_image(&self) -> Option<&CanvasImage> {
        (self.document.selected_item_ids.len() == 1)
            .then(|| self.image_by_item_id(self.document.selected_item_ids[0]))
            .flatten()
    }

    pub(super) fn selected_annotations(&self) -> Vec<&CanvasAnnotation> {
        self.document.annotations
            .iter()
            .filter(|annotation| self.is_annotation_selected(annotation.id))
            .collect()
    }

    pub(super) fn selected_world_bounds(&self) -> Option<egui::Rect> {
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
        self.document.annotations
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
        self.document.selected_annotation_ids.contains(&annotation_id)
    }

    pub(super) fn build_crop_request(
        &self,
        rect: egui::Rect,
        crop_state: &CropState,
    ) -> Option<CanvasCropRequest> {
        let image = self.image_by_item_id(crop_state.item_id)?;
        let image_rect = image_screen_rect(self, rect, image);
        let clamped_screen_rect = clamped_crop_screen_rect(
            crop_state.start_screen,
            crop_state.current_screen,
            image_rect,
        )?;
        let local_rect = screen_rect_to_local_crop_rect(
            clamped_screen_rect,
            image_rect,
            image.record.asset.width,
            image.record.asset.height,
        )?;
        let crop_rect = pixel_crop_rect_from_local(
            local_rect,
            image.record.asset.width,
            image.record.asset.height,
        )?;
        let crop_offset_local = crop_center_offset_local(&image.record.asset, local_rect);

        Some(CanvasCropRequest {
            item_id: image.record.item.id,
            source_asset: image.record.asset.clone(),
            crop_rect,
            crop_offset_local,
        })
    }

    pub(super) fn can_start_crop_on_item(&self, item_id: i64) -> bool {
        !self.interaction.crop_pending_item_ids.contains(&item_id)
    }

    pub(super) fn set_image_scale(&mut self, item_id: i64, scale: f32) -> bool {
        let Some(image) = self.image_by_item_id_mut(item_id) else {
            return false;
        };
        let min_scale = min_image_scale(&image.record.asset);
        let normalized = scale.max(min_scale);
        if (image.record.item.scale - normalized).abs() <= f32::EPSILON {
            return false;
        }
        image.record.item.scale = normalized;
        true
    }
}
