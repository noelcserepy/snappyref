use eframe::egui;

use crate::data::BoardItemWithAsset;
use crate::jobs::LoadedCanvasItem;

pub struct CanvasState {
    pan: egui::Vec2,
    zoom: f32,
    images: Vec<CanvasImage>,
    selected_item_id: Option<i64>,
    drag_state: Option<DragState>,
    dirty_item_ids: Vec<i64>,
    last_pointer_screen_pos: Option<egui::Pos2>,
}

struct CanvasImage {
    record: BoardItemWithAsset,
    texture: egui::TextureHandle,
}

struct DragState {
    item_id: i64,
    pointer_offset_world: egui::Vec2,
}

impl Default for CanvasState {
    fn default() -> Self {
        Self {
            pan: egui::Vec2::ZERO,
            zoom: 1.0,
            images: Vec::new(),
            selected_item_id: None,
            drag_state: None,
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

    fn world_to_screen(&self, world: egui::Pos2, rect: egui::Rect) -> egui::Pos2 {
        rect.center() + self.pan + world.to_vec2() * self.zoom
    }

    pub fn screen_to_world(&self, screen: egui::Pos2, rect: egui::Rect) -> egui::Pos2 {
        let world = (screen - rect.center() - self.pan) / self.zoom;
        egui::pos2(world.x, world.y)
    }

    pub fn preferred_drop_world(&self, rect: egui::Rect) -> egui::Pos2 {
        let screen_pos = self.last_pointer_screen_pos.unwrap_or(rect.center());
        self.screen_to_world(screen_pos, rect)
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
}

pub fn show(ui: &mut egui::Ui, rect: egui::Rect, state: &mut CanvasState) {
    let response = ui.allocate_rect(rect, egui::Sense::click_and_drag());
    handle_input(ui, rect, state, &response);

    let painter = ui.painter_at(rect);

    painter.rect_filled(rect, 0.0, egui::Color32::from_rgb(10, 12, 16));
    paint_grid(&painter, rect, state);
    paint_images(&painter, rect, state);
}

fn handle_input(
    ui: &egui::Ui,
    rect: egui::Rect,
    state: &mut CanvasState,
    response: &egui::Response,
) {
    if response.dragged_by(egui::PointerButton::Middle) {
        state.pan += response.drag_delta();
    }

    let pointer_pos = ui.input(|i| i.pointer.latest_pos());
    if response.hovered() {
        state.last_pointer_screen_pos = pointer_pos;
    }

    if ui.input(|i| i.pointer.button_pressed(egui::PointerButton::Primary))
        && let Some(pointer_pos) = pointer_pos
    {
        if let Some(item_id) = hit_test_image(state, rect, pointer_pos) {
            state.selected_item_id = Some(item_id);

            let pointer_world = state.screen_to_world(pointer_pos, rect);
            if let Some(image) = state
                .images
                .iter()
                .find(|image| image.record.item.id == item_id)
            {
                let item_center = egui::pos2(image.record.item.x, image.record.item.y);
                state.drag_state = Some(DragState {
                    item_id,
                    pointer_offset_world: item_center - pointer_world,
                });
            }
        } else {
            state.drag_state = None;
        }
    }

    if let Some(drag_state) = &state.drag_state
        && ui.input(|i| i.pointer.primary_down())
        && let Some(pointer_pos) = pointer_pos
    {
        let pointer_world = state.screen_to_world(pointer_pos, rect);
        let next_center = pointer_world + drag_state.pointer_offset_world;

        if let Some(image) = state
            .images
            .iter_mut()
            .find(|image| image.record.item.id == drag_state.item_id)
        {
            image.record.item.x = next_center.x;
            image.record.item.y = next_center.y;
        }
    }

    if ui.input(|i| i.pointer.button_released(egui::PointerButton::Primary))
        && let Some(drag_state) = state.drag_state.take()
    {
        state.dirty_item_ids.push(drag_state.item_id);
    }

    if !response.hovered() {
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
    state.zoom = (state.zoom * zoom_factor).clamp(0.2, 6.0);
    let screen_after = state.world_to_screen(world_before, rect);
    state.pan += pointer_pos - screen_after;
}

fn paint_images(painter: &egui::Painter, rect: egui::Rect, state: &CanvasState) {
    for image in &state.images {
        let image_rect = image_screen_rect(state, rect, image);

        painter.rect_filled(
            image_rect.expand(2.0),
            6.0,
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
            4.0,
            if state.selected_item_id == Some(image.record.item.id) {
                egui::Stroke::new(1.0, egui::Color32::WHITE)
            } else {
                egui::Stroke::new(1.0, egui::Color32::from_rgb(56, 62, 82))
            },
            egui::StrokeKind::Outside,
        );
    }
}

fn paint_grid(painter: &egui::Painter, rect: egui::Rect, state: &CanvasState) {
    let minor_step_world = 24.0;
    let major_every = 4;
    let minor_color = egui::Color32::from_rgb(18, 22, 30);
    let major_color = egui::Color32::from_rgb(28, 34, 46);
    let visible_world = rect.size() / state.zoom;
    let min_world = egui::pos2(-visible_world.x, -visible_world.y) + (-state.pan / state.zoom);
    let max_world = egui::pos2(visible_world.x, visible_world.y) + (-state.pan / state.zoom);

    let first_x = (min_world.x / minor_step_world).floor() as i32 - 1;
    let last_x = (max_world.x / minor_step_world).ceil() as i32 + 1;
    for index in first_x..=last_x {
        let world_x = index as f32 * minor_step_world;
        let screen_x = state.world_to_screen(egui::pos2(world_x, 0.0), rect).x;
        if screen_x < rect.left() || screen_x > rect.right() {
            continue;
        }
        let color = if index.rem_euclid(major_every) == 0 {
            major_color
        } else {
            minor_color
        };
        painter.line_segment(
            [
                egui::pos2(screen_x, rect.top()),
                egui::pos2(screen_x, rect.bottom()),
            ],
            egui::Stroke::new(1.0, color),
        );
    }

    let first_y = (min_world.y / minor_step_world).floor() as i32 - 1;
    let last_y = (max_world.y / minor_step_world).ceil() as i32 + 1;
    for index in first_y..=last_y {
        let world_y = index as f32 * minor_step_world;
        let screen_y = state.world_to_screen(egui::pos2(0.0, world_y), rect).y;
        if screen_y < rect.top() || screen_y > rect.bottom() {
            continue;
        }
        let color = if index.rem_euclid(major_every) == 0 {
            major_color
        } else {
            minor_color
        };
        painter.line_segment(
            [
                egui::pos2(rect.left(), screen_y),
                egui::pos2(rect.right(), screen_y),
            ],
            egui::Stroke::new(1.0, color),
        );
    }
}
