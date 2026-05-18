use eframe::egui;

use super::PixelCropRect;

pub(super) fn image_screen_center_for_record(
    record: &crate::data::Placement,
    pan: egui::Vec2,
    zoom: f32,
    screen_center: egui::Pos2,
) -> egui::Pos2 {
    screen_center + pan + egui::vec2(record.state.center.x, record.state.center.y) * zoom
}

pub(super) fn image_screen_size_for_record(
    record: &crate::data::Placement,
    zoom: f32,
) -> egui::Vec2 {
    record.scaled_visual_size() * zoom
}

pub(super) fn image_screen_corners_for_record(
    record: &crate::data::Placement,
    pan: egui::Vec2,
    zoom: f32,
    screen_center: egui::Pos2,
) -> [egui::Pos2; 4] {
    let center = image_screen_center_for_record(record, pan, zoom, screen_center);
    let half_size = image_screen_size_for_record(record, zoom) * 0.5;
    let sin = record.state.rotation.sin();
    let cos = record.state.rotation.cos();
    let rotate = |offset: egui::Vec2| {
        egui::vec2(
            offset.x * cos - offset.y * sin,
            offset.x * sin + offset.y * cos,
        )
    };

    [
        center + rotate(egui::vec2(-half_size.x, -half_size.y)),
        center + rotate(egui::vec2(half_size.x, -half_size.y)),
        center + rotate(egui::vec2(half_size.x, half_size.y)),
        center + rotate(egui::vec2(-half_size.x, half_size.y)),
    ]
}

pub(super) fn image_screen_rect_for_record(
    record: &crate::data::Placement,
    rect: egui::Rect,
    pan: egui::Vec2,
    zoom: f32,
    screen_center: egui::Pos2,
) -> egui::Rect {
    let _ = rect;
    image_screen_corners_for_record(record, pan, zoom, screen_center)
        .into_iter()
        .fold(egui::Rect::NOTHING, |mut bounds, corner| {
            bounds.extend_with(corner);
            bounds
        })
}

pub(super) fn image_rotation_handle_screen_pos(
    record: &crate::data::Placement,
    pan: egui::Vec2,
    zoom: f32,
    screen_center: egui::Pos2,
) -> (egui::Pos2, egui::Pos2) {
    let corners = image_screen_corners_for_record(record, pan, zoom, screen_center);
    let top_mid = corners[0].lerp(corners[1], 0.5);
    let bottom_mid = corners[3].lerp(corners[2], 0.5);
    let outward = (top_mid - bottom_mid).normalized();
    let handle = top_mid + outward * super::IMAGE_ROTATE_HANDLE_OFFSET_SCREEN;
    (top_mid, handle)
}

pub(super) fn min_image_scale(asset: &crate::data::ImageAsset) -> f32 {
    let width_scale = super::MIN_IMAGE_WORLD_SIZE / asset.width.max(1) as f32;
    let height_scale = super::MIN_IMAGE_WORLD_SIZE / asset.height.max(1) as f32;
    width_scale.max(height_scale)
}

pub(super) fn image_contains_screen_point(
    record: &crate::data::Placement,
    pointer_pos: egui::Pos2,
    pan: egui::Vec2,
    zoom: f32,
    screen_center: egui::Pos2,
) -> bool {
    let center = image_screen_center_for_record(record, pan, zoom, screen_center);
    let local = pointer_pos - center;
    let sin = (-record.state.rotation).sin();
    let cos = (-record.state.rotation).cos();
    let unrotated = egui::vec2(local.x * cos - local.y * sin, local.x * sin + local.y * cos);
    let half_size = image_screen_size_for_record(record, zoom) * 0.5;

    unrotated.x.abs() <= half_size.x && unrotated.y.abs() <= half_size.y
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
    item: &mut crate::data::PlacementState,
    asset_id: i64,
    crop_offset_local: egui::Vec2,
) {
    item.asset_id = Some(asset_id);
    let scale = item.scale.max(0.01);
    item.center.x += crop_offset_local.x * scale;
    item.center.y += crop_offset_local.y * scale;
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
