use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::Arc;

use eframe::egui;

use crate::data::{
    BoardItemWithAsset, CanvasAnnotation, CanvasAnnotationKind, CanvasBackground, UserSession,
};
use crate::jobs::{CroppedCanvasItem, LoadedCanvasItem};

const MIN_CANVAS_ZOOM: f32 = 0.01;
const MAX_CANVAS_ZOOM: f32 = 64.0;
const MIN_IMAGE_WORLD_SIZE: f32 = 24.0;
const IMAGE_RESIZE_HIT_RADIUS: f32 = 14.0;
const MIN_TEXTBOX_WORLD_SIZE: egui::Vec2 = egui::vec2(240.0, 56.0);
const TEXTBOX_FONT_SIZE: f32 = 20.0;
const TEXTBOX_RESIZE_HIT_RADIUS: f32 = 14.0;
const TEXTBOX_DRAG_THRESHOLD_SCREEN: f32 = 4.0;
const TEXTBOX_EMPTY_HOTSPOT_MIN_WIDTH: f32 = 80.0;
const TEXTBOX_EMPTY_HOTSPOT_MAX_WIDTH: f32 = 120.0;
const ANNOTATION_HIT_RADIUS: f32 = 10.0;
const BACKGROUND_BASE_WORLD_STEP: f32 = 64.0;
const BACKGROUND_MIN_SCREEN_STEP: f32 = 40.0;
const BACKGROUND_MAX_SCREEN_STEP: f32 = 112.0;
const BACKGROUND_MIN_LOD_LEVEL: i32 = -12;
const BACKGROUND_MAX_LOD_LEVEL: i32 = 12;

pub struct CanvasState {
    pan: egui::Vec2,
    zoom: f32,
    background: CanvasBackground,
    background_lod_level: i32,
    images: Vec<CanvasImage>,
    annotations: Vec<CanvasAnnotation>,
    asset_rgba_cache: HashMap<i64, Vec<u8>>,
    selected_item_ids: Vec<i64>,
    selected_annotation_id: Option<i64>,
    editing_text_annotation_id: Option<i64>,
    focus_text_editor: bool,
    drag_state: Option<DragState>,
    image_transform_state: Option<ImageTransformState>,
    textbox_transform_state: Option<TextboxTransformState>,
    marquee_state: Option<MarqueeState>,
    crop_state: Option<CropState>,
    annotation_draft_state: Option<AnnotationDraftState>,
    crop_pending_item_ids: Vec<i64>,
    pending_crop: Option<CanvasCropRequest>,
    dirty_item_ids: Vec<i64>,
    dirty_annotation_ids: Vec<i64>,
    pending_archived_annotation_ids: Vec<i64>,
    pending_annotation: Option<PendingAnnotation>,
    last_pointer_screen_pos: Option<egui::Pos2>,
    textbox_layout_cache: HashMap<i64, TextboxLayoutCacheEntry>,
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

#[derive(Clone, Copy)]
enum ImageTransformState {
    Resizing {
        item_id: i64,
        center_screen: egui::Pos2,
        start_scale: f32,
        start_radius_screen: f32,
    },
}

#[derive(Clone, Copy)]
enum TextboxTransformState {
    PendingMove {
        annotation_id: i64,
        pointer_start_world: egui::Pos2,
        pointer_start_screen: egui::Pos2,
        start_rect_world: egui::Rect,
    },
    Moving {
        annotation_id: i64,
        pointer_start_world: egui::Pos2,
        start_rect_world: egui::Rect,
    },
    Resizing {
        annotation_id: i64,
        dragged_handle: TextboxResizeHandle,
        anchor_world: egui::Pos2,
        start_rect_world: egui::Rect,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TextboxResizeHandle {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
    Top,
    Right,
    Bottom,
    Left,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ImageResizeHandle {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TextboxPointerRegion {
    Resize(TextboxResizeHandle),
    Text,
    Body,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TextboxLayoutCacheKey {
    text_hash: u64,
    wrap_width_bits: u32,
    zoom_bits: u32,
}

#[derive(Clone)]
struct TextboxLayoutCacheEntry {
    key: TextboxLayoutCacheKey,
    galley: Arc<egui::Galley>,
    required_world_height: f32,
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

#[derive(Clone)]
struct AnnotationDraftState {
    kind: CanvasAnnotationKind,
    start_world: egui::Pos2,
    current_world: egui::Pos2,
}

#[derive(Clone, Debug)]
pub struct PendingAnnotation {
    pub kind: CanvasAnnotationKind,
    pub start_world: egui::Pos2,
    pub end_world: egui::Pos2,
    pub text: String,
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
            background: CanvasBackground::default(),
            background_lod_level: 0,
            images: Vec::new(),
            annotations: Vec::new(),
            asset_rgba_cache: HashMap::new(),
            selected_item_ids: Vec::new(),
            selected_annotation_id: None,
            editing_text_annotation_id: None,
            focus_text_editor: false,
            drag_state: None,
            image_transform_state: None,
            textbox_transform_state: None,
            marquee_state: None,
            crop_state: None,
            annotation_draft_state: None,
            crop_pending_item_ids: Vec::new(),
            pending_crop: None,
            dirty_item_ids: Vec::new(),
            dirty_annotation_ids: Vec::new(),
            pending_archived_annotation_ids: Vec::new(),
            pending_annotation: None,
            last_pointer_screen_pos: None,
            textbox_layout_cache: HashMap::new(),
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
        self.background = session.canvas_background;
    }

    pub fn user_session(&self) -> UserSession {
        UserSession {
            camera_pan_x: self.pan.x,
            camera_pan_y: self.pan.y,
            camera_zoom: self.zoom,
            auto_image_scaling: true,
            import_gap: crate::data::DEFAULT_IMPORT_GAP,
            canvas_background: self.background,
        }
    }

    pub fn has_selection(&self) -> bool {
        self.has_image_selection() || self.selected_annotation_id.is_some()
    }

    pub fn has_image_selection(&self) -> bool {
        !self.selected_item_ids.is_empty()
    }

    pub fn selected_item_ids(&self) -> Vec<i64> {
        self.selected_item_ids.clone()
    }

    pub fn background(&self) -> CanvasBackground {
        self.background
    }

    pub fn set_background(&mut self, background: CanvasBackground) {
        self.background = background;
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

    pub fn take_pending_annotation(&mut self) -> Option<PendingAnnotation> {
        self.pending_annotation.take()
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

    pub fn add_annotation(&mut self, annotation: CanvasAnnotation) {
        self.invalidate_textbox_layout_cache(annotation.id);
        self.annotations.retain(|item| item.id != annotation.id);
        self.annotations.push(annotation.clone());
        if annotation.kind == CanvasAnnotationKind::Textbox
            && self.editing_text_annotation_id.is_none()
        {
            self.selected_annotation_id = Some(annotation.id);
        }
    }

    pub fn begin_textbox_edit(&mut self, annotation_id: i64) {
        if self.annotations.iter().any(|annotation| {
            annotation.id == annotation_id && annotation.kind == CanvasAnnotationKind::Textbox
        }) {
            self.selected_annotation_id = Some(annotation_id);
            self.editing_text_annotation_id = Some(annotation_id);
            self.focus_text_editor = true;
            self.selected_item_ids.clear();
            self.image_transform_state = None;
            self.textbox_transform_state = None;
        }
    }

    pub fn add_annotations(&mut self, annotations: Vec<CanvasAnnotation>) {
        self.annotations = annotations;
        self.textbox_layout_cache.clear();
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

    pub fn take_dirty_annotations(&mut self) -> Vec<CanvasAnnotation> {
        let dirty_ids = std::mem::take(&mut self.dirty_annotation_ids);
        dirty_ids
            .into_iter()
            .filter_map(|id| {
                self.annotations
                    .iter()
                    .find(|annotation| annotation.id == id)
                    .cloned()
            })
            .collect()
    }

    pub fn take_archived_annotation_ids(&mut self) -> Vec<i64> {
        std::mem::take(&mut self.pending_archived_annotation_ids)
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

        if let Some(annotation) = self.selected_annotation() {
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
        self.finish_textbox_edit();
        self.selected_item_ids = item_ids
            .iter()
            .copied()
            .filter(|item_id| {
                self.images
                    .iter()
                    .any(|image| image.record.item.id == *item_id)
            })
            .collect();
        self.selected_annotation_id = None;
        self.editing_text_annotation_id = None;
        self.image_transform_state = None;
        self.textbox_transform_state = None;
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
    pub fn delete_selected_images(&mut self) -> Vec<i64> {
        let selected_ids = self.selected_item_ids.clone();
        self.delete_items(&selected_ids)
    }

    pub fn delete_selected_annotation(&mut self) -> Option<i64> {
        let selected_id = self.selected_annotation_id?;
        self.remove_annotation_locally(selected_id);
        Some(selected_id)
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
        self.image_transform_state = None;
        self.textbox_transform_state = None;
        self.marquee_state = None;
        self.crop_state = None;
        self.annotation_draft_state = None;
        self.crop_pending_item_ids
            .retain(|item_id| !removed_ids.contains(item_id));
        self.pending_crop = None;
        removed_ids
    }

    fn is_selected(&self, item_id: i64) -> bool {
        self.selected_item_ids.contains(&item_id)
    }

    fn set_single_selection(&mut self, item_id: i64) {
        self.finish_textbox_edit();
        self.selected_item_ids.clear();
        self.selected_item_ids.push(item_id);
        self.selected_annotation_id = None;
        self.editing_text_annotation_id = None;
        self.image_transform_state = None;
        self.textbox_transform_state = None;
    }

    fn add_to_selection(&mut self, item_id: i64) {
        self.finish_textbox_edit();
        if !self.selected_item_ids.contains(&item_id) {
            self.selected_item_ids.push(item_id);
        }
        self.image_transform_state = None;
        self.textbox_transform_state = None;
    }

    fn clear_selection(&mut self) {
        self.finish_textbox_edit();
        self.selected_item_ids.clear();
        self.selected_annotation_id = None;
        self.editing_text_annotation_id = None;
        self.image_transform_state = None;
        self.textbox_transform_state = None;
    }

    fn set_marquee_selection(&mut self, item_ids: Vec<i64>, annotation_id: Option<i64>) {
        self.finish_textbox_edit();
        self.selected_item_ids = item_ids;
        self.selected_annotation_id = if self.selected_item_ids.is_empty() {
            annotation_id
        } else {
            None
        };
        self.editing_text_annotation_id = None;
        self.image_transform_state = None;
        self.textbox_transform_state = None;
    }

    fn mark_dirty(&mut self, item_id: i64) {
        if item_id > 0 && !self.dirty_item_ids.contains(&item_id) {
            self.dirty_item_ids.push(item_id);
        }
    }

    fn mark_annotation_dirty(&mut self, annotation_id: i64) {
        if annotation_id > 0 && !self.dirty_annotation_ids.contains(&annotation_id) {
            self.dirty_annotation_ids.push(annotation_id);
        }
    }

    fn remove_annotation_locally(&mut self, annotation_id: i64) {
        self.annotations
            .retain(|annotation| annotation.id != annotation_id);
        self.invalidate_textbox_layout_cache(annotation_id);
        if self.selected_annotation_id == Some(annotation_id) {
            self.selected_annotation_id = None;
        }
        if self.editing_text_annotation_id == Some(annotation_id) {
            self.editing_text_annotation_id = None;
        }
        self.focus_text_editor = false;
        self.image_transform_state = None;
        if self.textbox_transform_state.is_some() {
            self.textbox_transform_state = None;
        }
        self.dirty_annotation_ids
            .retain(|dirty_id| *dirty_id != annotation_id);
    }

    fn finish_textbox_edit(&mut self) {
        let Some(annotation_id) = self.editing_text_annotation_id else {
            self.focus_text_editor = false;
            return;
        };

        self.editing_text_annotation_id = None;
        self.focus_text_editor = false;
        self.image_transform_state = None;
        self.textbox_transform_state = None;

        let Some(annotation) = self
            .annotations
            .iter()
            .find(|annotation| annotation.id == annotation_id)
        else {
            if self.selected_annotation_id == Some(annotation_id) {
                self.selected_annotation_id = None;
            }
            return;
        };

        if annotation.kind == CanvasAnnotationKind::Textbox && annotation.text.trim().is_empty() {
            self.remove_annotation_locally(annotation_id);
            if annotation_id > 0
                && !self
                    .pending_archived_annotation_ids
                    .contains(&annotation_id)
            {
                self.pending_archived_annotation_ids.push(annotation_id);
            }
        }
    }

    fn stop_textbox_edit_without_cleanup(&mut self) {
        self.editing_text_annotation_id = None;
        self.focus_text_editor = false;
        self.image_transform_state = None;
        self.textbox_transform_state = None;
    }

    fn annotation_by_id(&self, annotation_id: i64) -> Option<&CanvasAnnotation> {
        self.annotations
            .iter()
            .find(|annotation| annotation.id == annotation_id)
    }

    fn annotation_by_id_mut(&mut self, annotation_id: i64) -> Option<&mut CanvasAnnotation> {
        self.annotations
            .iter_mut()
            .find(|annotation| annotation.id == annotation_id)
    }

    fn set_textbox_world_rect(&mut self, annotation_id: i64, world_rect: egui::Rect) -> bool {
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

    fn invalidate_textbox_layout_cache(&mut self, annotation_id: i64) {
        self.textbox_layout_cache.remove(&annotation_id);
    }

    fn cache_textbox_layout(
        &mut self,
        annotation_id: i64,
        text: &str,
        wrap_width: f32,
        zoom: f32,
        galley: Arc<egui::Galley>,
    ) -> TextboxLayoutCacheEntry {
        let key = textbox_layout_cache_key(text, wrap_width, zoom);
        let entry = TextboxLayoutCacheEntry {
            key,
            required_world_height: textbox_required_world_height(galley.size().y, zoom),
            galley,
        };
        self.textbox_layout_cache.insert(annotation_id, entry.clone());
        entry
    }

    fn cached_textbox_layout(
        &mut self,
        ui: &egui::Ui,
        annotation_id: i64,
        text: &str,
        wrap_width: f32,
        zoom: f32,
    ) -> TextboxLayoutCacheEntry {
        let key = textbox_layout_cache_key(text, wrap_width, zoom);
        if let Some(entry) = self.textbox_layout_cache.get(&annotation_id)
            && entry.key == key
        {
            return entry.clone();
        }

        let galley = layout_textbox_galley(ui, text, wrap_width, zoom);
        self.cache_textbox_layout(annotation_id, text, wrap_width, zoom, galley)
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

    fn image_by_item_id_mut(&mut self, item_id: i64) -> Option<&mut CanvasImage> {
        self.images
            .iter_mut()
            .find(|image| image.record.item.id == item_id)
    }

    fn selected_single_image(&self) -> Option<&CanvasImage> {
        (self.selected_item_ids.len() == 1)
            .then(|| self.image_by_item_id(self.selected_item_ids[0]))
            .flatten()
    }

    fn selected_annotation(&self) -> Option<&CanvasAnnotation> {
        let selected_id = self.selected_annotation_id?;
        self.annotations
            .iter()
            .find(|annotation| annotation.id == selected_id)
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

    fn set_image_scale(&mut self, item_id: i64, scale: f32) -> bool {
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

    let total_height = items.iter().map(|item| item.height + gap).sum::<f32>().max(
        items
            .iter()
            .map(|item| item.height + gap)
            .fold(0.0_f32, f32::max),
    );
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
                    || ((short_side_fit - best_short).abs() < 0.001
                        && long_side_fit > best_long)
                    || ((short_side_fit - best_short).abs() < 0.001
                        && (long_side_fit - best_long).abs() < 0.001
                        && (placed.y > best_rect.y
                            || ((placed.y - best_rect.y).abs() < 0.001
                                && placed.x >= best_rect.x))) => {}
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
            let contained = free_rects.iter().enumerate().any(|(other_index, other)| {
                other_index != index && rect_contained_in(rect, *other)
            });
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
                if let Some(merged) =
                    merge_adjacent_free_rects(free_rects[left_index], free_rects[right_index])
                {
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

    painter.rect_filled(rect, 0.0, crate::theme::BACKGROUND);
    paint_background(&painter, rect, state);
    paint_images(&painter, rect, state, debug_labels);
    paint_annotations(&painter, ui, rect, state);
    paint_selection_bounds(&painter, rect, state);
    paint_crop_marquee(&painter, rect, state);
    paint_annotation_draft(&painter, rect, state);
    paint_marquee(&painter, state);
    show_textbox_editor(ui, rect, state);
}

fn handle_input(
    ui: &egui::Ui,
    rect: egui::Rect,
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
    let pointer_in_canvas_rect = pointer_pos
        .map(|pointer_pos| rect.contains(pointer_pos))
        .unwrap_or(false);

    if pointer_in_canvas_rect && response.dragged_by(egui::PointerButton::Middle) {
        state.pan += response.drag_delta();
    }

    if pointer_in_canvas_rect {
        state.last_pointer_screen_pos = pointer_pos;
    }

    if let Some(pointer_pos) = pointer_pos
        && let Some(annotation_id) = state.selected_annotation_id
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
    }

    if pointer_in_canvas_rect
        && primary_pressed
        && let Some(pointer_pos) = pointer_pos
    {
        if let Some(kind) = draw_kind {
            let pointer_world = state.screen_to_world(pointer_pos, rect);
            state.clear_selection();
            state.drag_state = None;
            state.image_transform_state = None;
            state.textbox_transform_state = None;
            state.marquee_state = None;
            state.crop_state = None;
            state.annotation_draft_state = Some(AnnotationDraftState {
                kind,
                start_world: pointer_world,
                current_world: pointer_world,
            });
        } else if crop_key_held && let Some(item_id) = hit_test_image(state, rect, pointer_pos) {
            if !state.can_start_crop_on_item(item_id) {
                return;
            }
            state.set_single_selection(item_id);
            state.drag_state = None;
            state.textbox_transform_state = None;
            state.marquee_state = None;
            state.annotation_draft_state = None;
            state.crop_state = Some(CropState {
                item_id,
                start_screen: pointer_pos,
                current_screen: pointer_pos,
            });
        } else if let Some(image) = state.selected_single_image().map(|image| {
            (
                image.record.item.id,
                hit_test_image_resize_handle(state, rect, image, pointer_pos),
            )
        }) && let (item_id, Some(handle)) = image
        {
            begin_image_resize(state, rect, item_id, handle);
            return;
        } else if let Some(annotation_id) = state.selected_annotation_id
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
            if let Some(annotation) = annotation
                && annotation.kind == CanvasAnnotationKind::Textbox
            {
                if handle_textbox_primary_press(ui, rect, state, &annotation, pointer_pos) {
                    return;
                }
            } else {
                if state.editing_text_annotation_id.is_some() {
                    state.finish_textbox_edit();
                }
                state.selected_item_ids.clear();
                state.selected_annotation_id = Some(annotation_id);
                state.image_transform_state = None;
                state.textbox_transform_state = None;
                state.drag_state = None;
                state.marquee_state = None;
                state.crop_state = None;
                state.annotation_draft_state = None;
            }
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
            state.image_transform_state = None;
            state.textbox_transform_state = None;
            state.marquee_state = None;
            state.crop_state = None;
            state.annotation_draft_state = None;
        } else {
            state.drag_state = None;
            state.image_transform_state = None;
            state.textbox_transform_state = None;
            state.clear_selection();
            state.marquee_state = Some(MarqueeState {
                start_screen: pointer_pos,
                current_screen: pointer_pos,
            });
            state.crop_state = None;
            state.annotation_draft_state = None;
        }
    }

    if let Some(crop_state) = &mut state.crop_state
        && primary_down
        && let Some(pointer_pos) = pointer_pos
    {
        crop_state.current_screen = pointer_pos;
    }

    if primary_down
        && let Some(pointer_pos) = pointer_pos
        && let Some(draft_state) = &mut state.annotation_draft_state
    {
        let world = (pointer_pos - rect.center() - state.pan) / state.zoom;
        let pointer_world = egui::pos2(world.x, world.y);
        draft_state.current_world = pointer_world;
    }

    if state.crop_state.is_none()
        && state.annotation_draft_state.is_none()
        && primary_down
        && let Some(pointer_pos) = pointer_pos
        && let Some(transform_state) = state.textbox_transform_state
    {
        match transform_state {
            TextboxTransformState::PendingMove {
                annotation_id,
                pointer_start_world,
                pointer_start_screen,
                start_rect_world,
            } => {
                if pointer_pos.distance(pointer_start_screen) >= TEXTBOX_DRAG_THRESHOLD_SCREEN {
                    state.textbox_transform_state = Some(TextboxTransformState::Moving {
                        annotation_id,
                        pointer_start_world,
                        start_rect_world,
                    });
                }
            }
            TextboxTransformState::Moving {
                annotation_id,
                pointer_start_world,
                start_rect_world,
            } => {
                let pointer_world = state.screen_to_world(pointer_pos, rect);
                let world_rect =
                    moved_textbox_rect(start_rect_world, pointer_start_world, pointer_world);
                state.set_textbox_world_rect(annotation_id, world_rect);
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

    if state.crop_state.is_none()
        && state.annotation_draft_state.is_none()
        && primary_down
        && let Some(pointer_pos) = pointer_pos
        && let Some(transform_state) = state.image_transform_state
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
        }
    }

    if state.crop_state.is_none()
        && state.annotation_draft_state.is_none()
        && let Some(drag_state) = &state.drag_state
        && primary_down
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

    if primary_released {
        if let Some(crop_state) = state.crop_state.take() {
            state.pending_crop = state.build_crop_request(rect, &crop_state);
        } else if let Some(draft_state) = state.annotation_draft_state.take() {
            state.pending_annotation = pending_annotation_from_draft(draft_state);
        } else if let Some(transform_state) = state.image_transform_state.take() {
            let ImageTransformState::Resizing { item_id, .. } = transform_state;
            state.mark_dirty(item_id);
        } else if let Some(transform_state) = state.textbox_transform_state.take() {
            if let TextboxTransformState::Moving { annotation_id, .. }
            | TextboxTransformState::Resizing { annotation_id, .. } = transform_state
            {
                state.mark_annotation_dirty(annotation_id);
            }
        } else if let Some(drag_state) = state.drag_state.take() {
            for (item_id, _) in drag_state.starting_positions {
                state.mark_dirty(item_id);
            }
        }
        state.marquee_state = None;
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

    if state.crop_state.is_none()
        && state.annotation_draft_state.is_none()
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
            .collect::<Vec<_>>();
        let selected_annotation_id = if selected.is_empty() {
            state
                .annotations
                .iter()
                .rev()
                .find(|annotation| {
                    annotation.kind == CanvasAnnotationKind::Textbox
                        && annotation_screen_rect(state, rect, annotation).intersects(selection_rect)
                })
                .map(|annotation| annotation.id)
        } else {
            None
        };
        state.set_marquee_selection(selected, selected_annotation_id);
    }

    if !pointer_in_canvas_rect {
        return;
    }

    let Some(pointer_pos) = pointer_pos else {
        return;
    };

    if state.crop_state.is_some() || state.annotation_draft_state.is_some() {
        return;
    }

    if state.selected_annotation_id.is_some()
        && ui.input(|i| i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace))
    {
        return;
    }

    if let Some(annotation_id) = state.selected_annotation_id
        && ui.input(|i| i.key_pressed(egui::Key::Enter))
    {
        state.begin_textbox_edit(annotation_id);
        return;
    }

    if state.editing_text_annotation_id.is_some() {
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
        .images
        .iter()
        .rev()
        .find(|image| image_screen_rect(state, rect, image).contains(pointer_pos))
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
    state.selected_item_ids.clear();
    state.selected_item_ids.push(item_id);
    state.selected_annotation_id = None;
    state.drag_state = None;
    state.marquee_state = None;
    state.crop_state = None;
    state.annotation_draft_state = None;
    state.textbox_transform_state = None;
    state.image_transform_state = Some(ImageTransformState::Resizing {
        item_id,
        center_screen,
        start_scale,
        start_radius_screen,
    });
}

fn begin_textbox_pending_move(
    state: &mut CanvasState,
    rect: egui::Rect,
    annotation: &CanvasAnnotation,
    pointer_pos: egui::Pos2,
) {
    state.selected_item_ids.clear();
    state.selected_annotation_id = Some(annotation.id);
    state.drag_state = None;
    state.marquee_state = None;
    state.crop_state = None;
    state.annotation_draft_state = None;
    state.textbox_transform_state = Some(TextboxTransformState::PendingMove {
        annotation_id: annotation.id,
        pointer_start_world: state.screen_to_world(pointer_pos, rect),
        pointer_start_screen: pointer_pos,
        start_rect_world: textbox_world_rect(annotation),
    });
}

fn handle_textbox_primary_press(
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
            let world_rect = textbox_world_rect(annotation);
            state.selected_item_ids.clear();
            state.selected_annotation_id = Some(annotation.id);
            state.drag_state = None;
            state.image_transform_state = None;
            state.marquee_state = None;
            state.crop_state = None;
            state.annotation_draft_state = None;
            state.textbox_transform_state = Some(TextboxTransformState::Resizing {
                annotation_id: annotation.id,
                dragged_handle: handle,
                anchor_world: textbox_resize_handle_anchor_world_pos(world_rect, handle),
                start_rect_world: world_rect,
            });
            true
        }
        TextboxPointerRegion::Text => true,
        TextboxPointerRegion::Body => {
            if state.editing_text_annotation_id == Some(annotation.id) {
                state.stop_textbox_edit_without_cleanup();
            } else if state.editing_text_annotation_id.is_some() {
                state.finish_textbox_edit();
            }
            state.image_transform_state = None;
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
        .annotations
        .iter()
        .rev()
        .find(|annotation| annotation_hit_test(state, rect, annotation, pointer_pos))
        .map(|annotation| annotation.id)
}

fn image_screen_rect(state: &CanvasState, rect: egui::Rect, image: &CanvasImage) -> egui::Rect {
    let item = &image.record.item;
    let asset = &image.record.asset;
    let center = state.world_to_screen(egui::pos2(item.x, item.y), rect);
    let size =
        egui::vec2(asset.width as f32, asset.height as f32) * item.scale.max(0.01) * state.zoom;
    egui::Rect::from_center_size(center, size)
}

fn min_image_scale(asset: &crate::data::ImageAsset) -> f32 {
    let width_scale = MIN_IMAGE_WORLD_SIZE / asset.width.max(1) as f32;
    let height_scale = MIN_IMAGE_WORLD_SIZE / asset.height.max(1) as f32;
    width_scale.max(height_scale)
}

fn image_resize_handle_screen_pos(
    image_rect: egui::Rect,
    handle: ImageResizeHandle,
) -> egui::Pos2 {
    match handle {
        ImageResizeHandle::TopLeft => image_rect.left_top(),
        ImageResizeHandle::TopRight => image_rect.right_top(),
        ImageResizeHandle::BottomLeft => image_rect.left_bottom(),
        ImageResizeHandle::BottomRight => image_rect.right_bottom(),
    }
}

fn image_resize_cursor_icon(handle: ImageResizeHandle) -> egui::CursorIcon {
    match handle {
        ImageResizeHandle::TopLeft | ImageResizeHandle::BottomRight => {
            egui::CursorIcon::ResizeNwSe
        }
        ImageResizeHandle::TopRight | ImageResizeHandle::BottomLeft => {
            egui::CursorIcon::ResizeNeSw
        }
    }
}

fn hit_test_image_resize_handle(
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

fn resized_image_scale(
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
                crate::theme::stroke_strong(),
                egui::StrokeKind::Outside,
            );
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
}

fn paint_background(painter: &egui::Painter, rect: egui::Rect, state: &mut CanvasState) {
    let base = crate::theme::TEXT;
    state.background_lod_level =
        settled_background_lod_level(state.background_lod_level, state.zoom);
    let world_step = background_world_step(state.background_lod_level);
    let visible_world_rect = visible_world_rect(state, rect);
    match state.background {
        CanvasBackground::None => {}
        CanvasBackground::Dots => {
            let color = egui::Color32::from_rgba_premultiplied(base.r(), base.g(), base.b(), 3);
            let mut world_x = first_lattice_at_or_before(visible_world_rect.left(), world_step);
            while world_x <= visible_world_rect.right() + world_step {
                let mut world_y = first_lattice_at_or_before(visible_world_rect.top(), world_step);
                while world_y <= visible_world_rect.bottom() + world_step {
                    let screen_pos = state.world_to_screen(egui::pos2(world_x, world_y), rect);
                    painter.circle_filled(snap_point(screen_pos), 0.6, color);
                    world_y += world_step;
                }
                world_x += world_step;
            }
        }
        CanvasBackground::Grid => {
            let darker = crate::theme::TEXT_SUBTLE;
            let color =
                egui::Color32::from_rgba_premultiplied(darker.r(), darker.g(), darker.b(), 5);
            let stroke = egui::Stroke::new(0.3, color);
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
    }
}

fn settled_background_lod_level(current_level: i32, zoom: f32) -> i32 {
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

fn background_world_step(level: i32) -> f32 {
    BACKGROUND_BASE_WORLD_STEP * 2.0_f32.powi(level)
}

fn background_screen_step(level: i32, zoom: f32) -> f32 {
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

#[cfg(test)]
fn lattice_phase_world(world: f32, world_step: f32) -> f32 {
    world.rem_euclid(world_step) / world_step
}

#[cfg(test)]
fn lattice_phase_screen(screen_origin: f32, screen_position: f32, screen_step: f32) -> f32 {
    (screen_position - screen_origin).rem_euclid(screen_step) / screen_step
}

fn textbox_padding() -> egui::Vec2 {
    egui::vec2(crate::theme::SPACE_2, crate::theme::SPACE_2)
}

fn textbox_font_id(_ui: &egui::Ui, zoom: f32) -> egui::FontId {
    egui::FontId::proportional(TEXTBOX_FONT_SIZE * zoom.max(MIN_CANVAS_ZOOM))
}

fn textbox_content_rect(textbox_rect: egui::Rect) -> egui::Rect {
    let padding = textbox_padding();
    let min = egui::pos2(
        (textbox_rect.min.x + padding.x).min(textbox_rect.max.x),
        (textbox_rect.min.y + padding.y).min(textbox_rect.max.y),
    );
    let available_size = (textbox_rect.max - min).max(egui::Vec2::ZERO);
    let desired_size = (textbox_rect.size() - padding * 2.0).max(egui::Vec2::ZERO);
    egui::Rect::from_min_size(min, egui::vec2(
        desired_size.x.min(available_size.x),
        desired_size.y.min(available_size.y),
    ))
}

fn textbox_row_height(ui: &egui::Ui, zoom: f32) -> f32 {
    let font_id = textbox_font_id(ui, zoom);
    ui.fonts_mut(|fonts| fonts.row_height(&font_id))
}

fn textbox_empty_hotspot_rect(
    ui: &egui::Ui,
    content_rect: egui::Rect,
    zoom: f32,
) -> egui::Rect {
    let width = content_rect
        .width()
        .clamp(TEXTBOX_EMPTY_HOTSPOT_MIN_WIDTH, TEXTBOX_EMPTY_HOTSPOT_MAX_WIDTH);
    let size = egui::vec2(width, textbox_row_height(ui, zoom).min(content_rect.height()));
    egui::Rect::from_min_size(content_rect.min, size.min(content_rect.size()))
}

fn textbox_text_interactive_rect_from_galley(
    ui: &egui::Ui,
    content_rect: egui::Rect,
    galley: &egui::Galley,
    zoom: f32,
    is_empty: bool,
) -> egui::Rect {
    if is_empty {
        return textbox_empty_hotspot_rect(ui, content_rect, zoom);
    }

    egui::Rect::from_min_size(content_rect.min, galley.size()).intersect(content_rect)
}

fn textbox_pointer_region(
    ui: &egui::Ui,
    rect: egui::Rect,
    state: &mut CanvasState,
    annotation: &CanvasAnnotation,
    pointer_pos: egui::Pos2,
) -> Option<TextboxPointerRegion> {
    debug_assert_eq!(annotation.kind, CanvasAnnotationKind::Textbox);

    let editing = state.editing_text_annotation_id == Some(annotation.id);
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
            content_rect.width(),
            state.zoom,
        );
        let text_rect = textbox_text_interactive_rect_from_galley(
            ui,
            content_rect,
            &layout.galley,
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

fn textbox_layout_cache_key(text: &str, wrap_width: f32, zoom: f32) -> TextboxLayoutCacheKey {
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    TextboxLayoutCacheKey {
        text_hash: hasher.finish(),
        wrap_width_bits: wrap_width.max(24.0).to_bits(),
        zoom_bits: zoom.to_bits(),
    }
}

fn layout_textbox_galley(
    ui: &egui::Ui,
    text: &str,
    wrap_width: f32,
    zoom: f32,
) -> Arc<egui::Galley> {
    let job = egui::text::LayoutJob::simple(
        if text.is_empty() {
            " ".to_owned()
        } else {
            text.to_owned()
        },
        textbox_font_id(ui, zoom),
        crate::theme::TEXT,
        wrap_width.max(24.0),
    );
    ui.fonts_mut(|fonts| fonts.layout_job(job))
}

fn textbox_required_world_height(galley_height_screen: f32, zoom: f32) -> f32 {
    let padding = textbox_padding();
    let screen_height = galley_height_screen + padding.y * 2.0;
    (screen_height / zoom.max(MIN_CANVAS_ZOOM)).max(MIN_TEXTBOX_WORLD_SIZE.y)
}

fn expand_textbox_to_height(state: &mut CanvasState, index: usize, required_height: f32) -> bool {
    let annotation = &state.annotations[index];
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
    let annotation = &mut state.annotations[index];
    annotation.start_x = new_rect.min.x;
    annotation.start_y = new_rect.min.y;
    annotation.end_x = new_rect.max.x;
    annotation.end_y = new_rect.max.y;
    state.mark_annotation_dirty(annotation_id);
    true
}

fn paint_annotations(
    painter: &egui::Painter,
    ui: &egui::Ui,
    rect: egui::Rect,
    state: &mut CanvasState,
) {
    for index in 0..state.annotations.len() {
        let annotation = state.annotations[index].clone();
        let selected = state.selected_annotation_id == Some(annotation.id);
        let editing = state.editing_text_annotation_id == Some(annotation.id);
        let stroke = if selected || editing {
            crate::theme::stroke_strong()
        } else {
            egui::Stroke::new(
                1.0,
                egui::Color32::from_rgba_premultiplied(
                    crate::theme::TEXT.r(),
                    crate::theme::TEXT.g(),
                    crate::theme::TEXT.b(),
                    220,
                ),
            )
        };

        match annotation.kind {
            CanvasAnnotationKind::Line => {
                let (start, end) = annotation_screen_points(state, rect, &annotation);
                painter.line_segment([start, end], stroke);
            }
            CanvasAnnotationKind::Arrow => {
                let (start, end) = annotation_screen_points(state, rect, &annotation);
                paint_arrow_segment(painter, start, end, stroke);
            }
            CanvasAnnotationKind::Textbox => {
                let textbox_rect = annotation_screen_rect(state, rect, &annotation);
                let content_rect = textbox_content_rect(textbox_rect);
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
                        &state.cached_textbox_layout(
                            ui,
                            annotation.id,
                            &annotation.text,
                            content_rect.width(),
                            state.zoom,
                        )
                        .galley,
                        content_rect,
                    );
                }
            }
        }
    }
}

fn paint_textbox_text(
    painter: &egui::Painter,
    galley: &Arc<egui::Galley>,
    content_rect: egui::Rect,
) {
    painter
        .with_clip_rect(content_rect)
        .galley(content_rect.min, galley.clone(), crate::theme::TEXT);
}

fn paint_annotation_draft(painter: &egui::Painter, rect: egui::Rect, state: &CanvasState) {
    let Some(draft_state) = &state.annotation_draft_state else {
        return;
    };
    let draft = pending_annotation_from_draft(draft_state.clone()).unwrap_or(PendingAnnotation {
        kind: draft_state.kind,
        start_world: draft_state.start_world,
        end_world: draft_state.current_world,
        text: String::new(),
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
    let head = 10.0;
    let left = end - unit * head + egui::vec2(-unit.y, unit.x) * (head * 0.5);
    let right = end - unit * head + egui::vec2(unit.y, -unit.x) * (head * 0.5);
    painter.line_segment([end, left], stroke);
    painter.line_segment([end, right], stroke);
}

fn show_textbox_editor(ui: &egui::Ui, rect: egui::Rect, state: &mut CanvasState) {
    let Some(annotation_id) = state.editing_text_annotation_id else {
        return;
    };
    let Some(index) = state
        .annotations
        .iter()
        .position(|annotation| annotation.id == annotation_id)
    else {
        state.editing_text_annotation_id = None;
        state.focus_text_editor = false;
        return;
    };
    let textbox_rect = annotation_screen_rect(state, rect, &state.annotations[index]);
    let content_rect = textbox_content_rect(textbox_rect);
    let text = state.annotations[index].text.clone();
    let layout = state.cached_textbox_layout(
        ui,
        annotation_id,
        &text,
        content_rect.width(),
        state.zoom,
    );
    let text_rect = textbox_text_interactive_rect_from_galley(
        ui,
        content_rect,
        &layout.galley,
        state.zoom,
        text.is_empty(),
    );

    egui::Area::new(egui::Id::new(("textbox-editor", annotation_id)))
        .order(egui::Order::Foreground)
        .fixed_pos(text_rect.min)
        .show(ui.ctx(), |ui| {
            // Keep live text rendering inside the textbox body while zoom changes.
            ui.set_clip_rect(content_rect);
            let zoom = state.zoom;
            let mut layouter = |ui: &egui::Ui, text: &dyn egui::TextBuffer, width: f32| {
                let wrap_width = if content_rect.width() > 0.0 {
                    content_rect.width()
                } else {
                    width
                };
                layout_textbox_galley(ui, text.as_str(), wrap_width, zoom)
            };
            let output = ui
                .allocate_ui(text_rect.size(), |ui| {
                    ui.set_min_size(text_rect.size());
                    egui::TextEdit::multiline(&mut state.annotations[index].text)
                        .desired_width(content_rect.width())
                        .font(textbox_font_id(ui, state.zoom))
                        .layouter(&mut layouter)
                        .frame(egui::Frame::NONE)
                        .show(ui)
                })
                .inner;
            let text = state.annotations[index].text.clone();
            let entry = state.cache_textbox_layout(
                annotation_id,
                &text,
                content_rect.width(),
                state.zoom,
                output.galley.clone(),
            );
            let required_height = entry.required_world_height;
            expand_textbox_to_height(state, index, required_height);
            if state.focus_text_editor {
                output.response.request_focus();
                state.focus_text_editor = false;
            }
            if output.response.changed() {
                state.mark_annotation_dirty(annotation_id);
            }
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) && output.response.has_focus() {
                state.finish_textbox_edit();
            } else if output.response.lost_focus() {
                state.finish_textbox_edit();
            }
        });
}

fn paint_marquee(painter: &egui::Painter, state: &CanvasState) {
    let Some(marquee_state) = &state.marquee_state else {
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

fn paint_selection_bounds(painter: &egui::Painter, rect: egui::Rect, state: &CanvasState) {
    if state.selected_annotation_id.is_none() && state.selected_item_ids.len() <= 1 {
        return;
    }

    if state.selected_item_ids.is_empty()
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

fn annotation_screen_points(
    state: &CanvasState,
    rect: egui::Rect,
    annotation: &CanvasAnnotation,
) -> (egui::Pos2, egui::Pos2) {
    (
        state.world_to_screen(egui::pos2(annotation.start_x, annotation.start_y), rect),
        state.world_to_screen(egui::pos2(annotation.end_x, annotation.end_y), rect),
    )
}

fn textbox_world_rect(annotation: &CanvasAnnotation) -> egui::Rect {
    normalized_textbox_rect(
        egui::pos2(annotation.start_x, annotation.start_y),
        egui::pos2(annotation.end_x, annotation.end_y),
    )
}

fn textbox_screen_rect(
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

fn hit_test_textbox_resize_handle(
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

fn annotation_screen_rect(
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

fn normalized_textbox_rect(start: egui::Pos2, end: egui::Pos2) -> egui::Rect {
    let mut rect = egui::Rect::from_two_pos(start, end);
    if rect.width() < MIN_TEXTBOX_WORLD_SIZE.x {
        rect.max.x = rect.min.x + MIN_TEXTBOX_WORLD_SIZE.x;
    }
    if rect.height() < MIN_TEXTBOX_WORLD_SIZE.y {
        rect.max.y = rect.min.y + MIN_TEXTBOX_WORLD_SIZE.y;
    }
    rect
}

fn moved_textbox_rect(
    start_rect: egui::Rect,
    pointer_start_world: egui::Pos2,
    pointer_world: egui::Pos2,
) -> egui::Rect {
    let delta = pointer_world - pointer_start_world;
    start_rect.translate(delta)
}

fn resized_textbox_rect(
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

fn pending_annotation_from_draft(draft_state: AnnotationDraftState) -> Option<PendingAnnotation> {
    match draft_state.kind {
        CanvasAnnotationKind::Line | CanvasAnnotationKind::Arrow => {
            let distance = draft_state.start_world.distance(draft_state.current_world);
            (distance > 2.0).then_some(PendingAnnotation {
                kind: draft_state.kind,
                start_world: draft_state.start_world,
                end_world: draft_state.current_world,
                text: String::new(),
            })
        }
        CanvasAnnotationKind::Textbox => {
            let rect = normalized_textbox_rect(draft_state.start_world, draft_state.current_world);
            Some(PendingAnnotation {
                kind: CanvasAnnotationKind::Textbox,
                start_world: rect.min,
                end_world: rect.max,
                text: String::new(),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AnnotationDraftState, CanvasImage, CanvasState, ImageResizeHandle, SelectedLayoutItem,
        TextboxPointerRegion, TextboxResizeHandle,
        background_screen_step, background_world_step, best_compact_layout,
        crop_center_offset_local, handle_textbox_primary_press, hit_test_textbox_resize_handle,
        hit_test_image_resize_handle, horizontal_layout, image_screen_rect, min_image_scale,
        resized_image_scale, MIN_CANVAS_ZOOM, lattice_phase_screen, lattice_phase_world,
        layout_textbox_galley, legacy_best_compact_layout, merge_cropped_item_fields,
        moved_textbox_rect, normalized_textbox_rect, pending_annotation_from_draft,
        pixel_crop_rect_from_local, resized_textbox_rect, screen_rect_to_local_crop_rect,
        settled_background_lod_level, textbox_content_rect, textbox_empty_hotspot_rect,
        textbox_layout_cache_key, textbox_pointer_region, textbox_required_world_height,
        textbox_screen_rect, textbox_text_interactive_rect_from_galley, vertical_layout,
    };
    use crate::data::{BoardItem, BoardItemWithAsset, CanvasAnnotation, CanvasAnnotationKind, ImageAsset};
    use std::{path::PathBuf, sync::Arc};

    fn push_test_image(
        ui: &eframe::egui::Ui,
        state: &mut CanvasState,
        item_id: i64,
        width: u32,
        height: u32,
        scale: f32,
        center: eframe::egui::Pos2,
    ) {
        let texture = ui.ctx().load_texture(
            format!("test-image-{item_id}"),
            eframe::egui::ColorImage::filled([4, 4], eframe::egui::Color32::WHITE),
            eframe::egui::TextureOptions::LINEAR,
        );
        state.images.push(CanvasImage {
            record: BoardItemWithAsset {
                item: BoardItem {
                    id: item_id,
                    asset_id: item_id,
                    x: center.x,
                    y: center.y,
                    scale,
                    rotation: 0.0,
                    z_index: 0,
                    archived: false,
                },
                asset: ImageAsset {
                    id: item_id,
                    original_path: PathBuf::from(format!("image-{item_id}.png")),
                    filename: format!("image-{item_id}.png"),
                    mime_type: "image/png".to_owned(),
                    width,
                    height,
                    file_size: 0,
                    imported_at: 0,
                },
            },
            texture,
        });
    }

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
    fn textbox_rect_enforces_minimum_size() {
        let rect = normalized_textbox_rect(
            eframe::egui::pos2(20.0, 40.0),
            eframe::egui::pos2(32.0, 56.0),
        );

        assert_eq!(rect.min, eframe::egui::pos2(20.0, 40.0));
        assert_eq!(rect.width(), 240.0);
        assert_eq!(rect.height(), 56.0);
    }

    #[test]
    fn textbox_click_draft_uses_default_note_size() {
        let pending = pending_annotation_from_draft(AnnotationDraftState {
            kind: CanvasAnnotationKind::Textbox,
            start_world: eframe::egui::pos2(20.0, 40.0),
            current_world: eframe::egui::pos2(20.0, 40.0),
        })
        .unwrap();

        assert_eq!(pending.start_world, eframe::egui::pos2(20.0, 40.0));
        assert_eq!(pending.end_world, eframe::egui::pos2(260.0, 96.0));
    }

    #[test]
    fn textbox_drag_draft_preserves_explicit_size() {
        let pending = pending_annotation_from_draft(AnnotationDraftState {
            kind: CanvasAnnotationKind::Textbox,
            start_world: eframe::egui::pos2(20.0, 40.0),
            current_world: eframe::egui::pos2(340.0, 120.0),
        })
        .unwrap();

        assert_eq!(pending.start_world, eframe::egui::pos2(20.0, 40.0));
        assert_eq!(pending.end_world, eframe::egui::pos2(340.0, 120.0));
    }

    #[test]
    fn textbox_multiline_content_requires_more_than_min_height() {
        assert!(textbox_required_world_height(80.0, 1.0) > 56.0);

        eframe::egui::__run_test_ui(|ui| {
            let mut text =
                "First line\nSecond line\nThird line\nFourth line\nFifth line".to_owned();
            let output = eframe::egui::TextEdit::multiline(&mut text)
                .desired_width(80.0)
                .font(eframe::egui::FontId::proportional(24.0))
                .show(ui);

            assert!(output.galley.rows.len() >= 5);
        });
    }

    #[test]
    fn textbox_content_rect_respects_available_bounds() {
        let textbox_rect = eframe::egui::Rect::from_min_size(
            eframe::egui::pos2(10.0, 20.0),
            eframe::egui::vec2(8.0, 6.0),
        );
        let content_rect = textbox_content_rect(textbox_rect);

        assert!(textbox_rect.contains(content_rect.min));
        assert!(textbox_rect.contains(content_rect.max));
        assert!(content_rect.width() <= textbox_rect.width());
        assert!(content_rect.height() <= textbox_rect.height());
    }

    #[test]
    fn textbox_content_rect_stays_inside_zoomed_out_textbox() {
        let mut state = CanvasState::default();
        state.zoom = MIN_CANVAS_ZOOM;
        let canvas_rect = eframe::egui::Rect::from_min_max(
            eframe::egui::pos2(0.0, 0.0),
            eframe::egui::pos2(800.0, 600.0),
        );
        let annotation = CanvasAnnotation {
            id: 11,
            kind: CanvasAnnotationKind::Textbox,
            start_x: 20.0,
            start_y: 40.0,
            end_x: 260.0,
            end_y: 96.0,
            text: "zoomed out".to_owned(),
            archived: false,
        };

        let textbox_rect = textbox_screen_rect(&state, canvas_rect, &annotation);
        let content_rect = textbox_content_rect(textbox_rect);

        assert!(textbox_rect.contains(content_rect.min));
        assert!(textbox_rect.contains(content_rect.max));
        assert!(content_rect.width() <= textbox_rect.width());
        assert!(content_rect.height() <= textbox_rect.height());
    }

    #[test]
    fn textbox_layout_cache_reuses_entry_for_identical_inputs() {
        eframe::egui::__run_test_ui(|ui| {
            let mut state = CanvasState::default();
            let first = state.cached_textbox_layout(ui, 1, "cache me", 120.0, 1.0);
            let second = state.cached_textbox_layout(ui, 1, "cache me", 120.0, 1.0);

            assert!(Arc::ptr_eq(&first.galley, &second.galley));
            assert_eq!(first.key, second.key);
        });
    }

    #[test]
    fn textbox_layout_cache_replaces_entry_when_text_changes() {
        eframe::egui::__run_test_ui(|ui| {
            let mut state = CanvasState::default();
            let first = state.cached_textbox_layout(ui, 1, "before", 120.0, 1.0);
            let second = state.cached_textbox_layout(ui, 1, "after", 120.0, 1.0);

            assert!(!Arc::ptr_eq(&first.galley, &second.galley));
            assert_ne!(first.key, second.key);
        });
    }

    #[test]
    fn textbox_layout_cache_replaces_entry_when_zoom_changes() {
        eframe::egui::__run_test_ui(|ui| {
            let mut state = CanvasState::default();
            let first = state.cached_textbox_layout(ui, 1, "zoom", 120.0, 1.0);
            let second = state.cached_textbox_layout(ui, 1, "zoom", 120.0, 2.0);

            assert!(!Arc::ptr_eq(&first.galley, &second.galley));
            assert_ne!(first.key, second.key);
        });
    }

    #[test]
    fn textbox_editor_writeback_replaces_cached_layout_entry() {
        eframe::egui::__run_test_ui(|ui| {
            let mut state = CanvasState::default();
            let textbox_rect = eframe::egui::Rect::from_min_size(
                eframe::egui::pos2(0.0, 0.0),
                eframe::egui::vec2(240.0, 56.0),
            );
            let text_rect = textbox_content_rect(textbox_rect);
            let mut text = "updated text".to_owned();
            let mut layouter = |ui: &eframe::egui::Ui,
                                text: &dyn eframe::egui::TextBuffer,
                                width: f32| { layout_textbox_galley(ui, text.as_str(), width, 1.0) };

            state.cache_textbox_layout(
                9,
                "stale",
                text_rect.width(),
                1.0,
                layout_textbox_galley(ui, "stale", text_rect.width(), 1.0),
            );

            let output = ui
                .allocate_ui(text_rect.size(), |ui| {
                    eframe::egui::TextEdit::multiline(&mut text)
                        .desired_width(text_rect.width())
                        .layouter(&mut layouter)
                        .show(ui)
                })
                .inner;
            let entry = state.cache_textbox_layout(9, &text, text_rect.width(), 1.0, output.galley);

            assert_eq!(entry.key, textbox_layout_cache_key(&text, text_rect.width(), 1.0));
            assert_eq!(
                state.textbox_layout_cache.get(&9).unwrap().key,
                textbox_layout_cache_key(&text, text_rect.width(), 1.0)
            );
        });
    }

    #[test]
    fn textbox_move_applies_pointer_delta() {
        let moved = moved_textbox_rect(
            eframe::egui::Rect::from_min_max(
                eframe::egui::pos2(20.0, 40.0),
                eframe::egui::pos2(260.0, 96.0),
            ),
            eframe::egui::pos2(100.0, 100.0),
            eframe::egui::pos2(130.0, 145.0),
        );

        assert_eq!(moved.min, eframe::egui::pos2(50.0, 85.0));
        assert_eq!(moved.max, eframe::egui::pos2(290.0, 141.0));
    }

    #[test]
    fn textbox_resize_respects_minimum_size() {
        let resized = resized_textbox_rect(
            TextboxResizeHandle::BottomRight,
            eframe::egui::pos2(20.0, 40.0),
            eframe::egui::Rect::from_min_max(
                eframe::egui::pos2(20.0, 40.0),
                eframe::egui::pos2(260.0, 96.0),
            ),
            eframe::egui::pos2(30.0, 50.0),
        );

        assert_eq!(resized.min, eframe::egui::pos2(20.0, 40.0));
        assert_eq!(resized.width(), 240.0);
        assert_eq!(resized.height(), 56.0);
    }

    #[test]
    fn textbox_edge_resize_moves_one_side_only() {
        let resized = resized_textbox_rect(
            TextboxResizeHandle::Right,
            eframe::egui::pos2(20.0, 68.0),
            eframe::egui::Rect::from_min_max(
                eframe::egui::pos2(20.0, 40.0),
                eframe::egui::pos2(260.0, 96.0),
            ),
            eframe::egui::pos2(320.0, 20.0),
        );

        assert_eq!(resized.min, eframe::egui::pos2(20.0, 40.0));
        assert_eq!(resized.max, eframe::egui::pos2(320.0, 96.0));
    }

    #[test]
    fn textbox_resize_hit_test_matches_visible_right_edge_zone() {
        let state = CanvasState::default();
        let canvas_rect = eframe::egui::Rect::from_min_max(
            eframe::egui::pos2(0.0, 0.0),
            eframe::egui::pos2(800.0, 600.0),
        );
        let annotation = CanvasAnnotation {
            id: 1,
            kind: CanvasAnnotationKind::Textbox,
            start_x: 20.0,
            start_y: 40.0,
            end_x: 260.0,
            end_y: 96.0,
            text: String::new(),
            archived: false,
        };

        let textbox_rect = textbox_screen_rect(&state, canvas_rect, &annotation);
        let pointer_pos = eframe::egui::pos2(textbox_rect.right() + 8.0, textbox_rect.center().y);

        assert_eq!(
            hit_test_textbox_resize_handle(&state, canvas_rect, &annotation, pointer_pos),
            Some(TextboxResizeHandle::Right)
        );
    }

    #[test]
    fn image_resize_hit_test_matches_corner_zone() {
        eframe::egui::__run_test_ui(|ui| {
            let mut state = CanvasState::default();
            let canvas_rect = eframe::egui::Rect::from_min_max(
                eframe::egui::pos2(0.0, 0.0),
                eframe::egui::pos2(800.0, 600.0),
            );
            push_test_image(
                ui,
                &mut state,
                21,
                200,
                100,
                1.0,
                eframe::egui::pos2(200.0, 160.0),
            );
            let image = state.image_by_item_id(21).unwrap();
            let image_rect = image_screen_rect(&state, canvas_rect, image);
            let pointer_pos = image_rect.right_bottom() + eframe::egui::vec2(8.0, 8.0);

            assert_eq!(
                hit_test_image_resize_handle(&state, canvas_rect, image, pointer_pos),
                Some(ImageResizeHandle::BottomRight)
            );
        });
    }

    #[test]
    fn image_resize_scales_proportionally_from_center() {
        let asset = ImageAsset {
            id: 22,
            original_path: PathBuf::from("image-22.png"),
            filename: "image-22.png".to_owned(),
            mime_type: "image/png".to_owned(),
            width: 200,
            height: 100,
            file_size: 0,
            imported_at: 0,
        };
        let center = eframe::egui::pos2(300.0, 240.0);
        let start_radius = 111.8034;
        let pointer_pos = center + eframe::egui::vec2(200.0, 100.0);

        let scale = resized_image_scale(&asset, 1.0, start_radius, center, pointer_pos);

        assert!((scale - 2.0).abs() < 0.01);
    }

    #[test]
    fn image_resize_respects_minimum_scale() {
        let asset = ImageAsset {
            id: 23,
            original_path: PathBuf::from("image-23.png"),
            filename: "image-23.png".to_owned(),
            mime_type: "image/png".to_owned(),
            width: 400,
            height: 200,
            file_size: 0,
            imported_at: 0,
        };

        let scale = resized_image_scale(
            &asset,
            1.0,
            223.6068,
            eframe::egui::pos2(300.0, 240.0),
            eframe::egui::pos2(300.5, 240.5),
        );

        assert_eq!(scale, min_image_scale(&asset));
    }

    #[test]
    fn textbox_text_interactive_rect_matches_galley_bounds_for_non_empty_text() {
        eframe::egui::__run_test_ui(|ui| {
            let mut state = CanvasState::default();
            let canvas_rect = eframe::egui::Rect::from_min_max(
                eframe::egui::pos2(0.0, 0.0),
                eframe::egui::pos2(800.0, 600.0),
            );
            let annotation = CanvasAnnotation {
                id: 14,
                kind: CanvasAnnotationKind::Textbox,
                start_x: 20.0,
                start_y: 40.0,
                end_x: 320.0,
                end_y: 160.0,
                text: "short".to_owned(),
                archived: false,
            };

            let textbox_rect = textbox_screen_rect(&state, canvas_rect, &annotation);
            let content_rect = textbox_content_rect(textbox_rect);
            let layout = state.cached_textbox_layout(
                ui,
                annotation.id,
                &annotation.text,
                content_rect.width(),
                1.0,
            );
            let text_rect = textbox_text_interactive_rect_from_galley(
                ui,
                content_rect,
                &layout.galley,
                1.0,
                false,
            );

            assert!(text_rect.width() < content_rect.width());
            assert!(text_rect.height() <= content_rect.height());
            assert!(text_rect.contains(content_rect.min));
        });
    }

    #[test]
    fn textbox_empty_hotspot_preserves_draggable_body_area() {
        eframe::egui::__run_test_ui(|ui| {
            let content_rect = eframe::egui::Rect::from_min_size(
                eframe::egui::pos2(24.0, 24.0),
                eframe::egui::vec2(280.0, 96.0),
            );
            let hotspot = textbox_empty_hotspot_rect(ui, content_rect, 1.0);

            assert!(hotspot.width() < content_rect.width());
            assert!(hotspot.height() < content_rect.height());
            assert!(hotspot.contains(content_rect.min));
            assert!(!hotspot.contains(eframe::egui::pos2(
                content_rect.right() - 8.0,
                content_rect.bottom() - 8.0,
            )));
        });
    }

    #[test]
    fn textbox_body_press_on_unselected_textbox_starts_pending_move() {
        eframe::egui::__run_test_ui(|ui| {
            let mut state = CanvasState::default();
            let canvas_rect = eframe::egui::Rect::from_min_max(
                eframe::egui::pos2(0.0, 0.0),
                eframe::egui::pos2(800.0, 600.0),
            );
            state.add_annotation(CanvasAnnotation {
                id: 15,
                kind: CanvasAnnotationKind::Textbox,
                start_x: 20.0,
                start_y: 40.0,
                end_x: 320.0,
                end_y: 180.0,
                text: "short".to_owned(),
                archived: false,
            });

            let annotation = state.annotation_by_id(15).unwrap().clone();
            let textbox_rect = textbox_screen_rect(&state, canvas_rect, &annotation);
            let pointer_pos = eframe::egui::pos2(textbox_rect.center().x, textbox_rect.bottom() - 32.0);

            assert!(handle_textbox_primary_press(
                ui,
                canvas_rect,
                &mut state,
                &annotation,
                pointer_pos,
            ));
            assert_eq!(state.selected_annotation_id, Some(15));
            assert!(matches!(
                state.textbox_transform_state,
                Some(super::TextboxTransformState::PendingMove { annotation_id: 15, .. })
            ));
        });
    }

    #[test]
    fn textbox_body_press_while_editing_empty_exits_edit_and_starts_pending_move() {
        eframe::egui::__run_test_ui(|ui| {
            let mut state = CanvasState::default();
            let canvas_rect = eframe::egui::Rect::from_min_max(
                eframe::egui::pos2(0.0, 0.0),
                eframe::egui::pos2(800.0, 600.0),
            );
            state.add_annotation(CanvasAnnotation {
                id: 16,
                kind: CanvasAnnotationKind::Textbox,
                start_x: 20.0,
                start_y: 40.0,
                end_x: 320.0,
                end_y: 180.0,
                text: String::new(),
                archived: false,
            });
            state.begin_textbox_edit(16);

            let annotation = state.annotation_by_id(16).unwrap().clone();
            let textbox_rect = textbox_screen_rect(&state, canvas_rect, &annotation);
            let pointer_pos = eframe::egui::pos2(textbox_rect.center().x, textbox_rect.bottom() - 32.0);

            assert!(handle_textbox_primary_press(
                ui,
                canvas_rect,
                &mut state,
                &annotation,
                pointer_pos,
            ));
            assert_eq!(state.editing_text_annotation_id, None);
            assert!(state.annotation_by_id(16).is_some());
            assert!(matches!(
                state.textbox_transform_state,
                Some(super::TextboxTransformState::PendingMove { annotation_id: 16, .. })
            ));
        });
    }

    #[test]
    fn textbox_text_press_while_editing_stays_editor_owned() {
        eframe::egui::__run_test_ui(|ui| {
            let mut state = CanvasState::default();
            let canvas_rect = eframe::egui::Rect::from_min_max(
                eframe::egui::pos2(0.0, 0.0),
                eframe::egui::pos2(800.0, 600.0),
            );
            state.add_annotation(CanvasAnnotation {
                id: 17,
                kind: CanvasAnnotationKind::Textbox,
                start_x: 20.0,
                start_y: 40.0,
                end_x: 320.0,
                end_y: 180.0,
                text: "hello world".to_owned(),
                archived: false,
            });
            state.begin_textbox_edit(17);

            let annotation = state.annotation_by_id(17).unwrap().clone();
            let textbox_rect = textbox_screen_rect(&state, canvas_rect, &annotation);
            let content_rect = textbox_content_rect(textbox_rect);
            let layout = state.cached_textbox_layout(
                ui,
                annotation.id,
                &annotation.text,
                content_rect.width(),
                1.0,
            );
            let text_rect = textbox_text_interactive_rect_from_galley(
                ui,
                content_rect,
                &layout.galley,
                1.0,
                false,
            );
            let pointer_pos = text_rect.center();

            assert_eq!(
                textbox_pointer_region(ui, canvas_rect, &mut state, &annotation, pointer_pos),
                Some(TextboxPointerRegion::Text)
            );

            assert!(handle_textbox_primary_press(
                ui,
                canvas_rect,
                &mut state,
                &annotation,
                pointer_pos,
            ));
            assert_eq!(state.editing_text_annotation_id, Some(17));
            assert!(state.textbox_transform_state.is_none());
        });
    }

    #[test]
    fn begin_textbox_edit_selects_textbox_without_placeholder_content() {
        let mut state = CanvasState::default();
        state.add_annotation(CanvasAnnotation {
            id: 5,
            kind: CanvasAnnotationKind::Textbox,
            start_x: 20.0,
            start_y: 40.0,
            end_x: 260.0,
            end_y: 96.0,
            text: String::new(),
            archived: false,
        });

        state.begin_textbox_edit(5);

        assert_eq!(state.editing_text_annotation_id, Some(5));
        assert_eq!(state.selected_annotation_id, Some(5));
        assert!(
            state
                .annotation_by_id(5)
                .is_some_and(|annotation| annotation.text.is_empty())
        );
    }

    #[test]
    fn finishing_empty_textbox_edit_removes_and_archives_annotation() {
        let mut state = CanvasState::default();
        state.add_annotation(CanvasAnnotation {
            id: 7,
            kind: CanvasAnnotationKind::Textbox,
            start_x: 20.0,
            start_y: 40.0,
            end_x: 260.0,
            end_y: 80.0,
            text: String::new(),
            archived: false,
        });

        state.begin_textbox_edit(7);
        state.finish_textbox_edit();

        assert!(state.annotations.is_empty());
        assert_eq!(state.take_archived_annotation_ids(), vec![7]);
    }

    #[test]
    fn add_annotation_invalidates_textbox_layout_cache() {
        eframe::egui::__run_test_ui(|ui| {
            let mut state = CanvasState::default();
            state.cache_textbox_layout(
                5,
                "cached",
                120.0,
                1.0,
                layout_textbox_galley(ui, "cached", 120.0, 1.0),
            );

            state.add_annotation(CanvasAnnotation {
                id: 5,
                kind: CanvasAnnotationKind::Textbox,
                start_x: 20.0,
                start_y: 40.0,
                end_x: 260.0,
                end_y: 96.0,
                text: String::new(),
                archived: false,
            });

            assert!(!state.textbox_layout_cache.contains_key(&5));
        });
    }

    #[test]
    fn add_annotations_clears_textbox_layout_cache() {
        eframe::egui::__run_test_ui(|ui| {
            let mut state = CanvasState::default();
            state.cache_textbox_layout(
                1,
                "one",
                120.0,
                1.0,
                layout_textbox_galley(ui, "one", 120.0, 1.0),
            );
            state.cache_textbox_layout(
                2,
                "two",
                120.0,
                1.0,
                layout_textbox_galley(ui, "two", 120.0, 1.0),
            );

            state.add_annotations(Vec::new());

            assert!(state.textbox_layout_cache.is_empty());
        });
    }

    #[test]
    fn remove_annotation_locally_invalidates_textbox_layout_cache() {
        eframe::egui::__run_test_ui(|ui| {
            let mut state = CanvasState::default();
            state.add_annotation(CanvasAnnotation {
                id: 3,
                kind: CanvasAnnotationKind::Textbox,
                start_x: 20.0,
                start_y: 40.0,
                end_x: 260.0,
                end_y: 96.0,
                text: "hello".to_owned(),
                archived: false,
            });
            state.cache_textbox_layout(
                3,
                "hello",
                120.0,
                1.0,
                layout_textbox_galley(ui, "hello", 120.0, 1.0),
            );

            state.remove_annotation_locally(3);

            assert!(!state.textbox_layout_cache.contains_key(&3));
        });
    }

    #[test]
    fn set_textbox_world_rect_invalidates_textbox_layout_cache() {
        eframe::egui::__run_test_ui(|ui| {
            let mut state = CanvasState::default();
            state.add_annotation(CanvasAnnotation {
                id: 4,
                kind: CanvasAnnotationKind::Textbox,
                start_x: 20.0,
                start_y: 40.0,
                end_x: 260.0,
                end_y: 96.0,
                text: "resize".to_owned(),
                archived: false,
            });
            state.cache_textbox_layout(
                4,
                "resize",
                120.0,
                1.0,
                layout_textbox_galley(ui, "resize", 120.0, 1.0),
            );

            assert!(state.set_textbox_world_rect(
                4,
                eframe::egui::Rect::from_min_max(
                    eframe::egui::pos2(20.0, 40.0),
                    eframe::egui::pos2(320.0, 120.0),
                ),
            ));
            assert!(!state.textbox_layout_cache.contains_key(&4));
        });
    }

    #[test]
    fn marquee_selection_can_select_textbox_when_no_images_intersect() {
        let mut state = CanvasState::default();
        state.add_annotation(CanvasAnnotation {
            id: 12,
            kind: CanvasAnnotationKind::Textbox,
            start_x: 20.0,
            start_y: 40.0,
            end_x: 260.0,
            end_y: 96.0,
            text: "textbox".to_owned(),
            archived: false,
        });

        state.set_marquee_selection(Vec::new(), Some(12));

        assert!(state.selected_item_ids().is_empty());
        assert_eq!(state.selected_annotation_id, Some(12));
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
    fn lattice_phase_is_preserved_across_zoom_change_with_same_lod() {
        let mut state = CanvasState::default();
        let rect = eframe::egui::Rect::from_min_max(
            eframe::egui::pos2(0.0, 0.0),
            eframe::egui::pos2(800.0, 600.0),
        );
        let world_point = eframe::egui::pos2(150.0, 94.0);
        let world_step = background_world_step(0);

        state.zoom = 0.95;
        let screen_origin_a = state.world_to_screen(eframe::egui::Pos2::ZERO, rect);
        let screen_point_a = state.world_to_screen(world_point, rect);
        let phase_a = lattice_phase_screen(
            screen_origin_a.x,
            screen_point_a.x,
            background_screen_step(0, state.zoom),
        );

        state.zoom = 1.05;
        let screen_origin_b = state.world_to_screen(eframe::egui::Pos2::ZERO, rect);
        let screen_point_b = state.world_to_screen(world_point, rect);
        let phase_b = lattice_phase_screen(
            screen_origin_b.x,
            screen_point_b.x,
            background_screen_step(0, state.zoom),
        );

        let world_phase = lattice_phase_world(world_point.x, world_step);
        assert!((phase_a - world_phase).abs() < 0.0001);
        assert!((phase_b - world_phase).abs() < 0.0001);
        assert!((phase_a - phase_b).abs() < 0.0001);
    }

    #[test]
    fn lattice_phase_is_preserved_under_pan_and_zoom() {
        let mut state = CanvasState::default();
        let rect = eframe::egui::Rect::from_min_max(
            eframe::egui::pos2(0.0, 0.0),
            eframe::egui::pos2(800.0, 600.0),
        );
        let world_point = eframe::egui::pos2(222.0, -118.0);
        let world_step = background_world_step(-1);

        state.pan = eframe::egui::vec2(123.0, -41.0);
        state.zoom = 1.9;
        let screen_origin = state.world_to_screen(eframe::egui::Pos2::ZERO, rect);
        let screen_point = state.world_to_screen(world_point, rect);
        let phase_x = lattice_phase_screen(
            screen_origin.x,
            screen_point.x,
            background_screen_step(-1, state.zoom),
        );
        let phase_y = lattice_phase_screen(
            screen_origin.y,
            screen_point.y,
            background_screen_step(-1, state.zoom),
        );

        assert!((phase_x - lattice_phase_world(world_point.x, world_step)).abs() < 0.0001);
        assert!((phase_y - lattice_phase_world(world_point.y, world_step)).abs() < 0.0001);
    }

    #[test]
    fn background_lod_settles_inside_spacing_band() {
        for zoom in [0.12, 0.33, 0.9, 1.0, 1.8, 4.2, 12.0] {
            let level = settled_background_lod_level(0, zoom);
            let screen_step = background_screen_step(level, zoom);
            assert!(screen_step >= super::BACKGROUND_MIN_SCREEN_STEP);
            assert!(screen_step <= super::BACKGROUND_MAX_SCREEN_STEP);
        }
    }

    #[test]
    fn background_lod_has_hysteresis_near_thresholds() {
        let level_after_zoom_out = settled_background_lod_level(0, 0.70);
        assert_eq!(level_after_zoom_out, 0);

        let level_after_more_zoom_out = settled_background_lod_level(level_after_zoom_out, 0.60);
        assert_eq!(level_after_more_zoom_out, 1);

        let level_after_small_recover =
            settled_background_lod_level(level_after_more_zoom_out, 0.76);
        assert_eq!(level_after_small_recover, 1);

        let level_after_large_zoom_in =
            settled_background_lod_level(level_after_small_recover, 1.80);
        assert!(level_after_large_zoom_in < level_after_small_recover);
        let screen_step = background_screen_step(level_after_large_zoom_in, 1.80);
        assert!(screen_step >= super::BACKGROUND_MIN_SCREEN_STEP);
        assert!(screen_step <= super::BACKGROUND_MAX_SCREEN_STEP);
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
        for ((first_id, first_pos), (second_id, second_pos)) in first.0.iter().zip(second.0.iter())
        {
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
