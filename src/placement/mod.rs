pub(crate) mod arrangement;
pub(crate) mod content_resolution;
pub(crate) mod creation;
pub(crate) mod group_layout;
pub(crate) mod lifecycle;

pub(crate) mod initial_layout;
pub const DEFAULT_PLACEMENT_TARGET_AREA: f32 = crate::data::DEFAULT_PLACEMENT_TARGET_AREA;
pub fn placement_target_reference_span() -> f32 {
    DEFAULT_PLACEMENT_TARGET_AREA.sqrt()
}
pub fn display_scale_for_placement_target_area(
    asset_width: u32,
    asset_height: u32,
    placement_target_area: f32,
) -> f32 {
    if asset_width == 0 || asset_height == 0 {
        return 1.0;
    }

    let placement_target_area = placement_target_area.max(1.0);
    let asset_area = asset_width as f32 * asset_height as f32;
    (placement_target_area / asset_area).sqrt().min(1.0)
}

pub(crate) mod completion;
