use eframe::egui;

use crate::data::{BoardItemWithAsset, UserSession};
use crate::jobs::{CroppedCanvasItem, LoadedCanvasItem};

const MIN_CANVAS_ZOOM: f32 = 0.01;
const MAX_CANVAS_ZOOM: f32 = 64.0;

pub struct CanvasState {
    pan: egui::Vec2,
    zoom: f32,
    images: Vec<CanvasImage>,
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

#[derive(Clone)]
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
    pub crop_rect: PixelCropRect,
    pub crop_offset_local: egui::Vec2,
}

impl Default for CanvasState {
    fn default() -> Self {
        Self {
            pan: egui::Vec2::ZERO,
            zoom: 1.0,
            images: Vec::new(),
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
        self.crop_pending_item_ids.retain(|pending_id| *pending_id != item_id);
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

    pub fn arrange_selected_horizontal(&mut self) -> usize {
        let mut items = self.selected_layout_items();
        if items.len() <= 1 {
            return items.len();
        }

        items.sort_by(|left, right| left.x.total_cmp(&right.x));
        let current_center = self.selected_center_world().unwrap_or(egui::Pos2::ZERO);
        let gap = 16.0;
        let max_height = items.iter().map(|item| item.height).fold(0.0_f32, f32::max);

        let mut x_cursor = 0.0;
        let mut placements = Vec::with_capacity(items.len());
        for item in &items {
            let center = egui::pos2(x_cursor + item.width * 0.5, max_height - item.height * 0.5);
            placements.push((item.id, center));
            x_cursor += item.width + gap;
        }

        let total_width = x_cursor - gap;
        let local_center = egui::pos2(total_width * 0.5, max_height * 0.5);
        self.apply_arrangement(
            placements,
            current_center.to_vec2() - local_center.to_vec2(),
        )
    }

    pub fn arrange_selected_vertical(&mut self) -> usize {
        let mut items = self.selected_layout_items();
        if items.len() <= 1 {
            return items.len();
        }

        items.sort_by(|left, right| left.y.total_cmp(&right.y));
        let current_center = self.selected_center_world().unwrap_or(egui::Pos2::ZERO);
        let gap = 16.0;
        let max_width = items.iter().map(|item| item.width).fold(0.0_f32, f32::max);

        let mut y_cursor = 0.0;
        let mut placements = Vec::with_capacity(items.len());
        for item in &items {
            let center = egui::pos2(item.width * 0.5, y_cursor + item.height * 0.5);
            placements.push((item.id, center));
            y_cursor += item.height + gap;
        }

        let total_height = y_cursor - gap;
        let local_center = egui::pos2(max_width * 0.5, total_height * 0.5);
        self.apply_arrangement(
            placements,
            current_center.to_vec2() - local_center.to_vec2(),
        )
    }

    pub fn arrange_selected_compact(&mut self) -> usize {
        let mut items = self.selected_layout_items();
        if items.len() <= 1 {
            return items.len();
        }

        items.sort_by(|left, right| {
            left.y
                .total_cmp(&right.y)
                .then_with(|| left.x.total_cmp(&right.x))
        });
        let current_center = self.selected_center_world().unwrap_or(egui::Pos2::ZERO);

        let mut best_layout: Option<(Vec<(i64, egui::Pos2)>, f32, f32, f32, f32)> = None;
        for columns in 1..=items.len() {
            let (placements, width, height) = compact_layout(&items, columns, 0.0);
            let aspect_ratio = width.max(height) / width.min(height).max(1.0);
            let area = width * height;

            match &best_layout {
                Some((_, _, _, best_ratio, best_area))
                    if aspect_ratio > *best_ratio
                        || ((aspect_ratio - *best_ratio).abs() < 0.001 && area >= *best_area) => {}
                _ => best_layout = Some((placements, width, height, aspect_ratio, area)),
            }
        }

        let Some((placements, width, height, _, _)) = best_layout else {
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
        self.images.iter().find(|image| image.record.item.id == item_id)
    }

    fn build_crop_request(
        &self,
        rect: egui::Rect,
        crop_state: &CropState,
    ) -> Option<CanvasCropRequest> {
        let image = self.image_by_item_id(crop_state.item_id)?;
        let image_rect = image_screen_rect(self, rect, image);
        let clamped_screen_rect =
            clamped_crop_screen_rect(crop_state.start_screen, crop_state.current_screen, image_rect)?;
        let local_rect = screen_rect_to_local_crop_rect(
            clamped_screen_rect,
            image_rect,
            image.record.asset.width,
            image.record.asset.height,
        )?;
        let crop_rect =
            pixel_crop_rect_from_local(local_rect, image.record.asset.width, image.record.asset.height)?;
        let crop_offset_local = crop_center_offset_local(&image.record.asset, local_rect);

        Some(CanvasCropRequest {
            item_id: image.record.item.id,
            source_asset: image.record.asset.clone(),
            crop_rect,
            crop_offset_local,
        })
    }

    fn can_start_crop_on_item(&self, item_id: i64) -> bool {
        !self.crop_pending_item_ids.contains(&item_id)
    }
}

fn compact_layout(
    items: &[SelectedLayoutItem],
    columns: usize,
    gap: f32,
) -> (Vec<(i64, egui::Pos2)>, f32, f32) {
    let mut placements = Vec::with_capacity(items.len());
    let mut y_cursor = 0.0;
    let mut total_width = 0.0_f32;

    for row in items.chunks(columns) {
        let row_height = row.iter().map(|item| item.height).fold(0.0_f32, f32::max);
        let mut x_cursor = 0.0;

        for item in row {
            let center = egui::pos2(x_cursor + item.width * 0.5, y_cursor + item.height * 0.5);
            placements.push((item.id, center));
            x_cursor += item.width + gap;
        }

        total_width = total_width.max((x_cursor - gap).max(0.0));
        y_cursor += row_height + gap;
    }

    let total_height = (y_cursor - gap).max(0.0);
    (placements, total_width, total_height)
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
    let Some(crop_rect) =
        clamped_crop_screen_rect(crop_state.start_screen, crop_state.current_screen, image_rect)
    else {
        return;
    };

    painter.rect_stroke(
        image_rect,
        0.0,
        egui::Stroke::new(1.0, crate::theme::SHADE_BASE),
        egui::StrokeKind::Outside,
    );
    painter.rect_filled(
        crop_rect,
        0.0,
        egui::Color32::from_rgba_premultiplied(255, 255, 255, 24),
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
        current_screen.x.clamp(image_rect.left(), image_rect.right()),
        current_screen.y.clamp(image_rect.top(), image_rect.bottom()),
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

fn crop_center_offset_local(
    asset: &crate::data::ImageAsset,
    local_rect: egui::Rect,
) -> egui::Vec2 {
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
        crop_center_offset_local, merge_cropped_item_fields, pixel_crop_rect_from_local,
        screen_rect_to_local_crop_rect,
    };
    use crate::data::{BoardItem, ImageAsset};
    use std::path::PathBuf;

    #[test]
    fn converts_drag_rect_to_crop_pixels_and_live_scale_position() {
        let screen_rect =
            eframe::egui::Rect::from_min_max(eframe::egui::pos2(40.0, 20.0), eframe::egui::pos2(240.0, 180.0));
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
        let kept_local =
            eframe::egui::Rect::from_min_max(eframe::egui::pos2(10.0, 20.0), eframe::egui::pos2(40.0, 50.0));
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
}
