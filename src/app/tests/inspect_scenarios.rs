use super::support::INSPECT_DELETE_KEY;
use super::*;

#[test]
fn inspect_search_result_drag_to_canvas_writes_artifact() {
    let mut scenario = InspectScenario::new("search-drag-to-canvas");
    scenario.event(
        "seed",
        serde_json::json!({
            "seed": "search-result-on-empty-board",
        }),
    );

    let root = TempRoot::new("snappyref-inspect-search-drag");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let source_path = library
        .store_rgba_image("first", &vec![255; 120 * 80 * 4], 120, 80)
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
                    .register_original(&source_path, "image/png", 120, 80, 4, None)
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

    let source_screen = egui::pos2(344.0, 206.0);
    let drop_screen = egui::pos2(900.0, 600.0);
    scenario.target(
        "snap.search.result[0]",
        "search-result",
        egui::Rect::from_center_size(source_screen, egui::vec2(96.0, 96.0)),
    );
    scenario.target("snap.canvas", "canvas", test_screen_rect());
    scenario.assertion(
        "search result is available before drag starts",
        harness.state().search.overlay.results.len() == 1,
    );

    scenario.action("hover search result", source_screen);
    harness.hover_at(source_screen);
    scenario.action("press search result", source_screen);
    harness.drag_at(source_screen);
    scenario.action("move pointer to canvas", drop_screen);
    harness.hover_at(drop_screen);
    harness.step();

    let provisional = harness
        .state()
        .canvas
        .provisional_placements_in_selection_order();
    let provisional_count = provisional.len();
    let mid_bounds = harness.state().canvas.selected_bounds_world();
    let pointer_world = harness
        .state()
        .canvas
        .screen_to_world(drop_screen, test_screen_rect());
    scenario.fact(
        "pointer.drag.mid",
        serde_json::json!({
            "screen": inspect_pos(drop_screen),
            "world": inspect_pos(pointer_world),
        }),
    );
    scenario.fact(
        "placement.provisional.mid",
        serde_json::json!({
            "count": provisional_count,
            "bounds_world": mid_bounds.map(inspect_rect),
        }),
    );
    scenario.assertion(
        "mid-drag provisional placement exists",
        provisional_count == 1,
    );
    let center_tracks_pointer = mid_bounds.is_some_and(|bounds| {
        (bounds.center().x - pointer_world.x).abs() < 0.1
            && (bounds.center().y - pointer_world.y).abs() < 0.1
    });
    scenario.assertion(
        "mid-drag group center tracks pointer",
        center_tracks_pointer,
    );

    scenario.action("release on canvas", drop_screen);
    harness.drop_at(drop_screen);
    harness.step();
    harness.state_mut().flush_persistence();

    let active_items = library.load_placements().unwrap();
    let committed = active_items.first().cloned();
    scenario.fact(
        "placement.committed",
        serde_json::json!({
            "count": active_items.len(),
            "placement": committed.as_ref().map(inspect_placement),
        }),
    );
    scenario.assertion("release commits one placement", active_items.len() == 1);
    scenario.assertion(
        "committed placement is selected",
        committed.as_ref().is_some_and(|placement| {
            harness.state().canvas.selected_placement_ids() == vec![placement.state.id]
        }),
    );
    if let Some(placement) = committed.as_ref() {
        scenario.capture_board_state_frame("board-after-release", placement, drop_screen);
    }

    let report = scenario.finish();
    assert!(
        report.passed,
        "inspect scenario failed; artifact={}",
        report.artifact_dir.display()
    );
}

#[test]
fn inspect_semantic_search_install_accepts_prompt() {
    let mut scenario = InspectScenario::new("semantic-search-install");
    scenario.event(
        "seed",
        serde_json::json!({
            "seed": "semantic-search-missing-first-launch-prompt",
        }),
    );

    let root = TempRoot::new("snappyref-inspect-semantic-install");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let starts = std::rc::Rc::new(std::cell::Cell::new(0));
    let observed = starts.clone();
    let mut app = test_app(Some(library.clone()));
    app.settings.semantic_search_first_launch_prompt_seen = false;
    app.search
        .set_local_model_store_status_for_test(LocalModelStoreStatus::Missing);
    app.search.set_semantic_install_for_test(move |_| {
        observed.set(observed.get() + 1);
    });
    let mut harness = app_full_harness(app);

    scenario.assertion(
        "semantic search first-launch prompt is visible",
        harness
            .state()
            .should_show_semantic_search_first_launch_prompt(),
    );
    scenario.capture_screen("screen-prompt", &mut harness);
    let enable_rect = harness.get_by_label("Enable").rect();
    scenario.target("snap.semantic-search.install.enable", "button", enable_rect);
    scenario.action("click semantic search enable", enable_rect.center());
    harness.get_by_label("Enable").click();
    harness.run();

    scenario.fact(
        "semantic-search.install",
        serde_json::json!({
            "starts": starts.get(),
            "status": format!("{:?}", harness.state().search.local_model_store_status()),
            "app_status": harness.state().status,
            "prompt_seen": harness.state().settings.semantic_search_first_launch_prompt_seen,
        }),
    );
    harness.state_mut().flush_persistence();
    scenario.assertion("install action started", starts.get() == 1);
    scenario.assertion(
        "semantic search status is downloading",
        harness.state().search.local_model_store_status()
            == LocalModelStoreStatus::Downloading { percent: 0 },
    );
    scenario.assertion(
        "first-launch prompt is marked seen",
        library
            .load_user_session()
            .unwrap()
            .settings
            .semantic_search_first_launch_prompt_seen,
    );

    scenario.capture_screen("screen-install-started", &mut harness);

    let report = scenario.finish();
    assert!(
        report.passed,
        "inspect scenario failed; artifact={}",
        report.artifact_dir.display()
    );
}

#[test]
fn inspect_semantic_search_sidebar_status_writes_artifact() {
    let mut scenario = InspectScenario::new("semantic-search-sidebar-status");
    scenario.event(
        "seed",
        serde_json::json!({
            "seed": "sidebar-open-settings-tab",
        }),
    );

    let root = TempRoot::new("snappyref-inspect-semantic-sidebar-status");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let actual_status = crate::search::semantic::default_local_model_store_status();
    let mut app = test_app(Some(library));
    app.sidebar_open = true;
    app.sidebar_tab = SidebarTab::Settings;
    app.settings.semantic_search_first_launch_prompt_seen = true;
    app.search
        .set_local_model_store_status_for_test(actual_status.clone());
    let mut harness = app_full_harness(app);

    scenario.capture_screen("screen-settings-top", &mut harness);

    harness.get_by_label("SEMANTIC SEARCH").scroll_to_me();
    harness.run();
    let section_rect = harness.get_by_label("SEMANTIC SEARCH").rect();
    scenario.target(
        "snap.sidebar.semantic-search",
        "settings-section",
        section_rect,
    );

    let frame_stats = scenario.capture_screen("screen", &mut harness);
    scenario.fact(
        "semantic-search.local-model-store-status",
        serde_json::json!({
            "status": format!("{actual_status:?}"),
        }),
    );
    scenario.assertion(
        "semantic search settings section exists",
        section_rect.is_positive(),
    );
    scenario.assertion(
        "screen frame has expected viewport size",
        frame_stats.width == 1200 && frame_stats.height == 800,
    );
    scenario.assertion(
        "screen frame is a full app render",
        frame_stats.transparent_pixels == 0,
    );
    scenario.assertion("screen frame is nonblank", frame_stats.unique_colors > 1);

    let keys_page = harness.get_by_label("Keys").rect();
    scenario.action("open the Keys settings page", keys_page.center());
    inspect_click_at(&harness, keys_page.center(), egui::Modifiers::NONE);
    harness.run();
    harness.get_by_label("CORE ACTIONS");
    let shortcut_frame = scenario.capture_screen("screen-shortcuts", &mut harness);
    scenario.assertion(
        "shortcut groups render in the full app frame",
        shortcut_frame.width == 1200
            && shortcut_frame.height == 800
            && shortcut_frame.transparent_pixels == 0
            && shortcut_frame.unique_colors > 1,
    );

    harness.get_by_label("Undo").scroll_to_me();
    harness.run();
    scenario.capture_screen("screen-shortcuts-end", &mut harness);

    let report = scenario.finish();
    assert!(
        report.passed,
        "inspect scenario failed; artifact={}",
        report.artifact_dir.display()
    );
}

#[test]
fn inspect_sidebar_pages_writes_artifact() {
    let mut scenario = InspectScenario::new("sidebar-pages");
    scenario.event(
        "seed",
        serde_json::json!({
            "seed": "five-boards-long-names-update-available-search-downloading",
            "purpose": "every sidebar page: Boards at rest and hovered, then General with Semantic Search and About, then Keys",
        }),
    );

    let root = TempRoot::new("snappyref-inspect-sidebar-pages");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let source = library
        .store_rgba_image(
            "sidebar-swatch",
            &solid_rgba([224, 168, 72, 255], 40, 40),
            40,
            40,
        )
        .unwrap();
    let asset = library
        .register_original(&source, "image/png", 40, 40, 40 * 40 * 4, None)
        .unwrap();
    let add_placements = |board_id: i64, count: usize| {
        for index in 0..count {
            library
                .fixture_placement(
                    board_id,
                    Placement::loaded(
                        PlacementState::new(
                            library.allocate_document_id(),
                            egui::pos2(index as f32 * 50.0, 0.0),
                            1.0,
                            0.0,
                            0,
                            false,
                        ),
                        asset.clone(),
                    ),
                )
                .unwrap();
        }
    };
    let first = library.list_boards().unwrap()[0].clone();
    add_placements(first.id, 3);
    let active = library
        .fixture_board("Character turnarounds and expression sheets for the spring pitch")
        .unwrap();
    add_placements(active.id, 12);
    let hovered = library
        .fixture_board("Architecture references from the Lisbon trip, tiles and facades")
        .unwrap();
    add_placements(hovered.id, 7);
    library.fixture_board("Lighting").unwrap();
    library.set_active_board(active.id).unwrap();

    let mut app = test_app(Some(library.clone()));
    app.active_board_id = active.id;
    app.refresh_board_summaries();
    app.sidebar_open = true;
    app.sidebar_tab = SidebarTab::Boards;
    app.settings.semantic_search_first_launch_prompt_seen = true;
    app.search
        .set_local_model_store_status_for_test(LocalModelStoreStatus::Downloading { percent: 42 });
    let now_millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let entry = crate::updater::manifest::PlatformEntry {
        url: "https://example.invalid/snappyref.zip".to_owned(),
        sha256: "0".repeat(64),
        size: 1,
    };
    app.update_service.set_checked_state_for_test(
        crate::updater::UpdateState::Available {
            release: crate::updater::manifest::Manifest {
                version: "9.9.0".to_owned(),
                macos: entry.clone(),
                windows: entry,
            },
        },
        now_millis - 2 * 60 * 60 * 1000,
    );
    let mut harness = app_full_harness(app);
    harness.step();
    scenario.capture_screen("screen-000-boards", &mut harness);
    scenario.assertion(
        "every board has a row and New board follows them",
        harness.state().board_summaries().len() == 4
            && harness.query_by_label("New board").is_some(),
    );

    let row = harness
        .get_by_label(&format!("Board {}", hovered.name))
        .rect();
    scenario.target("sidebar.board-row", "button", row);
    harness.hover_at(row.center());
    harness.step();
    harness.step();
    scenario.capture_screen("screen-001-boards-hover", &mut harness);

    let settings_tab = harness.get_by_label("Settings").rect();
    scenario.action("open the Settings tab", settings_tab.center());
    inspect_click_at(&harness, settings_tab.center(), egui::Modifiers::NONE);
    // Park the pointer in the empty body so it hides no label.
    let park = egui::pos2(140.0, 600.0);
    harness.step();
    harness.hover_at(park);
    // The pending session save and update schedule keep repainting.
    harness.run_steps(6);
    scenario.capture_screen("screen-002-general", &mut harness);
    scenario.assertion(
        "General shows pointer, canvas, and window settings",
        harness.query_by_label("Always on top").is_some()
            && harness.query_by_label("Fill new textboxes").is_some(),
    );
    scenario.assertion(
        "General ends with the Semantic Search download and the available update",
        harness.query_by_label("Downloading 42%").is_some()
            && harness.query_by_label("Update").is_some(),
    );
    scenario.assertion(
        "switching tabs keeps the tabs in place",
        harness.get_by_label("Settings").rect() == settings_tab,
    );

    let keys = harness.get_by_label("Keys").rect();
    scenario.action("open the Keys page", keys.center());
    inspect_click_at(&harness, keys.center(), egui::Modifiers::NONE);
    harness.step();
    harness.hover_at(park);
    harness.run_steps(6);
    scenario.capture_screen("screen-003-keys", &mut harness);
    scenario.assertion(
        "Keys lists the shortcut groups",
        harness.query_by_label("CORE ACTIONS").is_some(),
    );

    let report = scenario.finish();
    assert!(
        report.passed,
        "inspect scenario failed; artifact={}",
        report.artifact_dir.display()
    );
}

#[test]
fn inspect_sample_image_import_writes_artifact() {
    let mut scenario = InspectScenario::new("sample-image-import");
    scenario.event(
        "seed",
        serde_json::json!({
            "seed": "three-generated-sample-images",
        }),
    );

    let root = TempRoot::new("snappyref-inspect-sample-image-import");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let app = test_app(Some(library));
    let mut harness = app_full_harness(app);

    let samples = [
        InspectImportSample {
            name: "red-landscape.png",
            width: 240,
            height: 140,
            color: [224, 68, 76, 255],
            center: egui::pos2(-180.0, -40.0),
        },
        InspectImportSample {
            name: "green-square.png",
            width: 180,
            height: 180,
            color: [74, 178, 102, 255],
            center: egui::pos2(0.0, -40.0),
        },
        InspectImportSample {
            name: "blue-portrait.png",
            width: 140,
            height: 240,
            color: [78, 126, 226, 255],
            center: egui::pos2(180.0, -40.0),
        },
    ];
    scenario.drive_import_lifecycle(&mut harness, &samples);

    let report = scenario.finish();
    assert!(
        report.passed,
        "inspect scenario failed; artifact={}",
        report.artifact_dir.display()
    );
}

#[test]
fn inspect_delete_board_modal_flash_writes_artifact() {
    let mut scenario = InspectScenario::new("delete-board-modal-flash");
    scenario.event(
        "scenario.seed",
        serde_json::json!({
            "seed": "boards-sidebar-delete-click",
            "viewport": inspect_rect(test_screen_rect()),
        }),
    );

    let root = TempRoot::new("snappyref-inspect-delete-board-flash");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let second = library.fixture_board("Second").unwrap();
    let mut app = test_app(Some(library.clone()));
    app.sidebar_open = true;
    app.sidebar_tab = SidebarTab::Boards;
    app.settings.semantic_search_first_launch_prompt_seen = true;

    let mut harness = app_full_harness(app);
    harness.step();
    scenario.capture_screen("screen-000-sidebar-open", &mut harness);

    let delete_rect = harness.get_by_label("Delete board Second").rect();
    scenario.target("snap.sidebar.board-delete", "button", delete_rect);

    harness.hover_at(delete_rect.center());
    harness.step();
    scenario.capture_screen("screen-001-delete-hovered", &mut harness);

    scenario.action("click delete board", delete_rect.center());
    inspect_click_at(&harness, delete_rect.center(), egui::Modifiers::NONE);
    harness.step();
    scenario.capture_screen("screen-002-dialog-open-first-frame", &mut harness);
    let first_modal_frame = scenario
        .artifact_dir
        .join("frames/screen-002-dialog-open-first-frame.png");
    let debug_red_pixels =
        inspect_bright_red_pixels_in_rect(&first_modal_frame, sidebar_rect(test_screen_rect()));
    scenario.fact(
        "delete-board-modal.sidebar-debug-red-pixels",
        serde_json::json!({
            "count": debug_red_pixels,
        }),
    );

    harness.step();
    scenario.capture_screen("screen-003-dialog-open-next-frame", &mut harness);

    scenario.fact(
        "delete-board-modal.state",
        serde_json::json!({
            "pending": harness.state().pending_board_delete.is_some(),
            "sidebar_open": harness.state().sidebar_open,
            "target_board": second.name,
        }),
    );
    scenario.assertion(
        "delete board click opens confirmation",
        harness.state().pending_board_delete.is_some(),
    );
    scenario.assertion(
        "sidebar has no bright debug-red flash on first modal frame",
        debug_red_pixels == 0,
    );
    scenario.assertion(
        "sidebar remains open behind modal",
        harness.state().sidebar_open,
    );

    let report = scenario.finish();
    assert!(
        report.passed,
        "inspect scenario failed; artifact={}",
        report.artifact_dir.display()
    );
}

#[test]
fn inspect_board_lifecycle_undo_redo_writes_artifact() {
    let mut scenario = InspectScenario::new("board-lifecycle-undo-redo");
    scenario.event(
        "scenario.seed",
        serde_json::json!({
            "seed": "one-initial-board",
            "purpose": "pointer-create, rename command, confirmed pointer-delete, then keyboard lifecycle replay",
        }),
    );

    let root = TempRoot::new("snappyref-inspect-board-lifecycle");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let first = library.list_boards().unwrap()[0].clone();
    let mut app = test_app(Some(library.clone()));
    app.board_editing.persistence = crate::app::DocumentPersistence::queued(library.clone());
    app.sidebar_open = true;
    app.sidebar_tab = SidebarTab::Boards;
    app.settings.semantic_search_first_launch_prompt_seen = true;
    let mut harness = app_full_harness_with_step_dt(app, 0.1);
    harness.step();
    scenario.capture_screen("screen-000-initial-board", &mut harness);

    let create_rect = harness.get_by_label("New board").rect();
    scenario.target("sidebar.new-board", "button", create_rect);
    scenario.action("pointer click New board", create_rect.center());
    inspect_click_at(&harness, create_rect.center(), egui::Modifiers::NONE);
    harness.step();
    harness.state_mut().flush_persistence();
    let created = library
        .list_boards()
        .unwrap()
        .into_iter()
        .find(|board| board.id != first.id)
        .unwrap();
    scenario.assertion(
        "pointer create activates one exact new Board",
        created.name == "Board 2" && harness.state().active_board_id == created.id,
    );
    scenario.capture_screen("screen-001-created-board", &mut harness);

    let source = library
        .store_rgba_image(
            "lifecycle-blue",
            &solid_rgba([78, 126, 226, 255], 140, 100),
            140,
            100,
        )
        .unwrap();
    let placement = library
        .fixture_placement(
            library.load_user_session().unwrap().active_board_id,
            crate::data::Placement::loaded(
                crate::data::PlacementState::new(
                    library.allocate_document_id(),
                    egui::pos2(80.0, -30.0),
                    1.0,
                    0.0,
                    0,
                    false,
                ),
                library
                    .register_original(&source, "image/png", 140, 100, 140 * 100 * 4, None)
                    .unwrap(),
            ),
        )
        .unwrap();
    harness
        .state_mut()
        .board_editing
        .documents
        .restore_document(&library.load_board_document(created.id).unwrap());
    let expected_view = crate::data::ViewportState {
        camera_pan_x: 90.0,
        camera_pan_y: -45.0,
        camera_zoom: 1.25,
        canvas_background: CanvasBackground::Grid,
    };
    harness
        .state_mut()
        .canvas
        .apply_viewport_state(&expected_view);
    let ctx = harness.ctx.clone();
    harness.state_mut().canvas.add_prepared_placement_image(
        &ctx,
        inspect_prepared_loaded_placement(
            placement.clone(),
            [78, 126, 226, 255],
            140,
            100,
            None,
            crate::image_preparation::ImportOrigin::Existing,
        ),
    );
    for _ in 0..7 {
        harness.step();
    }

    scenario.event(
        "action",
        serde_json::json!({"name": "rename the created Board"}),
    );
    harness
        .state_mut()
        .handle_board_action(Some(crate::ui::sidebar::SidebarBoardAction::Rename(
            created.id,
            "Lifecycle Board".to_owned(),
        )));
    harness.step();
    scenario.assertion(
        "rename action commits the exact requested name",
        harness.state().board_summaries().iter().any(|summary| {
            summary.board.id == created.id && summary.board.name == "Lifecycle Board"
        }),
    );
    scenario.capture_screen("screen-002-renamed-board-with-content", &mut harness);

    let delete_rect = harness.get_by_label("Delete board Lifecycle Board").rect();
    scenario.action("pointer click Board delete", delete_rect.center());
    inspect_click_at(&harness, delete_rect.center(), egui::Modifiers::NONE);
    harness.step();
    scenario.capture_screen("screen-003-delete-confirmation", &mut harness);
    let confirm_rect = harness.get_by_label("Delete board").rect();
    scenario.action("pointer click delete confirmation", confirm_rect.center());
    inspect_click_at(&harness, confirm_rect.center(), egui::Modifiers::NONE);
    harness.step();
    scenario.assertion(
        "confirmed delete activates the valid fallback",
        harness.state().active_board_id == first.id
            && !harness
                .state()
                .board_summaries()
                .iter()
                .any(|summary| summary.board.id == created.id),
    );
    scenario.capture_screen("screen-004-deleted-fallback-active", &mut harness);

    scenario.event(
        "action",
        serde_json::json!({"name": "press Command-Z to restore deleted Board"}),
    );
    harness.key_press_modifiers(inspect_command_modifier(), egui::Key::Z);
    harness.step();
    let restored = harness.state().canvas.placement_for_id(placement.state.id);
    scenario.fact(
        "board-lifecycle.after-delete-undo",
        serde_json::json!({
            "expected_board_id": created.id,
            "active_board_id": harness.state().active_board_id,
            "name": harness.state().board_summaries().iter()
                .find(|summary| summary.board.id == created.id)
                .map(|summary| summary.board.name.clone()),
            "placement_id": restored.as_ref().map(|placement| placement.state.id),
            "viewport": {
                "pan_x": harness.state().canvas.viewport_state().camera_pan_x,
                "pan_y": harness.state().canvas.viewport_state().camera_pan_y,
                "zoom": harness.state().canvas.viewport_state().camera_zoom,
                "background": harness.state().canvas.viewport_state().canvas_background.as_str(),
            },
        }),
    );
    scenario.assertion(
        "Undo restores exact Board identity, name, content, and view",
        harness.state().active_board_id == created.id
            && harness.state().board_summaries().iter().any(|summary| {
                summary.board.id == created.id && summary.board.name == "Lifecycle Board"
            })
            && restored.is_some_and(|placement| placement.is_loaded())
            && harness.state().canvas.viewport_state() == expected_view,
    );
    scenario.capture_screen("screen-005-delete-undone-content-restored", &mut harness);

    scenario.event(
        "action",
        serde_json::json!({"name": "press Command-Shift-Z to delete Board again"}),
    );
    harness.key_press_modifiers(
        egui::Modifiers {
            command: true,
            shift: true,
            ..Default::default()
        },
        egui::Key::Z,
    );
    harness.step();
    scenario.assertion(
        "Redo deletes the same Board and restores fallback",
        harness.state().active_board_id == first.id
            && !harness
                .state()
                .board_summaries()
                .iter()
                .any(|summary| summary.board.id == created.id),
    );

    harness.key_press_modifiers(inspect_command_modifier(), egui::Key::Z);
    harness.step();
    harness.key_press_modifiers(inspect_command_modifier(), egui::Key::Z);
    harness.step();
    scenario.assertion(
        "second Undo restores the exact pre-rename name",
        harness.state().active_board_id == created.id
            && harness
                .state()
                .board_summaries()
                .iter()
                .any(|summary| summary.board.id == created.id && summary.board.name == "Board 2"),
    );
    harness.key_press_modifiers(inspect_command_modifier(), egui::Key::Z);
    harness.step();
    scenario.assertion(
        "third Undo reverses creation to the exact prior active Board",
        harness.state().active_board_id == first.id
            && !harness
                .state()
                .board_summaries()
                .iter()
                .any(|summary| summary.board.id == created.id),
    );

    let redo = egui::Modifiers {
        command: true,
        shift: true,
        ..Default::default()
    };
    harness.key_press_modifiers(redo, egui::Key::Z);
    harness.step();
    scenario.assertion(
        "creation Redo restores the same Board ID",
        harness.state().active_board_id == created.id
            && harness
                .state()
                .board_summaries()
                .iter()
                .any(|summary| summary.board.id == created.id && summary.board.name == "Board 2"),
    );
    harness.key_press_modifiers(redo, egui::Key::Z);
    harness.step();
    scenario.assertion(
        "rename Redo restores the exact final name",
        harness.state().board_summaries().iter().any(|summary| {
            summary.board.id == created.id && summary.board.name == "Lifecycle Board"
        }),
    );
    harness.key_press_modifiers(redo, egui::Key::Z);
    harness.step();
    scenario.assertion(
        "delete Redo ends on the same fallback Board",
        harness.state().active_board_id == first.id
            && !harness
                .state()
                .board_summaries()
                .iter()
                .any(|summary| summary.board.id == created.id),
    );
    scenario.capture_screen("screen-006-full-lifecycle-redone", &mut harness);

    harness.state_mut().flush_persistence();
    scenario.assertion(
        "final lifecycle replay is durable",
        library.load_user_session().unwrap().active_board_id == first.id
            && library
                .list_boards()
                .unwrap()
                .iter()
                .all(|board| board.id != created.id),
    );

    let report = scenario.finish();
    assert!(
        report.passed,
        "inspect scenario failed; artifact={}",
        report.artifact_dir.display()
    );
}

#[test]
fn inspect_compact_windows_column_images_writes_artifact() {
    let mut scenario = InspectScenario::new("compact-windows-column-images");
    scenario.event(
        "seed",
        serde_json::json!({
            "seed": "copied-windows-library-column-asset-sizes",
            "source_assets": [
                "1779098228313-7-dmytro-koplyk--xbiOACMg8A-unsplash.jpg",
                "1779098229588-9-steve-gribble-5SzX75APpo0-unsplash.jpg",
                "1779013778968-6-sung-jin-cho-6iNPrmVctzQ-unsplash.jpg"
            ],
        }),
    );

    let mut app = test_app(None);
    app.settings.placement_gap = 0.0;
    let mut harness = app_full_harness(app);
    let ctx = harness.ctx.clone();
    let mut id = 1_i64;

    for index in 0..21 {
        add_inspect_compact_image(
            &mut harness,
            &ctx,
            id,
            346,
            260,
            if index % 2 == 0 {
                [74, 84, 76, 255]
            } else {
                [112, 122, 110, 255]
            },
            egui::pos2(-480.0, -320.0 + index as f32 * 260.0),
        );
        id += 1;
    }
    add_inspect_compact_image(
        &mut harness,
        &ctx,
        id,
        369,
        246,
        [150, 156, 140, 255],
        egui::pos2(-480.0, -320.0 + 21.0 * 260.0),
    );
    id += 1;
    for index in 0..21 {
        add_inspect_compact_image(
            &mut harness,
            &ctx,
            id,
            369,
            244,
            if index % 2 == 0 {
                [82, 92, 74, 255]
            } else {
                [122, 132, 118, 255]
            },
            egui::pos2(-110.0, -320.0 + index as f32 * 244.0),
        );
        id += 1;
    }

    harness
        .state_mut()
        .canvas
        .select_placements(&(1_i64..id).collect::<Vec<_>>());
    harness.step();

    scenario.capture_screen("screen-000-before-compact", &mut harness);
    scenario.fact(
        "compact.windows-column.before",
        compact_selection_fact(harness.state()),
    );
    harness.key_press(egui::Key::Q);
    harness.step();
    harness
        .state_mut()
        .canvas
        .focus_selection_in_view(test_screen_rect(), test_screen_rect());
    harness.step();
    let after_stats = scenario.capture_screen("screen-001-after-q-compact", &mut harness);
    let after_fact = compact_selection_fact(harness.state());
    scenario.fact("compact.windows-column.after", after_fact.clone());
    scenario.assertion(
        "compact copied Windows column frame is opaque",
        after_stats.transparent_pixels == 0,
    );
    scenario.assertion(
        "q compact keeps copied Windows column within sixteen by nine",
        after_fact["bounds_aspect"]
            .as_f64()
            .is_some_and(|aspect| aspect <= 16.0 / 9.0),
    );

    let report = scenario.finish();
    assert!(
        report.passed,
        "inspect scenario failed; artifact={}",
        report.artifact_dir.display()
    );
}

#[test]
fn inspect_compact_mixed_column_images_writes_artifact() {
    let mut scenario = InspectScenario::new("compact-mixed-column-images");
    scenario.event(
        "seed",
        serde_json::json!({
            "seed": "many-tiny-landscape-images-plus-mixed-sizes",
        }),
    );

    let mut app = test_app(None);
    app.settings.placement_gap = 12.0;
    let mut harness = app_full_harness(app);
    let ctx = harness.ctx.clone();
    let mut id = 1_i64;

    for index in 0..24 {
        let width = 20_u32 + (index % 3) as u32;
        let height = 24_u32 + (index % 2) as u32;
        let color = if index % 2 == 0 {
            [82, 92, 74, 255]
        } else {
            [120, 128, 116, 255]
        };
        add_inspect_compact_image(
            &mut harness,
            &ctx,
            id,
            width,
            height,
            color,
            egui::pos2(-500.0 + index as f32 * 14.0, -260.0 + index as f32 * 10.0),
        );
        id += 1;
    }

    for (width, height, color, center) in [
        (160, 100, [206, 184, 150, 255], egui::pos2(120.0, 0.0)),
        (120, 150, [92, 132, 172, 255], egui::pos2(270.0, -70.0)),
        (90, 70, [178, 170, 158, 255], egui::pos2(400.0, 40.0)),
        (70, 110, [198, 94, 70, 255], egui::pos2(540.0, -100.0)),
        (54, 64, [220, 216, 198, 255], egui::pos2(650.0, 20.0)),
        (92, 42, [78, 126, 226, 255], egui::pos2(760.0, -60.0)),
    ] {
        add_inspect_compact_image(&mut harness, &ctx, id, width, height, color, center);
        id += 1;
    }

    harness
        .state_mut()
        .canvas
        .select_placements(&(1_i64..id).collect::<Vec<_>>());
    harness.step();

    scenario.capture_screen("screen-000-before-compact", &mut harness);
    scenario.fact(
        "compact.mixed-column.before",
        compact_selection_fact(harness.state()),
    );
    harness.key_press(egui::Key::Q);
    harness.step();
    let after_stats = scenario.capture_screen("screen-001-after-q-compact", &mut harness);
    let after_fact = compact_selection_fact(harness.state());
    scenario.fact("compact.mixed-column.after", after_fact.clone());
    scenario.assertion(
        "compact mixed-column frame is opaque",
        after_stats.transparent_pixels == 0,
    );
    scenario.assertion(
        "q compact avoids a dominant tall thumbnail column",
        after_fact["max_column_count"]
            .as_u64()
            .is_some_and(|count| count <= 12),
    );
    scenario.assertion(
        "q compact keeps mixed selection in a rectangle-like bounds",
        after_fact["bounds_aspect"]
            .as_f64()
            .is_some_and(|aspect| aspect <= 3.0),
    );

    let report = scenario.finish();
    assert!(
        report.passed,
        "inspect scenario failed; artifact={}",
        report.artifact_dir.display()
    );
}

#[test]
fn inspect_compact_equal_size_images_writes_artifact() {
    let mut scenario = InspectScenario::new("compact-equal-size-images");
    scenario.event(
        "seed",
        serde_json::json!({
            "seed": "six-equal-size-selected-images",
        }),
    );

    let mut app = test_app(None);
    app.settings.placement_gap = 12.0;
    let mut harness = app_full_harness(app);
    let ctx = harness.ctx.clone();
    let colors = [
        [228, 171, 72, 255],
        [155, 108, 218, 255],
        [76, 190, 184, 255],
        [206, 89, 94, 255],
        [97, 145, 228, 255],
        [133, 190, 92, 255],
    ];

    for id in 1_i64..=6 {
        let asset = Asset {
            id,
            original_path: PathBuf::from(format!("compact-equal-{id}.png")),
            filename: format!("compact-equal-{id}.png"),
            mime_type: "image/png".to_owned(),
            width: 150,
            height: 40,
        };
        let placement = Placement::loaded(
            PlacementState::with_display_size(
                id,
                egui::pos2(-360.0 + id as f32 * 150.0, -120.0 + id as f32 * 20.0),
                egui::vec2(150.0, 40.0),
                1.0,
                0.0,
                id as i32,
                false,
            ),
            asset,
        );
        harness.state_mut().canvas.add_prepared_placement_image(
            &ctx,
            PreparedPlacementImage {
                preview_rgba: solid_rgba(colors[(id - 1) as usize], 150, 40),
                preview_width: 150,
                preview_height: 40,
                ..loaded_record(placement)
            },
        );
    }
    harness
        .state_mut()
        .canvas
        .select_placements(&(1_i64..=6).collect::<Vec<_>>());
    harness.step();

    scenario.capture_screen("screen-000-before-compact", &mut harness);
    scenario.fact(
        "compact.equal.before",
        compact_selection_fact(harness.state()),
    );
    harness.key_press(egui::Key::Q);
    harness.step();
    let after_stats = scenario.capture_screen("screen-001-after-q-compact", &mut harness);
    let after_fact = compact_selection_fact(harness.state());
    scenario.fact("compact.equal.after", after_fact.clone());
    scenario.assertion(
        "compact equal-size frame is opaque",
        after_stats.transparent_pixels == 0,
    );
    scenario.assertion(
        "q compact arranges six equal-size images wider than one column",
        after_fact["column_count"]
            .as_u64()
            .is_some_and(|count| count > 1),
    );

    let report = scenario.finish();
    assert!(
        report.passed,
        "inspect scenario failed; artifact={}",
        report.artifact_dir.display()
    );
}

#[test]
fn inspect_search_composer_writes_artifact() {
    use crate::search::palette::{ImageSearchPivot, SearchAssetPreviewEntry, SearchOverlayStatus};

    let mut scenario = InspectScenario::new("search-composer");
    let root = TempRoot::new("snappyref-inspect-search-composer");
    let colors: [[u8; 4]; 10] = [
        [228, 171, 72, 255],
        [155, 108, 218, 255],
        [72, 140, 196, 255],
        [210, 96, 84, 255],
        [96, 170, 120, 255],
        [236, 206, 160, 255],
        [60, 64, 92, 255],
        [196, 120, 150, 255],
        [140, 140, 136, 255],
        [44, 110, 128, 255],
    ];
    let entries = colors
        .iter()
        .enumerate()
        .map(|(index, &color)| {
            let (width, height) = if index % 2 == 0 {
                (180, 120)
            } else {
                (120, 160)
            };
            let path = root.join(format!("result-{index}.png"));
            image::save_buffer(
                &path,
                &solid_rgba(color, width, height),
                width,
                height,
                image::ColorType::Rgba8,
            )
            .unwrap();
            SearchResultAssetEntry {
                asset_id: 9000 + index as i64,
                asset_preview_path: Some(path),
                asset_width: width,
                asset_height: height,
            }
        })
        .collect::<Vec<_>>();
    scenario.event(
        "seed",
        serde_json::json!({"seed": "ten-solid-results-four-recents", "count": entries.len()}),
    );

    let library = Library::initialize(root.join("library")).unwrap();
    let mut app = test_app(Some(library));
    app.settings.semantic_search_first_launch_prompt_seen = true;
    app.search.set_recent_searches(
        [
            "warm interior light",
            "brutalist stairwell",
            "fog over hills",
            "red chair",
        ]
        .map(String::from)
        .to_vec(),
    );
    app.search.overlay.open = true;
    app.search.overlay.status = SearchOverlayStatus::Ready;
    app.search.overlay.active_recent_search_index = 1;
    let mut harness = app_full_harness(app);

    let settle = |harness: &mut Harness<'static, SnappyRefApp>| {
        for _ in 0..4 {
            harness.step();
        }
    };
    let wait_for_previews = |harness: &mut Harness<'static, SnappyRefApp>| -> bool {
        for _ in 0..200 {
            harness.step();
            let overlay = &harness.state().search.overlay;
            let pending = overlay
                .results
                .iter()
                .map(|entry| entry.asset_id)
                .chain(overlay.image_pivot.as_ref().map(|pivot| pivot.asset_id))
                .any(|id| {
                    !matches!(
                        overlay.asset_preview_cache.get(&id),
                        Some(SearchAssetPreviewEntry::Ready(_))
                    )
                });
            if !pending {
                harness.step();
                return true;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        false
    };
    let mut all_opaque = true;

    settle(&mut harness);
    all_opaque &= scenario
        .capture_screen("screen-000-recents", &mut harness)
        .transparent_pixels
        == 0;

    let search = &mut harness.state_mut().search;
    search.overlay.query = "warm light over hills".into();
    search.overlay.results = entries.clone();
    search.overlay.has_more_results = true;
    search.overlay.result_selection.replace(1);
    search.overlay.result_selection.toggle(2);
    let previews_ready = wait_for_previews(&mut harness);
    settle(&mut harness);
    all_opaque &= scenario
        .capture_screen("screen-001-results-selected", &mut harness)
        .transparent_pixels
        == 0;

    let search = &mut harness.state_mut().search;
    search.overlay.query = "purple giraffe on a bicycle".into();
    search.overlay.results.clear();
    search.overlay.result_selection.clear();
    search.overlay.has_more_results = false;
    settle(&mut harness);
    all_opaque &= scenario
        .capture_screen("screen-002-no-results", &mut harness)
        .transparent_pixels
        == 0;

    let search = &mut harness.state_mut().search;
    search.overlay.query.clear();
    search.overlay.image_pivot = Some(ImageSearchPivot {
        asset_id: entries[0].asset_id,
        asset_preview_path: entries[0].asset_preview_path.clone(),
    });
    search.overlay.results = entries[1..].to_vec();
    search.overlay.result_selection.replace(0);
    let previews_ready = previews_ready && wait_for_previews(&mut harness);
    settle(&mut harness);
    all_opaque &= scenario
        .capture_screen("screen-003-similar", &mut harness)
        .transparent_pixels
        == 0;

    let search = &mut harness.state_mut().search;
    search.overlay.image_pivot = None;
    search.overlay.results.clear();
    search.overlay.result_selection.clear();
    search.set_local_model_store_status_for_test(LocalModelStoreStatus::Missing);
    settle(&mut harness);
    all_opaque &= scenario
        .capture_screen("screen-004-install", &mut harness)
        .transparent_pixels
        == 0;
    harness.key_press(egui::Key::Enter);
    settle(&mut harness);
    scenario.assertion(
        "Enter does not start the install",
        harness.state().search.local_model_store_status() == LocalModelStoreStatus::Missing,
    );

    harness
        .state_mut()
        .search
        .set_local_model_store_status_for_test(LocalModelStoreStatus::Downloading { percent: 42 });
    settle(&mut harness);
    all_opaque &= scenario
        .capture_screen("screen-005-downloading", &mut harness)
        .transparent_pixels
        == 0;

    harness
        .state_mut()
        .search
        .set_local_model_store_status_for_test(LocalModelStoreStatus::Failed(
            "network: timeout".to_owned(),
        ));
    settle(&mut harness);
    all_opaque &= scenario
        .capture_screen("screen-006-install-error", &mut harness)
        .transparent_pixels
        == 0;

    harness
        .state_mut()
        .search
        .set_local_model_store_status_for_test(LocalModelStoreStatus::Missing);
    settle(&mut harness);
    let not_now = harness.get_by_label("Not now").rect();
    scenario.action("click Not now on the install card", not_now.center());
    inspect_click_at(&harness, not_now.center(), egui::Modifiers::NONE);
    settle(&mut harness);
    scenario.assertion(
        "Not now closes search and leaves Semantic Search uninstalled",
        !harness.state().search.overlay.open
            && harness.state().search.local_model_store_status() == LocalModelStoreStatus::Missing,
    );

    scenario.assertion("every composer frame is opaque", all_opaque);
    scenario.assertion("result previews decoded before capture", previews_ready);
    let report = scenario.finish();
    assert!(
        report.passed,
        "inspect scenario failed; artifact={}",
        report.artifact_dir.display()
    );
}

#[test]
fn inspect_blue_image_search_drag_writes_artifact() {
    let mut scenario = InspectScenario::new("blue-image-search-drag");
    scenario.event(
        "seed",
        serde_json::json!({
            "seed": "blue-image-plus-three-search-results",
        }),
    );

    let root = TempRoot::new("snappyref-inspect-blue-image-search-drag");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let blue_path = library
        .store_rgba_image(
            "blue-selected",
            &solid_rgba([78, 126, 226, 255], 140, 240),
            140,
            240,
        )
        .unwrap();
    let blue = library
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
                    .register_original(&blue_path, "image/png", 140, 240, 140 * 240 * 4, None)
                    .unwrap(),
            ),
        )
        .unwrap();
    let result_specs = [
        ("amber-result", [228, 171, 72, 255], 220_u32, 150_u32),
        ("violet-result", [155, 108, 218, 255], 180_u32, 180_u32),
        ("teal-result", [76, 190, 184, 255], 150_u32, 220_u32),
    ];
    let mut result_placements = Vec::new();
    for (index, (name, color, width, height)) in result_specs.into_iter().enumerate() {
        let path = library
            .store_rgba_image(name, &solid_rgba(color, width, height), width, height)
            .unwrap();
        let placement = library
            .fixture_placement(
                library.load_user_session().unwrap().active_board_id,
                crate::data::Placement::loaded(
                    crate::data::PlacementState::new(
                        library.allocate_document_id(),
                        egui::pos2(index as f32 * 32.0, 0.0),
                        1.0,
                        0.0,
                        0,
                        false,
                    ),
                    library
                        .register_original(
                            &path,
                            "image/png",
                            width,
                            height,
                            width as u64 * height as u64 * 4,
                            None,
                        )
                        .unwrap(),
                ),
            )
            .unwrap();
        library.fixture_board_document(library.load_user_session().unwrap().active_board_id, crate::canvas::CanvasDocumentChanges {archived_placement_ids: [placement.state.id].to_vec(), stack_order: Some(library.load_board_document(library.load_user_session().unwrap().active_board_id).unwrap().stack_order.into_iter().filter(|item| !matches!(item, crate::data::CanvasStackItem::Placement(id) if [placement.state.id].contains(id))).collect()), ..Default::default()}).unwrap();
        result_placements.push(placement);
    }

    let mut app = test_app(Some(library.clone()));
    app.search
        .set_local_model_store_status_for_test(LocalModelStoreStatus::Installed);
    let mut harness = app_full_harness(app);
    let blue_prepared = inspect_prepared_loaded_placement(
        blue.clone(),
        [78, 126, 226, 255],
        140,
        240,
        None,
        crate::image_preparation::ImportOrigin::Existing,
    );
    let ctx = harness.ctx.clone();
    harness
        .state_mut()
        .canvas
        .add_prepared_placement_image(&ctx, blue_prepared);
    harness.step();
    scenario.capture_screen("screen-000-app-open-blue-image", &mut harness);

    let blue_start_screen = inspect_world_to_screen(egui::pos2(0.0, 0.0), test_screen_rect());
    scenario.action("click blue image", blue_start_screen);
    inspect_click_at(&harness, blue_start_screen, egui::Modifiers::NONE);
    harness.step();
    scenario.capture_screen("screen-001-blue-selected", &mut harness);

    let blue_drop_screen = blue_start_screen + egui::vec2(110.0, 0.0);
    scenario.action("drag blue image right", blue_drop_screen);
    harness.hover_at(blue_start_screen);
    harness.drag_at(blue_start_screen);
    harness.hover_at(blue_drop_screen);
    harness.step();
    scenario.capture_screen("screen-002-blue-moving-right", &mut harness);
    harness.drop_at(blue_drop_screen);
    harness.step();
    scenario.capture_screen("screen-003-blue-moved-right", &mut harness);

    scenario.action(
        "start image search from selected blue image",
        blue_drop_screen,
    );
    let blue_asset = blue.require_loaded_asset();
    harness.state_mut().search.overlay.open = true;
    harness.state_mut().search.overlay.focus_requested = true;
    harness.state_mut().search.overlay.image_pivot =
        Some(crate::search::palette::ImageSearchPivot {
            asset_id: blue_asset.id,
            asset_preview_path: Some(blue_asset.original_path.clone()),
        });
    harness.state_mut().search.overlay.status =
        crate::search::palette::SearchOverlayStatus::Loading("Searching…".to_owned());
    harness.step();
    scenario.capture_screen("screen-004-image-search-started", &mut harness);

    let results = result_placements
        .iter()
        .map(inspect_search_result_entry)
        .collect::<Vec<_>>();
    harness.state_mut().search.overlay.results = results;
    harness.state_mut().search.overlay.status = crate::search::palette::SearchOverlayStatus::Ready;
    harness.step();
    scenario.capture_screen("screen-005-search-results-ready", &mut harness);

    let first_result = inspect_search_result_tile_center(0, result_placements.len());
    let second_result = inspect_search_result_tile_center(1, result_placements.len());
    let third_result = inspect_search_result_tile_center(2, result_placements.len());
    scenario.action("select first search result", first_result);
    inspect_click_at(&harness, first_result, egui::Modifiers::NONE);
    harness.step();
    scenario.action("add second search result to selection", second_result);
    inspect_click_at(&harness, second_result, inspect_command_modifier());
    harness.step();
    scenario.action("add third search result to selection", third_result);
    inspect_click_at(&harness, third_result, inspect_command_modifier());
    harness.step();
    scenario.capture_screen("screen-006-three-search-results-selected", &mut harness);
    let selected_before_drag = harness
        .state()
        .search
        .overlay
        .result_selection
        .selected()
        .len();

    let provisional_screen = egui::pos2(890.0, 610.0);
    scenario.action(
        "drag selected search results over canvas",
        provisional_screen,
    );
    harness.hover_at(first_result);
    harness.drag_at(first_result);
    harness.hover_at(provisional_screen);
    harness.step();
    harness.step();
    let provisional_count = harness
        .state()
        .canvas
        .provisional_placements_in_selection_order()
        .len();
    scenario.fact(
        "blue-image-search-drag.provisional",
        serde_json::json!({
            "count": provisional_count,
        }),
    );
    scenario.capture_screen(
        "screen-007-provisional-search-placements-before-drop",
        &mut harness,
    );

    scenario.action("drop search placements on canvas", provisional_screen);
    harness.drop_at(provisional_screen);
    harness.step();
    scenario.capture_screen("screen-008-search-images-dropped-loaded", &mut harness);

    let final_placement_count = harness.state().canvas.all_placement_ids().len();
    scenario.fact(
        "blue-image-search-drag.final",
        serde_json::json!({
            "selected_results_before_drag": selected_before_drag,
            "final_placement_count": final_placement_count,
            "status": harness.state().status,
        }),
    );
    scenario.assertion(
        "three search results selected before drag",
        selected_before_drag == 3,
    );
    scenario.assertion(
        "three provisional search placements visible before drop",
        provisional_count == 3,
    );
    scenario.assertion(
        "search drag created three additional placements",
        final_placement_count == 4,
    );
    scenario.assertion(
        "search images loaded after drop",
        harness.state().placement_creation.pending_len() == 0,
    );

    let report = scenario.finish();
    assert!(
        report.passed,
        "inspect scenario failed; artifact={}",
        report.artifact_dir.display()
    );
}

#[test]
fn inspect_placement_click_hold_flash_writes_artifact() {
    let mut scenario = InspectScenario::new("placement-click-hold-flash");
    scenario.event(
        "seed",
        serde_json::json!({
            "seed": "single-blue-placement-click-hold",
            "purpose": "capture frames around primary-click selection held past movement debounce",
        }),
    );

    let root = TempRoot::new("snappyref-inspect-placement-click-hold-flash");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let blue_path = library
        .store_rgba_image(
            "blue-click-hold",
            &solid_rgba([78, 126, 226, 255], 140, 240),
            140,
            240,
        )
        .unwrap();
    let blue = library
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
                    .register_original(&blue_path, "image/png", 140, 240, 140 * 240 * 4, None)
                    .unwrap(),
            ),
        )
        .unwrap();

    let app = test_app(Some(library.clone()));
    let mut harness = app_full_harness_with_step_dt(app, 1.0 / 60.0);
    let blue_prepared = inspect_prepared_loaded_placement(
        blue.clone(),
        [78, 126, 226, 255],
        140,
        240,
        None,
        crate::image_preparation::ImportOrigin::Existing,
    );
    let ctx = harness.ctx.clone();
    harness
        .state_mut()
        .canvas
        .add_prepared_placement_image(&ctx, blue_prepared);
    harness.step();

    let placement_id = blue.state.id;
    let click_screen = inspect_world_to_screen(egui::pos2(0.0, 0.0), test_screen_rect());
    let placement_screen_rect =
        egui::Rect::from_center_size(click_screen, egui::vec2(140.0, 240.0));
    scenario.target("blue placement", "placement", placement_screen_rect);
    scenario.capture_click_hold_fact("screen-000-before-press", &harness, placement_id);
    scenario.capture_screen("screen-000-before-press", &mut harness);

    scenario.action("primary press on blue placement", click_screen);
    harness.event(egui::Event::PointerButton {
        pos: click_screen,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::NONE,
    });
    harness.step();
    scenario.capture_click_hold_fact("screen-001-pressed-selected", &harness, placement_id);
    scenario.capture_screen("screen-001-pressed-selected", &mut harness);

    for index in 2..=8 {
        harness.step();
        let name = format!("screen-{index:03}-held");
        scenario.capture_click_hold_fact(&name, &harness, placement_id);
        scenario.capture_screen(&name, &mut harness);
    }
    let held_time_seconds = harness.ctx.input(|input| input.time);
    scenario.assertion(
        "selection border stays visible during stationary held click",
        harness
            .state()
            .canvas
            .selection_border_visible(held_time_seconds),
    );

    scenario.action("primary release on blue placement", click_screen);
    harness.event(egui::Event::PointerButton {
        pos: click_screen,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    });
    harness.step();
    scenario.capture_click_hold_fact("screen-009-released", &harness, placement_id);
    scenario.capture_screen("screen-009-released", &mut harness);

    let selected = harness.state().canvas.selected_placement_ids();
    scenario.assertion(
        "blue placement remains selected after click hold",
        selected == vec![placement_id],
    );

    let report = scenario.finish();
    assert!(
        report.passed,
        "inspect scenario failed; artifact={}",
        report.artifact_dir.display()
    );
}

#[test]
fn inspect_delete_undo_redo_placement_writes_artifact() {
    let mut scenario = InspectScenario::new("delete-undo-redo-placement");
    scenario.event(
        "seed",
        serde_json::json!({
            "seed": "single-blue-placement",
            "purpose": "click placement, delete it, then exercise Undo and Redo shortcuts",
        }),
    );

    let root = TempRoot::new("snappyref-inspect-delete-undo-redo-placement");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let blue_path = library
        .store_rgba_image(
            "blue-undo-redo",
            &solid_rgba([78, 126, 226, 255], 140, 100),
            140,
            100,
        )
        .unwrap();
    let blue = library
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
                    .register_original(&blue_path, "image/png", 140, 100, 140 * 100 * 4, None)
                    .unwrap(),
            ),
        )
        .unwrap();

    let mut app = test_app(Some(library.clone()));
    app.board_editing.persistence = crate::app::DocumentPersistence::queued(library.clone());
    let mut harness = app_full_harness(app);
    let ctx = harness.ctx.clone();
    harness.state_mut().canvas.add_prepared_placement_image(
        &ctx,
        inspect_prepared_loaded_placement(
            blue.clone(),
            [78, 126, 226, 255],
            140,
            100,
            None,
            crate::image_preparation::ImportOrigin::Existing,
        ),
    );
    harness.step();

    let placement_id = blue.state.id;
    let click_screen = inspect_world_to_screen(blue.state.geometry.center, test_screen_rect());
    scenario.target(
        "blue placement",
        "placement",
        egui::Rect::from_center_size(click_screen, egui::vec2(140.0, 100.0)),
    );
    scenario.capture_screen("screen-000-before-click", &mut harness);

    scenario.action("click blue placement", click_screen);
    inspect_click_at(&harness, click_screen, egui::Modifiers::NONE);
    harness.step();
    scenario.assertion(
        "pointer click selects placement",
        harness.state().canvas.selected_placement_ids() == vec![placement_id],
    );
    scenario.capture_screen("screen-001-selected", &mut harness);

    scenario.event(
        "action",
        serde_json::json!({"name": "press Delete shortcut"}),
    );
    harness.key_press(INSPECT_DELETE_KEY);
    harness.step();
    scenario.fact(
        "placement.after-delete",
        serde_json::json!({
            "present": harness.state().canvas.placement_for_id(placement_id).is_some(),
            "status": harness.state().status,
        }),
    );
    scenario.assertion(
        "Delete removes placement from active canvas",
        harness
            .state()
            .canvas
            .placement_for_id(placement_id)
            .is_none(),
    );
    scenario.capture_screen("screen-002-deleted", &mut harness);

    let undo_modifiers = egui::Modifiers {
        command: true,
        ..Default::default()
    };
    scenario.event(
        "action",
        serde_json::json!({"name": "press Command-Z Undo shortcut"}),
    );
    for pressed in [true, false] {
        harness.event_modifiers(
            egui::Event::Key {
                key: egui::Key::Z,
                physical_key: Some(egui::Key::Z),
                pressed,
                repeat: false,
                modifiers: undo_modifiers,
            },
            undo_modifiers,
        );
    }
    harness.step();
    let restored = harness.state().canvas.placement_for_id(placement_id);
    scenario.fact(
        "placement.after-undo",
        serde_json::json!({
            "present": restored.is_some(),
            "loaded": restored.as_ref().is_some_and(|placement| placement.is_loaded()),
            "status": harness.state().status,
        }),
    );
    scenario.assertion(
        "Undo restores loaded placement to active canvas",
        restored.is_some_and(|placement| placement.is_loaded()),
    );
    scenario.capture_screen("screen-003-undone", &mut harness);

    let redo_modifiers = egui::Modifiers {
        command: true,
        shift: true,
        ..Default::default()
    };
    scenario.event(
        "action",
        serde_json::json!({"name": "press Command-Shift-Z Redo shortcut"}),
    );
    for pressed in [true, false] {
        harness.event_modifiers(
            egui::Event::Key {
                key: egui::Key::Z,
                physical_key: Some(egui::Key::Z),
                pressed,
                repeat: false,
                modifiers: redo_modifiers,
            },
            redo_modifiers,
        );
    }
    harness.step();
    scenario.fact(
        "placement.after-redo",
        serde_json::json!({
            "present": harness.state().canvas.placement_for_id(placement_id).is_some(),
            "status": harness.state().status,
        }),
    );
    scenario.assertion(
        "Redo removes placement from active canvas again",
        harness
            .state()
            .canvas
            .placement_for_id(placement_id)
            .is_none(),
    );
    scenario.capture_screen("screen-004-redone", &mut harness);

    harness.state_mut().flush_persistence();
    scenario.assertion(
        "explicit flush makes final Redo durable",
        library.load_placements().unwrap().is_empty(),
    );

    let report = scenario.finish();
    assert!(
        report.passed,
        "inspect scenario failed; artifact={}",
        report.artifact_dir.display()
    );
}

#[test]
fn inspect_crop_undo_redo_placement_writes_artifact() {
    let mut scenario = InspectScenario::new("crop-undo-redo-placement");
    scenario.event(
        "seed",
        serde_json::json!({
            "seed": "single-loaded-placement",
            "purpose": "select the image, crop with C plus a pointer drag, then exercise Undo and Redo",
        }),
    );

    let root = TempRoot::new("snappyref-inspect-crop-undo-redo-placement");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let width = 160;
    let height = 120;
    let source_rgba = (0..height)
        .flat_map(|y| {
            (0..width).flat_map(move |x| {
                let red = (x * 255 / width) as u8;
                let blue = (y * 255 / height) as u8;
                [red, 96, blue, 255]
            })
        })
        .collect::<Vec<_>>();
    let source_path = library
        .store_rgba_image("crop-source", &source_rgba, width, height)
        .unwrap();
    let loading = library
        .fixture_placement(
            library.load_user_session().unwrap().active_board_id,
            library.new_committed_loading_placement(
                egui::Pos2::ZERO,
                1.0,
                eframe::egui::vec2(width as f32, height as f32),
                crate::data::ContentResolutionSource::ExternalPath {
                    path: source_path.to_string_lossy().into_owned(),
                },
                Some("Loading".to_owned()),
            ),
        )
        .unwrap();
    let source = crate::data::Placement::loaded(
        crate::data::PlacementState::new(loading.state.id, egui::Pos2::ZERO, 1.0, 0.0, 0, false),
        library
            .register_original(
                &source_path,
                "image/png",
                width,
                height,
                source_rgba.len() as u64,
                None,
            )
            .unwrap(),
    );
    let source_asset_id = source.require_loaded_asset().id;

    let mut app = test_app(Some(library.clone()));
    app.settings.semantic_search_first_launch_prompt_seen = true;
    app.search
        .set_local_model_store_status_for_test(LocalModelStoreStatus::Missing);
    app.board_editing.persistence = crate::app::DocumentPersistence::queued(library.clone());
    let mut harness = app_full_harness(app);
    let ctx = harness.ctx.clone();
    harness.state_mut().import_worker = Some(crate::image_preparation::ImagePrepWorker::new(
        library.clone(),
        ctx.clone(),
        None,
    ));
    let active_board_id = harness.state().active_board_id;
    harness.state_mut().canvas.add_prepared_placement_image(
        &ctx,
        PreparedPlacementImage {
            target_board_id: active_board_id,
            preview_rgba: source_rgba,
            preview_width: width,
            preview_height: height,
            ..loaded_record(source.clone())
        },
    );
    harness.step();

    let placement_id = source.state.id;
    let image_center = inspect_world_to_screen(source.state.geometry.center, test_screen_rect());
    let crop_start = image_center + egui::vec2(-55.0, -40.0);
    let crop_end = image_center + egui::vec2(45.0, 30.0);
    scenario.target(
        "source image",
        "placement",
        egui::Rect::from_center_size(image_center, egui::vec2(width as f32, height as f32)),
    );
    scenario.capture_screen("screen-000-source", &mut harness);

    scenario.action("click source image", image_center);
    inspect_click_at(&harness, image_center, egui::Modifiers::NONE);
    harness.step();
    scenario.assertion(
        "pointer click selects source image",
        harness.state().canvas.selected_placement_ids() == vec![placement_id],
    );
    scenario.capture_screen("screen-001-selected", &mut harness);

    scenario.event(
        "action",
        serde_json::json!({
            "name": "hold C and pointer-drag crop rectangle",
            "start": inspect_pos(crop_start),
            "end": inspect_pos(crop_end),
        }),
    );
    harness.key_down(egui::Key::C);
    harness.hover_at(crop_start);
    harness.drag_at(crop_start);
    harness.step();
    harness.hover_at(crop_end);
    harness.step();
    scenario.capture_screen("screen-002-crop-drag", &mut harness);
    harness.drop_at(crop_end);
    harness.key_up(egui::Key::C);
    harness.step();

    let crop_deadline = Instant::now() + Duration::from_secs(5);
    while harness
        .state()
        .canvas
        .placement_for_id(placement_id)
        .is_some_and(|placement| placement.require_loaded_asset().id == source_asset_id)
        && Instant::now() < crop_deadline
    {
        std::thread::sleep(Duration::from_millis(10));
        harness.step();
    }
    let cropped = harness
        .state()
        .canvas
        .placement_for_id(placement_id)
        .unwrap();
    let cropped_asset_id = cropped.require_loaded_asset().id;
    scenario.fact(
        "placement.after-crop",
        serde_json::json!({
            "source_asset_id": source_asset_id,
            "asset_id": cropped_asset_id,
            "asset_dimensions": [cropped.require_loaded_asset().width, cropped.require_loaded_asset().height],
            "status": harness.state().status,
        }),
    );
    scenario.assertion(
        "pointer crop creates one derived Asset",
        cropped_asset_id != source_asset_id,
    );
    scenario.capture_screen("screen-003-cropped", &mut harness);

    scenario.event(
        "action",
        serde_json::json!({"name": "press Command-Z Undo shortcut"}),
    );
    harness.key_press_modifiers(inspect_command_modifier(), egui::Key::Z);
    harness.step();
    let undo_deadline = Instant::now() + Duration::from_secs(5);
    while harness.state().placement_creation.pending_len() > 0 && Instant::now() < undo_deadline {
        std::thread::sleep(Duration::from_millis(10));
        harness.step();
    }
    let after_undo = harness
        .state()
        .canvas
        .placement_for_id(placement_id)
        .unwrap();
    scenario.fact(
        "placement.after-undo",
        serde_json::json!({
            "asset_id": after_undo.require_loaded_asset().id,
            "status": harness.state().status,
        }),
    );
    scenario.assertion(
        "one Undo restores the source Asset",
        after_undo.require_loaded_asset().id == source_asset_id
            && harness.state().status == "Undid Crop image",
    );
    scenario.capture_screen("screen-004-undone", &mut harness);

    scenario.event(
        "action",
        serde_json::json!({"name": "press Command-Z a second time"}),
    );
    harness.key_press_modifiers(inspect_command_modifier(), egui::Key::Z);
    harness.step();
    scenario.fact(
        "placement.after-second-undo",
        serde_json::json!({
            "asset_id": harness
                .state()
                .canvas
                .placement_for_id(placement_id)
                .unwrap()
                .require_loaded_asset()
                .id,
            "status": harness.state().status,
        }),
    );
    scenario.assertion(
        "second Undo does not reverse the crop again",
        harness
            .state()
            .canvas
            .placement_for_id(placement_id)
            .unwrap()
            .require_loaded_asset()
            .id
            == source_asset_id
            && harness.state().status == "Undid Crop image",
    );
    scenario.capture_screen("screen-005-second-undo", &mut harness);

    let redo_modifiers = egui::Modifiers {
        command: true,
        shift: true,
        ..Default::default()
    };
    scenario.event(
        "action",
        serde_json::json!({"name": "press Command-Shift-Z Redo shortcut"}),
    );
    harness.key_press_modifiers(redo_modifiers, egui::Key::Z);
    harness.step();
    let redo_deadline = Instant::now() + Duration::from_secs(5);
    while harness.state().placement_creation.pending_len() > 0 && Instant::now() < redo_deadline {
        std::thread::sleep(Duration::from_millis(10));
        harness.step();
    }
    let after_redo = harness
        .state()
        .canvas
        .placement_for_id(placement_id)
        .unwrap();
    scenario.fact(
        "placement.after-redo",
        serde_json::json!({
            "asset_id": after_redo.require_loaded_asset().id,
            "status": harness.state().status,
        }),
    );
    scenario.assertion(
        "Redo restores the same cropped Asset",
        after_redo.require_loaded_asset().id == cropped_asset_id
            && harness.state().status == "Redid Crop image",
    );
    scenario.capture_screen("screen-006-redone", &mut harness);

    harness.state_mut().flush_persistence();
    let durable = &library.load_placements().unwrap()[0];
    assert!(
        durable.is_loaded(),
        "crop durability lost loaded content: {durable:?}; status={}",
        harness.state().status
    );
    scenario.assertion(
        "explicit flush preserves the cropped Asset relation",
        durable.require_loaded_asset().id == cropped_asset_id,
    );

    let report = scenario.finish();
    assert!(
        report.passed,
        "inspect scenario failed; artifact={}",
        report.artifact_dir.display()
    );
}

#[test]
fn inspect_pending_drop_batch_undo_writes_artifact() {
    let mut scenario = InspectScenario::new("pending-drop-batch-undo");
    scenario.event(
        "seed",
        serde_json::json!({
            "seed": "two-local-png-files-and-controlled-image-preparation",
            "purpose": "drop one committed batch, Undo while both Placements are Loading, then drain late results",
        }),
    );

    let root = TempRoot::new("snappyref-inspect-pending-drop-batch-undo");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let sources = [
        (
            library
                .store_rgba_image(
                    "pending-drop-blue",
                    &solid_rgba([78, 126, 226, 255], 160, 100),
                    160,
                    100,
                )
                .unwrap(),
            [78, 126, 226, 255],
            160,
            100,
        ),
        (
            library
                .store_rgba_image(
                    "pending-drop-gold",
                    &solid_rgba([224, 168, 72, 255], 100, 140),
                    100,
                    140,
                )
                .unwrap(),
            [224, 168, 72, 255],
            100,
            140,
        ),
    ];
    let (worker, result_tx) = crate::image_preparation::ImagePrepWorker::controlled();
    let mut app = test_app(Some(library.clone()));
    app.board_editing.persistence = crate::app::DocumentPersistence::queued(library.clone());
    app.import_worker = Some(worker);
    let mut harness = app_full_harness(app);
    let drop_pos = egui::pos2(900.0, 600.0);
    scenario.target(
        "canvas drop point",
        "canvas",
        egui::Rect::from_center_size(drop_pos, egui::vec2(24.0, 24.0)),
    );
    scenario.capture_screen("screen-000-before-drop", &mut harness);

    scenario.action("hover two files over canvas", drop_pos);
    harness.hover_at(drop_pos);
    harness.input_mut().hovered_files = sources
        .iter()
        .map(|(path, _, _, _)| egui::HoveredFile {
            path: Some(path.clone()),
            ..Default::default()
        })
        .collect();
    harness.step();
    scenario.fact(
        "drop.provisional",
        serde_json::json!({
            "count": harness.state().canvas.provisional_placements_in_selection_order().len(),
        }),
    );
    scenario.assertion(
        "hover creates two provisional Placements",
        harness
            .state()
            .canvas
            .provisional_placements_in_selection_order()
            .len()
            == 2,
    );
    scenario.capture_screen("screen-001-hover-provisional", &mut harness);

    scenario.action("drop two files on canvas", drop_pos);
    harness.input_mut().hovered_files.clear();
    harness.input_mut().dropped_files = sources
        .iter()
        .map(|(path, _, _, _)| dropped_file(path.clone()))
        .collect();
    harness.step();
    let placement_ids = harness.state().canvas.selected_placement_ids();
    let worker_stats = harness.state().import_worker.as_ref().unwrap().stats();
    scenario.fact(
        "drop.accepted",
        serde_json::json!({
            "placement_ids": placement_ids,
            "pending": harness.state().placement_creation.pending_len(),
            "queued_worker_requests": worker_stats.queued_startup
                + worker_stats.queued_interactive
                + worker_stats.queued_background,
        }),
    );
    scenario.assertion(
        "drop accepts one two-Placement Loading batch",
        placement_ids.len() == 2 && harness.state().placement_creation.pending_len() == 2,
    );
    scenario.capture_screen("screen-002-accepted-loading-batch", &mut harness);

    let loaded_placement = crate::data::Placement::loaded(
        crate::data::PlacementState::new(
            placement_ids[0],
            harness
                .state()
                .canvas
                .placement_for_id(placement_ids[0])
                .unwrap()
                .state
                .geometry
                .center,
            1.0,
            0.0,
            0,
            false,
        ),
        library
            .register_original(
                &sources[0].0,
                "image/png",
                sources[0].2,
                sources[0].3,
                std::fs::metadata(&sources[0].0).unwrap().len(),
                None,
            )
            .unwrap(),
    );
    let late_loaded = inspect_prepared_loaded_placement(
        loaded_placement,
        sources[0].1,
        sources[0].2,
        sources[0].3,
        Some(1),
        crate::image_preparation::ImportOrigin::Path,
    );

    let undo_modifiers = egui::Modifiers {
        command: true,
        ..Default::default()
    };
    scenario.event(
        "action",
        serde_json::json!({"name": "press Command-Z while both preparations are pending"}),
    );
    for pressed in [true, false] {
        harness.event_modifiers(
            egui::Event::Key {
                key: egui::Key::Z,
                physical_key: Some(egui::Key::Z),
                pressed,
                repeat: false,
                modifiers: undo_modifiers,
            },
            undo_modifiers,
        );
    }
    harness.step();
    scenario.fact(
        "drop.after-undo",
        serde_json::json!({
            "present_count": placement_ids
                .iter()
                .filter(|id| harness.state().canvas.placement_for_id(**id).is_some())
                .count(),
            "pending": harness.state().placement_creation.pending_len(),
            "status": harness.state().status,
        }),
    );
    scenario.assertion(
        "one Undo removes the whole accepted batch while work remains pending",
        placement_ids
            .iter()
            .all(|id| harness.state().canvas.placement_for_id(*id).is_none())
            && harness.state().placement_creation.pending_len() == 2
            && harness.state().status == "Undid Import images",
    );
    scenario.capture_screen("screen-003-undone-while-pending", &mut harness);

    result_tx
        .send(crate::image_preparation::ImagePrepResult::Loaded(Box::new(
            late_loaded,
        )))
        .unwrap();
    result_tx
        .send(crate::image_preparation::ImagePrepResult::Failed {
            target_board_id: Some(harness.state().active_board_id),
            queue_id: Some(2),
            placement_id: Some(placement_ids[1]),
            context: sources[1].0.display().to_string(),
            error: "controlled late decode failure".to_owned(),
        })
        .unwrap();
    scenario.event(
        "wait",
        serde_json::json!({"name": "drain late Loaded and Failed worker results"}),
    );
    harness.step();
    scenario.fact(
        "drop.after-late-results",
        serde_json::json!({
            "present_count": placement_ids
                .iter()
                .filter(|id| harness.state().canvas.placement_for_id(**id).is_some())
                .count(),
            "pending": harness.state().placement_creation.pending_len(),
            "status": harness.state().status,
        }),
    );
    scenario.assertion(
        "late results drain without restoring the undone batch",
        placement_ids
            .iter()
            .all(|id| harness.state().canvas.placement_for_id(*id).is_none())
            && harness.state().placement_creation.pending_len() == 0
            && harness.state().status == "Undid Import images",
    );
    scenario.capture_screen("screen-004-late-results-discarded", &mut harness);

    harness.state_mut().flush_persistence();
    scenario.assertion(
        "the undone batch stays absent from durable Board state",
        library.load_placements().unwrap().is_empty(),
    );

    let report = scenario.finish();
    assert!(
        report.passed,
        "inspect scenario failed; artifact={}",
        report.artifact_dir.display()
    );
}

/// Downloads the real model bundle. Run via
/// `docs/skills/inspect/scripts/run-scenario.sh semantic-search-real-install`.
#[test]
#[ignore]
fn inspect_semantic_search_real_install_writes_artifact() {
    let mut scenario = InspectScenario::new("semantic-search-real-install");
    scenario.event(
        "seed",
        serde_json::json!({
            "seed": "sidebar-open-settings-tab-real-semantic-install",
        }),
    );

    let root = TempRoot::new("snappyref-inspect-semantic-real-install");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let initial_status = crate::search::semantic::default_local_model_store_status();
    let mut app = test_app(Some(library));
    app.sidebar_open = true;
    app.sidebar_tab = SidebarTab::Settings;
    app.settings.semantic_search_first_launch_prompt_seen = true;
    app.search
        .set_local_model_store_status_for_test(initial_status.clone());
    app.search.set_semantic_install_for_test(|tx| {
        std::thread::spawn(move || {
            let data_root = match crate::local_data_location::resolve_data_root() {
                Ok(root) => root,
                Err(error) => {
                    let _ = tx.send(
                        crate::search::semantic::SemanticSearchInstallProgress::Failed { error },
                    );
                    return;
                }
            };
            let descriptor = crate::search::semantic::default_semantic_search_asset_descriptor();
            let canceled = std::sync::atomic::AtomicBool::new(false);
            let _ = crate::search::semantic::install_semantic_search_asset(
                &data_root,
                &descriptor,
                &canceled,
                |progress| {
                    let _ = tx.send(progress);
                },
            );
        });
    });

    let mut harness = app_full_harness(app);
    harness.get_by_label("Search").click();
    harness.run();
    scenario.capture_screen("screen-000-before-install", &mut harness);
    scenario.fact(
        "semantic-search.initial-status",
        serde_json::json!({
            "status": format!("{initial_status:?}"),
        }),
    );

    if matches!(initial_status, LocalModelStoreStatus::Installed) {
        scenario.assertion("semantic search already installed", true);
        scenario.capture_screen("screen-001-installed", &mut harness);
        let report = scenario.finish();
        assert!(
            report.passed,
            "inspect scenario failed; artifact={}",
            report.artifact_dir.display()
        );
        return;
    }

    let install_rect = harness.get_by_label("Install").rect();
    scenario.target(
        "snap.sidebar.semantic-search.install",
        "button",
        install_rect,
    );
    scenario.action("click semantic search install", install_rect.center());
    harness.get_by_label("Install").click();
    harness.step();
    scenario.capture_screen("screen-001-install-clicked", &mut harness);

    scenario.drive_progress(
        &mut harness,
        InspectProgressConfig {
            start_frame_index: 2,
            timeout: Duration::from_secs(15 * 60),
            poll_interval: Duration::from_millis(250),
            fact_name: "semantic-search.install-status",
            timeout_fact_name: "semantic-search.install-timeout-status",
            terminal_assertion: "semantic search install reached terminal state",
            timeout_assertion: "semantic search install completed before timeout",
        },
        |app| app.search.local_model_store_status(),
        inspect_status_slug,
        |status| {
            matches!(
                status,
                LocalModelStoreStatus::Installed | LocalModelStoreStatus::Failed(_)
            )
        },
        |status| matches!(status, LocalModelStoreStatus::Installed),
    );

    let report = scenario.finish();
    assert!(
        report.passed,
        "inspect scenario failed; artifact={}",
        report.artifact_dir.display()
    );
}
