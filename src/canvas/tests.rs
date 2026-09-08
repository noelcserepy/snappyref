use super::geometry::{
    crop_center_offset_local, merge_cropped_item_fields, min_loaded_placement_scale,
    pixel_crop_rect_from_local, screen_rect_to_local_crop_rect,
};
use super::interaction::{
    handle_textbox_primary_press, hit_test_canvas_item, hit_test_placement_resize_handle,
    hit_test_textbox_resize_handle, moved_textbox_rect, normalized_textbox_rect,
    open_context_menu_at_pointer, pending_annotation_from_draft, placement_screen_rect,
    resized_placement_transform, resized_textbox_rect, textbox_screen_rect,
};
use super::render::{
    annotation_paint_stats_for_annotations, background_lod_layers, background_screen_step,
    editing_textbox_toolbar_rect, layout_textbox_galley, optimized_image_pass_eligible_for_state,
    settled_background_lod_level, textbox_content_rect, textbox_empty_hotspot_rect,
    textbox_font_size, textbox_layout_cache_key, textbox_pointer_region,
    textbox_required_world_height, textbox_text_interactive_rect_from_galley,
    unified_canvas_paint_order_for_state,
};
use super::{
    AnnotationDraftState, CanvasCropRequest, CanvasInputPolicy, CanvasState, CanvasTexture,
    FOCUS_SELECTION_PADDING_SCREEN, MIN_CANVAS_ZOOM, PixelCropRect, PlacementArrangementInput,
    PlacementImage, PlacementResizeHandle, TEXTBOX_EDITOR_TOOLBAR_CLEARANCE,
    TEXTBOX_EDITOR_TOOLBAR_VIEWPORT_MARGIN, TextboxPointerRegion, TextboxResizeHandle,
};
use crate::data::{
    Asset, CanvasAnnotation, CanvasAnnotationColor, CanvasAnnotationKind, CanvasImageLod,
    CanvasStackItem, CanvasStrokeWidth, CanvasTextSize, ContentResolutionSource, Placement,
    PlacementContent, PlacementLifecycle, PlacementState, rotated_display_size,
};
use crate::placement::arrangement::{
    SettleDirection, balanced_grid_layout_for_sizes, best_compact_layout, compact_layout_for_sizes,
    directional_settle_layout, horizontal_layout, layout_bounds_for_items, vertical_layout,
};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

fn push_test_image(
    ui: &eframe::egui::Ui,
    state: &mut CanvasState,
    placement_id: i64,
    width: u32,
    height: u32,
    scale: f32,
    center: eframe::egui::Pos2,
) {
    let texture = ui.ctx().load_texture(
        format!("test-image-{placement_id}"),
        eframe::egui::ColorImage::filled([4, 4], eframe::egui::Color32::WHITE),
        eframe::egui::TextureOptions::LINEAR,
    );
    state.install_placement_image(
        Placement::loaded(
            PlacementState::new(
                placement_id,
                eframe::egui::pos2(center.x, center.y),
                scale,
                0.0,
                0,
                false,
            ),
            Asset {
                id: placement_id,
                original_path: PathBuf::from(format!("image-{placement_id}.png")),
                filename: format!("image-{placement_id}.png"),
                mime_type: "image/png".to_owned(),
                width,
                height,
            },
        ),
        PlacementImage {
            texture: CanvasTexture::Managed(texture),
            loaded_lod: CanvasImageLod::Working,
            animation: None,
        },
    );
}

#[test]
fn viewport_only_input_zooms_without_preview_state() {
    let ctx = eframe::egui::Context::default();
    let mut state = CanvasState::default();
    state.set_pointer_mode(crate::data::PointerMode::Trackpad);
    let screen_rect = eframe::egui::Rect::from_min_size(
        eframe::egui::Pos2::ZERO,
        eframe::egui::vec2(800.0, 600.0),
    );

    let _ = ctx.run_ui(
        eframe::egui::RawInput {
            events: vec![
                eframe::egui::Event::PointerMoved(screen_rect.center()),
                eframe::egui::Event::Zoom(0.5),
            ],
            ..Default::default()
        },
        |ui| {
            super::show(
                ui,
                screen_rect,
                screen_rect,
                &mut state,
                None,
                false,
                CanvasInputPolicy::ViewportOnly,
                false,
            );
        },
    );

    assert!(state.viewport_state().camera_zoom < 1.0);
}

fn default_text_size() -> CanvasTextSize {
    CanvasTextSize::default()
}

fn default_annotation_color() -> CanvasAnnotationColor {
    CanvasAnnotationColor::Default
}

fn default_stroke_width() -> CanvasStrokeWidth {
    CanvasStrokeWidth::default()
}

fn test_record(placement_id: i64, asset_id: i64, width: u32, height: u32) -> Placement {
    Placement::loaded(
        PlacementState {
            id: placement_id,
            geometry: crate::data::RectGeometry::new(
                eframe::egui::Pos2::ZERO,
                eframe::egui::Vec2::ZERO,
                1.0,
                0.0,
                placement_id as i32,
            ),
            archived: false,
        },
        Asset {
            id: asset_id,
            original_path: PathBuf::from(format!("image-{asset_id}.png")),
            filename: format!("image-{asset_id}.png"),
            mime_type: "image/png".to_owned(),
            width,
            height,
        },
    )
}

fn unloaded_test_record(
    placement_id: i64,
    content: PlacementContent,
    display_width: f32,
    display_height: f32,
    scale: f32,
    center: eframe::egui::Pos2,
) -> Placement {
    let mut placement = Placement {
        state: PlacementState::with_display_size(
            placement_id,
            center,
            eframe::egui::vec2(display_width, display_height),
            scale,
            0.0,
            placement_id as i32,
            false,
        ),
        content: content.clone(),
        lifecycle: PlacementLifecycle::Committed,
    };
    placement.content = content;
    placement.lifecycle = PlacementLifecycle::Committed;
    placement
}

fn native_loaded_item(
    placement: Placement,
    texture: Arc<crate::image_preparation::NativeTextureHandle>,
) -> crate::image_preparation::PreparedPlacementImage {
    crate::image_preparation::PreparedPlacementImage {
        target_board_id: 1,
        queue_id: None,
        preview_rgba: vec![
            255;
            placement.require_loaded_asset().width as usize
                * placement.require_loaded_asset().height as usize
                * 4
        ],
        preview_width: placement.require_loaded_asset().width,
        preview_height: placement.require_loaded_asset().height,
        animated_preview: None,
        preview_texture: crate::image_preparation::ImagePrepPreviewTexture::Native(texture),
        loaded_lod: CanvasImageLod::Working,
        existing_load_diagnostics: None,
        placement,
        origin: crate::image_preparation::ImportOrigin::Existing,
        trace: None,
    }
}

fn animated_loaded_item(
    _ui: &eframe::egui::Ui,
    placement: Placement,
) -> crate::image_preparation::PreparedPlacementImage {
    let first_frame = vec![0, 0, 255, 255];
    let second_frame = vec![0, 255, 0, 255];
    crate::image_preparation::PreparedPlacementImage {
        target_board_id: 1,
        queue_id: None,
        preview_rgba: first_frame.clone(),
        preview_width: 1,
        preview_height: 1,
        animated_preview: Some(crate::image_preparation::AnimatedPreview {
            frames: vec![
                crate::image_preparation::AnimatedPreviewFrame {
                    rgba: first_frame,
                    delay_seconds: 0.1,
                },
                crate::image_preparation::AnimatedPreviewFrame {
                    rgba: second_frame,
                    delay_seconds: 0.1,
                },
            ],
            width: 1,
            height: 1,
        }),
        preview_texture: crate::image_preparation::ImagePrepPreviewTexture::Cpu,
        loaded_lod: CanvasImageLod::Working,
        existing_load_diagnostics: None,
        placement,
        origin: crate::image_preparation::ImportOrigin::Existing,
        trace: None,
    }
}

#[test]
fn provisional_placements_are_filtered_from_document_changes() {
    let mut state = CanvasState::default();
    let mut placement = unloaded_test_record(
        -1,
        PlacementContent::Loading {
            source: ContentResolutionSource::SearchAsset { asset_id: 42 },
            status_message: Some("Loading".to_owned()),
        },
        120.0,
        80.0,
        1.0,
        eframe::egui::pos2(10.0, 20.0),
    );
    placement.lifecycle = PlacementLifecycle::Provisional;
    state.add_provisional_unresolved_placement(placement);
    state.mark_dirty(-1);

    let changes = state.take_document_changes();

    assert!(changes.updated_placements.is_empty());
    assert!(changes.stack_order.is_none());
}

#[test]
fn cancel_removes_provisional_placements_without_archiving_rows() {
    let mut state = CanvasState::default();
    let mut placement = unloaded_test_record(
        -1,
        PlacementContent::Loading {
            source: ContentResolutionSource::SearchAsset { asset_id: 42 },
            status_message: Some("Loading".to_owned()),
        },
        120.0,
        80.0,
        1.0,
        eframe::egui::pos2(10.0, 20.0),
    );
    placement.lifecycle = PlacementLifecycle::Provisional;
    state.add_provisional_unresolved_placement(placement);
    state.select_placements(&[-1]);

    let removed = state.remove_provisional_placements();
    let changes = state.take_document_changes();

    assert_eq!(removed, vec![-1]);
    assert!(state.placement_for_id(-1).is_none());
    assert!(changes.updated_placements.is_empty());
    assert!(changes.archived_annotation_ids.is_empty());
}

#[test]
fn compact_guided_layout_for_sizes_snap_as_one_batch() {
    eframe::egui::__run_test_ui(|ui| {
        let mut state = CanvasState::default();
        push_test_image(
            ui,
            &mut state,
            1,
            100,
            100,
            1.0,
            eframe::egui::pos2(0.0, 0.0),
        );

        let arrangement_inputs = state.compact_guided_layout_for_sizes(
            &[
                eframe::egui::vec2(100.0, 100.0),
                eframe::egui::vec2(100.0, 100.0),
            ],
            eframe::egui::pos2(102.0, 0.0),
            eframe::egui::Rect::from_min_size(
                eframe::egui::Pos2::ZERO,
                eframe::egui::vec2(800.0, 600.0),
            ),
            24.0,
        );

        let bounds = arrangement_inputs
            .iter()
            .map(|(center, size)| eframe::egui::Rect::from_center_size(*center, *size))
            .reduce(|left, right| left.union(right))
            .expect("preview bounds");
        assert_eq!(arrangement_inputs.len(), 2);
        assert_eq!(bounds.left(), 50.0);
        assert_eq!(state.guide_snap_indicator_count(), 1);
    });
}

#[test]
fn apply_target_area_to_selected_placements_normalizes_displayed_area() {
    eframe::egui::__run_test_ui(|ui| {
        let mut state = CanvasState::default();
        push_test_image(
            ui,
            &mut state,
            31,
            200,
            100,
            1.0,
            eframe::egui::pos2(160.0, 120.0),
        );
        push_test_image(
            ui,
            &mut state,
            32,
            100,
            100,
            1.0,
            eframe::egui::pos2(360.0, 120.0),
        );
        state.select_placements(&[31, 32]);

        let changed = state.apply_target_area_to_selected_placements(90_000.0);

        assert_eq!(changed, 2);
        for placement_id in [31, 32] {
            let image = state.placement_by_id(placement_id).unwrap();
            let area = image.require_loaded_asset().width as f32
                * image.require_loaded_asset().height as f32
                * image.state.geometry.scale
                * image.state.geometry.scale;
            assert!((area - 90_000.0).abs() < 0.1);
        }
    });
}

#[test]
fn average_selected_placement_area_uses_displayed_area() {
    eframe::egui::__run_test_ui(|ui| {
        let mut state = CanvasState::default();
        push_test_image(
            ui,
            &mut state,
            41,
            100,
            100,
            1.0,
            eframe::egui::pos2(120.0, 120.0),
        );
        push_test_image(
            ui,
            &mut state,
            42,
            200,
            100,
            1.0,
            eframe::egui::pos2(320.0, 120.0),
        );
        state.select_placements(&[41, 42]);

        let average_area = state.average_selected_placement_area().unwrap();

        assert!((average_area - 15_000.0).abs() < 0.1);
    });
}

#[test]
fn background_lod_settles_inside_spacing_band() {
    for zoom in [0.12, 0.33, 0.9, 1.0, 1.8, 4.2, 12.0] {
        let level = settled_background_lod_level(0, zoom);
        let screen_step = background_screen_step(level, zoom);
        assert!(screen_step >= super::BACKGROUND_MIN_SCREEN_STEP);
        assert!(screen_step <= super::BACKGROUND_MAX_SCREEN_STEP);
    }
}

#[test]
fn background_lod_has_hysteresis_near_thresholds() {
    let level_after_zoom_out = settled_background_lod_level(0, 0.70);
    assert_eq!(level_after_zoom_out, 0);

    let level_after_more_zoom_out = settled_background_lod_level(level_after_zoom_out, 0.60);
    assert_eq!(level_after_more_zoom_out, 1);

    let level_after_small_recover = settled_background_lod_level(level_after_more_zoom_out, 0.76);
    assert_eq!(level_after_small_recover, 1);

    let level_after_large_zoom_in = settled_background_lod_level(level_after_small_recover, 1.80);
    assert!(level_after_large_zoom_in < level_after_small_recover);
    let screen_step = background_screen_step(level_after_large_zoom_in, 1.80);
    assert!(screen_step >= super::BACKGROUND_MIN_SCREEN_STEP);
    assert!(screen_step <= super::BACKGROUND_MAX_SCREEN_STEP);
}

#[test]
fn background_lod_layers_crossfade_near_zoom_thresholds() {
    let zoom_near_zoom_out_switch =
        (super::BACKGROUND_MIN_SCREEN_STEP + 12.0) / super::BACKGROUND_BASE_WORLD_STEP;
    let layers = background_lod_layers(0, zoom_near_zoom_out_switch);
    assert_eq!(layers.len(), 2);
    assert_eq!(layers[0].level, 1);
    assert_eq!(layers[1].level, 0);
    assert!((layers[0].alpha - 0.5).abs() < 0.001);
    assert!((layers[1].alpha - 0.5).abs() < 0.001);

    let zoom_near_zoom_in_switch =
        (super::BACKGROUND_MAX_SCREEN_STEP - 12.0) / super::BACKGROUND_BASE_WORLD_STEP;
    let layers = background_lod_layers(0, zoom_near_zoom_in_switch);
    assert_eq!(layers.len(), 2);
    assert_eq!(layers[0].level, -1);
    assert_eq!(layers[1].level, 0);
    assert!((layers[0].alpha - 0.5).abs() < 0.001);
    assert!((layers[1].alpha - 0.5).abs() < 0.001);
}

#[test]
fn horizontal_and_vertical_layouts_use_requested_gap() {
    let items = vec![
        arrangement_input(1, 80.0, 40.0),
        arrangement_input(2, 60.0, 40.0),
        arrangement_input(3, 40.0, 40.0),
    ];
    let gap = 18.0;

    let (horizontal, _, _) = horizontal_layout(&items, gap).unwrap();
    let (vertical, _, _) = vertical_layout(&items, gap).unwrap();

    assert_eq!(center_gap_x(&items, &horizontal, 1, 2), gap);
    assert_eq!(center_gap_y(&items, &vertical, 1, 2), gap);
}

#[test]
fn compact_layout_respects_gap() {
    let items = vec![
        arrangement_input(1, 80.0, 80.0),
        arrangement_input(2, 80.0, 60.0),
        arrangement_input(3, 60.0, 80.0),
        arrangement_input(4, 60.0, 60.0),
    ];
    let gap = 12.0;

    let (placements, _, _) = best_compact_layout(&items, gap).unwrap();

    for left in &items {
        for right in &items {
            if left.id >= right.id {
                continue;
            }
            let left_rect = placement_rect(&items, &placements, left.id);
            let right_rect = placement_rect(&items, &placements, right.id);
            assert!(rectangles_respect_gap(left_rect, right_rect, gap));
        }
    }
}

#[test]
fn directional_settle_right_moves_sparse_rows_to_wall() {
    let items = vec![
        positioned_arrangement_input(1, 50.0, 50.0, 100.0, 40.0),
        positioned_arrangement_input(2, 170.0, 50.0, 100.0, 40.0),
        positioned_arrangement_input(3, 50.0, 160.0, 100.0, 40.0),
    ];
    let gap = 20.0;

    let placements = directional_settle_layout(&items, SettleDirection::Right, gap, 1.0).unwrap();

    let top_right = placement_rect(&items, &placements, 2);
    let top_left = placement_rect(&items, &placements, 1);
    let bottom_lone = placement_rect(&items, &placements, 3);

    assert_eq!(top_right.right(), 220.0);
    assert_eq!(top_left.right() + gap, top_right.left());
    assert_eq!(bottom_lone.right(), 220.0);
}

#[test]
fn directional_settle_uses_gap_expanded_cross_axis_blocking() {
    let items = vec![
        positioned_arrangement_input(1, 150.0, 50.0, 80.0, 40.0),
        positioned_arrangement_input(2, 20.0, 105.0, 80.0, 40.0),
    ];
    let gap = 20.0;

    let placements = directional_settle_layout(&items, SettleDirection::Right, gap, 1.0).unwrap();

    let settled_first = placement_rect(&items, &placements, 1);
    let settled_second = placement_rect(&items, &placements, 2);
    assert_eq!(settled_first.right(), 190.0);
    assert_eq!(settled_second.right() + gap, settled_first.left());
}

#[test]
fn directional_settle_grace_does_not_block_rows_at_exact_gap() {
    let items = vec![
        positioned_arrangement_input(1, 150.0, 50.0, 80.0, 40.0),
        positioned_arrangement_input(2, 20.0, 110.0, 80.0, 40.0),
    ];
    let gap = 20.0;

    let placements = directional_settle_layout(&items, SettleDirection::Right, gap, 1.0).unwrap();

    let settled_first = placement_rect(&items, &placements, 1);
    let settled_second = placement_rect(&items, &placements, 2);
    assert_eq!(settled_first.right(), 190.0);
    assert_eq!(settled_second.right(), 190.0);
}

#[test]
fn directional_settle_keeps_target_edge_fixed_and_can_shrink_opposite_edge() {
    let items = vec![
        positioned_arrangement_input(1, 50.0, 50.0, 80.0, 40.0),
        positioned_arrangement_input(2, 250.0, 50.0, 80.0, 40.0),
    ];
    let original_bounds = layout_bounds_for_items(&items).unwrap();

    let placements = directional_settle_layout(&items, SettleDirection::Right, 20.0, 1.0).unwrap();
    let settled_items = placements
        .iter()
        .map(|(id, center)| {
            let item = items.iter().find(|item| item.id == *id).unwrap();
            positioned_arrangement_input(*id, center.x, center.y, item.width(), item.height())
        })
        .collect::<Vec<_>>();
    let settled_bounds = layout_bounds_for_items(&settled_items).unwrap();

    assert_eq!(settled_bounds.right(), original_bounds.right());
    assert!(settled_bounds.left() > original_bounds.left());
}

#[test]
fn directional_settle_layout_is_idempotent() {
    let items = vec![
        positioned_arrangement_input(1, 10.0, 50.0, 100.0, 40.0),
        positioned_arrangement_input(2, 170.0, 50.0, 100.0, 40.0),
        positioned_arrangement_input(3, 50.0, 160.0, 100.0, 40.0),
    ];
    let gap = 20.0;
    let first = directional_settle_layout(&items, SettleDirection::Right, gap, 1.0).unwrap();
    let settled_items = first
        .iter()
        .map(|(id, center)| {
            let item = items.iter().find(|item| item.id == *id).unwrap();
            positioned_arrangement_input(*id, center.x, center.y, item.width(), item.height())
        })
        .collect::<Vec<_>>();

    let second =
        directional_settle_layout(&settled_items, SettleDirection::Right, gap, 1.0).unwrap();

    assert_eq!(first.len(), second.len());
    for (first_id, first_pos) in &first {
        let second_pos = second
            .iter()
            .find(|(second_id, _)| second_id == first_id)
            .map(|(_, pos)| *pos)
            .unwrap();
        assert!((first_pos.x - second_pos.x).abs() < 0.001);
        assert!((first_pos.y - second_pos.y).abs() < 0.001);
    }
}

#[test]
fn compact_layout_is_more_square_than_row_or_column_extremes() {
    let items = vec![
        arrangement_input(1, 180.0, 90.0),
        arrangement_input(2, 140.0, 80.0),
        arrangement_input(3, 110.0, 160.0),
        arrangement_input(4, 90.0, 110.0),
        arrangement_input(5, 70.0, 70.0),
    ];
    let gap = 12.0;

    let (_, compact_width, compact_height) = best_compact_layout(&items, gap).unwrap();
    let (_, row_width, row_height) = horizontal_layout(&items, gap).unwrap();
    let (_, column_width, column_height) = vertical_layout(&items, gap).unwrap();

    let compact_ratio = aspect_ratio(compact_width, compact_height);
    let row_ratio = aspect_ratio(row_width, row_height);
    let column_ratio = aspect_ratio(column_width, column_height);

    assert!(compact_ratio < row_ratio);
    assert!(compact_ratio < column_ratio);
}

#[test]
fn compact_layout_is_deterministic_for_mixed_sizes() {
    let items = hole_prone_items();
    let gap = 12.0;

    let first = best_compact_layout(&items, gap).unwrap();
    let second = best_compact_layout(&items, gap).unwrap();

    assert_eq!(first.1, second.1);
    assert_eq!(first.2, second.2);
    assert_eq!(first.0.len(), second.0.len());
    for ((first_id, first_pos), (second_id, second_pos)) in first.0.iter().zip(second.0.iter()) {
        assert_eq!(first_id, second_id);
        assert!(
            (first_pos.x - second_pos.x).abs() < 0.001,
            "item {first_id} x moved from {} to {}",
            first_pos.x,
            second_pos.x
        );
        assert!(
            (first_pos.y - second_pos.y).abs() < 0.001,
            "item {first_id} y moved from {} to {}",
            first_pos.y,
            second_pos.y
        );
    }
}

#[test]
fn compact_layout_for_sizes_preserves_input_order() {
    let sizes = [
        eframe::egui::vec2(90.0, 40.0),
        eframe::egui::vec2(300.0, 520.0),
        eframe::egui::vec2(140.0, 180.0),
    ];

    let arrangement_inputs =
        compact_layout_for_sizes(&sizes, 12.0, eframe::egui::pos2(100.0, 200.0));

    assert_eq!(arrangement_inputs.len(), sizes.len());
    for (index, (_, size)) in arrangement_inputs.iter().enumerate() {
        assert_eq!(*size, sizes[index]);
    }
}

#[test]
fn balanced_grid_layout_avoids_single_column_for_equal_approximate_batch() {
    let sizes = vec![eframe::egui::vec2(360.0, 240.0); 13];
    let arrangement_inputs =
        balanced_grid_layout_for_sizes(&sizes, 24.0, eframe::egui::pos2(100.0, 200.0));
    let distinct_x =
        arrangement_inputs
            .iter()
            .fold(Vec::<i32>::new(), |mut values, (center, _)| {
                let rounded = center.x.round() as i32;
                if !values.contains(&rounded) {
                    values.push(rounded);
                }
                values
            });
    let distinct_y =
        arrangement_inputs
            .iter()
            .fold(Vec::<i32>::new(), |mut values, (center, _)| {
                let rounded = center.y.round() as i32;
                if !values.contains(&rounded) {
                    values.push(rounded);
                }
                values
            });

    assert_eq!(arrangement_inputs.len(), sizes.len());
    assert!(distinct_x.len() > 1);
    assert!(distinct_y.len() > 1);
}

#[test]
fn compact_layout_keeps_hole_prone_fixture_dense() {
    let items = hole_prone_items();
    let gap = 12.0;

    let (_, compact_width, compact_height) = best_compact_layout(&items, gap).unwrap();
    let item_area = items
        .iter()
        .map(|item| item.width() * item.height())
        .sum::<f32>();
    let occupancy = item_area / (compact_width * compact_height);

    assert!(
        occupancy > 0.75,
        "compact arrangement should keep the mixed-size fixture dense"
    );
}

#[test]
fn compact_layout_prefers_dense_candidate_over_square_with_holes() {
    let items = vec![
        arrangement_input(1, 300.0, 300.0),
        arrangement_input(2, 40.0, 50.0),
        arrangement_input(3, 50.0, 90.0),
        arrangement_input(4, 300.0, 60.0),
    ];
    let gap = 12.0;

    let (_, compact_width, compact_height) = best_compact_layout(&items, gap).unwrap();
    let item_area = items
        .iter()
        .map(|item| (item.width() + gap) * (item.height() + gap))
        .sum::<f32>();
    let occupancy = item_area / (compact_width * compact_height);

    assert!(
        occupancy > 0.75,
        "compact arrangement should prefer fewer holes over a more square empty box"
    );
}

#[test]
fn compact_layout_has_no_overlaps_for_hole_prone_fixture() {
    let items = hole_prone_items();
    let gap = 12.0;
    let (placements, _, _) = best_compact_layout(&items, gap).unwrap();

    for left in &items {
        for right in &items {
            if left.id >= right.id {
                continue;
            }
            let left_rect = placement_rect(&items, &placements, left.id);
            let right_rect = placement_rect(&items, &placements, right.id);
            assert!(!left_rect.intersects(right_rect));
        }
    }
}

#[test]
fn compact_arrangement_normalizes_one_world_unit_width_difference() {
    let mut state = CanvasState::default();
    let gap = 12.0;

    eframe::egui::__run_test_ui(|ui| {
        push_test_image(
            ui,
            &mut state,
            1,
            100,
            100,
            1.0,
            eframe::egui::pos2(0.0, 0.0),
        );
        push_test_image(
            ui,
            &mut state,
            2,
            101,
            100,
            1.0,
            eframe::egui::pos2(130.0, 0.0),
        );
    });
    state.select_placements(&[1, 2]);

    assert_eq!(state.arrange_selected_compact(gap), 2);

    let first = state
        .document
        .placements
        .iter()
        .find(|image| image.state.id == 1)
        .unwrap();
    let second = state
        .document
        .placements
        .iter()
        .find(|image| image.state.id == 2)
        .unwrap();
    let first_width = first.require_loaded_asset().width as f32 * first.state.geometry.scale;
    let second_width = second.require_loaded_asset().width as f32 * second.state.geometry.scale;
    assert!((first_width - second_width).abs() < 0.001);
}

#[test]
fn compact_arrangement_rejects_width_normalization_that_disrupts_height() {
    let mut state = CanvasState::default();
    let gap = 12.0;

    eframe::egui::__run_test_ui(|ui| {
        push_test_image(
            ui,
            &mut state,
            1,
            10,
            1000,
            1.0,
            eframe::egui::pos2(0.0, 0.0),
        );
        push_test_image(
            ui,
            &mut state,
            2,
            11,
            1000,
            1.0,
            eframe::egui::pos2(40.0, 0.0),
        );
    });
    state.select_placements(&[1, 2]);

    assert_eq!(state.arrange_selected_compact(gap), 2);

    let first = state
        .document
        .placements
        .iter()
        .find(|image| image.state.id == 1)
        .unwrap();
    assert!((first.state.geometry.scale - 1.0).abs() < 0.001);
}

#[test]
fn compact_preview_layout_does_not_normalize_sizes() {
    let sizes = [
        eframe::egui::vec2(100.0, 100.0),
        eframe::egui::vec2(101.0, 100.0),
    ];

    let arrangement_inputs = compact_layout_for_sizes(&sizes, 12.0, eframe::egui::pos2(0.0, 0.0));

    assert_eq!(arrangement_inputs[0].1, sizes[0]);
    assert_eq!(arrangement_inputs[1].1, sizes[1]);
}

#[test]
fn context_click_uses_frontmost_mixed_canvas_stack_item() {
    let mut state = CanvasState::default();
    let rect = eframe::egui::Rect::from_min_size(
        eframe::egui::Pos2::ZERO,
        eframe::egui::vec2(400.0, 300.0),
    );
    eframe::egui::__run_test_ui(|ui| {
        push_test_image(ui, &mut state, 1, 80, 80, 1.0, eframe::egui::pos2(0.0, 0.0));
    });
    state.add_annotation(CanvasAnnotation::new(
        20,
        CanvasAnnotationKind::Textbox,
        eframe::egui::pos2(-40.0, -40.0),
        eframe::egui::pos2(40.0, 40.0),
        "note".to_owned(),
        default_text_size(),
        default_stroke_width(),
        default_annotation_color(),
        false,
        false,
    ));
    state.set_canvas_stack_order(vec![
        CanvasStackItem::Annotation(20),
        CanvasStackItem::Placement(1),
    ]);

    open_context_menu_at_pointer(&mut state, rect, rect.center());

    assert_eq!(state.selected_placement_ids(), vec![1]);
    assert!(state.selected_annotation_ids().is_empty());
    assert!(state.take_context_menu_request().is_some());
}

#[test]
fn canvas_paint_order_uses_unified_stack_order_for_mixed_items() {
    let mut state = CanvasState::default();
    eframe::egui::__run_test_ui(|ui| {
        push_test_image(ui, &mut state, 1, 80, 80, 1.0, eframe::egui::pos2(0.0, 0.0));
        push_test_image(ui, &mut state, 2, 80, 80, 1.0, eframe::egui::pos2(0.0, 0.0));
    });
    state.add_annotation(CanvasAnnotation::new(
        20,
        CanvasAnnotationKind::Textbox,
        eframe::egui::pos2(-40.0, -40.0),
        eframe::egui::pos2(40.0, 40.0),
        "note".to_owned(),
        default_text_size(),
        default_stroke_width(),
        default_annotation_color(),
        false,
        false,
    ));
    state.set_canvas_stack_order(vec![
        CanvasStackItem::Placement(1),
        CanvasStackItem::Annotation(20),
        CanvasStackItem::Placement(2),
    ]);

    assert_eq!(
        unified_canvas_paint_order_for_state(&state),
        vec![
            CanvasStackItem::Placement(1),
            CanvasStackItem::Annotation(20),
            CanvasStackItem::Placement(2),
        ]
    );
}

#[test]
fn selected_canvas_item_moves_to_absolute_front_and_back() {
    let mut state = CanvasState::default();
    eframe::egui::__run_test_ui(|ui| {
        push_test_image(ui, &mut state, 1, 80, 80, 1.0, eframe::egui::pos2(0.0, 0.0));
        push_test_image(ui, &mut state, 2, 80, 80, 1.0, eframe::egui::pos2(0.0, 0.0));
    });
    state.add_annotation(CanvasAnnotation::new(
        20,
        CanvasAnnotationKind::Textbox,
        eframe::egui::pos2(-40.0, -40.0),
        eframe::egui::pos2(40.0, 40.0),
        "note".to_owned(),
        default_text_size(),
        default_stroke_width(),
        default_annotation_color(),
        false,
        false,
    ));
    state.set_canvas_stack_order(vec![
        CanvasStackItem::Placement(1),
        CanvasStackItem::Annotation(20),
        CanvasStackItem::Placement(2),
    ]);
    state.select_content(&[1], &[]);

    assert!(state.move_selected_canvas_items_to_front());
    assert_eq!(
        state.ordered_canvas_items_back_to_front(),
        vec![
            CanvasStackItem::Annotation(20),
            CanvasStackItem::Placement(2),
            CanvasStackItem::Placement(1),
        ]
    );

    assert!(state.move_selected_canvas_items_to_back());
    assert_eq!(
        state.ordered_canvas_items_back_to_front(),
        vec![
            CanvasStackItem::Placement(1),
            CanvasStackItem::Annotation(20),
            CanvasStackItem::Placement(2),
        ]
    );
}

#[test]
fn mixed_selected_canvas_items_move_as_ordered_block() {
    let mut state = CanvasState::default();
    eframe::egui::__run_test_ui(|ui| {
        push_test_image(ui, &mut state, 1, 80, 80, 1.0, eframe::egui::pos2(0.0, 0.0));
        push_test_image(ui, &mut state, 2, 80, 80, 1.0, eframe::egui::pos2(0.0, 0.0));
        push_test_image(ui, &mut state, 3, 80, 80, 1.0, eframe::egui::pos2(0.0, 0.0));
    });
    for annotation_id in [20, 21] {
        state.add_annotation(CanvasAnnotation::new(
            annotation_id,
            CanvasAnnotationKind::Textbox,
            eframe::egui::pos2(-40.0, -40.0),
            eframe::egui::pos2(40.0, 40.0),
            format!("note {annotation_id}"),
            default_text_size(),
            default_stroke_width(),
            default_annotation_color(),
            false,
            false,
        ));
    }
    state.set_canvas_stack_order(vec![
        CanvasStackItem::Placement(1),
        CanvasStackItem::Annotation(20),
        CanvasStackItem::Placement(2),
        CanvasStackItem::Annotation(21),
        CanvasStackItem::Placement(3),
    ]);
    state.select_content(&[3, 1], &[20]);

    assert!(state.move_selected_canvas_items_to_front());
    assert_eq!(
        state.ordered_canvas_items_back_to_front(),
        vec![
            CanvasStackItem::Placement(2),
            CanvasStackItem::Annotation(21),
            CanvasStackItem::Placement(1),
            CanvasStackItem::Annotation(20),
            CanvasStackItem::Placement(3),
        ]
    );

    assert!(state.move_selected_canvas_items_to_back());
    assert_eq!(
        state.ordered_canvas_items_back_to_front(),
        vec![
            CanvasStackItem::Placement(1),
            CanvasStackItem::Annotation(20),
            CanvasStackItem::Placement(3),
            CanvasStackItem::Placement(2),
            CanvasStackItem::Annotation(21),
        ]
    );
}

fn arrangement_input(id: i64, width: f32, height: f32) -> PlacementArrangementInput {
    positioned_arrangement_input(id, 0.0, 0.0, width, height)
}

fn positioned_arrangement_input(
    id: i64,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
) -> PlacementArrangementInput {
    PlacementArrangementInput::new(
        id,
        eframe::egui::pos2(x, y),
        eframe::egui::vec2(width, height),
    )
}

fn center_gap_x(
    items: &[PlacementArrangementInput],
    placements: &[(i64, eframe::egui::Pos2)],
    left_id: i64,
    right_id: i64,
) -> f32 {
    let left = placement_rect(items, placements, left_id);
    let right = placement_rect(items, placements, right_id);
    right.left() - left.right()
}

fn center_gap_y(
    items: &[PlacementArrangementInput],
    placements: &[(i64, eframe::egui::Pos2)],
    top_id: i64,
    bottom_id: i64,
) -> f32 {
    let top = placement_rect(items, placements, top_id);
    let bottom = placement_rect(items, placements, bottom_id);
    bottom.top() - top.bottom()
}

fn placement_rect(
    items: &[PlacementArrangementInput],
    placements: &[(i64, eframe::egui::Pos2)],
    placement_id: i64,
) -> eframe::egui::Rect {
    let item = items.iter().find(|item| item.id == placement_id).unwrap();
    let center = placements
        .iter()
        .find(|(candidate_placement_id, _)| *candidate_placement_id == placement_id)
        .map(|(_, center)| *center)
        .unwrap();
    eframe::egui::Rect::from_center_size(center, eframe::egui::vec2(item.width(), item.height()))
}

fn aspect_ratio(width: f32, height: f32) -> f32 {
    width.max(height) / width.min(height).max(1.0)
}

fn hole_prone_items() -> Vec<PlacementArrangementInput> {
    vec![
        arrangement_input(1, 300.0, 520.0),
        arrangement_input(2, 180.0, 260.0),
        arrangement_input(3, 160.0, 260.0),
        arrangement_input(4, 150.0, 220.0),
        arrangement_input(5, 150.0, 220.0),
        arrangement_input(6, 140.0, 180.0),
        arrangement_input(7, 140.0, 180.0),
    ]
}

fn rectangles_respect_gap(left: eframe::egui::Rect, right: eframe::egui::Rect, gap: f32) -> bool {
    left.right() + gap <= right.left()
        || right.right() + gap <= left.left()
        || left.bottom() + gap <= right.top()
        || right.bottom() + gap <= left.top()
}

fn placement_visual_bounds_world(image: &Placement) -> eframe::egui::Rect {
    let display_size = rotated_display_size(
        eframe::egui::vec2(
            image.require_loaded_asset().width as f32 * image.state.geometry.scale.max(0.01),
            image.require_loaded_asset().height as f32 * image.state.geometry.scale.max(0.01),
        ),
        image.state.geometry.rotation,
    );
    eframe::egui::Rect::from_center_size(
        eframe::egui::pos2(image.state.geometry.center.x, image.state.geometry.center.y),
        display_size,
    )
}

fn selected_placement_positions(state: &CanvasState) -> Vec<(i64, eframe::egui::Pos2)> {
    let mut positions = state
        .document
        .placements
        .iter()
        .filter(|image| state.is_selected(image.state.id))
        .map(|image| {
            (
                image.state.id,
                eframe::egui::pos2(image.state.geometry.center.x, image.state.geometry.center.y),
            )
        })
        .collect::<Vec<_>>();
    positions.sort_by_key(|(placement_id, _)| *placement_id);
    positions
}

mod image_content;

mod selection_transform;

mod annotations;
