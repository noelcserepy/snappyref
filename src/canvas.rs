use eframe::egui;

use crate::data::{BoardItemWithAsset, UserSession};
use crate::jobs::LoadedCanvasItem;

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
    pub filename: String,
    pub imported_at: i64,
    pub selected: bool,
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

    pub fn apply_user_session(&mut self, session: &UserSession) {
        self.pan = egui::vec2(session.camera_pan_x, session.camera_pan_y);
        self.zoom = session.camera_zoom.clamp(0.2, 6.0);
    }

    pub fn user_session(&self) -> UserSession {
        UserSession {
            camera_pan_x: self.pan.x,
            camera_pan_y: self.pan.y,
            camera_zoom: self.zoom,
        }
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

    pub fn sidebar_entries(&self) -> Vec<SidebarEntry> {
        let mut entries = self
            .images
            .iter()
            .map(|image| SidebarEntry {
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
        if !self.dirty_item_ids.contains(&item_id) {
            self.dirty_item_ids.push(item_id);
        }
    }
}

pub fn show(ui: &mut egui::Ui, rect: egui::Rect, state: &mut CanvasState) {
    let response = ui.allocate_rect(rect, egui::Sense::click_and_drag());
    handle_input(ui, rect, state, &response);

    let painter = ui.painter_at(rect);

    painter.rect_filled(rect, 0.0, crate::theme::SHADE_90);
    paint_images(&painter, rect, state);
    paint_marquee(&painter, state);
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
