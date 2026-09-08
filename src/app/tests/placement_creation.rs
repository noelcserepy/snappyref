use super::*;

#[test]
fn rapid_board_switches_ignore_stale_import_completions_and_last_switch_wins() {
    let ctx = egui::Context::default();
    let root = TempRoot::new("snappyref-rapid-switch-stale-imports");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let first = library.list_boards().unwrap()[0].clone();
    let second = library.create_board("Second").unwrap();
    let third = library.create_board("Third").unwrap();
    let mut app = test_app(Some(library));
    app.active_board_id = first.id;

    app.switch_to_board(second.id);
    app.install_test_prepared_image(&ctx, test_loaded_item(41, first.id));
    app.switch_to_board(third.id);
    app.install_test_prepared_image(&ctx, test_loaded_item(42, second.id));
    app.switch_to_board(first.id);
    app.install_test_prepared_image(&ctx, test_loaded_item(43, third.id));

    assert_eq!(app.active_board_id, first.id);
    assert!(app.canvas.placement_for_id(41).is_none());
    assert!(app.canvas.placement_for_id(42).is_none());
    assert!(app.canvas.placement_for_id(43).is_none());
}

#[test]
fn enter_places_unplaced_library_search_result_at_import_scale() {
    let root = TempRoot::new("snappyref-library-search-enter-import-scale");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let width = 1000_u32;
    let height = 1000_u32;
    let first_path = library
        .store_rgba_image(
            "library-only-large",
            &vec![255; width as usize * height as usize * 4],
            width,
            height,
        )
        .unwrap();
    let library_item = library
        .persist_imported_image(
            &first_path,
            "image/png",
            width,
            height,
            4,
            egui::pos2(0.0, 0.0),
            1.0,
        )
        .unwrap();
    library
        .archive_placements(&[library_item.state.id])
        .unwrap();
    let empty_board = library.create_board("Empty").unwrap();
    library.set_active_board(empty_board.id).unwrap();
    let mut app = test_app(Some(library.clone()));
    app.active_board_id = empty_board.id;
    app.search.overlay.open = true;
    app.search.overlay.results = vec![SearchResultAssetEntry {
        asset_id: library_item.require_loaded_asset().id,
        asset_preview_path: None,
        asset_width: 1,
        asset_height: 1,
    }];
    app.search.overlay.result_selection.replace(0);
    let mut harness = app_harness(app, |ctx, app| {
        app.show_search_overlay(ctx, test_screen_rect());
    });

    harness.key_press(egui::Key::Enter);
    harness.step();
    harness.hover_at(egui::pos2(900.0, 600.0));
    harness.drag_at(egui::pos2(900.0, 600.0));
    harness.drop_at(egui::pos2(900.0, 600.0));
    harness.step();

    harness.state_mut().flush_persistence();
    let active_items = library.load_placements().unwrap();
    let expected_scale = crate::placement::display_scale_for_placement_target_area(
        width,
        height,
        true,
        crate::data::DEFAULT_PLACEMENT_TARGET_AREA,
    );
    assert_eq!(active_items.len(), 1);
    assert_eq!(active_items[0].state.geometry.scale, expected_scale);
}

#[test]
fn dropped_library_search_result_is_visible_before_worker_decode_finishes() {
    let ctx = egui::Context::default();
    let root = TempRoot::new("snappyref-library-search-visible-before-worker");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let first_path = library
        .store_rgba_image("library-only", &[255, 0, 0, 255], 1, 1)
        .unwrap();
    let library_item = library
        .persist_imported_image(&first_path, "image/png", 1, 1, 4, egui::pos2(0.0, 0.0), 1.0)
        .unwrap();
    library
        .archive_placements(&[library_item.state.id])
        .unwrap();
    let empty_board = library.create_board("Empty").unwrap();
    library.set_active_board(empty_board.id).unwrap();
    let mut app = test_app(Some(library.clone()));
    app.import_worker = Some(crate::image_preparation::ImagePrepWorker::new(
        library.clone(),
        ctx.clone(),
        None,
    ));
    app.active_board_id = empty_board.id;
    app.search.overlay.open = true;
    app.search.overlay.results = vec![SearchResultAssetEntry {
        asset_id: library_item.require_loaded_asset().id,
        asset_preview_path: None,
        asset_width: 1,
        asset_height: 1,
    }];
    app.search.overlay.result_selection.replace(0);
    let mut harness = app_harness(app, |ctx, app| {
        app.show_search_overlay(ctx, test_screen_rect());
    });

    harness.key_press(egui::Key::Enter);
    harness.step();
    harness.hover_at(egui::pos2(900.0, 600.0));
    harness.drag_at(egui::pos2(900.0, 600.0));
    harness.drop_at(egui::pos2(900.0, 600.0));
    harness.step();

    harness.state_mut().flush_persistence();
    let active_items = library.load_placements().unwrap();
    assert_eq!(active_items.len(), 1);
    assert!(
        harness
            .state()
            .canvas
            .placement_for_id(active_items[0].state.id)
            .is_some()
    );
    assert_eq!(harness.state().placement_creation.batches.len(), 0);
}

#[test]
fn completed_multi_file_import_batch_selects_and_compacts_loaded_geometry() {
    let ctx = egui::Context::default();
    let mut app = test_app(None);
    app.placement_creation
        .batches
        .register_multi_item_batch(app.active_board_id, vec![1, 2, 3]);

    for (id, width, height) in [(1_i64, 180_u32, 80_u32), (2, 70, 210), (3, 140, 120)] {
        let asset = Asset {
            id,
            original_path: PathBuf::from(format!("fixture-{id}.png")),
            filename: format!("fixture-{id}.png"),
            mime_type: "image/png".to_owned(),
            width,
            height,
        };
        let placement = Placement::loaded(
            PlacementState::new(id, eframe::egui::pos2(0.0, 0.0), 1.0, 0.0, id as i32, false),
            asset,
        );
        app.install_test_prepared_image(
            &ctx,
            PreparedPlacementImage {
                queue_id: Some(id as u64),
                preview_rgba: vec![255; width as usize * height as usize * 4],
                preview_width: width,
                preview_height: height,
                origin: crate::image_preparation::ImportOrigin::Path,
                ..loaded_record(placement)
            },
        );
    }

    assert_eq!(app.canvas.selected_placement_ids(), vec![1, 2, 3]);
    assert!([1_i64, 2, 3].iter().any(|placement_id| {
        let placement = app.canvas.placement_for_id(*placement_id).unwrap();
        placement.state.geometry.center.x.abs() > 0.001
            || placement.state.geometry.center.y.abs() > 0.001
    }));
    assert!(app.placement_creation.batches.is_empty());
}

#[test]
fn completed_import_batch_compacts_loaded_items_after_out_of_order_failures() {
    let ctx = egui::Context::default();
    let mut app = test_app(None);
    app.placement_creation
        .batches
        .register_multi_item_batch(app.active_board_id, vec![1, 2, 3]);

    let mut third = test_loaded_item(3, app.active_board_id);
    third.placement.require_loaded_asset_mut().width = 140;
    third.placement.require_loaded_asset_mut().height = 80;
    third.placement.state.geometry.size = egui::vec2(140.0, 80.0);
    third.placement.state.geometry.center.x = 4000.0;
    third.placement.state.geometry.center.y = -1000.0;
    app.install_test_prepared_image(&ctx, third);

    app.accept_preparation_result(
        &ctx,
        crate::image_preparation::ImagePrepResult::Failed {
            target_board_id: Some(app.active_board_id),
            queue_id: Some(2),
            placement_id: None,
            context: "test".to_owned(),
            error: "failed".to_owned(),
        },
    );

    let mut first = test_loaded_item(1, app.active_board_id);
    first.placement.require_loaded_asset_mut().width = 90;
    first.placement.require_loaded_asset_mut().height = 170;
    first.placement.state.geometry.size = egui::vec2(90.0, 170.0);
    first.placement.state.geometry.center.x = -2500.0;
    first.placement.state.geometry.center.y = 3600.0;
    app.install_test_prepared_image(&ctx, first);

    assert_eq!(app.canvas.selected_placement_ids(), vec![3, 1]);
    let third_center = app
        .canvas
        .placement_for_id(3)
        .unwrap()
        .state
        .geometry
        .center;
    let first_center = app
        .canvas
        .placement_for_id(1)
        .unwrap()
        .state
        .geometry
        .center;
    assert_ne!(third_center, egui::pos2(4000.0, -1000.0));
    assert_ne!(first_center, egui::pos2(-2500.0, 3600.0));
    assert!(
        third_center.distance(first_center)
            < egui::pos2(4000.0, -1000.0).distance(egui::pos2(-2500.0, 3600.0))
    );
    assert!(app.placement_creation.batches.is_empty());
}

#[test]
fn completed_inactive_board_import_batch_does_not_mutate_active_canvas() {
    let ctx = egui::Context::default();
    let mut app = test_app(None);
    app.placement_creation
        .batches
        .register_multi_item_batch(1, vec![1, 2]);
    app.active_board_id = 2;

    app.install_test_prepared_image(&ctx, test_loaded_item(1, 1));
    app.install_test_prepared_image(&ctx, test_loaded_item(2, 1));

    assert!(app.canvas.selected_placement_ids().is_empty());
    assert!(app.canvas.placement_for_id(1).is_none());
    assert!(app.placement_creation.batches.is_empty());
}

#[test]
fn import_reservation_creates_loading_placements_for_known_and_fallback_dimensions() {
    let root = TempRoot::new("snappyref-import-reservation");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let second = library.create_board("Second").unwrap();
    let mut app = test_app(Some(library));
    app.active_board_id = second.id;

    let known = app
        .reserve_import(
            Some((100, 80)),
            egui::Pos2::ZERO,
            crate::data::ContentResolutionSource::Unknown {
                description: "known test import".to_owned(),
            },
        )
        .unwrap();
    let unknown = app
        .reserve_import(
            None,
            egui::pos2(200.0, 0.0),
            crate::data::ContentResolutionSource::Unknown {
                description: "unknown test import".to_owned(),
            },
        )
        .unwrap();

    assert_eq!(known.target_board_id, second.id);
    assert_eq!(known.queue_id, 1);
    assert!(known.placement_id > 0);
    assert_eq!(known.reserved_world, egui::Pos2::ZERO);
    assert_eq!(unknown.target_board_id, second.id);
    assert_eq!(unknown.queue_id, 2);
    assert!(unknown.placement_id > 0);
    assert_eq!(unknown.reserved_world, egui::pos2(200.0, 0.0));
}

#[test]
fn failed_import_fails_the_accepted_placement_before_it_is_durable() {
    let root = TempRoot::new("snappyref-failed-import-accepted");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let mut app = test_app(Some(library.clone()));
    app.board_editing.persistence = crate::app::DocumentPersistence::queued(library.clone());
    let reservation = app
        .reserve_import(
            Some((100, 80)),
            egui::Pos2::ZERO,
            crate::data::ContentResolutionSource::Unknown {
                description: "doomed import".to_owned(),
            },
        )
        .unwrap();
    let placement_id = reservation.placement_id;

    // The creation unit may still be queued; failure handling must not
    // depend on the row existing.
    app.accept_preparation_result(
        &egui::Context::default(),
        crate::image_preparation::ImagePrepResult::Failed {
            target_board_id: Some(reservation.target_board_id),
            queue_id: Some(reservation.queue_id),
            placement_id: Some(placement_id),
            context: "decode".to_owned(),
            error: "failed".to_owned(),
        },
    );

    let record = app.canvas.placement_for_id(placement_id).unwrap();
    assert!(
        matches!(record.content, crate::data::PlacementContent::Failed { .. }),
        "{:?}",
        record.content
    );
    assert_eq!(app.canvas.selected_placement_ids(), vec![placement_id]);
    assert_eq!(app.status, "decode: failed");

    app.flush_persistence();
    let stored = library
        .load_placements()
        .unwrap()
        .into_iter()
        .find(|placement| placement.state.id == placement_id)
        .unwrap();
    assert!(matches!(
        stored.content,
        crate::data::PlacementContent::Failed { .. }
    ));
}

#[test]
fn completed_import_for_inactive_board_does_not_enter_active_canvas() {
    let ctx = egui::Context::default();
    let root = TempRoot::new("snappyref-inactive-import-completion");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let second = library.create_board("Second").unwrap();
    let mut app = test_app(Some(library));
    app.active_board_id = 1;
    let loaded = test_loaded_item(41, second.id);

    app.install_test_prepared_image(&ctx, loaded);

    assert!(app.canvas.placement_for_id(41).is_none());
    assert_eq!(app.status, "Imported image");
}

#[test]
fn accepted_import_for_inactive_board_keeps_history_on_target_board() {
    let ctx = egui::Context::default();
    let root = TempRoot::new("snappyref-inactive-import-history");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let first = library.list_boards().unwrap().remove(0);
    let second = library.create_board("Second").unwrap();
    let mut app = test_app(Some(library.clone()));
    app.board_editing.persistence = crate::app::DocumentPersistence::queued(library.clone());
    let reservation = app
        .reserve_import(
            Some((1, 1)),
            egui::Pos2::ZERO,
            crate::data::ContentResolutionSource::Unknown {
                description: "inactive Board import".to_owned(),
            },
        )
        .unwrap();
    let placement_id = reservation.placement_id;
    app.switch_to_board(second.id);

    let mut loaded = persisted_loaded_item(&library, placement_id, first.id);
    loaded.queue_id = Some(reservation.queue_id);
    loaded.origin = crate::image_preparation::ImportOrigin::Path;
    app.install_test_prepared_image(&ctx, loaded);

    assert!(app.canvas.placement_for_id(placement_id).is_none());
    app.switch_to_board(first.id);
    app.undo_document_history(&ctx);
    app.flush_persistence();

    assert!(
        library
            .load_placements_for_board(first.id)
            .unwrap()
            .is_empty()
    );
    assert_eq!(app.status, "Undid Import image");

    app.redo_document_history(&ctx);
    app.flush_persistence();

    assert_eq!(
        library.load_placements_for_board(first.id).unwrap().len(),
        1
    );
    assert_eq!(app.status, "Redid Import image");
}

#[test]
fn delayed_image_load_preserves_geometry_edited_after_request() {
    let ctx = egui::Context::default();
    let mut app = test_app(None);
    let mut initial = test_loaded_item(42, app.active_board_id);
    initial.queue_id = None;
    initial.origin = crate::image_preparation::ImportOrigin::Existing;
    app.install_test_prepared_image(&ctx, initial);

    // The worker captured this placement before the user moved and resized it.
    let mut delayed = test_loaded_item(42, app.active_board_id);
    delayed.queue_id = None;
    delayed.origin = crate::image_preparation::ImportOrigin::Existing;
    delayed.loaded_lod = crate::data::CanvasImageLod::Detail;
    app.canvas.select_placements(&[42]);
    assert_eq!(
        app.canvas.move_selected_placement_group_to_center(
            egui::pos2(120.0, 80.0),
            test_viewport_world(),
            0.0,
        ),
        1
    );
    assert_eq!(app.canvas.apply_target_area_to_selected_placements(4.0), 1);
    let accepted_geometry = app.canvas.placement_for_id(42).unwrap().state.geometry;
    assert_ne!(accepted_geometry, delayed.placement.state.geometry);

    app.install_test_prepared_image(&ctx, delayed);

    assert_eq!(
        app.canvas.placement_for_id(42).unwrap().state.geometry,
        accepted_geometry,
        "a delayed image load must not overwrite newer canvas geometry"
    );
}

#[test]
fn completed_import_for_active_board_enters_active_canvas() {
    let ctx = egui::Context::default();
    let mut app = test_app(None);
    app.active_board_id = 1;
    let loaded = test_loaded_item(42, 1);

    app.install_test_prepared_image(&ctx, loaded);

    assert!(app.canvas.placement_for_id(42).is_some());
    assert_eq!(app.status, "Imported image");
}

#[test]
fn normalize_import_url_accepts_http_and_https_only() {
    assert_eq!(
        crate::sources::url::normalize_import_url(" https://example.com/cat.png?size=large "),
        Some("https://example.com/cat.png?size=large".to_owned())
    );
    assert_eq!(
        crate::sources::url::normalize_import_url("http://example.com/dog.gif"),
        Some("http://example.com/dog.gif".to_owned())
    );
    assert_eq!(
        crate::sources::url::normalize_import_url("file:///tmp/cat.png"),
        None
    );
    assert_eq!(crate::sources::url::normalize_import_url("not a url"), None);
}

#[test]
fn import_requests_preserve_import_area_cap_when_auto_scaling_setting_is_off() {
    let mut app = test_app(None);
    app.settings.auto_image_scaling = false;

    let config = app.import_request_config();

    assert!(config.auto_image_scaling);
}

#[test]
fn raw_dropped_file_enqueues_external_import() {
    let ctx = egui::Context::default();
    let root = TempRoot::new("snappyref-raw-file-drop");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let source_path = library
        .store_rgba_image("drop-source", &[255, 0, 0, 255], 1, 1)
        .unwrap();
    let mut app = test_app(Some(library.clone()));
    app.board_editing.persistence = crate::app::DocumentPersistence::queued(library.clone());
    app.import_worker = Some(crate::image_preparation::ImagePrepWorker::new(
        library.clone(),
        ctx.clone(),
        None,
    ));
    let mut harness = app_harness(app, |ctx, app| {
        egui::Area::new(egui::Id::new("drop-test-surface"))
            .fixed_pos(egui::Pos2::ZERO)
            .show(ctx, |ui| {
                ui.set_min_size(test_screen_rect().size());
                app.handle_hovered_files(ui, test_screen_rect());
                app.handle_dropped_files(ui, test_screen_rect());
            });
    });
    let drop_pos = egui::pos2(900.0, 600.0);

    harness.hover_at(drop_pos);
    harness.input_mut().hovered_files = vec![egui::HoveredFile {
        path: Some(source_path.clone()),
        ..Default::default()
    }];
    harness.step();
    assert_eq!(
        harness
            .state()
            .canvas
            .provisional_placements_in_selection_order()
            .len(),
        1
    );

    harness.input_mut().hovered_files.clear();
    harness.input_mut().dropped_files = vec![egui::DroppedFile {
        path: Some(source_path.clone()),
        name: source_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("drop-source.png")
            .to_owned(),
        ..Default::default()
    }];
    harness.step();

    assert!(
        harness
            .state()
            .canvas
            .provisional_placements_in_selection_order()
            .is_empty()
    );
    assert_eq!(harness.state().placement_creation.pending_len(), 1);
    harness.state_mut().flush_persistence();
    let items = library.load_placements().unwrap();
    assert_eq!(items.len(), 1);
    let screen_rect = test_screen_rect();
    let placement = &items[0];
    let expected_world = harness
        .state()
        .canvas
        .screen_to_world(drop_pos, screen_rect);
    assert!(
        egui::pos2(
            placement.state.geometry.center.x,
            placement.state.geometry.center.y
        )
        .distance(expected_world)
            < 1.0
    );

    harness.state_mut().undo_document_history(&ctx);
    assert_eq!(harness.state().status, "Undid Import image");
    assert!(
        harness
            .state()
            .canvas
            .placement_for_id(placement.state.id)
            .is_none()
    );
}

#[test]
fn missing_drop_event_fallback_is_single_use_after_hover_ends() {
    let mut app = test_app(None);
    let hover_pos = egui::pos2(320.0, 240.0);
    let source = crate::image_preparation::ImportSource::Path(std::path::PathBuf::from(
        "C:/Users/Admin/Pictures/drop.png",
    ));
    app.pending_missing_drop_fallback = Some(crate::sources::external_drag::ExternalDragIntake {
        sources: vec![source.clone()],
        screen_pos: Some(hover_pos),
    });

    let fallback = app
        .take_missing_drop_event_fallback()
        .expect("hover-end fallback should be available once");

    assert_eq!(fallback.sources, vec![source]);
    assert_eq!(fallback.screen_pos, Some(hover_pos));
    assert!(app.take_missing_drop_event_fallback().is_none());
}
