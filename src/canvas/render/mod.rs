mod annotations;
mod background;
mod image_pass;
pub(super) mod images;
mod overlays;
mod textbox_editor;

#[cfg(test)]
pub(super) use annotations::annotation_paint_stats_for_annotations;
pub(super) use annotations::paint_annotation_draft;
pub(super) use background::paint_background;
#[cfg(test)]
pub(super) use background::{
    background_lod_layers, background_screen_step, background_world_step,
    settled_background_lod_level,
};
pub(super) use image_pass::{canvas_texture_from_preview, paint_canvas_items};
#[cfg(test)]
pub(super) use image_pass::{
    optimized_image_pass_eligible_for_state, unified_canvas_paint_order_for_state,
};
pub(super) use overlays::{
    paint_crop_marquee, paint_guide_snap_indicators, paint_hover_hairline, paint_marquee,
    paint_provisional_placement_previews, paint_selected_placement_borders, paint_selection_bounds,
};
pub(super) use textbox_editor::{
    annotation_style_toolbar_rect, editing_textbox_toolbar_rect, layout_textbox_galley,
    show_annotation_style_toolbar, show_textbox_editor, textbox_layout_cache_key,
    textbox_pointer_region, textbox_required_world_height,
};
#[cfg(test)]
pub(super) use textbox_editor::{
    textbox_content_rect, textbox_empty_hotspot_rect, textbox_font_size,
    textbox_text_interactive_rect_from_galley,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canvas::{CanvasImageLod, CanvasState, CanvasTexture, PlacementImage};
    use crate::data::{Asset, Placement, PlacementState};
    use eframe::egui;
    use std::path::PathBuf;

    #[test]
    fn visible_overview_image_promotes_to_working_lod() {
        let mut state = CanvasState::default();
        let (placement, image) = test_image(7, CanvasImageLod::Overview, [256, 256]);
        state.install_placement_image(placement, image);
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));

        state.request_image_lod_load(
            7,
            image_pass::desired_image_lod(&state, rect, 7).expect("overview should promote"),
        );

        assert_eq!(
            state.render.image_lod_loads.get(&7).map(|load| load.lod),
            Some(CanvasImageLod::Working)
        );
    }

    #[test]
    fn working_image_promotes_to_detail_only_when_screen_size_exceeds_texture_threshold() {
        let mut state = CanvasState {
            zoom: 2.0,
            ..Default::default()
        };
        let (placement, image) = test_image(9, CanvasImageLod::Working, [256, 256]);
        state.install_placement_image(placement, image);
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));

        assert_eq!(
            image_pass::desired_image_lod(&state, rect, 9),
            Some(CanvasImageLod::Detail)
        );
    }

    #[test]
    fn selection_border_hides_while_placement_movement_is_recent() {
        let state = CanvasState {
            interaction: crate::canvas::CanvasInteractionState {
                gesture: Some(crate::canvas::ActiveGesture::PlacementDrag(
                    crate::canvas::DragState {
                        pointer_start_world: egui::Pos2::ZERO,
                        starting_items: vec![
                            crate::placement::arrangement::PlacementArrangementInput::new(
                                1,
                                egui::pos2(0.0, 0.0),
                                egui::vec2(100.0, 100.0),
                            ),
                        ],
                        snap_preview_rect_world: None,
                        started_at_seconds: 10.0,
                        last_movement_seconds: 10.5,
                        last_world_delta: egui::vec2(4.0, 0.0),
                        moved: true,
                    },
                )),
                ..Default::default()
            },
            ..Default::default()
        };

        assert!(!image_pass::selection_border_visible(&state, 11.0));
        assert!(image_pass::selection_border_visible(&state, 12.6));
    }

    #[test]
    fn selected_placement_border_top_overlay_includes_lower_z_selection() {
        let mut state = CanvasState::default();
        for id in [1, 2] {
            let (placement, image) =
                test_image_at(id, CanvasImageLod::Working, [100, 100], egui::Pos2::ZERO);
            state.install_placement_image(placement, image);
        }
        state.select_placements(&[1]);
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));

        assert_eq!(
            unified_canvas_paint_order_for_state(&state),
            vec![
                crate::data::CanvasStackItem::Placement(1),
                crate::data::CanvasStackItem::Placement(2),
            ]
        );
        assert_eq!(
            overlays::selected_placement_border_paint_ids(&state, rect, 0.0),
            vec![1]
        );

        state.interaction.selected_placement_ids = vec![2];
        assert_eq!(
            overlays::selected_placement_border_paint_ids(&state, rect, 0.0),
            vec![2]
        );
    }

    fn test_image(
        placement_id: i64,
        loaded_lod: CanvasImageLod,
        texture_size: [usize; 2],
    ) -> (Placement, PlacementImage) {
        test_image_at(placement_id, loaded_lod, texture_size, egui::Pos2::ZERO)
    }

    fn test_image_at(
        placement_id: i64,
        loaded_lod: CanvasImageLod,
        texture_size: [usize; 2],
        center_world: egui::Pos2,
    ) -> (Placement, PlacementImage) {
        (
            Placement::loaded(
                PlacementState::new(
                    placement_id,
                    eframe::egui::pos2(center_world.x, center_world.y),
                    1.0,
                    0.0,
                    placement_id as i32,
                    false,
                ),
                Asset {
                    id: placement_id,
                    original_path: PathBuf::from(format!("image-{placement_id}.png")),
                    filename: format!("image-{placement_id}.png"),
                    mime_type: "image/png".to_owned(),
                    width: texture_size[0] as u32,
                    height: texture_size[1] as u32,
                },
            ),
            PlacementImage {
                texture: CanvasTexture::Svg {
                    uri: format!("bytes://image-{placement_id}.svg"),
                    bytes: std::sync::Arc::<[u8]>::from([]),
                    size: texture_size,
                },
                loaded_lod,
                animation: None,
            },
        )
    }
}
