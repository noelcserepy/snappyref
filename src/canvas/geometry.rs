use eframe::egui;

use super::PixelCropRect;

pub(super) fn image_screen_rect_for_record(
    record: &crate::data::BoardItemWithAsset,
    rect: egui::Rect,
    pan: egui::Vec2,
    zoom: f32,
    screen_center: egui::Pos2,
) -> egui::Rect {
    let center = screen_center + pan + egui::vec2(record.item.x, record.item.y) * zoom;
    let size = egui::vec2(record.asset.width as f32, record.asset.height as f32)
        * record.item.scale.max(0.01)
        * zoom;
    let _ = rect;
    egui::Rect::from_center_size(center, size)
}

pub(super) fn min_image_scale(asset: &crate::data::ImageAsset) -> f32 {
    let width_scale = super::MIN_IMAGE_WORLD_SIZE / asset.width.max(1) as f32;
    let height_scale = super::MIN_IMAGE_WORLD_SIZE / asset.height.max(1) as f32;
    width_scale.max(height_scale)
}

pub(super) fn clamped_crop_screen_rect(
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

pub(super) fn screen_rect_to_local_crop_rect(
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

pub(super) fn pixel_crop_rect_from_local(
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

pub(super) fn crop_center_offset_local(
    asset: &crate::data::ImageAsset,
    local_rect: egui::Rect,
) -> egui::Vec2 {
    let asset_center = egui::pos2(asset.width as f32 * 0.5, asset.height as f32 * 0.5);
    let local_center = local_rect.center();
    local_center - asset_center
}

pub(super) fn merge_cropped_item_fields(
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
pub(super) fn lattice_phase_world(world: f32, world_step: f32) -> f32 {
    world.rem_euclid(world_step) / world_step
}

#[cfg(test)]
pub(super) fn lattice_phase_screen(
    screen_origin: f32,
    screen_position: f32,
    screen_step: f32,
) -> f32 {
    (screen_position - screen_origin).rem_euclid(screen_step) / screen_step
}
