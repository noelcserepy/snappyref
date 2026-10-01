pub(crate) mod arrangement;
pub(crate) mod content_resolution;
pub(crate) mod creation;
pub(crate) mod group_layout;
pub(crate) mod group_pixels;
pub(crate) mod lifecycle;

pub(crate) mod initial_layout;
pub use crate::data::DEFAULT_PLACEMENT_TARGET_AREA;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn large_images_display_at_target_area_and_small_images_keep_full_size() {
        for (width, height, target_area) in [
            (1000, 1500, DEFAULT_PLACEMENT_TARGET_AREA),
            (1920, 1080, DEFAULT_PLACEMENT_TARGET_AREA),
            (1000, 1500, 40_000.0),
        ] {
            let scale = display_scale_for_placement_target_area(width, height, target_area);
            let area = width as f32 * height as f32 * scale * scale;
            assert!(
                (area - target_area).abs() < 1.0,
                "{width}x{height} displays at {area}, expected {target_area}"
            );
        }
        assert_eq!(
            display_scale_for_placement_target_area(180, 240, DEFAULT_PLACEMENT_TARGET_AREA),
            1.0
        );
    }
}
