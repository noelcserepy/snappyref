use super::*;

#[test]
fn deleting_last_shared_native_texture_triggers_single_cleanup() {
    let ctx = eframe::egui::Context::default();
    let mut state = CanvasState::default();
    let cleanup_count = Arc::new(AtomicUsize::new(0));
    let handle = crate::image_preparation::NativeTextureHandle::for_tests(
        eframe::egui::TextureId::User(101),
        [4, 4],
        {
            let cleanup_count = cleanup_count.clone();
            move |_| {
                cleanup_count.fetch_add(1, Ordering::SeqCst);
            }
        },
    );
    let first = test_record(1, 1, 4, 4);

    state.add_prepared_placement_image(&ctx, native_loaded_item(first, handle.clone()));
    state
        .add_loaded_reference_placement(test_record(2, 1, 4, 4))
        .unwrap();
    drop(handle);

    state.delete_placements(&[1]);
    assert_eq!(cleanup_count.load(Ordering::SeqCst), 0);

    state.delete_placements(&[2]);
    assert_eq!(cleanup_count.load(Ordering::SeqCst), 1);
}

#[test]
fn crop_replacement_keeps_old_native_texture_until_last_shared_reference_drops() {
    let ctx = eframe::egui::Context::default();
    let mut state = CanvasState::default();
    let old_cleanup = Arc::new(AtomicUsize::new(0));
    let new_cleanup = Arc::new(AtomicUsize::new(0));
    let old_texture = crate::image_preparation::NativeTextureHandle::for_tests(
        eframe::egui::TextureId::User(201),
        [4, 4],
        {
            let old_cleanup = old_cleanup.clone();
            move |_| {
                old_cleanup.fetch_add(1, Ordering::SeqCst);
            }
        },
    );
    let new_texture = crate::image_preparation::NativeTextureHandle::for_tests(
        eframe::egui::TextureId::User(202),
        [2, 2],
        {
            let new_cleanup = new_cleanup.clone();
            move |_| {
                new_cleanup.fetch_add(1, Ordering::SeqCst);
            }
        },
    );

    state.add_prepared_placement_image(
        &ctx,
        native_loaded_item(test_record(1, 1, 4, 4), old_texture.clone()),
    );
    state
        .add_loaded_reference_placement(test_record(2, 1, 4, 4))
        .unwrap();
    drop(old_texture);

    state
        .apply_cropped_item(
            &ctx,
            crate::image_preparation::PreparedCropPreview {
                placement_id: 1,
                stored_path: PathBuf::from("crop.png"),
                width: 2,
                height: 2,
                file_size: 0,
                preview_rgba: vec![200; 16],
                preview_width: 2,
                preview_height: 2,
                preview_texture: crate::image_preparation::ImagePrepPreviewTexture::Native(
                    new_texture.clone(),
                ),
                loaded_lod: CanvasImageLod::Working,
                crop_offset_local: eframe::egui::Vec2::ZERO,
            },
            Asset {
                id: 2,
                original_path: PathBuf::from("crop.png"),
                filename: "crop.png".to_owned(),
                mime_type: "image/png".to_owned(),
                width: 2,
                height: 2,
            },
            PlacementState::new(1, eframe::egui::pos2(0.0, 0.0), 1.0, 0.0, 1, false),
        )
        .unwrap();
    drop(new_texture);

    assert_eq!(old_cleanup.load(Ordering::SeqCst), 0);
    assert_eq!(new_cleanup.load(Ordering::SeqCst), 0);

    state.delete_placements(&[2]);
    assert_eq!(old_cleanup.load(Ordering::SeqCst), 1);
    assert_eq!(new_cleanup.load(Ordering::SeqCst), 0);

    state.delete_placements(&[1]);
    assert_eq!(new_cleanup.load(Ordering::SeqCst), 1);
}

#[test]
fn reference_items_preserve_gif_animation_after_source_item_is_removed() {
    let mut state = CanvasState::default();

    eframe::egui::__run_test_ui(|ui| {
        state.add_prepared_placement_image(ui.ctx(), animated_loaded_item(test_record(1, 1, 1, 1)));
    });
    state
        .add_loaded_reference_placement(test_record(2, 1, 1, 1))
        .unwrap();

    assert!(
        state
            .render
            .placement_images
            .get(&2)
            .unwrap()
            .animation
            .is_some()
    );
    assert_eq!(
        state.advance_animated_image(2, 0.15).unwrap().0.as_ref(),
        &[0, 255, 0, 255]
    );

    state.delete_placements(&[1]);

    assert_eq!(
        state.advance_animated_image(2, 0.25).unwrap().0.as_ref(),
        &[0, 0, 255, 255]
    );
    assert!(
        state
            .render
            .placement_images
            .get(&2)
            .is_some_and(|image| image.animation.is_some())
    );
}

#[test]
fn converts_drag_rect_to_crop_pixels_and_live_scale_position() {
    let screen_rect = eframe::egui::Rect::from_min_max(
        eframe::egui::pos2(40.0, 20.0),
        eframe::egui::pos2(240.0, 180.0),
    );
    let local_rect = screen_rect_to_local_crop_rect(screen_rect, screen_rect, 100, 80).unwrap();
    let crop_rect = pixel_crop_rect_from_local(local_rect, 100, 80).unwrap();

    assert_eq!(crop_rect.x, 0);
    assert_eq!(crop_rect.y, 0);
    assert_eq!(crop_rect.width, 100);
    assert_eq!(crop_rect.height, 80);

    let item = PlacementState::new(1, eframe::egui::pos2(200.0, 120.0), 2.0, 0.0, 0, false);
    let asset = Asset {
        id: 1,
        original_path: PathBuf::from("image.png"),
        filename: "image.png".to_owned(),
        mime_type: "image/png".to_owned(),
        width: 100,
        height: 80,
    };
    let kept_local = eframe::egui::Rect::from_min_max(
        eframe::egui::pos2(10.0, 20.0),
        eframe::egui::pos2(40.0, 50.0),
    );
    let crop_offset_local = crop_center_offset_local(&asset, kept_local);
    let mut merged_item = item.clone();
    merge_cropped_item_fields(&mut merged_item, crop_offset_local);

    assert_eq!(merged_item.geometry.center.x, 150.0);
    assert_eq!(merged_item.geometry.center.y, 110.0);
}

#[test]
fn rounds_local_rect_outward_to_cover_selected_pixels() {
    let local_rect = eframe::egui::Rect::from_min_max(
        eframe::egui::pos2(10.2, 11.8),
        eframe::egui::pos2(40.1, 30.2),
    );
    let crop_rect = pixel_crop_rect_from_local(local_rect, 100, 100).unwrap();

    assert_eq!(crop_rect.x, 10);
    assert_eq!(crop_rect.y, 11);
    assert_eq!(crop_rect.width, 31);
    assert_eq!(crop_rect.height, 20);
}

#[test]
fn unloaded_placements_use_visual_bounds_for_layout_hit_test_and_delete() {
    eframe::egui::__run_test_ui(|ui| {
        let mut state = CanvasState::default();
        let canvas_rect = eframe::egui::Rect::from_min_size(
            eframe::egui::Pos2::ZERO,
            eframe::egui::vec2(800.0, 600.0),
        );
        let texture = ui.ctx().load_texture(
            "unloaded-placement-test",
            eframe::egui::ColorImage::filled([1, 1], eframe::egui::Color32::WHITE),
            eframe::egui::TextureOptions::LINEAR,
        );
        state.install_placement_image(
            unloaded_test_record(
                101,
                PlacementContent::Loading {
                    source: ContentResolutionSource::Unknown {
                        description: "test".to_owned(),
                    },
                    status_message: None,
                },
                120.0,
                80.0,
                2.0,
                eframe::egui::pos2(10.0, 20.0),
            ),
            PlacementImage {
                texture: CanvasTexture::Managed(texture),
                loaded_lod: CanvasImageLod::Working,
                animation: None,
            },
        );
        state
            .document
            .stack_order
            .push(CanvasStackItem::Placement(101));
        state.select_placements(&[101]);

        let arrangement_inputs = state.selected_arrangement_inputs();
        assert_eq!(arrangement_inputs.len(), 1);
        assert_eq!(arrangement_inputs[0].id, 101);
        assert_eq!(
            arrangement_inputs[0].center(),
            eframe::egui::pos2(10.0, 20.0)
        );
        assert_eq!(
            arrangement_inputs[0].size(),
            eframe::egui::vec2(240.0, 160.0)
        );
        assert_eq!(
            state.selected_bounds_world().unwrap(),
            eframe::egui::Rect::from_center_size(
                eframe::egui::pos2(10.0, 20.0),
                eframe::egui::vec2(240.0, 160.0),
            )
        );
        assert_eq!(
            hit_test_canvas_item(&state, canvas_rect, canvas_rect.center()),
            Some(crate::canvas::CanvasHit::Placement(101))
        );
        assert_eq!(
            state.delete_placements(&state.selected_placement_ids()),
            vec![101]
        );
        assert!(state.placement_for_id(101).is_none());
    });
}

#[test]
fn merge_only_updates_crop_owned_item_fields() {
    let mut item = PlacementState::new(
        7,
        eframe::egui::pos2(12.0, 18.0),
        1.5,
        std::f32::consts::FRAC_PI_2,
        9,
        false,
    );

    merge_cropped_item_fields(&mut item, eframe::egui::vec2(80.0, 96.0));

    // A quarter turn maps the scaled local offset (120, 144) to (-144, 120).
    assert!(
        item.geometry
            .center
            .distance(eframe::egui::pos2(-132.0, 138.0))
            < 0.001
    );
    assert_eq!(item.geometry.scale, 1.5);
    assert_eq!(item.geometry.rotation, std::f32::consts::FRAC_PI_2);
    assert_eq!(item.geometry.z_index, 9);
    assert!(!item.archived);
}

#[test]
fn pending_crop_guard_blocks_second_crop_until_cleared() {
    let mut state = crate::canvas::CanvasState::default();
    let handle = crate::image_preparation::NativeTextureHandle::for_tests(
        eframe::egui::TextureId::User(42),
        [4, 4],
        |_| {},
    );
    state.install_placement_image(
        test_record(42, 42, 4, 4),
        PlacementImage {
            texture: CanvasTexture::Native(handle),
            loaded_lod: CanvasImageLod::Working,
            animation: None,
        },
    );

    assert!(state.can_start_crop_on_placement(42));
    state.mark_crop_pending(42);
    assert!(!state.can_start_crop_on_placement(42));
    state.clear_crop_pending(42);
    assert!(state.can_start_crop_on_placement(42));
}

#[test]
fn crop_guard_rejects_svg_items() {
    let mut state = crate::canvas::CanvasState::default();
    let mut placement = test_record(42, 42, 4, 4);
    placement.require_loaded_asset_mut().mime_type = "image/svg+xml".to_owned();
    state.install_placement_image(
        placement,
        PlacementImage {
            texture: CanvasTexture::Svg {
                uri: "memory://test.svg".to_owned(),
                bytes: Arc::<[u8]>::from(b"<svg viewBox='0 0 4 4'/>".as_slice()),
                size: [4, 4],
            },
            loaded_lod: CanvasImageLod::Working,
            animation: None,
        },
    );

    assert!(!state.can_start_crop_on_placement(42));
}

#[test]
fn pending_crop_preview_uses_cached_pixels_before_worker_finishes() {
    let ctx = eframe::egui::Context::default();
    let mut state = crate::canvas::CanvasState::default();
    let placement = test_record(42, 42, 8, 8);
    let source_rgba = (0..16)
        .flat_map(|index| [index, 0, 0, 255])
        .collect::<Vec<_>>();
    state.add_prepared_placement_image(
        &ctx,
        crate::image_preparation::PreparedPlacementImage {
            target_board_id: 1,
            queue_id: None,
            placement,
            preview_rgba: source_rgba.clone(),
            preview_width: 4,
            preview_height: 4,
            animated_preview: None,
            preview_texture: crate::image_preparation::ImagePrepPreviewTexture::Cpu,
            loaded_lod: CanvasImageLod::Working,
            existing_load_diagnostics: None,
            origin: crate::image_preparation::ImportOrigin::Existing,
            trace: None,
        },
    );
    state
        .render
        .asset_rgba_cache
        .insert(42, source_rgba.clone().into());

    let request = CanvasCropRequest {
        placement_id: 42,
        source_asset: test_record(42, 42, 8, 8).require_loaded_asset().clone(),
        crop_rect: PixelCropRect {
            x: 2,
            y: 2,
            width: 4,
            height: 4,
        },
        crop_offset_local: eframe::egui::vec2(2.0, 2.0),
    };

    state.apply_pending_crop_preview(&ctx, &request).unwrap();

    let preview = state.preview_for_placement_id(42).unwrap();
    assert_eq!(preview.texture_size, eframe::egui::vec2(2.0, 2.0));
    let placement = state.placement_for_id(42).unwrap();
    assert_eq!(
        (
            placement.require_loaded_asset().width,
            placement.require_loaded_asset().height
        ),
        (4, 4)
    );
    assert_eq!(
        (
            placement.state.geometry.center.x,
            placement.state.geometry.center.y
        ),
        (2.0, 2.0)
    );

    let original = state.snapshot_item_image(42).unwrap();
    assert_eq!(original.0.require_loaded_asset().width, 8);
    assert_eq!(original.1.as_ref(), source_rgba);
}

#[test]
fn pending_crop_restore_snapshot_shares_warmed_pixels() {
    let ctx = eframe::egui::Context::default();
    let mut state = crate::canvas::CanvasState::default();
    let placement = test_record(42, 42, 8, 8);
    let source_rgba = vec![255; 4 * 4 * 4];
    let shared_rgba = crate::canvas::SharedRgba::from(source_rgba.clone());
    state.add_prepared_placement_image(
        &ctx,
        crate::image_preparation::PreparedPlacementImage {
            target_board_id: 1,
            queue_id: None,
            placement,
            preview_rgba: source_rgba,
            preview_width: 4,
            preview_height: 4,
            animated_preview: None,
            preview_texture: crate::image_preparation::ImagePrepPreviewTexture::Cpu,
            loaded_lod: CanvasImageLod::Working,
            existing_load_diagnostics: None,
            origin: crate::image_preparation::ImportOrigin::Existing,
            trace: None,
        },
    );
    state
        .render
        .asset_rgba_cache
        .insert(42, shared_rgba.clone());

    let request = CanvasCropRequest {
        placement_id: 42,
        source_asset: test_record(42, 42, 8, 8).require_loaded_asset().clone(),
        crop_rect: PixelCropRect {
            x: 2,
            y: 2,
            width: 4,
            height: 4,
        },
        crop_offset_local: eframe::egui::vec2(2.0, 2.0),
    };

    state.apply_pending_crop_preview(&ctx, &request).unwrap();

    let pending = state.render.pending_crop_previews.get(&42).unwrap();
    assert!(Arc::ptr_eq(&pending.rgba, &shared_rgba));
    let original = state.snapshot_item_image(42).unwrap();
    assert!(Arc::ptr_eq(&original.1, &shared_rgba));
}

#[test]
fn loaded_raster_does_not_eagerly_cache_cpu_pixels() {
    let ctx = eframe::egui::Context::default();
    let mut state = crate::canvas::CanvasState::default();

    state.add_prepared_placement_image(
        &ctx,
        crate::image_preparation::PreparedPlacementImage {
            target_board_id: 1,
            queue_id: None,
            placement: test_record(42, 42, 8, 8),
            preview_rgba: vec![255; 4 * 4 * 4],
            preview_width: 4,
            preview_height: 4,
            animated_preview: None,
            preview_texture: crate::image_preparation::ImagePrepPreviewTexture::Cpu,
            loaded_lod: CanvasImageLod::Working,
            existing_load_diagnostics: None,
            origin: crate::image_preparation::ImportOrigin::Existing,
            trace: None,
        },
    );

    assert!(state.render.asset_rgba_cache.is_empty());
}

#[test]
fn deselection_prunes_warmed_cpu_pixels() {
    let ctx = eframe::egui::Context::default();
    let mut state = crate::canvas::CanvasState::default();

    state.add_prepared_placement_image(
        &ctx,
        crate::image_preparation::PreparedPlacementImage {
            target_board_id: 1,
            queue_id: None,
            placement: test_record(42, 42, 8, 8),
            preview_rgba: vec![255; 4 * 4 * 4],
            preview_width: 4,
            preview_height: 4,
            animated_preview: None,
            preview_texture: crate::image_preparation::ImagePrepPreviewTexture::Cpu,
            loaded_lod: CanvasImageLod::Working,
            existing_load_diagnostics: None,
            origin: crate::image_preparation::ImportOrigin::Existing,
            trace: None,
        },
    );
    state.select_content(&[42], &[]);
    state
        .render
        .asset_rgba_cache
        .insert(42, vec![255; 4 * 4 * 4].into());

    state.select_content(&[], &[]);

    assert!(state.render.asset_rgba_cache.is_empty());
}

#[test]
fn final_crop_integration_after_pending_preview_applies_crop_offset_once() {
    let ctx = eframe::egui::Context::default();
    let mut state = crate::canvas::CanvasState::default();
    let placement = test_record(42, 42, 8, 8);
    state.add_prepared_placement_image(
        &ctx,
        crate::image_preparation::PreparedPlacementImage {
            target_board_id: 1,
            queue_id: None,
            placement,
            preview_rgba: vec![255; 4 * 4 * 4],
            preview_width: 4,
            preview_height: 4,
            animated_preview: None,
            preview_texture: crate::image_preparation::ImagePrepPreviewTexture::Cpu,
            loaded_lod: CanvasImageLod::Working,
            existing_load_diagnostics: None,
            origin: crate::image_preparation::ImportOrigin::Existing,
            trace: None,
        },
    );
    state
        .render
        .asset_rgba_cache
        .insert(42, vec![255; 4 * 4 * 4].into());
    let request = CanvasCropRequest {
        placement_id: 42,
        source_asset: test_record(42, 42, 8, 8).require_loaded_asset().clone(),
        crop_rect: PixelCropRect {
            x: 2,
            y: 2,
            width: 4,
            height: 4,
        },
        crop_offset_local: eframe::egui::vec2(2.0, 2.0),
    };
    state.apply_pending_crop_preview(&ctx, &request).unwrap();
    let cropped = crate::image_preparation::PreparedCropPreview {
        placement_id: 42,
        stored_path: std::path::PathBuf::from("cropped.png"),
        width: 4,
        height: 4,
        file_size: 0,
        preview_rgba: vec![128; 2 * 2 * 4],
        preview_width: 2,
        preview_height: 2,
        preview_texture: crate::image_preparation::ImagePrepPreviewTexture::Cpu,
        loaded_lod: CanvasImageLod::Overview,
        crop_offset_local: eframe::egui::vec2(2.0, 2.0),
    };
    let updated_item = state.projected_cropped_item(&cropped).unwrap();
    let mut asset = test_record(42, 99, 4, 4).require_loaded_asset().clone();
    asset.original_path = std::path::PathBuf::from("cropped.png");

    state
        .apply_cropped_item(&ctx, cropped, asset, updated_item)
        .unwrap();

    let placement = state.placement_for_id(42).unwrap();
    assert_eq!(placement.asset_id(), Some(99));
    assert_eq!(
        (
            placement.state.geometry.center.x,
            placement.state.geometry.center.y
        ),
        (2.0, 2.0)
    );
}

#[test]
fn context_click_on_unselected_image_selects_it_before_requesting_menu() {
    let mut state = CanvasState::default();
    let rect = eframe::egui::Rect::from_min_size(
        eframe::egui::Pos2::ZERO,
        eframe::egui::vec2(400.0, 300.0),
    );
    eframe::egui::__run_test_ui(|ui| {
        push_test_image(ui, &mut state, 1, 40, 40, 1.0, eframe::egui::pos2(0.0, 0.0));
        push_test_image(
            ui,
            &mut state,
            2,
            40,
            40,
            1.0,
            eframe::egui::pos2(100.0, 0.0),
        );
    });
    state.select_placements(&[1]);

    open_context_menu_at_pointer(
        &mut state,
        rect,
        rect.center() + eframe::egui::vec2(100.0, 0.0),
    );

    assert_eq!(state.selected_placement_ids(), vec![2]);
    assert!(state.take_context_menu_request().is_some());
}

#[test]
fn provisional_placements_use_interleaved_image_painting() {
    let mut state = CanvasState::default();
    eframe::egui::__run_test_ui(|ui| {
        push_test_image(ui, &mut state, 1, 80, 80, 1.0, eframe::egui::pos2(0.0, 0.0));
    });
    state.set_canvas_stack_order(vec![CanvasStackItem::Placement(1)]);
    state.add_provisional_unresolved_placement(Placement {
        state: PlacementState::with_display_size(
            -1,
            eframe::egui::pos2(0.0, 0.0),
            eframe::egui::vec2(80.0, 80.0),
            1.0,
            0.0,
            1,
            false,
        ),
        content: PlacementContent::Loading {
            source: ContentResolutionSource::SearchAsset { asset_id: 7 },
            status_message: Some("Loading".to_owned()),
        },
        lifecycle: PlacementLifecycle::Provisional,
    });

    assert_eq!(
        state.ordered_canvas_items_back_to_front(),
        vec![
            CanvasStackItem::Placement(1),
            CanvasStackItem::Placement(-1)
        ]
    );
    assert!(!optimized_image_pass_eligible_for_state(&state));
}

#[test]
fn rotated_crop_uses_visible_pixels_and_preserves_world_position() {
    eframe::egui::__run_test_ui(|ui| {
        let mut state = CanvasState::default();
        let viewport = eframe::egui::Rect::from_min_size(
            eframe::egui::Pos2::ZERO,
            eframe::egui::vec2(800.0, 600.0),
        );
        push_test_image(ui, &mut state, 501, 100, 80, 1.0, eframe::egui::Pos2::ZERO);
        state.set_placement_center_and_rotation(
            501,
            eframe::egui::Pos2::ZERO,
            std::f32::consts::FRAC_PI_2,
        );
        let request = state
            .build_crop_request(
                viewport,
                &crate::canvas::CropState {
                    placement_id: 501,
                    start_screen: viewport.center() + eframe::egui::vec2(-40.0, -50.0),
                    current_screen: viewport.center() + eframe::egui::vec2(40.0, 0.0),
                },
            )
            .unwrap();
        assert_eq!(
            request.crop_rect,
            PixelCropRect {
                x: 0,
                y: 0,
                width: 50,
                height: 80
            }
        );
        let mut placement = state.placement_for_id(501).unwrap().state.clone();
        merge_cropped_item_fields(&mut placement, request.crop_offset_local);
        assert!(
            placement
                .geometry
                .center
                .distance(eframe::egui::pos2(0.0, -25.0))
                < 0.001
        );
    });
}
