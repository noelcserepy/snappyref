use eframe::egui;
use serde::Serialize;

use crate::data::{BoardItemWithAsset, UserSession};
use crate::jobs::LoadedCanvasItem;

const MIN_CANVAS_ZOOM: f32 = 0.01;
const MAX_CANVAS_ZOOM: f32 = 64.0;

pub struct CanvasState {
    pan: egui::Vec2,
    zoom: f32,
    images: Vec<CanvasImage>,
    selected_item_ids: Vec<i64>,
    drag_state: Option<DragState>,
    marquee_state: Option<MarqueeState>,
    dirty_item_ids: Vec<i64>,
    last_pointer_screen_pos: Option<egui::Pos2>,
}

pub struct SidebarEntry {
    pub item_id: i64,
    pub filename: String,
    pub imported_at: i64,
    pub selected: bool,
}

#[cfg_attr(not(debug_assertions), allow(dead_code))]
#[derive(Clone, Debug, Serialize)]
pub struct CanvasSnapshot {
    pub camera_pan_x: f32,
    pub camera_pan_y: f32,
    pub camera_zoom: f32,
    pub selected_item_ids: Vec<i64>,
    pub items: Vec<CanvasItemSnapshot>,
}

#[cfg_attr(not(debug_assertions), allow(dead_code))]
#[derive(Clone, Debug, Serialize)]
pub struct CanvasItemSnapshot {
    pub item_id: i64,
    pub asset_id: i64,
    pub filename: String,
    pub mime_type: String,
    pub x: f32,
    pub y: f32,
    pub scale: f32,
    pub rotation: f32,
    pub z_index: i32,
    pub selected: bool,
    pub screen_min_x: f32,
    pub screen_min_y: f32,
    pub screen_max_x: f32,
    pub screen_max_y: f32,
}

struct CanvasImage {
    record: BoardItemWithAsset,
    texture: egui::TextureHandle,
}

struct DragState {
    pointer_start_world: egui::Pos2,
    starting_positions: Vec<(i64, egui::Pos2)>,
}

struct MarqueeState {
    start_screen: egui::Pos2,
    current_screen: egui::Pos2,
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

    #[cfg_attr(not(debug_assertions), allow(dead_code))]
    pub fn replace_items(&mut self, ctx: &egui::Context, items: Vec<LoadedCanvasItem>) {
        self.clear();
        for item in items {
            self.add_loaded_item(ctx, item);
        }
    }

    #[cfg_attr(not(debug_assertions), allow(dead_code))]
    pub fn clear(&mut self) {
        self.images.clear();
        self.selected_item_ids.clear();
        self.drag_state = None;
        self.marquee_state = None;
        self.dirty_item_ids.clear();
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

    #[cfg_attr(not(debug_assertions), allow(dead_code))]
    pub fn set_camera(&mut self, pan_x: f32, pan_y: f32, zoom: f32) {
        self.pan = egui::vec2(pan_x, pan_y);
        self.zoom = zoom.clamp(MIN_CANVAS_ZOOM, MAX_CANVAS_ZOOM);
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

    #[cfg_attr(not(debug_assertions), allow(dead_code))]
    pub fn viewport_center_world(&self, rect: egui::Rect) -> egui::Pos2 {
        self.screen_to_world(rect.center(), rect)
    }

    pub fn sidebar_entries(&self) -> Vec<SidebarEntry> {
        let mut entries = self
            .images
            .iter()
            .map(|image| SidebarEntry {
                item_id: image.record.item.id,
                filename: image.record.asset.filename.clone(),
                imported_at: image.record.asset.imported_at,
                selected: self.is_selected(image.record.item.id),
            })
            .collect::<Vec<_>>();

        entries.sort_by(|left, right| {
            right
                .imported_at
                .cmp(&left.imported_at)
                .then_with(|| left.filename.cmp(&right.filename))
        });
        entries
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
            .filter(|item_id| self.images.iter().any(|image| image.record.item.id == *item_id))
            .collect();
    }

    #[cfg_attr(not(debug_assertions), allow(dead_code))]
    pub fn move_selected_by(&mut self, dx: f32, dy: f32) {
        let selected_ids = self.selected_item_ids.clone();
        let mut moved_ids = Vec::new();
        for image in &mut self.images {
            if selected_ids.contains(&image.record.item.id) {
                image.record.item.x += dx;
                image.record.item.y += dy;
                moved_ids.push(image.record.item.id);
            }
        }
        for item_id in moved_ids {
            self.mark_dirty(item_id);
        }
    }

    #[cfg_attr(not(debug_assertions), allow(dead_code))]
    pub fn set_item_position(&mut self, item_id: i64, x: f32, y: f32) -> Result<(), String> {
        let image = self
            .images
            .iter_mut()
            .find(|image| image.record.item.id == item_id)
            .ok_or_else(|| format!("item {item_id} not found"))?;

        image.record.item.x = x;
        image.record.item.y = y;
        self.mark_dirty(item_id);
        Ok(())
    }

    #[cfg_attr(not(debug_assertions), allow(dead_code))]
    pub fn set_item_scale(&mut self, item_id: i64, scale: f32) -> Result<(), String> {
        let clamped_scale = scale.max(0.01);
        let image = self
            .images
            .iter_mut()
            .find(|image| image.record.item.id == item_id)
            .ok_or_else(|| format!("item {item_id} not found"))?;

        image.record.item.scale = clamped_scale;
        self.mark_dirty(item_id);
        Ok(())
    }

    #[cfg_attr(not(debug_assertions), allow(dead_code))]
    pub fn focus_item(
        &mut self,
        item_id: i64,
        rect: egui::Rect,
        zoom: Option<f32>,
    ) -> Result<(), String> {
        let (item_x, item_y) = self
            .images
            .iter()
            .find(|image| image.record.item.id == item_id)
            .map(|image| (image.record.item.x, image.record.item.y))
            .ok_or_else(|| format!("item {item_id} not found"))?;

        if let Some(zoom) = zoom {
            self.zoom = zoom.clamp(MIN_CANVAS_ZOOM, MAX_CANVAS_ZOOM);
        }

        let target_world = egui::vec2(item_x, item_y) * self.zoom;
        self.pan = -target_world;
        let centered_screen = self.world_to_screen(egui::pos2(item_x, item_y), rect);
        self.pan += rect.center() - centered_screen;
        Ok(())
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
        removed_ids
    }

    #[cfg_attr(not(debug_assertions), allow(dead_code))]
    pub fn snapshot(&self, rect: egui::Rect) -> CanvasSnapshot {
        let items = self
            .images
            .iter()
            .map(|image| {
                let screen_rect = image_screen_rect(self, rect, image);
                CanvasItemSnapshot {
                    item_id: image.record.item.id,
                    asset_id: image.record.asset.id,
                    filename: image.record.asset.filename.clone(),
                    mime_type: image.record.asset.mime_type.clone(),
                    x: image.record.item.x,
                    y: image.record.item.y,
                    scale: image.record.item.scale,
                    rotation: image.record.item.rotation,
                    z_index: image.record.item.z_index,
                    selected: self.is_selected(image.record.item.id),
                    screen_min_x: screen_rect.min.x,
                    screen_min_y: screen_rect.min.y,
                    screen_max_x: screen_rect.max.x,
                    screen_max_y: screen_rect.max.y,
                }
            })
            .collect();

        CanvasSnapshot {
            camera_pan_x: self.pan.x,
            camera_pan_y: self.pan.y,
            camera_zoom: self.zoom,
            selected_item_ids: self.selected_item_ids.clone(),
            items,
        }
    }

    fn is_selected(&self, item_id: i64) -> bool {
        self.selected_item_ids.contains(&item_id)
    }

    fn set_single_selection(&mut self, item_id: i64) {
        self.selected_item_ids.clear();
        self.selected_item_ids.push(item_id);
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
}

pub fn show(ui: &mut egui::Ui, rect: egui::Rect, state: &mut CanvasState, debug_labels: bool) {
    let response = ui.allocate_rect(rect, egui::Sense::click_and_drag());
    handle_input(ui, rect, state, &response);

    let painter = ui.painter_at(rect);

    painter.rect_filled(rect, 0.0, crate::theme::SHADE_90);
    paint_images(&painter, rect, state, debug_labels);
    paint_marquee(&painter, state);
}

fn handle_input(
    ui: &egui::Ui,
    rect: egui::Rect,
    state: &mut CanvasState,
    response: &egui::Response,
) {
    let pointer_pos = ui.input(|i| i.pointer.latest_pos());
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
        if let Some(item_id) = hit_test_image(state, rect, pointer_pos) {
            if !state.is_selected(item_id) {
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
        } else {
            state.drag_state = None;
            state.clear_selection();
            state.marquee_state = Some(MarqueeState {
                start_screen: pointer_pos,
                current_screen: pointer_pos,
            });
        }
    }

    if let Some(drag_state) = &state.drag_state
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
        if let Some(drag_state) = state.drag_state.take() {
            for (item_id, _) in drag_state.starting_positions {
                state.mark_dirty(item_id);
            }
        }
        state.marquee_state = None;
    }

    if let Some(marquee_state) = &mut state.marquee_state
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

fn paint_images(painter: &egui::Painter, rect: egui::Rect, state: &CanvasState, debug_labels: bool) {
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
        painter.rect_stroke(
            image_rect,
            0.0,
            if state.is_selected(image.record.item.id) {
                egui::Stroke::new(1.0, crate::theme::SHADE_BASE)
            } else {
                egui::Stroke::new(1.0, crate::theme::SHADE_60)
            },
            egui::StrokeKind::Outside,
        );

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
