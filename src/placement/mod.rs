pub(crate) mod arrangement;
pub(crate) mod content_resolution;
pub(crate) mod creation;
pub(crate) mod group_layout;
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
    fn scales_down_large_portrait_images_to_target_area() {
        let scale =
            display_scale_for_placement_target_area(1000, 1500, DEFAULT_PLACEMENT_TARGET_AREA);
        assert!(
            (scale - (DEFAULT_PLACEMENT_TARGET_AREA / 1_500_000.0).sqrt()).abs() < f32::EPSILON
        );
    }

    #[test]
    fn scales_down_large_landscape_images_by_area_cap() {
        let scale =
            display_scale_for_placement_target_area(1920, 1080, DEFAULT_PLACEMENT_TARGET_AREA);
        assert!(
            (scale - (DEFAULT_PLACEMENT_TARGET_AREA / 2_073_600.0).sqrt()).abs() < f32::EPSILON
        );
    }

    #[test]
    fn scales_down_large_images_to_configured_target_area() {
        let target_area = 40_000.0;
        let scale = display_scale_for_placement_target_area(1000, 1500, target_area);

        assert!((scale - (target_area / 1_500_000.0).sqrt()).abs() < f32::EPSILON);
    }

    #[test]
    fn keeps_smaller_images_at_full_scale() {
        assert_eq!(
            display_scale_for_placement_target_area(180, 240, DEFAULT_PLACEMENT_TARGET_AREA),
            1.0
        );
    }
}
