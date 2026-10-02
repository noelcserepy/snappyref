use super::*;

#[test]
fn embedding_phase_markers_report_summary_durations() {
    let markers = EmbeddingPhaseMarkers {
        first_embedding_queued_ms: Some(100.0),
        semantic_model_load_started_ms: Some(125.0),
        semantic_model_ready_ms: Some(425.0),
        first_embedding_started_ms: Some(450.0),
        first_embedding_done_ms: Some(475.0),
        all_embeddings_done_ms: Some(825.0),
        semantic_model_unload_requested_ms: Some(900.0),
        semantic_model_unloaded_ms: Some(940.0),
        model_load_timing: Some(ModelLoadTimingSummary {
            assets_resolved_ms: 5.0,
            tokenizer_loaded_ms: 0.0,
            text_session_loaded_ms: 0.0,
            vision_session_loaded_ms: 290.0,
            total_ms: 300.0,
        }),
    };

    let summary = markers.summary().unwrap();

    assert_eq!(summary.queue_to_model_load_started_ms, Some(25.0));
    assert_eq!(summary.model_cold_mount_ms, Some(300.0));
    assert_eq!(
        summary.first_embedding_start_after_model_ready_ms,
        Some(25.0)
    );
    assert_eq!(summary.first_embedding_after_model_ready_ms, Some(50.0));
    assert_eq!(summary.first_embedding_run_ms, Some(25.0));
    assert_eq!(summary.total_import_embedding_drain_ms, Some(725.0));
    assert_eq!(summary.model_unload_ms, Some(40.0));
    assert_eq!(
        summary.model_load_timing.unwrap().vision_session_loaded_ms,
        290.0
    );
}

#[test]
fn first_launch_semantic_search_prompt_waits_for_storage_and_missing_assets() {
    let root = TempRoot::new("snappyref-semantic-prompt");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let mut app = test_app(Some(library));

    assert!(!app.should_show_semantic_search_first_launch_prompt());

    app.search
        .set_local_model_store_status_for_test(LocalModelStoreStatus::Missing);
    assert!(app.should_show_semantic_search_first_launch_prompt());

    app.settings.semantic_search_first_launch_prompt_seen = true;
    assert!(!app.should_show_semantic_search_first_launch_prompt());
}

#[test]
fn first_launch_semantic_search_prompt_skips_without_storage() {
    let mut app = test_app(None);
    app.search
        .set_local_model_store_status_for_test(LocalModelStoreStatus::Missing);

    assert!(!app.should_show_semantic_search_first_launch_prompt());
}

#[test]
fn first_launch_semantic_search_prompt_not_now_persists_suppression() {
    let root = TempRoot::new("snappyref-semantic-prompt-not-now");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let mut app = test_app(Some(library.clone()));
    app.search
        .set_local_model_store_status_for_test(LocalModelStoreStatus::Missing);

    app.mark_semantic_search_first_launch_prompt_seen();

    assert!(!app.should_show_semantic_search_first_launch_prompt());
    app.flush_persistence();
    assert!(
        library
            .load_user_session()
            .unwrap()
            .settings
            .semantic_search_first_launch_prompt_seen
    );
}

#[test]
fn first_launch_semantic_search_prompt_enable_persists_seen_and_starts_install() {
    let ctx = egui::Context::default();
    let root = TempRoot::new("snappyref-semantic-prompt-enable");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let starts = std::rc::Rc::new(std::cell::Cell::new(0));
    let observed = starts.clone();
    let mut app = test_app(Some(library.clone()));
    app.search
        .set_local_model_store_status_for_test(LocalModelStoreStatus::Missing);
    app.search.set_semantic_install_for_test(move |_| {
        observed.set(observed.get() + 1);
    });

    app.enable_semantic_search_from_first_launch_prompt(&ctx);

    assert_eq!(starts.get(), 1);
    assert_eq!(
        app.search.local_model_store_status(),
        LocalModelStoreStatus::Downloading { percent: 0 }
    );
    app.flush_persistence();
    assert!(
        library
            .load_user_session()
            .unwrap()
            .settings
            .semantic_search_first_launch_prompt_seen
    );
}

#[test]
fn search_using_image_context_action_survives_activation_click_outside_overlay() {
    let ctx = egui::Context::default();
    let mut app = test_app(None);
    app.canvas
        .add_prepared_placement_image(&ctx, test_loaded_item(42, 1));
    app.canvas.select_placements(&[42]);

    app.handle_context_menu_action(
        crate::app::shortcuts::ContextMenuAction::SearchUsingImage,
        &ctx,
        egui::Rect::EVERYTHING,
    );
    let pivot = app.search.overlay.image_pivot.as_ref().unwrap();
    assert_eq!(pivot.asset_id, 42);
    assert!(pivot.asset_preview_path.is_none());
    assert!(app.search.overlay.suppress_next_outside_click);

    let _ = crate::ui::test_frame::run_ui(
        &ctx,
        egui::RawInput {
            events: vec![
                egui::Event::PointerMoved(egui::pos2(520.0, 320.0)),
                egui::Event::PointerButton {
                    pos: egui::pos2(520.0, 320.0),
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::default(),
                },
            ],
            ..Default::default()
        },
        |ui| {
            app.show_search_overlay(
                ui.ctx(),
                egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(960.0, 640.0)),
            )
        },
    );

    assert!(app.search.overlay.open);
    assert!(!app.search.overlay.suppress_next_outside_click);
}

#[test]
fn settings_semantic_search_install_and_cancel_share_download_state() {
    let mut app = test_app(None);
    app.sidebar_open = true;
    app.sidebar_tab = SidebarTab::Options;
    app.search
        .set_local_model_store_status_for_test(LocalModelStoreStatus::Missing);
    let mut harness = app_harness(app, |ctx, app| {
        render_sidebar_for_test(ctx, app);
    });

    harness.get_by_label("Semantic Search").scroll_to_me();
    harness.run();
    harness.get_by_label("Install").click();
    harness.run();

    assert_eq!(
        harness.state().search.local_model_store_status(),
        LocalModelStoreStatus::Downloading { percent: 0 }
    );
    assert_eq!(harness.state().status, "Installing Semantic Search");

    harness.get_by_label("Semantic Search").scroll_to_me();
    harness.run();
    harness.get_by_label("Cancel").click();
    harness.run();

    assert_eq!(
        harness.state().search.local_model_store_status(),
        LocalModelStoreStatus::Missing
    );
    assert_eq!(harness.state().status, "Semantic Search install canceled");
}

#[test]
fn settings_semantic_search_failed_state_retries_shared_install() {
    let mut app = test_app(None);
    app.sidebar_open = true;
    app.sidebar_tab = SidebarTab::Options;
    app.search
        .set_local_model_store_status_for_test(LocalModelStoreStatus::Failed(
            "network: timeout".to_owned(),
        ));
    let mut harness = app_harness(app, |ctx, app| {
        render_sidebar_for_test(ctx, app);
    });

    harness.get_by_label("Semantic Search").scroll_to_me();
    harness.run();
    harness.get_by_label("Retry").click();
    harness.run();

    assert_eq!(
        harness.state().search.local_model_store_status(),
        LocalModelStoreStatus::Downloading { percent: 0 }
    );
    assert_eq!(harness.state().status, "Installing Semantic Search");
}

#[test]
fn settings_semantic_search_remove_requires_confirmation() {
    let root = TempRoot::new("snappyref-semantic-search-remove-confirm");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let mut app = test_app(Some(library));
    app.sidebar_open = true;
    app.sidebar_tab = SidebarTab::Options;
    app.settings.semantic_search_first_launch_prompt_seen = true;
    app.search
        .set_local_model_store_status_for_test(LocalModelStoreStatus::Installed);
    let mut harness = app_harness(app, |ctx, app| {
        render_sidebar_for_test(ctx, app);
        app.show_semantic_search_remove_confirmation(ctx);
    });

    harness.get_by_label("Semantic Search").scroll_to_me();
    harness.run();
    harness.get_by_label("Remove").click();
    harness.run();

    assert!(harness.state().pending_semantic_search_remove);
    assert_eq!(
        harness.state().search.local_model_store_status(),
        LocalModelStoreStatus::Installed
    );

    harness.get_by_label("Remove Semantic Search").click();
    harness.run();

    assert!(!harness.state().pending_semantic_search_remove);
    assert_eq!(
        harness.state().search.local_model_store_status(),
        LocalModelStoreStatus::Missing
    );
    assert!(
        harness
            .state()
            .settings
            .semantic_search_first_launch_prompt_seen
    );
}

#[test]
fn search_shortcut_opens_missing_palette_on_install_prompt() {
    let ctx = egui::Context::default();
    let mut app = test_app(None);
    app.search
        .set_local_model_store_status_for_test(LocalModelStoreStatus::Missing);

    let _ = crate::ui::test_frame::run_ui(
        &ctx,
        key_press_input(egui::Modifiers::COMMAND, egui::Key::F),
        |ui| {
            app.search.handle_shortcut(
                ui.ctx(),
                app.library.as_ref(),
                &mut app.canvas,
                &mut app.status,
                app.settings.canvas_image_max_side,
            )
        },
    );

    assert!(app.search.overlay.open);
    assert!(app.search.overlay.results.is_empty());
    assert_eq!(
        app.search.local_model_store_status(),
        LocalModelStoreStatus::Missing
    );
}

#[test]
fn long_search_error_does_not_expand_palette_width() {
    let mut app = test_app(None);
    app.search.overlay.open = true;
    app.search.overlay.query = "boat".to_owned();
    app.search.overlay.status = crate::search::palette::SearchOverlayStatus::Error(format!(
        "failed to open image /tmp/snappyref-test/library/originals/{}-image-2025-08-05-160929.jpg: Format error decoding Jpeg: Error parsing image. Illegal start bytes: 8950",
        "1777813588290-19"
    ));
    let mut harness = app_harness(app, |ctx, app| {
        app.show_search_overlay(ctx, test_screen_rect());
    });

    harness.step();

    let overlay_width = harness
        .state()
        .search
        .overlay
        .last_expanded_overlay_rect
        .map(|rect| rect.width())
        .unwrap();
    assert!(overlay_width <= PALETTE_WIDTH + 2.0);
}

#[test]
fn command_enter_library_search_results_show_pending_cursor_arrangement_inputs() {
    let root = TempRoot::new("snappyref-library-search-cmd-enter-provisional-placement");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let first_path = library
        .store_rgba_image("first", &vec![255; 10 * 10 * 4], 10, 10)
        .unwrap();
    let second_path = library
        .store_rgba_image("second", &vec![180; 20 * 10 * 4], 20, 10)
        .unwrap();
    let first = library
        .fixture_placement(
            library.load_user_session().unwrap().active_board_id,
            crate::data::Placement::loaded(
                crate::data::PlacementState::new(
                    library.allocate_document_id(),
                    egui::pos2(0.0, 0.0),
                    1.0,
                    0.0,
                    0,
                    false,
                ),
                library
                    .register_original(&first_path, "image/png", 10, 10, 4, None)
                    .unwrap(),
            ),
        )
        .unwrap();
    let second = library
        .fixture_placement(
            library.load_user_session().unwrap().active_board_id,
            crate::data::Placement::loaded(
                crate::data::PlacementState::new(
                    library.allocate_document_id(),
                    egui::pos2(20.0, 0.0),
                    1.0,
                    0.0,
                    0,
                    false,
                ),
                library
                    .register_original(&second_path, "image/png", 20, 10, 4, None)
                    .unwrap(),
            ),
        )
        .unwrap();
    library.fixture_board_document(library.load_user_session().unwrap().active_board_id, crate::canvas::CanvasDocumentChanges {archived_placement_ids: [first.state.id, second.state.id].to_vec(), stack_order: Some(library.load_board_document(library.load_user_session().unwrap().active_board_id).unwrap().stack_order.into_iter().filter(|item| !matches!(item, crate::data::CanvasStackItem::Placement(id) if [first.state.id, second.state.id].contains(id))).collect()), ..Default::default()})
        .unwrap();
    let empty_board = library.fixture_board("Empty").unwrap();
    library.set_active_board(empty_board.id).unwrap();
    let mut app = test_app(Some(library.clone()));
    app.active_board_id = empty_board.id;
    app.refresh_board_summaries();
    app.search.overlay.open = true;
    app.search.overlay.results = vec![
        SearchResultAssetEntry {
            asset_id: first.require_loaded_asset().id,
            asset_preview_path: None,
            asset_width: 1,
            asset_height: 1,
        },
        SearchResultAssetEntry {
            asset_id: second.require_loaded_asset().id,
            asset_preview_path: None,
            asset_width: 1,
            asset_height: 1,
        },
    ];
    app.search.overlay.result_selection.replace(0);
    let mut harness = app_harness(app, |ctx, app| {
        app.show_search_overlay(ctx, test_screen_rect());
    });

    harness.key_press_modifiers(
        egui::Modifiers {
            command: true,
            ..Default::default()
        },
        egui::Key::Enter,
    );
    harness.step();

    assert_eq!(
        harness
            .state()
            .canvas
            .provisional_placements_in_selection_order()
            .len(),
        2
    );
}

#[test]
fn search_asset_preview_uses_known_asset_dimensions_without_decoding_original_path() {
    let root = TempRoot::new("snappyref-library-search-known-dimensions-preview");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let first_path = library
        .store_rgba_image("first", &vec![255; 10 * 10 * 4], 10, 10)
        .unwrap();
    let second_path = library
        .store_rgba_image("second", &vec![180; 20 * 10 * 4], 20, 10)
        .unwrap();
    let first = library
        .fixture_placement(
            library.load_user_session().unwrap().active_board_id,
            crate::data::Placement::loaded(
                crate::data::PlacementState::new(
                    library.allocate_document_id(),
                    egui::pos2(0.0, 0.0),
                    1.0,
                    0.0,
                    0,
                    false,
                ),
                library
                    .register_original(&first_path, "image/png", 10, 10, 4, None)
                    .unwrap(),
            ),
        )
        .unwrap();
    let second = library
        .fixture_placement(
            library.load_user_session().unwrap().active_board_id,
            crate::data::Placement::loaded(
                crate::data::PlacementState::new(
                    library.allocate_document_id(),
                    egui::pos2(20.0, 0.0),
                    1.0,
                    0.0,
                    0,
                    false,
                ),
                library
                    .register_original(&second_path, "image/png", 20, 10, 4, None)
                    .unwrap(),
            ),
        )
        .unwrap();
    library.fixture_board_document(library.load_user_session().unwrap().active_board_id, crate::canvas::CanvasDocumentChanges {archived_placement_ids: [first.state.id, second.state.id].to_vec(), stack_order: Some(library.load_board_document(library.load_user_session().unwrap().active_board_id).unwrap().stack_order.into_iter().filter(|item| !matches!(item, crate::data::CanvasStackItem::Placement(id) if [first.state.id, second.state.id].contains(id))).collect()), ..Default::default()})
        .unwrap();
    let empty_board = library.fixture_board("Empty").unwrap();
    library.set_active_board(empty_board.id).unwrap();
    let mut app = test_app(Some(library.clone()));
    app.active_board_id = empty_board.id;
    app.refresh_board_summaries();
    app.search.overlay.open = true;
    app.search.overlay.results = vec![
        SearchResultAssetEntry {
            asset_id: first.require_loaded_asset().id,
            asset_preview_path: None,
            asset_width: first.require_loaded_asset().width,
            asset_height: first.require_loaded_asset().height,
        },
        SearchResultAssetEntry {
            asset_id: second.require_loaded_asset().id,
            asset_preview_path: None,
            asset_width: second.require_loaded_asset().width,
            asset_height: second.require_loaded_asset().height,
        },
    ];
    app.search.overlay.result_selection.replace(0);
    app.search.overlay.result_selection.toggle(1);
    let mut harness = app_harness(app, |ctx, app| {
        app.show_search_overlay(ctx, test_screen_rect());
    });

    harness.key_press(egui::Key::Enter);
    harness.step();

    let layout = harness
        .state()
        .canvas
        .provisional_placements_in_selection_order()
        .into_iter()
        .map(|placement| {
            crate::placement::arrangement::PlacementArrangementInput::new(
                placement.state.id,
                placement.state.geometry.center,
                placement.state.geometry.scaled_size(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(layout.len(), 2);
    let import_size_world = |asset: &crate::data::Asset| {
        crate::placement::creation::display_size_for_dimensions(
            asset.width,
            asset.height,
            harness.state().settings.placement_target_area,
        )
    };
    let first_size = import_size_world(first.require_loaded_asset());
    let second_size = import_size_world(second.require_loaded_asset());
    assert!(layout.iter().any(|item| {
        (item.width() - first_size.x).abs() < 0.1 && (item.height() - first_size.y).abs() < 0.1
    }));
    assert!(layout.iter().any(|item| {
        (item.width() - second_size.x).abs() < 0.1 && (item.height() - second_size.y).abs() < 0.1
    }));
}

#[test]
fn dragging_library_search_result_out_of_palette_creates_provisional_placement() {
    let root = TempRoot::new("snappyref-library-search-pointer-drag-provisional-placement");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let source_path = library
        .store_rgba_image("first", &vec![255; 10 * 10 * 4], 10, 10)
        .unwrap();
    let source = library
        .fixture_placement(
            library.load_user_session().unwrap().active_board_id,
            crate::data::Placement::loaded(
                crate::data::PlacementState::new(
                    library.allocate_document_id(),
                    egui::pos2(0.0, 0.0),
                    1.0,
                    0.0,
                    0,
                    false,
                ),
                library
                    .register_original(&source_path, "image/png", 10, 10, 4, None)
                    .unwrap(),
            ),
        )
        .unwrap();
    library.fixture_board_document(library.load_user_session().unwrap().active_board_id, crate::canvas::CanvasDocumentChanges {archived_placement_ids: [source.state.id].to_vec(), stack_order: Some(library.load_board_document(library.load_user_session().unwrap().active_board_id).unwrap().stack_order.into_iter().filter(|item| !matches!(item, crate::data::CanvasStackItem::Placement(id) if [source.state.id].contains(id))).collect()), ..Default::default()}).unwrap();
    let empty_board = library.fixture_board("Empty").unwrap();
    library.set_active_board(empty_board.id).unwrap();
    let mut app = test_app(Some(library.clone()));
    app.active_board_id = empty_board.id;
    app.refresh_board_summaries();
    app.search.overlay.open = true;
    app.search.overlay.results = vec![SearchResultAssetEntry {
        asset_id: source.require_loaded_asset().id,
        asset_preview_path: None,
        asset_width: source.require_loaded_asset().width,
        asset_height: source.require_loaded_asset().height,
    }];
    app.search.overlay.result_selection.replace(0);
    let mut harness = app_harness(app, |ctx, app| {
        app.show_search_overlay(ctx, test_screen_rect());
    });

    harness.hover_at(egui::pos2(344.0, 206.0));
    harness.drag_at(egui::pos2(344.0, 206.0));
    harness.hover_at(egui::pos2(900.0, 600.0));
    harness.step();

    assert_eq!(
        harness
            .state()
            .canvas
            .provisional_placements_in_selection_order()
            .len(),
        1
    );
}

#[test]
fn dragging_multiple_library_search_results_centers_provisional_group_on_pointer() {
    let root = TempRoot::new("snappyref-library-search-pointer-drag-multi-center");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let first_path = library
        .store_rgba_image("first", &vec![255; 120 * 80 * 4], 120, 80)
        .unwrap();
    let second_path = library
        .store_rgba_image("second", &vec![180; 80 * 120 * 4], 80, 120)
        .unwrap();
    let first = library
        .fixture_placement(
            library.load_user_session().unwrap().active_board_id,
            crate::data::Placement::loaded(
                crate::data::PlacementState::new(
                    library.allocate_document_id(),
                    egui::pos2(0.0, 0.0),
                    1.0,
                    0.0,
                    0,
                    false,
                ),
                library
                    .register_original(&first_path, "image/png", 120, 80, 4, None)
                    .unwrap(),
            ),
        )
        .unwrap();
    let second = library
        .fixture_placement(
            library.load_user_session().unwrap().active_board_id,
            crate::data::Placement::loaded(
                crate::data::PlacementState::new(
                    library.allocate_document_id(),
                    egui::pos2(0.0, 0.0),
                    1.0,
                    0.0,
                    0,
                    false,
                ),
                library
                    .register_original(&second_path, "image/png", 80, 120, 4, None)
                    .unwrap(),
            ),
        )
        .unwrap();
    library.fixture_board_document(library.load_user_session().unwrap().active_board_id, crate::canvas::CanvasDocumentChanges {archived_placement_ids: [first.state.id, second.state.id].to_vec(), stack_order: Some(library.load_board_document(library.load_user_session().unwrap().active_board_id).unwrap().stack_order.into_iter().filter(|item| !matches!(item, crate::data::CanvasStackItem::Placement(id) if [first.state.id, second.state.id].contains(id))).collect()), ..Default::default()})
        .unwrap();
    let empty_board = library.fixture_board("Empty").unwrap();
    library.set_active_board(empty_board.id).unwrap();
    let mut app = test_app(Some(library.clone()));
    app.active_board_id = empty_board.id;
    app.refresh_board_summaries();
    app.search.overlay.open = true;
    app.search.overlay.results = vec![
        SearchResultAssetEntry {
            asset_id: first.require_loaded_asset().id,
            asset_preview_path: None,
            asset_width: first.require_loaded_asset().width,
            asset_height: first.require_loaded_asset().height,
        },
        SearchResultAssetEntry {
            asset_id: second.require_loaded_asset().id,
            asset_preview_path: None,
            asset_width: second.require_loaded_asset().width,
            asset_height: second.require_loaded_asset().height,
        },
    ];
    app.search.overlay.result_selection.replace(0);
    app.search.overlay.result_selection.toggle(1);
    let mut harness = app_harness(app, |ctx, app| {
        app.show_search_overlay(ctx, test_screen_rect());
    });

    let pointer_screen = egui::pos2(900.0, 600.0);
    harness.hover_at(egui::pos2(344.0, 206.0));
    harness.drag_at(egui::pos2(344.0, 206.0));
    harness.hover_at(pointer_screen);
    harness.step();

    let bounds = harness.state().canvas.selected_bounds_world().unwrap();
    let pointer_world = harness
        .state()
        .canvas
        .screen_to_world(pointer_screen, test_screen_rect());
    assert_eq!(
        harness
            .state()
            .canvas
            .provisional_placements_in_selection_order()
            .len(),
        2
    );
    assert!(
        (bounds.center().x - pointer_world.x).abs() < 0.1,
        "group center x {:?} should match pointer world x {:?}",
        bounds.center().x,
        pointer_world.x
    );
    assert!(
        (bounds.center().y - pointer_world.y).abs() < 0.1,
        "group center y {:?} should match pointer world y {:?}",
        bounds.center().y,
        pointer_world.y
    );
}

#[test]
fn active_search_provisional_drag_keeps_canvas_edit_input_disabled() {
    let ctx = egui::Context::default();
    let mut app = test_app(None);
    app.search.overlay.open = true;
    app.search.overlay.drag_state = Some(crate::search::palette::SearchResultDragState {
        asset_ids: vec![1, 2],
        outside_overlay: true,
        provisional_batch_active: true,
        auto_fit_applied: false,
    });

    assert_eq!(
        app.canvas_input_policy(&ctx),
        crate::canvas::CanvasInputPolicy::ViewportOnly
    );
}

#[test]
fn empty_external_hover_does_not_cancel_search_provisional_placement() {
    let mut app = test_app(None);
    crate::placement::lifecycle::start_provisional_batch(
        &mut app.canvas,
        vec![
            crate::placement::creation::ProvisionalSourceIntent::ExistingAsset {
                asset_id: 10,
                dimensions: (100, 100),
            },
        ],
        egui::pos2(0.0, 0.0),
        test_screen_rect(),
        crate::placement::creation::PlacementCreationSettings {
            placement_target_area: app.settings.placement_target_area,
            gap: app.settings.placement_gap,
        },
    );
    let mut harness = app_harness(app, |ctx, app| {
        egui::Area::new(egui::Id::new("external-hover-test-surface"))
            .fixed_pos(egui::Pos2::ZERO)
            .show(ctx, |ui| {
                ui.set_min_size(test_screen_rect().size());
                app.handle_hovered_files(ui, test_screen_rect());
            });
    });

    harness.hover_at(egui::pos2(900.0, 600.0));
    harness.step();

    assert_eq!(
        harness
            .state()
            .canvas
            .provisional_placements_in_selection_order()
            .len(),
        1
    );

    harness.step();

    assert_eq!(
        harness
            .state()
            .canvas
            .provisional_placements_in_selection_order()
            .len(),
        1
    );
}

#[test]
fn submitted_search_is_saved_to_recent_searches() {
    let root = TempRoot::new("snappyref-search-recent-session");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let mut app = test_app(Some(library.clone()));
    app.search.overlay.open = true;
    app.search.overlay.query = "brutalist chair".to_owned();
    app.search.overlay.last_query_edit_at = Some(Instant::now() - Duration::from_millis(200));
    let mut harness = app_harness(app, |ctx, app| {
        app.show_search_overlay(ctx, test_screen_rect());
    });

    harness.step();

    harness.state_mut().flush_persistence();

    assert_eq!(
        library.load_user_session().unwrap().recent_searches,
        vec!["brutalist chair".to_owned()]
    );
}

#[test]
fn empty_recent_searches_do_not_submit_boilerplate_query() {
    let root = TempRoot::new("snappyref-search-no-boilerplate-recent");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let mut app = test_app(Some(library.clone()));
    app.search.overlay.open = true;
    let mut harness = app_harness(app, |ctx, app| {
        app.show_search_overlay(ctx, test_screen_rect());
    });

    harness.key_press(egui::Key::Enter);
    harness.step();

    assert!(harness.state().search.overlay.query.is_empty());
    assert!(
        library
            .load_user_session()
            .unwrap()
            .recent_searches
            .is_empty()
    );
}

#[test]
fn recent_search_keyboard_dispatch_restores_query_and_persists_order() {
    let root = TempRoot::new("search-recent-keyboard");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let mut app = test_app(Some(library.clone()));
    app.search
        .set_recent_searches(vec!["stone arches".into(), "blue chairs".into()]);
    app.search.overlay.open = true;
    app.search.overlay.focus_requested = true;
    app.search
        .set_local_model_store_status_for_test(LocalModelStoreStatus::Installed);
    let mut harness = app_harness(app, |ctx, app| {
        app.show_search_overlay(ctx, test_screen_rect());
    });
    harness.key_press(egui::Key::ArrowDown);
    harness.step();
    harness.key_press(egui::Key::Enter);
    harness.step();
    assert_eq!(harness.state().search.overlay.query, "blue chairs");
    harness.state_mut().flush_persistence();
    assert_eq!(
        library.load_user_session().unwrap().recent_searches,
        vec!["blue chairs", "stone arches"]
    );
    assert!(harness.state().canvas.all_placement_ids().is_empty());
}
