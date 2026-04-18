use std::collections::HashMap;

use eframe::egui;

use crate::data::{BoardItemWithAsset, UserSession};
use crate::jobs::{CroppedCanvasItem, LoadedCanvasItem};

const MIN_CANVAS_ZOOM: f32 = 0.01;
const MAX_CANVAS_ZOOM: f32 = 64.0;

pub struct CanvasState {
    pan: egui::Vec2,
    zoom: f32,
    images: Vec<CanvasImage>,
    asset_rgba_cache: HashMap<i64, Vec<u8>>,
    selected_item_ids: Vec<i64>,
    drag_state: Option<DragState>,
    marquee_state: Option<MarqueeState>,
    crop_state: Option<CropState>,
    crop_pending_item_ids: Vec<i64>,
    pending_crop: Option<CanvasCropRequest>,
    dirty_item_ids: Vec<i64>,
    last_pointer_screen_pos: Option<egui::Pos2>,
}

struct CanvasImage {
    record: BoardItemWithAsset,
    texture: egui::TextureHandle,
}

#[derive(Clone, Debug)]
struct SelectedLayoutItem {
    id: i64,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

struct DragState {
    pointer_start_world: egui::Pos2,
    starting_positions: Vec<(i64, egui::Pos2)>,
}

struct MarqueeState {
    start_screen: egui::Pos2,
    current_screen: egui::Pos2,
}

struct CropState {
    item_id: i64,
    start_screen: egui::Pos2,
    current_screen: egui::Pos2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PixelCropRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Debug)]
pub struct CanvasCropRequest {
    pub item_id: i64,
    pub source_asset: crate::data::ImageAsset,
    pub source_rgba: Vec<u8>,
    pub crop_rect: PixelCropRect,
    pub crop_offset_local: egui::Vec2,
}

impl Default for CanvasState {
    fn default() -> Self {
        Self {
            pan: egui::Vec2::ZERO,
            zoom: 1.0,
            images: Vec::new(),
            asset_rgba_cache: HashMap::new(),
            selected_item_ids: Vec::new(),
            drag_state: None,
            marquee_state: None,
            crop_state: None,
            crop_pending_item_ids: Vec::new(),
            pending_crop: None,
            dirty_item_ids: Vec::new(),
            last_pointer_screen_pos: None,
        }
    }
}

impl CanvasState {
    pub fn add_loaded_item(&mut self, ctx: &egui::Context, loaded: LoadedCanvasItem) {
        self.asset_rgba_cache
            .insert(loaded.record.asset.id, loaded.rgba.clone());
        let size = [
            loaded.record.asset.width as usize,
            loaded.record.asset.height as usize,
        ];
        let color_image = egui::ColorImage::from_rgba_unmultiplied(size, &loaded.rgba);
        let texture = ctx.load_texture(
            format!(
                "asset-{}-item-{}",
                loaded.record.asset.id, loaded.record.item.id
            ),
            color_image,
            egui::TextureOptions::LINEAR,
        );

        self.images
            .retain(|image| image.record.item.id != loaded.record.item.id);
        self.images.push(CanvasImage {
            record: loaded.record,
            texture,
        });
        self.images.sort_by_key(|image| image.record.item.z_index);
    }

    pub fn add_reference_item(&mut self, record: BoardItemWithAsset) -> Result<(), String> {
        let texture = self
            .images
            .iter()
            .find(|image| image.record.asset.id == record.asset.id)
            .map(|image| image.texture.clone())
            .ok_or_else(|| format!("missing source texture for asset {}", record.asset.id))?;

        self.images.push(CanvasImage { record, texture });
        self.images.sort_by_key(|image| image.record.item.z_index);
        Ok(())
    }

    pub fn snapshot_item_image(
        &self,
        item_id: i64,
    ) -> Result<(BoardItemWithAsset, Vec<u8>), String> {
        let image = self
            .image_by_item_id(item_id)
            .ok_or_else(|| format!("missing canvas item {}", item_id))?;
        let rgba = self
            .asset_rgba_cache
            .get(&image.record.asset.id)
            .cloned()
            .ok_or_else(|| format!("missing cached pixels for asset {}", image.record.asset.id))?;
        Ok((image.record.clone(), rgba))
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
        let size = [asset.width as usize, asset.height as usize];
        let color_image = egui::ColorImage::from_rgba_unmultiplied(size, &cropped.rgba);
        self.asset_rgba_cache.insert(asset.id, cropped.rgba.clone());
        let texture = ctx.load_texture(
            format!("asset-{}-item-{}", asset.id, cropped.item_id),
            color_image,
            egui::TextureOptions::LINEAR,
        );

        let Some(image) = self
            .images
            .iter_mut()
            .find(|image| image.record.item.id == cropped.item_id)
        else {
            return Err(format!("missing canvas item {}", cropped.item_id));
        };

        image.record.item = updated_item;
        image.record.asset = asset;
        image.texture = texture;
        Ok(())
    }

    fn world_to_screen(&self, world: egui::Pos2, rect: egui::Rect) -> egui::Pos2 {
        rect.center() + self.pan + world.to_vec2() * self.zoom
    }

    pub fn screen_to_world(&self, screen: egui::Pos2, rect: egui::Rect) -> egui::Pos2 {
        let world = (screen - rect.center() - self.pan) / self.zoom;
        egui::pos2(world.x, world.y)
    }

    pub fn apply_user_session(&mut self, session: &UserSession) {
        self.pan = egui::vec2(session.camera_pan_x, session.camera_pan_y);
        self.zoom = session.camera_zoom.clamp(MIN_CANVAS_ZOOM, MAX_CANVAS_ZOOM);
    }

    pub fn user_session(&self) -> UserSession {
        UserSession {
            camera_pan_x: self.pan.x,
            camera_pan_y: self.pan.y,
            camera_zoom: self.zoom,
            auto_image_scaling: true,
            import_gap: crate::data::DEFAULT_IMPORT_GAP,
        }
    }

    pub fn has_selection(&self) -> bool {
        !self.selected_item_ids.is_empty()
    }

    pub fn selected_item_ids(&self) -> Vec<i64> {
        self.selected_item_ids.clone()
    }

    pub fn selected_center_world(&self) -> Option<egui::Pos2> {
        let mut min = egui::Pos2::new(f32::INFINITY, f32::INFINITY);
        let mut max = egui::Pos2::new(f32::NEG_INFINITY, f32::NEG_INFINITY);
        let mut found = false;

        for image in &self.images {
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
        self.pending_crop.take()
    }

    pub fn mark_crop_pending(&mut self, item_id: i64) {
        if !self.crop_pending_item_ids.contains(&item_id) {
            self.crop_pending_item_ids.push(item_id);
        }
    }

    pub fn clear_crop_pending(&mut self, item_id: i64) {
        self.crop_pending_item_ids
            .retain(|pending_id| *pending_id != item_id);
    }

    pub fn evict_cached_asset(&mut self, asset_id: i64) {
        self.asset_rgba_cache.remove(&asset_id);
    }

    pub fn take_dirty_items(&mut self) -> Vec<crate::data::BoardItem> {
        let dirty_ids = std::mem::take(&mut self.dirty_item_ids);
        dirty_ids
            .into_iter()
            .filter_map(|id| {
                self.images
                    .iter()
                    .find(|image| image.record.item.id == id)
                    .map(|image| image.record.item.clone())
            })
            .collect()
    }

    pub fn selection_screen_rect(&self, rect: egui::Rect) -> Option<egui::Rect> {
        let mut selected_rect: Option<egui::Rect> = None;

        for image in &self.images {
            if !self.is_selected(image.record.item.id) {
                continue;
            }

            let image_rect = image_screen_rect(self, rect, image);
            selected_rect = Some(match selected_rect {
                Some(rect) => rect.union(image_rect),
                None => image_rect,
            });
        }

        selected_rect
    }

    pub fn selected_bounds_world(&self) -> Option<egui::Rect> {
        let mut selected_rect: Option<egui::Rect> = None;

        for image in &self.images {
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

    #[cfg_attr(not(debug_assertions), allow(dead_code))]
    pub fn select_items(&mut self, item_ids: &[i64]) {
        self.selected_item_ids = item_ids
            .iter()
            .copied()
            .filter(|item_id| {
                self.images
                    .iter()
                    .any(|image| image.record.item.id == *item_id)
            })
            .collect();
    }

    pub fn arrange_selected_horizontal(&mut self, gap: f32) -> usize {
        let mut items = self.selected_layout_items();
        if items.len() <= 1 {
            return items.len();
        }

        items.sort_by(|left, right| left.x.total_cmp(&right.x));
        let current_center = self.selected_center_world().unwrap_or(egui::Pos2::ZERO);
        let Some((placements, total_width, total_height)) = horizontal_layout(&items, gap) else {
            return 0;
        };
        let local_center = egui::pos2(total_width * 0.5, total_height * 0.5);
        self.apply_arrangement(
            placements,
            current_center.to_vec2() - local_center.to_vec2(),
        )
    }

    pub fn arrange_selected_vertical(&mut self, gap: f32) -> usize {
        let mut items = self.selected_layout_items();
        if items.len() <= 1 {
            return items.len();
        }

        items.sort_by(|left, right| left.y.total_cmp(&right.y));
        let current_center = self.selected_center_world().unwrap_or(egui::Pos2::ZERO);
        let Some((placements, total_width, total_height)) = vertical_layout(&items, gap) else {
            return 0;
        };
        let local_center = egui::pos2(total_width * 0.5, total_height * 0.5);
        self.apply_arrangement(
            placements,
            current_center.to_vec2() - local_center.to_vec2(),
        )
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

        let Some((placements, width, height)) = best_compact_layout(&items, gap) else {
            return 0;
        };
        let local_center = egui::pos2(width * 0.5, height * 0.5);
        self.apply_arrangement(
            placements,
            current_center.to_vec2() - local_center.to_vec2(),
        )
    }

    #[cfg_attr(not(debug_assertions), allow(dead_code))]
    pub fn delete_selected(&mut self) -> Vec<i64> {
        let selected_ids = self.selected_item_ids.clone();
        self.delete_items(&selected_ids)
    }

    #[cfg_attr(not(debug_assertions), allow(dead_code))]
    pub fn delete_items(&mut self, item_ids: &[i64]) -> Vec<i64> {
        if item_ids.is_empty() {
            return Vec::new();
        }

        let mut removed_ids = Vec::new();
        self.images.retain(|image| {
            let keep = !item_ids.contains(&image.record.item.id);
            if !keep {
                removed_ids.push(image.record.item.id);
            }
            keep
        });
        self.selected_item_ids
            .retain(|item_id| !removed_ids.contains(item_id));
        self.dirty_item_ids
            .retain(|item_id| !removed_ids.contains(item_id));
        self.drag_state = None;
        self.marquee_state = None;
        self.crop_state = None;
        self.crop_pending_item_ids
            .retain(|item_id| !removed_ids.contains(item_id));
        self.pending_crop = None;
        removed_ids
    }

    fn is_selected(&self, item_id: i64) -> bool {
        self.selected_item_ids.contains(&item_id)
    }

    fn set_single_selection(&mut self, item_id: i64) {
        self.selected_item_ids.clear();
        self.selected_item_ids.push(item_id);
    }

    fn add_to_selection(&mut self, item_id: i64) {
        if !self.selected_item_ids.contains(&item_id) {
            self.selected_item_ids.push(item_id);
        }
    }

    fn clear_selection(&mut self) {
        self.selected_item_ids.clear();
    }

    fn set_selection(&mut self, item_ids: Vec<i64>) {
        self.selected_item_ids = item_ids;
    }

    fn mark_dirty(&mut self, item_id: i64) {
        if item_id > 0 && !self.dirty_item_ids.contains(&item_id) {
            self.dirty_item_ids.push(item_id);
        }
    }

    fn selected_layout_items(&self) -> Vec<SelectedLayoutItem> {
        self.images
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

    fn apply_arrangement(
        &mut self,
        placements: Vec<(i64, egui::Pos2)>,
        offset: egui::Vec2,
    ) -> usize {
        let mut arranged_ids = Vec::with_capacity(placements.len());

        for (item_id, local_center) in placements {
            if let Some(image) = self
                .images
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

    fn image_by_item_id(&self, item_id: i64) -> Option<&CanvasImage> {
        self.images
            .iter()
            .find(|image| image.record.item.id == item_id)
    }

    fn build_crop_request(
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
            source_rgba: self.asset_rgba_cache.get(&image.record.asset.id)?.clone(),
            crop_rect,
            crop_offset_local,
        })
    }

    fn can_start_crop_on_item(&self, item_id: i64) -> bool {
        !self.crop_pending_item_ids.contains(&item_id)
    }
}

fn horizontal_layout(
    items: &[SelectedLayoutItem],
    gap: f32,
) -> Option<(Vec<(i64, egui::Pos2)>, f32, f32)> {
    if items.is_empty() {
        return None;
    }

    let max_height = items.iter().map(|item| item.height).fold(0.0_f32, f32::max);
    let mut x_cursor = 0.0;
    let mut placements = Vec::with_capacity(items.len());

    for item in items {
        let center = egui::pos2(x_cursor + item.width * 0.5, max_height - item.height * 0.5);
        placements.push((item.id, center));
        x_cursor += item.width + gap;
    }

    let total_width = (x_cursor - gap).max(0.0);
    Some((placements, total_width, max_height))
}

fn vertical_layout(
    items: &[SelectedLayoutItem],
    gap: f32,
) -> Option<(Vec<(i64, egui::Pos2)>, f32, f32)> {
    if items.is_empty() {
        return None;
    }

    let max_width = items.iter().map(|item| item.width).fold(0.0_f32, f32::max);
    let mut y_cursor = 0.0;
    let mut placements = Vec::with_capacity(items.len());

    for item in items {
        let center = egui::pos2(item.width * 0.5, y_cursor + item.height * 0.5);
        placements.push((item.id, center));
        y_cursor += item.height + gap;
    }

    let total_height = (y_cursor - gap).max(0.0);
    Some((placements, max_width, total_height))
}

fn best_compact_layout(
    items: &[SelectedLayoutItem],
    gap: f32,
) -> Option<(Vec<(i64, egui::Pos2)>, f32, f32)> {
    if items.is_empty() {
        return None;
    }

    let mut best_layout: Option<(Vec<(i64, egui::Pos2)>, f32, f32, f32, f32, usize)> = None;
    for (candidate_index, bin_width) in compact_candidate_widths(items, gap).into_iter().enumerate()
    {
        let Some((placements, width, height)) = maxrects_pack_layout(items, bin_width, gap) else {
            continue;
        };
        let area = width * height;
        let squareness = width.max(height) / width.min(height).max(1.0);

        match &best_layout {
            Some((_, _, _, best_squareness, best_area, best_index))
                if area > *best_area
                    || ((area - *best_area).abs() < 0.001 && squareness > *best_squareness)
                    || ((area - *best_area).abs() < 0.001
                        && (squareness - *best_squareness).abs() < 0.001
                        && candidate_index >= *best_index) => {}
            _ => {
                best_layout = Some((placements, width, height, squareness, area, candidate_index));
            }
        }
    }

    best_layout.map(|(placements, width, height, _, _, _)| (placements, width, height))
}

fn compact_candidate_widths(items: &[SelectedLayoutItem], gap: f32) -> Vec<f32> {
    let total_area = items
        .iter()
        .map(|item| (item.width + gap) * (item.height + gap))
        .sum::<f32>();
    let max_width = items
        .iter()
        .map(|item| item.width + gap)
        .fold(0.0_f32, f32::max);
    let ideal_width = total_area.sqrt().max(max_width);
    let mut widths = vec![ideal_width];
    let mut running_width = 0.0_f32;

    for item in items {
        let effective_width = item.width + gap;
        running_width += effective_width;
        widths.push(running_width.max(max_width));
    }

    widths.sort_by(|left, right| left.total_cmp(right));
    widths.dedup_by(|left, right| (*left - *right).abs() < 0.5);
    widths
}

fn maxrects_pack_layout(
    items: &[SelectedLayoutItem],
    bin_width: f32,
    gap: f32,
) -> Option<(Vec<(i64, egui::Pos2)>, f32, f32)> {
    if items.is_empty() {
        return None;
    }

    let total_height = items
        .iter()
        .map(|item| item.height + gap)
        .sum::<f32>()
        .max(items.iter().map(|item| item.height + gap).fold(0.0_f32, f32::max));
    let mut free_rects = vec![PackRect {
        x: 0.0,
        y: 0.0,
        width: bin_width,
        height: total_height,
    }];
    let mut placements = Vec::with_capacity(items.len());

    for item in items {
        let padded = padded_item_rect(item, gap);
        if padded.width > bin_width + 0.001 {
            return None;
        }

        let Some(placed) = place_maxrects_item(&mut free_rects, padded) else {
            return None;
        };
        placements.push((
            item.id,
            egui::pos2(placed.x + item.width * 0.5, placed.y + item.height * 0.5),
        ));
    }

    let (layout_width, layout_height) = placed_bounds(items, &placements)?;
    Some((placements, layout_width, layout_height))
}

fn place_maxrects_item(free_rects: &mut Vec<PackRect>, padded: PackRect) -> Option<PackRect> {
    let mut best_candidate: Option<(usize, PackRect, f32, f32)> = None;

    for (index, free) in free_rects.iter().copied().enumerate() {
        if padded.width > free.width + 0.001 || padded.height > free.height + 0.001 {
            continue;
        }

        let short_side_fit = (free.width - padded.width).min(free.height - padded.height);
        let long_side_fit = (free.width - padded.width).max(free.height - padded.height);
        let placed = PackRect {
            x: free.x,
            y: free.y,
            width: padded.width,
            height: padded.height,
        };

        match best_candidate {
            Some((_, best_rect, best_short, best_long))
                if short_side_fit > best_short
                    || ((short_side_fit - best_short).abs() < 0.001 && long_side_fit > best_long)
                    || ((short_side_fit - best_short).abs() < 0.001
                        && (long_side_fit - best_long).abs() < 0.001
                        && (placed.y > best_rect.y
                            || ((placed.y - best_rect.y).abs() < 0.001 && placed.x >= best_rect.x))) => {}
            _ => best_candidate = Some((index, placed, short_side_fit, long_side_fit)),
        }
    }

    let (candidate_index, placed, _, _) = best_candidate?;
    split_free_rects(free_rects, candidate_index, placed);
    Some(placed)
}

fn split_free_rects(free_rects: &mut Vec<PackRect>, candidate_index: usize, placed: PackRect) {
    let mut next = Vec::with_capacity(free_rects.len() + 4);

    for (index, free) in free_rects.iter().copied().enumerate() {
        if index == candidate_index || rects_intersect(free, placed) {
            next.extend(split_free_rect(free, placed));
        } else {
            next.push(free);
        }
    }

    prune_free_rects(&mut next);
    *free_rects = next;
}

fn split_free_rect(free: PackRect, placed: PackRect) -> Vec<PackRect> {
    if !rects_intersect(free, placed) {
        return vec![free];
    }

    let mut result = Vec::with_capacity(4);

    if placed.y > free.y + 0.001 {
        result.push(PackRect {
            x: free.x,
            y: free.y,
            width: free.width,
            height: placed.y - free.y,
        });
    }

    if placed.bottom() < free.bottom() - 0.001 {
        result.push(PackRect {
            x: free.x,
            y: placed.bottom(),
            width: free.width,
            height: free.bottom() - placed.bottom(),
        });
    }

    if placed.x > free.x + 0.001 {
        result.push(PackRect {
            x: free.x,
            y: free.y,
            width: placed.x - free.x,
            height: free.height,
        });
    }

    if placed.right() < free.right() - 0.001 {
        result.push(PackRect {
            x: placed.right(),
            y: free.y,
            width: free.right() - placed.right(),
            height: free.height,
        });
    }

    result
}

fn prune_free_rects(free_rects: &mut Vec<PackRect>) {
    free_rects.retain(|rect| rect.width > 0.001 && rect.height > 0.001);

    loop {
        let mut changed = false;

        let mut index = 0;
        while index < free_rects.len() {
            let rect = free_rects[index];
            let contained = free_rects
                .iter()
                .enumerate()
                .any(|(other_index, other)| other_index != index && rect_contained_in(rect, *other));
            if contained {
                free_rects.remove(index);
                changed = true;
            } else {
                index += 1;
            }
        }

        let mut left_index = 0;
        while left_index < free_rects.len() {
            let mut right_index = left_index + 1;
            while right_index < free_rects.len() {
                if let Some(merged) = merge_adjacent_free_rects(
                    free_rects[left_index],
                    free_rects[right_index],
                ) {
                    free_rects[left_index] = merged;
                    free_rects.remove(right_index);
                    changed = true;
                } else {
                    right_index += 1;
                }
            }
            left_index += 1;
        }

        if !changed {
            break;
        }
    }
}

fn rects_intersect(left: PackRect, right: PackRect) -> bool {
    left.x < right.right() - 0.001
        && left.right() > right.x + 0.001
        && left.y < right.bottom() - 0.001
        && left.bottom() > right.y + 0.001
}

fn rect_contained_in(inner: PackRect, outer: PackRect) -> bool {
    inner.x >= outer.x - 0.001
        && inner.y >= outer.y - 0.001
        && inner.right() <= outer.right() + 0.001
        && inner.bottom() <= outer.bottom() + 0.001
}

fn merge_adjacent_free_rects(left: PackRect, right: PackRect) -> Option<PackRect> {
    if (left.x - right.x).abs() < 0.001
        && (left.width - right.width).abs() < 0.001
        && (left.bottom() - right.y).abs() < 0.001
    {
        return Some(PackRect {
            x: left.x,
            y: left.y,
            width: left.width,
            height: left.height + right.height,
        });
    }

    if (left.x - right.x).abs() < 0.001
        && (left.width - right.width).abs() < 0.001
        && (right.bottom() - left.y).abs() < 0.001
    {
        return Some(PackRect {
            x: right.x,
            y: right.y,
            width: right.width,
            height: left.height + right.height,
        });
    }

    if (left.y - right.y).abs() < 0.001
        && (left.height - right.height).abs() < 0.001
        && (left.right() - right.x).abs() < 0.001
    {
        return Some(PackRect {
            x: left.x,
            y: left.y,
            width: left.width + right.width,
            height: left.height,
        });
    }

    if (left.y - right.y).abs() < 0.001
        && (left.height - right.height).abs() < 0.001
        && (right.right() - left.x).abs() < 0.001
    {
        return Some(PackRect {
            x: right.x,
            y: right.y,
            width: left.width + right.width,
            height: right.height,
        });
    }

    None
}

fn padded_item_rect(item: &SelectedLayoutItem, gap: f32) -> PackRect {
    PackRect {
        x: 0.0,
        y: 0.0,
        width: item.width + gap,
        height: item.height + gap,
    }
}

#[cfg(test)]
fn legacy_best_compact_layout(
    items: &[SelectedLayoutItem],
    gap: f32,
) -> Option<(Vec<(i64, egui::Pos2)>, f32, f32)> {
    if items.is_empty() {
        return None;
    }

    let mut best_layout: Option<(Vec<(i64, egui::Pos2)>, f32, f32, f32, f32, usize)> = None;
    for (candidate_index, bin_width) in legacy_compact_candidate_widths(items, gap)
        .into_iter()
        .enumerate()
    {
        let Some((placements, width, height)) = legacy_skyline_pack_layout(items, bin_width, gap)
        else {
            continue;
        };
        let squareness = width.max(height) / width.min(height).max(1.0);
        let area = width * height;

        match &best_layout {
            Some((_, _, _, best_squareness, best_area, best_index))
                if squareness > *best_squareness
                    || ((squareness - *best_squareness).abs() < 0.001 && area > *best_area)
                    || ((squareness - *best_squareness).abs() < 0.001
                        && (area - *best_area).abs() < 0.001
                        && candidate_index >= *best_index) => {}
            _ => {
                best_layout = Some((placements, width, height, squareness, area, candidate_index));
            }
        }
    }

    best_layout.map(|(placements, width, height, _, _, _)| (placements, width, height))
}

#[cfg(test)]
fn legacy_compact_candidate_widths(items: &[SelectedLayoutItem], gap: f32) -> Vec<f32> {
    let total_area = items
        .iter()
        .map(|item| item.width * item.height)
        .sum::<f32>();
    let max_width = items.iter().map(|item| item.width).fold(0.0_f32, f32::max);
    let ideal_width = total_area.sqrt().max(max_width);
    let mut widths = vec![ideal_width];
    let mut running_width = 0.0_f32;

    for (index, item) in items.iter().enumerate() {
        running_width += item.width;
        if index > 0 {
            running_width += gap;
        }
        widths.push(running_width.max(max_width));
    }

    widths.sort_by(|left, right| left.total_cmp(right));
    widths.dedup_by(|left, right| (*left - *right).abs() < 0.5);
    widths
}

#[cfg(test)]
fn legacy_skyline_pack_layout(
    items: &[SelectedLayoutItem],
    bin_width: f32,
    gap: f32,
) -> Option<(Vec<(i64, egui::Pos2)>, f32, f32)> {
    if items.is_empty() {
        return None;
    }

    let mut skyline = vec![LegacySkylineNode {
        x: 0.0,
        y: 0.0,
        width: bin_width,
    }];
    let mut placements = Vec::with_capacity(items.len());

    for item in items {
        let occupied_width = item.width + gap;
        let occupied_height = item.height + gap;
        if occupied_width > bin_width + 0.001 {
            return None;
        }

        let mut best_candidate: Option<(usize, f32, f32)> = None;
        for node_index in 0..skyline.len() {
            let Some(candidate_y) =
                legacy_skyline_candidate_y(&skyline, node_index, occupied_width, bin_width)
            else {
                continue;
            };
            let candidate_x = skyline[node_index].x;
            match best_candidate {
                Some((_, best_y, best_x))
                    if candidate_y > best_y
                        || ((candidate_y - best_y).abs() < 0.001 && candidate_x >= best_x) => {}
                _ => best_candidate = Some((node_index, candidate_y, candidate_x)),
            }
        }

        let Some((node_index, place_y, place_x)) = best_candidate else {
            return None;
        };
        legacy_skyline_insert(
            &mut skyline,
            node_index,
            place_x,
            place_y,
            occupied_width,
            occupied_height,
        );
        placements.push((
            item.id,
            egui::pos2(place_x + item.width * 0.5, place_y + item.height * 0.5),
        ));
    }

    let (layout_width, layout_height) = placed_bounds(items, &placements)?;
    Some((placements, layout_width, layout_height))
}

#[cfg(test)]
fn legacy_skyline_candidate_y(
    skyline: &[LegacySkylineNode],
    start_index: usize,
    occupied_width: f32,
    bin_width: f32,
) -> Option<f32> {
    let start = skyline[start_index];
    if start.x + occupied_width > bin_width + 0.001 {
        return None;
    }

    let mut width_left = occupied_width;
    let mut index = start_index;
    let mut candidate_y = start.y;

    while width_left > 0.0 {
        let node = *skyline.get(index)?;
        candidate_y = candidate_y.max(node.y);
        width_left -= node.width;
        index += 1;
    }

    Some(candidate_y)
}

#[cfg(test)]
fn legacy_skyline_insert(
    skyline: &mut Vec<LegacySkylineNode>,
    node_index: usize,
    x: f32,
    y: f32,
    occupied_width: f32,
    occupied_height: f32,
) {
    let new_y = y + occupied_height;
    skyline.insert(
        node_index,
        LegacySkylineNode {
            x,
            y: new_y,
            width: occupied_width,
        },
    );

    let index = node_index + 1;
    let end_x = x + occupied_width;
    while index < skyline.len() {
        let node = skyline[index];
        if node.x >= end_x {
            break;
        }

        let overlap = end_x - node.x;
        if overlap >= node.width - 0.001 {
            skyline.remove(index);
        } else {
            skyline[index].x += overlap;
            skyline[index].width -= overlap;
            break;
        }
    }

    legacy_merge_skyline_nodes(skyline);
}

#[cfg(test)]
fn legacy_merge_skyline_nodes(skyline: &mut Vec<LegacySkylineNode>) {
    let mut index = 0;
    while index + 1 < skyline.len() {
        if (skyline[index].y - skyline[index + 1].y).abs() < 0.001 {
            skyline[index].width += skyline[index + 1].width;
            skyline.remove(index + 1);
        } else {
            index += 1;
        }
    }
}

fn placed_bounds(
    items: &[SelectedLayoutItem],
    placements: &[(i64, egui::Pos2)],
) -> Option<(f32, f32)> {
    let mut min_x = f32::INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut max_y = f32::NEG_INFINITY;

    for (item_id, center) in placements {
        let item = items.iter().find(|item| item.id == *item_id)?;
        min_x = min_x.min(center.x - item.width * 0.5);
        min_y = min_y.min(center.y - item.height * 0.5);
        max_x = max_x.max(center.x + item.width * 0.5);
        max_y = max_y.max(center.y + item.height * 0.5);
    }

    Some(((max_x - min_x).max(0.0), (max_y - min_y).max(0.0)))
}

#[derive(Clone, Copy, Debug)]
struct PackRect {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

impl PackRect {
    fn right(self) -> f32 {
        self.x + self.width
    }

    fn bottom(self) -> f32 {
        self.y + self.height
    }
}

#[cfg(test)]
#[derive(Clone, Copy)]
struct LegacySkylineNode {
    x: f32,
    y: f32,
    width: f32,
}

#[cfg(test)]
pub(crate) fn legacy_compact_layout_area_for_sizes(
    items: &[(i64, f32, f32)],
    gap: f32,
) -> Option<f32> {
    let items = items
        .iter()
        .map(|(id, width, height)| SelectedLayoutItem {
            id: *id,
            x: 0.0,
            y: 0.0,
            width: *width,
            height: *height,
        })
        .collect::<Vec<_>>();
    let (_, width, height) = legacy_best_compact_layout(&items, gap)?;
    Some(width * height)
}

pub fn show(ui: &mut egui::Ui, rect: egui::Rect, state: &mut CanvasState, debug_labels: bool) {
    let response = ui.allocate_rect(rect, egui::Sense::click_and_drag());
    handle_input(ui, rect, state, &response);

    let painter = ui.painter_at(rect);

    painter.rect_filled(rect, 0.0, crate::theme::SHADE_90);
    paint_images(&painter, rect, state, debug_labels);
    paint_selection_bounds(&painter, rect, state);
    paint_crop_marquee(&painter, rect, state);
    paint_marquee(&painter, state);
}

fn handle_input(
    ui: &egui::Ui,
    rect: egui::Rect,
    state: &mut CanvasState,
    response: &egui::Response,
) {
    let pointer_pos = ui.input(|i| i.pointer.latest_pos());
    let crop_key_held = ui.input(|i| i.key_down(egui::Key::C));
    let shift_held = ui.input(|i| i.modifiers.shift);
    let pointer_on_canvas = pointer_pos
        .map(|pointer_pos| ui.ctx().layer_id_at(pointer_pos) == Some(response.layer_id))
        .unwrap_or(false);

    if pointer_on_canvas && response.dragged_by(egui::PointerButton::Middle) {
        state.pan += response.drag_delta();
    }

    if response.hovered() {
        state.last_pointer_screen_pos = pointer_pos;
    }

    if pointer_on_canvas
        && ui.input(|i| i.pointer.button_pressed(egui::PointerButton::Primary))
        && let Some(pointer_pos) = pointer_pos
    {
        if crop_key_held && let Some(item_id) = hit_test_image(state, rect, pointer_pos) {
            if !state.can_start_crop_on_item(item_id) {
                return;
            }
            state.set_single_selection(item_id);
            state.drag_state = None;
            state.marquee_state = None;
            state.crop_state = Some(CropState {
                item_id,
                start_screen: pointer_pos,
                current_screen: pointer_pos,
            });
        } else if let Some(item_id) = hit_test_image(state, rect, pointer_pos) {
            if shift_held {
                state.add_to_selection(item_id);
            } else if !state.is_selected(item_id) {
                state.set_single_selection(item_id);
            }

            let pointer_world = state.screen_to_world(pointer_pos, rect);
            let starting_positions = state
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

            state.drag_state = Some(DragState {
                pointer_start_world: pointer_world,
                starting_positions,
            });
            state.marquee_state = None;
            state.crop_state = None;
        } else {
            state.drag_state = None;
            state.clear_selection();
            state.marquee_state = Some(MarqueeState {
                start_screen: pointer_pos,
                current_screen: pointer_pos,
            });
            state.crop_state = None;
        }
    }

    if let Some(crop_state) = &mut state.crop_state
        && ui.input(|i| i.pointer.primary_down())
        && let Some(pointer_pos) = pointer_pos
    {
        crop_state.current_screen = pointer_pos;
    }

    if state.crop_state.is_none()
        && let Some(drag_state) = &state.drag_state
        && ui.input(|i| i.pointer.primary_down())
        && let Some(pointer_pos) = pointer_pos
    {
        let pointer_world = state.screen_to_world(pointer_pos, rect);
        let world_delta = pointer_world - drag_state.pointer_start_world;

        for (item_id, start_pos) in &drag_state.starting_positions {
            if let Some(image) = state
                .images
                .iter_mut()
                .find(|image| image.record.item.id == *item_id)
            {
                image.record.item.x = start_pos.x + world_delta.x;
                image.record.item.y = start_pos.y + world_delta.y;
            }
        }
    }

    if ui.input(|i| i.pointer.button_released(egui::PointerButton::Primary)) {
        if let Some(crop_state) = state.crop_state.take() {
            state.pending_crop = state.build_crop_request(rect, &crop_state);
        } else if let Some(drag_state) = state.drag_state.take() {
            for (item_id, _) in drag_state.starting_positions {
                state.mark_dirty(item_id);
            }
        }
        state.marquee_state = None;
    }

    if state.crop_state.is_none()
        && let Some(marquee_state) = &mut state.marquee_state
        && ui.input(|i| i.pointer.primary_down())
        && let Some(pointer_pos) = pointer_pos
    {
        marquee_state.current_screen = pointer_pos;
        let selection_rect =
            egui::Rect::from_two_pos(marquee_state.start_screen, marquee_state.current_screen);
        let selected = state
            .images
            .iter()
            .filter(|image| image_screen_rect(state, rect, image).intersects(selection_rect))
            .map(|image| image.record.item.id)
            .collect();
        state.set_selection(selected);
    }

    if !pointer_on_canvas || !response.hovered() {
        return;
    }

    let Some(pointer_pos) = pointer_pos else {
        return;
    };

    if state.crop_state.is_some() {
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

fn hit_test_image(state: &CanvasState, rect: egui::Rect, pointer_pos: egui::Pos2) -> Option<i64> {
    state
        .images
        .iter()
        .rev()
        .find(|image| image_screen_rect(state, rect, image).contains(pointer_pos))
        .map(|image| image.record.item.id)
}

fn image_screen_rect(state: &CanvasState, rect: egui::Rect, image: &CanvasImage) -> egui::Rect {
    let item = &image.record.item;
    let asset = &image.record.asset;
    let center = state.world_to_screen(egui::pos2(item.x, item.y), rect);
    let size =
        egui::vec2(asset.width as f32, asset.height as f32) * item.scale.max(0.01) * state.zoom;
    egui::Rect::from_center_size(center, size)
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

fn paint_images(
    painter: &egui::Painter,
    rect: egui::Rect,
    state: &CanvasState,
    debug_labels: bool,
) {
    for image in &state.images {
        let image_rect = image_screen_rect(state, rect, image);

        painter.rect_filled(
            image_rect.expand(2.0),
            0.0,
            egui::Color32::from_rgba_premultiplied(0, 0, 0, 120),
        );
        painter.image(
            image.texture.id(),
            image_rect,
            egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );
        if state.is_selected(image.record.item.id) {
            painter.rect_stroke(
                image_rect,
                0.0,
                egui::Stroke::new(1.0, crate::theme::SHADE_BASE),
                egui::StrokeKind::Outside,
            );
        }

        if debug_labels {
            let label = format!(
                "#{}  ({:.0}, {:.0})",
                image.record.item.id, image.record.item.x, image.record.item.y
            );
            painter.text(
                image_rect.min + egui::vec2(6.0, 6.0),
                egui::Align2::LEFT_TOP,
                label,
                egui::FontId::proportional(12.0),
                crate::theme::SHADE_BASE,
            );
        }
    }
}

fn paint_marquee(painter: &egui::Painter, state: &CanvasState) {
    let Some(marquee_state) = &state.marquee_state else {
        return;
    };

    let rect = egui::Rect::from_two_pos(marquee_state.start_screen, marquee_state.current_screen);
    painter.rect_stroke(
        rect,
        0.0,
        egui::Stroke::new(1.0, crate::theme::SHADE_BASE),
        egui::StrokeKind::Outside,
    );
}

fn paint_crop_marquee(painter: &egui::Painter, rect: egui::Rect, state: &CanvasState) {
    let Some(crop_state) = &state.crop_state else {
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
        egui::Stroke::new(1.0, crate::theme::SHADE_BASE),
        egui::StrokeKind::Outside,
    );
    painter.rect_stroke(
        crop_rect,
        0.0,
        egui::Stroke::new(1.0, crate::theme::SHADE_BASE),
        egui::StrokeKind::Outside,
    );
}

fn paint_selection_bounds(painter: &egui::Painter, rect: egui::Rect, state: &CanvasState) {
    if state.selected_item_ids.len() <= 1 {
        return;
    }

    let Some(selection_rect) = state.selection_screen_rect(rect) else {
        return;
    };

    painter.rect_stroke(
        selection_rect.expand(6.0),
        0.0,
        egui::Stroke::new(1.0, crate::theme::SHADE_BASE),
        egui::StrokeKind::Outside,
    );
}

fn clamped_crop_screen_rect(
    start_screen: egui::Pos2,
    current_screen: egui::Pos2,
    image_rect: egui::Rect,
) -> Option<egui::Rect> {
    let clamped_current = egui::pos2(
        current_screen
            .x
            .clamp(image_rect.left(), image_rect.right()),
        current_screen
            .y
            .clamp(image_rect.top(), image_rect.bottom()),
    );
    let rect = egui::Rect::from_two_pos(start_screen, clamped_current);
    (rect.width() > 0.0 && rect.height() > 0.0).then_some(rect)
}

fn screen_rect_to_local_crop_rect(
    screen_rect: egui::Rect,
    image_rect: egui::Rect,
    asset_width: u32,
    asset_height: u32,
) -> Option<egui::Rect> {
    let image_size = image_rect.size();
    if image_size.x <= 0.0 || image_size.y <= 0.0 {
        return None;
    }

    let min = egui::pos2(
        ((screen_rect.min.x - image_rect.min.x) / image_size.x * asset_width as f32)
            .clamp(0.0, asset_width as f32),
        ((screen_rect.min.y - image_rect.min.y) / image_size.y * asset_height as f32)
            .clamp(0.0, asset_height as f32),
    );
    let max = egui::pos2(
        ((screen_rect.max.x - image_rect.min.x) / image_size.x * asset_width as f32)
            .clamp(0.0, asset_width as f32),
        ((screen_rect.max.y - image_rect.min.y) / image_size.y * asset_height as f32)
            .clamp(0.0, asset_height as f32),
    );

    Some(egui::Rect::from_min_max(min, max))
}

fn pixel_crop_rect_from_local(
    local_rect: egui::Rect,
    asset_width: u32,
    asset_height: u32,
) -> Option<PixelCropRect> {
    let min_x = local_rect.min.x.floor().clamp(0.0, asset_width as f32) as u32;
    let min_y = local_rect.min.y.floor().clamp(0.0, asset_height as f32) as u32;
    let max_x = local_rect.max.x.ceil().clamp(0.0, asset_width as f32) as u32;
    let max_y = local_rect.max.y.ceil().clamp(0.0, asset_height as f32) as u32;
    let width = max_x.saturating_sub(min_x);
    let height = max_y.saturating_sub(min_y);

    (width > 0 && height > 0).then_some(PixelCropRect {
        x: min_x,
        y: min_y,
        width,
        height,
    })
}

fn crop_center_offset_local(asset: &crate::data::ImageAsset, local_rect: egui::Rect) -> egui::Vec2 {
    let asset_center = egui::pos2(asset.width as f32 * 0.5, asset.height as f32 * 0.5);
    let local_center = local_rect.center();
    local_center - asset_center
}

fn merge_cropped_item_fields(
    item: &mut crate::data::BoardItem,
    asset_id: i64,
    crop_offset_local: egui::Vec2,
) {
    item.asset_id = asset_id;
    let scale = item.scale.max(0.01);
    item.x += crop_offset_local.x * scale;
    item.y += crop_offset_local.y * scale;
}

#[cfg(test)]
mod tests {
    use super::{
        SelectedLayoutItem, best_compact_layout, crop_center_offset_local, horizontal_layout,
        legacy_best_compact_layout, merge_cropped_item_fields, pixel_crop_rect_from_local,
        screen_rect_to_local_crop_rect, vertical_layout,
    };
    use crate::data::{BoardItem, ImageAsset};
    use std::path::PathBuf;

    #[test]
    fn converts_drag_rect_to_crop_pixels_and_live_scale_position() {
        let screen_rect = eframe::egui::Rect::from_min_max(
            eframe::egui::pos2(40.0, 20.0),
            eframe::egui::pos2(240.0, 180.0),
        );
        let local_rect = screen_rect_to_local_crop_rect(screen_rect, screen_rect, 100, 80).unwrap();
        let crop_rect = pixel_crop_rect_from_local(local_rect, 100, 80).unwrap();

        assert_eq!(crop_rect.x, 0);
        assert_eq!(crop_rect.y, 0);
        assert_eq!(crop_rect.width, 100);
        assert_eq!(crop_rect.height, 80);

        let item = BoardItem {
            id: 1,
            asset_id: 1,
            x: 200.0,
            y: 120.0,
            scale: 2.0,
            rotation: 0.0,
            z_index: 0,
            archived: false,
        };
        let asset = ImageAsset {
            id: 1,
            original_path: PathBuf::from("image.png"),
            filename: "image.png".to_owned(),
            mime_type: "image/png".to_owned(),
            width: 100,
            height: 80,
            file_size: 0,
            imported_at: 0,
        };
        let kept_local = eframe::egui::Rect::from_min_max(
            eframe::egui::pos2(10.0, 20.0),
            eframe::egui::pos2(40.0, 50.0),
        );
        let crop_offset_local = crop_center_offset_local(&asset, kept_local);
        let mut merged_item = item.clone();
        merge_cropped_item_fields(&mut merged_item, 2, crop_offset_local);

        assert_eq!(merged_item.asset_id, 2);
        assert_eq!(merged_item.x, 150.0);
        assert_eq!(merged_item.y, 110.0);
    }

    #[test]
    fn rounds_local_rect_outward_to_cover_selected_pixels() {
        let local_rect = eframe::egui::Rect::from_min_max(
            eframe::egui::pos2(10.2, 11.8),
            eframe::egui::pos2(40.1, 30.2),
        );
        let crop_rect = pixel_crop_rect_from_local(local_rect, 100, 100).unwrap();

        assert_eq!(crop_rect.x, 10);
        assert_eq!(crop_rect.y, 11);
        assert_eq!(crop_rect.width, 31);
        assert_eq!(crop_rect.height, 20);
    }

    #[test]
    fn merge_only_updates_crop_owned_item_fields() {
        let mut item = BoardItem {
            id: 7,
            asset_id: 2,
            x: 12.0,
            y: 18.0,
            scale: 1.5,
            rotation: 0.25,
            z_index: 9,
            archived: false,
        };

        merge_cropped_item_fields(&mut item, 11, eframe::egui::vec2(80.0, 96.0));

        assert_eq!(item.asset_id, 11);
        assert_eq!(item.x, 132.0);
        assert_eq!(item.y, 162.0);
        assert_eq!(item.scale, 1.5);
        assert_eq!(item.rotation, 0.25);
        assert_eq!(item.z_index, 9);
        assert!(!item.archived);
    }

    #[test]
    fn pending_crop_guard_blocks_second_crop_until_cleared() {
        let mut state = super::CanvasState::default();

        assert!(state.can_start_crop_on_item(42));
        state.mark_crop_pending(42);
        assert!(!state.can_start_crop_on_item(42));
        state.clear_crop_pending(42);
        assert!(state.can_start_crop_on_item(42));
    }

    #[test]
    fn horizontal_and_vertical_layouts_use_requested_gap() {
        let items = vec![
            layout_item(1, 80.0, 40.0),
            layout_item(2, 60.0, 40.0),
            layout_item(3, 40.0, 40.0),
        ];
        let gap = 18.0;

        let (horizontal, _, _) = horizontal_layout(&items, gap).unwrap();
        let (vertical, _, _) = vertical_layout(&items, gap).unwrap();

        assert_eq!(center_gap_x(&items, &horizontal, 1, 2), gap);
        assert_eq!(center_gap_y(&items, &vertical, 1, 2), gap);
    }

    #[test]
    fn compact_layout_respects_gap() {
        let items = vec![
            layout_item(1, 80.0, 80.0),
            layout_item(2, 80.0, 60.0),
            layout_item(3, 60.0, 80.0),
            layout_item(4, 60.0, 60.0),
        ];
        let gap = 12.0;

        let (placements, _, _) = best_compact_layout(&items, gap).unwrap();

        for left in &items {
            for right in &items {
                if left.id >= right.id {
                    continue;
                }
                let left_rect = placement_rect(&items, &placements, left.id);
                let right_rect = placement_rect(&items, &placements, right.id);
                assert!(rectangles_respect_gap(left_rect, right_rect, gap));
            }
        }
    }

    #[test]
    fn compact_layout_is_more_square_than_row_or_column_extremes() {
        let items = vec![
            layout_item(1, 180.0, 90.0),
            layout_item(2, 140.0, 80.0),
            layout_item(3, 110.0, 160.0),
            layout_item(4, 90.0, 110.0),
            layout_item(5, 70.0, 70.0),
        ];
        let gap = 12.0;

        let (_, compact_width, compact_height) = best_compact_layout(&items, gap).unwrap();
        let (_, row_width, row_height) = horizontal_layout(&items, gap).unwrap();
        let (_, column_width, column_height) = vertical_layout(&items, gap).unwrap();

        let compact_ratio = aspect_ratio(compact_width, compact_height);
        let row_ratio = aspect_ratio(row_width, row_height);
        let column_ratio = aspect_ratio(column_width, column_height);

        assert!(compact_ratio < row_ratio);
        assert!(compact_ratio < column_ratio);
    }

    #[test]
    fn compact_layout_is_deterministic_for_mixed_sizes() {
        let items = hole_prone_items();
        let gap = 12.0;

        let first = best_compact_layout(&items, gap).unwrap();
        let second = best_compact_layout(&items, gap).unwrap();

        assert_eq!(first.1, second.1);
        assert_eq!(first.2, second.2);
        assert_eq!(first.0.len(), second.0.len());
        for ((first_id, first_pos), (second_id, second_pos)) in first.0.iter().zip(second.0.iter()) {
            assert_eq!(first_id, second_id);
            assert!((first_pos.x - second_pos.x).abs() < 0.001);
            assert!((first_pos.y - second_pos.y).abs() < 0.001);
        }
    }

    #[test]
    fn compact_layout_has_smaller_area_than_legacy_skyline_on_hole_prone_fixture() {
        let items = hole_prone_items();
        let gap = 12.0;

        let (_, compact_width, compact_height) = best_compact_layout(&items, gap).unwrap();
        let (_, legacy_width, legacy_height) = legacy_best_compact_layout(&items, gap).unwrap();

        assert!(compact_width * compact_height < legacy_width * legacy_height);
    }

    #[test]
    fn compact_layout_has_no_overlaps_for_hole_prone_fixture() {
        let items = hole_prone_items();
        let gap = 12.0;
        let (placements, _, _) = best_compact_layout(&items, gap).unwrap();

        for left in &items {
            for right in &items {
                if left.id >= right.id {
                    continue;
                }
                let left_rect = placement_rect(&items, &placements, left.id);
                let right_rect = placement_rect(&items, &placements, right.id);
                assert!(!left_rect.intersects(right_rect));
            }
        }
    }

    #[test]
    fn compact_layout_handles_larger_mixed_selection() {
        let mut items = Vec::new();
        let base = [
            (300.0, 520.0),
            (180.0, 260.0),
            (160.0, 260.0),
            (150.0, 220.0),
            (140.0, 180.0),
            (220.0, 140.0),
        ];
        for index in 0..48 {
            let (width, height) = base[index % base.len()];
            items.push(layout_item(index as i64 + 1, width, height));
        }

        let gap = 12.0;
        let (placements, _, _) = best_compact_layout(&items, gap).unwrap();

        assert_eq!(placements.len(), items.len());
        for left in &items {
            for right in &items {
                if left.id >= right.id {
                    continue;
                }
                let left_rect = placement_rect(&items, &placements, left.id);
                let right_rect = placement_rect(&items, &placements, right.id);
                assert!(rectangles_respect_gap(left_rect, right_rect, gap));
            }
        }
    }

    fn layout_item(id: i64, width: f32, height: f32) -> SelectedLayoutItem {
        SelectedLayoutItem {
            id,
            x: 0.0,
            y: 0.0,
            width,
            height,
        }
    }

    fn center_gap_x(
        items: &[SelectedLayoutItem],
        placements: &[(i64, eframe::egui::Pos2)],
        left_id: i64,
        right_id: i64,
    ) -> f32 {
        let left = placement_rect(items, placements, left_id);
        let right = placement_rect(items, placements, right_id);
        right.left() - left.right()
    }

    fn center_gap_y(
        items: &[SelectedLayoutItem],
        placements: &[(i64, eframe::egui::Pos2)],
        top_id: i64,
        bottom_id: i64,
    ) -> f32 {
        let top = placement_rect(items, placements, top_id);
        let bottom = placement_rect(items, placements, bottom_id);
        bottom.top() - top.bottom()
    }

    fn placement_rect(
        items: &[SelectedLayoutItem],
        placements: &[(i64, eframe::egui::Pos2)],
        item_id: i64,
    ) -> eframe::egui::Rect {
        let item = items.iter().find(|item| item.id == item_id).unwrap();
        let center = placements
            .iter()
            .find(|(placement_id, _)| *placement_id == item_id)
            .map(|(_, center)| *center)
            .unwrap();
        eframe::egui::Rect::from_center_size(center, eframe::egui::vec2(item.width, item.height))
    }

    fn aspect_ratio(width: f32, height: f32) -> f32 {
        width.max(height) / width.min(height).max(1.0)
    }

    fn hole_prone_items() -> Vec<SelectedLayoutItem> {
        vec![
            layout_item(1, 300.0, 520.0),
            layout_item(2, 180.0, 260.0),
            layout_item(3, 160.0, 260.0),
            layout_item(4, 150.0, 220.0),
            layout_item(5, 150.0, 220.0),
            layout_item(6, 140.0, 180.0),
            layout_item(7, 140.0, 180.0),
        ]
    }

    fn rectangles_respect_gap(
        left: eframe::egui::Rect,
        right: eframe::egui::Rect,
        gap: f32,
    ) -> bool {
        left.right() + gap <= right.left()
            || right.right() + gap <= left.left()
            || left.bottom() + gap <= right.top()
            || right.bottom() + gap <= left.top()
    }
}
