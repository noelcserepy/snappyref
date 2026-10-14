mod annotations;
mod background;
mod image_pass;
pub(super) mod images;
mod overlays;
mod textbox_editor;

#[cfg(test)]
pub(super) use annotations::annotation_should_paint;
pub(super) use annotations::paint_annotation_draft;
pub(super) use annotations::{annotation_color, stroke_width_value};
pub(super) use background::paint_background;
#[cfg(test)]
pub(super) use background::{
    background_lod_layers, background_screen_step, settled_background_lod_level,
};
#[cfg(test)]
pub(super) use image_pass::optimized_image_pass_eligible_for_state;
pub(super) use image_pass::{canvas_texture_from_preview, paint_canvas_items};
pub(super) use overlays::{
    paint_crop_marquee, paint_guide_snap_indicators, paint_hover_hairline, paint_marquee,
    paint_provisional_placement_previews, paint_selected_placement_borders, paint_selection_bounds,
};
pub(super) use textbox_editor::{
    fit_label_to_text, layout_textbox_galley, show_textbox_editor, textbox_layout_cache_key,
    textbox_pointer_region,
};
pub(crate) use textbox_editor::{textbox_font_size, textbox_size_for_text};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canvas::{CanvasImageLod, CanvasState, CanvasTexture, PlacementImage};
    use crate::data::{Asset, Placement, PlacementState};
    use eframe::egui;
    use std::path::PathBuf;

    #[test]
    fn visible_image_promotes_one_lod_step_for_its_screen_size() {
        // (zoom, loaded LOD, desired LOD) for a 256px texture in an 800x600 canvas.
        let cases = [
            (1.0, CanvasImageLod::Overview, CanvasImageLod::Working),
            (2.0, CanvasImageLod::Working, CanvasImageLod::Detail),
        ];
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));

        for (zoom, loaded_lod, desired_lod) in cases {
            let mut state = CanvasState {
                zoom,
                ..Default::default()
            };
            let (placement, image) = test_image_at(7, loaded_lod, [256, 256], egui::Pos2::ZERO);
            state.install_placement_image(placement, image);

            assert_eq!(
                image_pass::desired_image_lod(&state, rect, 7),
                Some(desired_lod),
                "zoom {zoom}, loaded {loaded_lod:?}"
            );
        }
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

        assert!(!state.selection_border_visible(11.0));
        assert!(state.selection_border_visible(12.6));
    }

    #[test]
    fn selected_placement_border_paints_above_overlapping_higher_z_image() {
        let ctx = egui::Context::default();
        let mut state = CanvasState::default();
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
        let mut texture_ids = Vec::new();
        for (id, center) in [(1, egui::pos2(0.0, 0.0)), (2, egui::pos2(20.0, 0.0))] {
            let (placement, mut image) =
                test_image_at(id, CanvasImageLod::Working, [100, 100], center);
            let texture = ctx.load_texture(
                format!("border-test-{id}"),
                egui::ColorImage::filled([100, 100], egui::Color32::WHITE),
                egui::TextureOptions::LINEAR,
            );
            texture_ids.push(texture.id());
            image.texture = CanvasTexture::Managed(texture);
            state.install_placement_image(placement, image);
        }
        state.select_placements(&[1]);
        let output = crate::ui::test_frame::run_ui(
            &ctx,
            egui::RawInput {
                screen_rect: Some(rect),
                ..Default::default()
            },
            |ui| {
                crate::canvas::show(
                    ui,
                    rect,
                    rect,
                    &mut state,
                    None,
                    false,
                    crate::canvas::CanvasInputPolicy::Blocked,
                    false,
                );
            },
        );
        // Shape submission order is the renderer's paint order. The selected
        // lower image's outline must follow both overlapping image meshes.
        let painted = output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Mesh(mesh) if texture_ids.contains(&mesh.texture_id) => {
                    Some((mesh.texture_id, None))
                }
                egui::Shape::Path(path)
                    if path.closed
                        && path.points
                            == vec![
                                egui::pos2(350.0, 250.0),
                                egui::pos2(450.0, 250.0),
                                egui::pos2(450.0, 350.0),
                                egui::pos2(350.0, 350.0),
                            ] =>
                {
                    Some((egui::TextureId::default(), Some(path.stroke.color.clone())))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            painted,
            vec![
                (texture_ids[0], None),
                (texture_ids[1], None),
                (
                    egui::TextureId::default(),
                    Some(egui::epaint::ColorMode::Solid(crate::ui::theme::TEXT))
                ),
            ]
        );
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
