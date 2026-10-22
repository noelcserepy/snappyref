use super::support::{
    INSPECT_DELETE_KEY, check_compact_placements, inspect_displayed_galleys, inspect_displayed_text,
};
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
    app.search
        .set_local_model_store_status_for_test(LocalModelStoreStatus::Installed);
    app.canvas
        .apply_viewport_state(&crate::data::ViewportState {
            camera_pan_x: 0.0,
            camera_pan_y: 0.0,
            camera_zoom: 1.0,
            ..Default::default()
        });
    let mut harness = app_full_harness(app);
    scenario.capture_screen("screen-before-drag", &mut harness);

    let source_screen = inspect_search_result_tile_center(0, 1);
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
    let pointer_world = egui::pos2(300.0, 200.0);
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
    scenario.assertion(
        "drop preserves the source asset and pointer center",
        committed.as_ref().is_some_and(|p| {
            p.require_loaded_asset().id == source.require_loaded_asset().id
                && p.state.geometry.center.distance(pointer_world) < 0.1
        }),
    );
    scenario.capture_screen("screen-after-release", &mut harness);

    scenario.finish();
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
    harness.key_press_modifiers(inspect_command_modifier(), egui::Key::F);
    harness.run();
    scenario.assertion(
        "search shortcut stays behind the open prompt",
        !harness.state().search.is_open(),
    );
    let enable_rect = harness.get_by_label("Enable").rect();
    scenario.target("snap.semantic-search.install.enable", "button", enable_rect);
    scenario.event("key", serde_json::json!({ "key": "Enter" }));
    harness.key_press(egui::Key::Enter);
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
    scenario.assertion("Enter enables and starts the install", starts.get() == 1);
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

    harness
        .state_mut()
        .search
        .set_local_model_store_status_for_test(LocalModelStoreStatus::Installed);
    harness.state_mut().pending_semantic_search_remove = true;
    harness.run();
    scenario.capture_screen("screen-remove-dialog", &mut harness);
    harness.key_press(egui::Key::Escape);
    harness.run();
    scenario.assertion(
        "Escape cancels Remove and keeps the model",
        !harness.state().pending_semantic_search_remove
            && harness.state().search.local_model_store_status()
                == LocalModelStoreStatus::Installed,
    );

    scenario.finish();
}

#[test]
fn inspect_image_search_after_install_writes_artifact() {
    let mut scenario = InspectScenario::new("image-search-after-install");
    scenario.event(
        "seed",
        serde_json::json!({
            "seed": "unindexed-selected-image-two-indexed-library-images-model-missing",
            "embedding_helper": "offline fixture: 400 ms load, 150 ms per image, every image embeds as the first unit vector",
        }),
    );

    let root = TempRoot::new("snappyref-inspect-image-search-after-install");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let board_id = library.load_user_session().unwrap().active_board_id;
    // Search shows thumbnails from the canvas cache an import writes.
    let add_board_image = |name: &str, color: [u8; 4], x: f32, z_index: i32| {
        let path = library
            .store_rgba_image(name, &solid_rgba(color, 100, 100), 100, 100)
            .unwrap();
        let asset = library
            .register_original(&path, "image/png", 100, 100, 100 * 100 * 4, None)
            .unwrap();
        library
            .store_canvas_image_cache(
                asset.id,
                crate::data::DEFAULT_CANVAS_IMAGE_MAX_SIDE,
                &solid_rgba(color, 100, 100),
                100,
                100,
            )
            .unwrap();
        library
            .fixture_placement(
                board_id,
                crate::data::Placement::loaded(
                    crate::data::PlacementState::new(
                        library.allocate_document_id(),
                        egui::pos2(x, 0.0),
                        1.0,
                        0.0,
                        z_index,
                        false,
                    ),
                    asset,
                ),
            )
            .unwrap()
    };
    // Backfill embeds the lower neighbors first, so the selected image is
    // still unindexed when the helper reports ready.
    for (index, color) in [[200, 92, 92, 255], [92, 200, 120, 255], [210, 190, 90, 255]]
        .into_iter()
        .enumerate()
    {
        add_board_image(
            &format!("neighbor-{index}"),
            color,
            -390.0 + index as f32 * 130.0,
            index as i32,
        );
    }
    let source = add_board_image("blue-selected", [78, 126, 226, 255], 0.0, 3);
    // The fixture helper embeds the selected image as the first unit vector,
    // so the near image ranks above the far one.
    let dimension = crate::search::semantic::DEFAULT_SEMANTIC_SEARCH_MODEL.dimension;
    let mut indexed = Vec::new();
    for (name, color, leading) in [
        ("near-result", [92, 140, 230, 255], [0.9_f32, 0.1]),
        ("far-result", [228, 171, 72, 255], [0.1_f32, 0.9]),
    ] {
        let path = library
            .store_rgba_image(name, &solid_rgba(color, 160, 160), 160, 160)
            .unwrap();
        let asset = library
            .register_original(&path, "image/png", 160, 160, 160 * 160 * 4, None)
            .unwrap();
        library
            .store_canvas_image_cache(
                asset.id,
                crate::data::DEFAULT_CANVAS_IMAGE_MAX_SIDE,
                &solid_rgba(color, 160, 160),
                160,
                160,
            )
            .unwrap();
        let mut embedding = vec![0.0_f32; dimension];
        embedding[..2].copy_from_slice(&leading);
        library
            .upsert_asset_embedding_blob(
                asset.id,
                crate::search::semantic::DEFAULT_SEMANTIC_SEARCH_MODEL.model_key,
                &embedding,
            )
            .unwrap();
        indexed.push(asset.id);
    }

    let install = std::rc::Rc::new(std::cell::RefCell::new(None));
    let installer = install.clone();
    let mut app = test_app(Some(library.clone()));
    // Canvas image loads must drain, or the app never goes idle for backfill.
    app.import_worker = Some(crate::image_preparation::ImagePrepWorker::new(
        library.clone(),
        egui::Context::default(),
        None,
    ));
    app.settings.semantic_search_first_launch_prompt_seen = true;
    app.search
        .set_local_model_store_status_for_test(LocalModelStoreStatus::Missing);
    app.search
        .set_embedding_helper_for_test(crate::search::semantic::EmbeddingHelperLaunch::Fixture);
    app.search
        .set_semantic_install_for_test(move |tx| *installer.borrow_mut() = Some(tx));
    app.board_lifecycle().restore().unwrap();
    let mut harness = app_full_harness(app);
    harness.step();

    let source_screen = inspect_world_to_screen(egui::pos2(0.0, 0.0), test_screen_rect());
    scenario.action("click blue image", source_screen);
    inspect_click_at(&harness, source_screen, egui::Modifiers::NONE);
    harness.step();
    scenario.event("key", serde_json::json!({ "key": "Mod+F" }));
    harness.key_press_modifiers(inspect_command_modifier(), egui::Key::F);
    harness.step();
    harness.step();
    scenario.assertion(
        "Mod+F on the selected image opens search on the install card",
        harness.state().search.is_open()
            && harness
                .state()
                .search
                .overlay
                .image_pivot
                .as_ref()
                .map(|pivot| pivot.asset_id)
                == Some(source.require_loaded_asset().id)
            && harness.state().search.local_model_needs_install(),
    );
    scenario.capture_screen("screen-000-install-card", &mut harness);

    let install_rect = harness.get_by_label("Install").rect();
    scenario.target("snap.search.install-card.install", "button", install_rect);
    scenario.action("click Install", install_rect.center());
    harness.get_by_label("Install").click();
    harness.step();
    harness.step();
    let tx = install
        .borrow_mut()
        .take()
        .expect("Install starts the test installer");
    tx.send(crate::search::semantic::SemanticSearchInstallProgress::Installed)
        .unwrap();

    let sample = |app: &SnappyRefApp| {
        let search = &app.search;
        let line = if search.local_model_needs_install()
            || matches!(
                search.local_model_store_status(),
                LocalModelStoreStatus::Downloading { .. }
            ) {
            format!("{:?}", search.local_model_store_status())
        } else if let crate::search::palette::SearchOverlayStatus::Error(error) =
            &search.overlay.status
        {
            format!("Search error: {error}")
        } else {
            crate::search::palette::palette_status_line(&search.overlay.status)
                .1
                .to_owned()
        };
        let results = search
            .overlay
            .results
            .iter()
            .map(|result| result.asset_id)
            .collect::<Vec<_>>();
        (line, results)
    };
    let slug = |(line, results): &(String, Vec<i64>)| {
        let line = line
            .trim_end_matches('…')
            .to_lowercase()
            .replace(|c: char| !c.is_ascii_alphanumeric(), "-");
        format!("{line}-{}-results", results.len())
    };
    let finished = |(line, results): &(String, Vec<i64>)| {
        !results.is_empty() || line.starts_with("Search error")
    };
    let observed = std::cell::RefCell::new(Vec::<String>::new());
    // Step without captures until the wait shows. A slow capture between steps
    // could let the image finish indexing inside one step and hide the wait.
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    let waiting = loop {
        harness.step();
        let state = sample(harness.state());
        observed.borrow_mut().push(state.0.clone());
        if state.0 == "Indexing…" || finished(&state) || std::time::Instant::now() >= deadline {
            break state;
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    scenario.capture_screen(&format!("screen-001-{}", slug(&waiting)), &mut harness);
    let terminal = scenario.drive_progress(
        &mut harness,
        InspectProgressConfig {
            start_frame_index: 2,
            timeout: Duration::from_secs(30),
            poll_interval: Duration::from_millis(10),
            fact_name: "image-search.status",
            timeout_fact_name: "image-search.timeout",
            terminal_assertion: "image search shows the indexed results",
            timeout_assertion: "image search finishes before timeout",
        },
        |app| {
            let state = sample(app);
            observed.borrow_mut().push(state.0.clone());
            state
        },
        slug,
        finished,
        |(_, results)| *results == indexed,
    );
    let previews_ready = inspect_wait_for_search_previews(&mut harness);
    scenario.capture_screen("screen-result-previews", &mut harness);
    scenario.assertion("result previews decoded before capture", previews_ready);
    let observed = observed.into_inner();
    scenario.fact(
        "image-search.after-install",
        serde_json::json!({
            "terminal": format!("{terminal:?}"),
            "status_lines": observed.iter().fold(Vec::<&String>::new(), |mut lines, line| {
                if lines.last() != Some(&line) {
                    lines.push(line);
                }
                lines
            }),
            "expected_results": indexed,
        }),
    );
    scenario.assertion(
        "search says Indexing while the selected image is embedded",
        observed.iter().any(|line| line == "Indexing…"),
    );
    scenario.assertion(
        "search never shows an error on the way",
        !observed.iter().any(|line| line.starts_with("Search error")),
    );
    scenario.finish();
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
    let actual_status = LocalModelStoreStatus::Downloading { percent: 42 };
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
    scenario.assertion(
        "seeded model status displays Downloading 42% and Cancel",
        harness.query_by_label("Downloading 42%").is_some()
            && harness.query_by_label("Cancel").is_some(),
    );
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

    scenario.finish();
}

#[test]
fn inspect_sidebar_pages_writes_artifact() {
    inspect_sidebar_pages(InspectScenario::new("sidebar-pages"));
}

/// Windows spells modifiers out as Ctrl, Alt, and Shift chips, the widest
/// shortcuts the sidebar shows.
#[test]
fn inspect_sidebar_pages_windows_writes_artifact() {
    let _windows = crate::ui::shortcut::WindowsForm::force();
    inspect_sidebar_pages(InspectScenario::new("sidebar-pages-windows"));
}

fn elided_text(harness: &Harness<'_, SnappyRefApp>) -> Vec<String> {
    inspect_displayed_galleys(harness)
        .into_iter()
        .filter(|t| t.elided)
        .map(|t| t.text)
        .collect()
}

/// True when every one of `texts` shows in the frame, not cut short.
fn in_full(harness: &Harness<'_, SnappyRefApp>, texts: &[&str]) -> bool {
    let shown = inspect_displayed_galleys(harness);
    texts
        .iter()
        .all(|text| shown.iter().any(|t| t.text == *text && !t.elided))
}

fn inspect_sidebar_pages(mut scenario: InspectScenario) {
    scenario.event(
        "seed",
        serde_json::json!({
            "seed": "four-boards-long-names-update-available-search-downloading",
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
    let lighting = library.fixture_board("Lighting").unwrap();
    add_placements(lighting.id, 1);
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
    let text = super::support::inspect_displayed_text(&harness);
    scenario.fact("boards.rendered-labels", serde_json::json!(text));
    scenario.assertion(
        "board counts identify items with singular and plural labels",
        ["3 items", "12 items", "7 items", "1 item"]
            .iter()
            .all(|label| text.iter().any(|shown| shown == label)),
    );
    let boards_tab = harness.get_by_label("Boards").rect();
    let first_row = harness.get_by_label("Board My Board").rect();
    scenario.assertion(
        "Boards and Settings navigation sits above the board list",
        boards_tab.bottom() < first_row.top(),
    );
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
    let hovered_text = super::support::inspect_displayed_text(&harness);
    scenario.assertion(
        "hover replaces only the hovered board count with Delete",
        hovered_text.iter().any(|text| text == "Delete")
            && !hovered_text.iter().any(|text| text == "7 items")
            && hovered_text.iter().any(|text| text == "12 items"),
    );

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
        harness.query_by_label("Always on top").is_some(),
    );
    let text = inspect_displayed_galleys(&harness);
    let on_top = text.iter().find(|t| t.text == "Always on top").unwrap();
    scenario.target("sidebar.always-on-top.name", "text", on_top.rect);
    scenario.assertion(
        "Always on top shows in full, clear of its shortcut chips",
        !on_top.elided
            && text
                .iter()
                .filter(|t| t.text != "Always on top")
                .all(|t| !t.rect.intersects(on_top.rect)),
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
    // The widest rows: long actions with Ctrl Shift chips on Windows.
    let top_shown = in_full(&harness, &["Ungroup selection", "Export board to PureRef"]);
    let mut cut = elided_text(&harness);
    harness.get_by_label("Undo").scroll_to_me();
    harness.run_steps(6);
    scenario.capture_screen("screen-003-keys-end", &mut harness);
    let end_shown = in_full(
        &harness,
        &[
            "Toggle always on top",
            "Directional settle arrange",
            "Selection avg area",
        ],
    );
    cut.extend(elided_text(&harness));
    scenario.fact("keys.cut-short", serde_json::json!(cut));
    scenario.assertion(
        "every Keys action shows in full, the widest rows included",
        top_shown && end_shown && cut.is_empty(),
    );
    scenario.assertion(
        "Settings navigation sits above its General and Keys pages",
        harness.get_by_label("Settings").rect().bottom()
            < harness.get_by_label("Keys").rect().top(),
    );

    scenario.assertion(
        "Keys lists the shortcut groups",
        harness.query_by_label("CORE ACTIONS").is_some(),
    );

    harness.set_size(egui::vec2(640.0, 360.0));
    harness.run_steps(6);
    scenario.capture_screen("screen-004-short-keys", &mut harness);
    let short_tab = harness.get_by_label("Settings").rect();
    scenario.assertion(
        "short window keeps both levels of navigation visible",
        short_tab.bottom() < harness.get_by_label("Keys").rect().top()
            && short_tab.top() >= 40.0
            && harness.get_by_label("Keys").rect().bottom() < 360.0,
    );
    harness.get_by_label("General").click();
    harness.run_steps(6);
    harness.get_by_label("Update").scroll_to_me();
    harness.run_steps(6);
    scenario.capture_screen("screen-005-short-general-end", &mut harness);
    let update = harness.get_by_label("Update").rect();
    scenario.assertion(
        "short General page scrolls to Update without moving navigation",
        harness.get_by_label("Settings").rect() == short_tab
            && update.top() > harness.get_by_label("General").rect().bottom()
            && update.bottom() < 360.0,
    );
    harness.get_by_label("Boards").click();
    harness.run_steps(6);
    scenario.capture_screen("screen-006-short-boards", &mut harness);
    scenario.assertion(
        "short window returns to Boards with New board reachable",
        harness.state().sidebar_tab == SidebarTab::Boards
            && harness.get_by_label("Boards").rect().bottom()
                < harness.get_by_label("Board My Board").rect().top()
            && harness.get_by_label("New board").rect().bottom() < 360.0,
    );

    scenario.finish();
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

    scenario.finish();
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

    harness.key_press(egui::Key::Enter);
    harness.step();
    scenario.assertion(
        "Enter does not confirm a destructive dialog",
        harness.state().pending_board_delete.is_some(),
    );
    harness.key_press(egui::Key::Escape);
    harness.step();
    scenario.assertion(
        "Escape cancels and keeps the board",
        harness.state().pending_board_delete.is_none()
            && library
                .list_boards()
                .unwrap()
                .iter()
                .any(|board| board.id == second.id),
    );

    scenario.finish();
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

    scenario.finish();
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
    let before = harness.state().canvas.committed_placements();
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
        "Q produces the concrete windows-column arrangement",
        [
            (1, egui::pos2(-1044.5, 1405.5)),
            (2, egui::pos2(-698.5, 1405.5)),
            (43, egui::pos2(443.0, 3413.5)),
        ]
        .into_iter()
        .all(|(id, center)| {
            harness
                .state()
                .canvas
                .placement_for_id(id)
                .is_some_and(|p| p.state.geometry.center.distance(center) < 0.05)
        }),
    );
    check_compact_placements(&mut scenario, &before, harness.state());
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

    scenario.finish();
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
    let before = harness.state().canvas.committed_placements();
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
        "Q produces the concrete mixed-column arrangement",
        [
            (1, egui::pos2(72.0, 55.5)),
            (2, egui::pos2(139.5, 17.0)),
            (30, egui::pos2(255.0, -50.5)),
        ]
        .into_iter()
        .all(|(id, center)| {
            harness
                .state()
                .canvas
                .placement_for_id(id)
                .is_some_and(|p| p.state.geometry.center.distance(center) < 0.05)
        }),
    );
    check_compact_placements(&mut scenario, &before, harness.state());
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

    scenario.finish();
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
    let before = harness.state().canvas.committed_placements();
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
        "Q produces the concrete equal arrangement",
        [
            (1, egui::pos2(84.0, -102.0)),
            (2, egui::pos2(246.0, -102.0)),
            (3, egui::pos2(84.0, -50.0)),
            (4, egui::pos2(246.0, -50.0)),
            (5, egui::pos2(84.0, 2.0)),
            (6, egui::pos2(246.0, 2.0)),
        ]
        .into_iter()
        .all(|(id, center)| {
            harness
                .state()
                .canvas
                .placement_for_id(id)
                .is_some_and(|p| p.state.geometry.center.distance(center) < 0.05)
        }),
    );
    check_compact_placements(&mut scenario, &before, harness.state());
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

    scenario.finish();
}

#[test]
fn inspect_search_composer_writes_artifact() {
    use crate::search::palette::{ImageSearchPivot, SearchOverlayStatus};

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
    app.search
        .set_local_model_store_status_for_test(LocalModelStoreStatus::Installed);
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
    let mut all_opaque = true;

    settle(&mut harness);
    all_opaque &= scenario
        .capture_screen("screen-000-recents", &mut harness)
        .transparent_pixels
        == 0;
    scenario.assertion(
        "recents state displays all four saved searches",
        [
            "warm interior light",
            "brutalist stairwell",
            "fog over hills",
            "red chair",
        ]
        .into_iter()
        .all(|label| {
            inspect_displayed_text(&harness)
                .iter()
                .any(|text| text == label)
        }),
    );

    let search = &mut harness.state_mut().search;
    search.overlay.query = "warm light over hills".into();
    search.overlay.results = entries.clone();
    search.overlay.has_more_results = true;
    search.overlay.result_selection.replace(1);
    search.overlay.result_selection.toggle(2);
    let previews_ready = inspect_wait_for_search_previews(&mut harness);
    settle(&mut harness);
    all_opaque &= scenario
        .capture_screen("screen-001-results-selected", &mut harness)
        .transparent_pixels
        == 0;
    scenario.assertion(
        "results state displays the typed query",
        harness
            .get_by_role(egui::accesskit::Role::TextInput)
            .value()
            .as_deref()
            == Some("warm light over hills"),
    );
    let pixels = image::open(
        scenario
            .artifact_dir
            .join("frames/screen-001-results-selected.png"),
    )
    .unwrap()
    .to_rgba8();
    scenario.assertion(
        "results state renders each of the ten colored thumbnails",
        colors
            .iter()
            .all(|color| pixels.pixels().filter(|p| p.0 == *color).count() >= 500),
    );

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
    scenario.assertion(
        "empty results display the query and No results",
        harness
            .get_by_role(egui::accesskit::Role::TextInput)
            .value()
            .as_deref()
            == Some("purple giraffe on a bicycle")
            && harness.query_by_label("No results").is_some(),
    );

    let search = &mut harness.state_mut().search;
    search.overlay.query.clear();
    search.overlay.image_pivot = Some(ImageSearchPivot {
        asset_id: entries[0].asset_id,
        asset_preview_path: entries[0].asset_preview_path.clone(),
    });
    search.overlay.results = entries[1..].to_vec();
    search.overlay.result_selection.replace(0);
    let previews_ready = previews_ready && inspect_wait_for_search_previews(&mut harness);
    settle(&mut harness);
    all_opaque &= scenario
        .capture_screen("screen-003-similar", &mut harness)
        .transparent_pixels
        == 0;
    scenario.assertion(
        "similar state displays the image pivot and refinement input",
        inspect_displayed_text(&harness)
            .iter()
            .any(|text| text == "similar to #9000")
            && harness
                .get_by_role(egui::accesskit::Role::TextInput)
                .value()
                .as_deref()
                == Some(""),
    );

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
    scenario.assertion(
        "missing model displays the install card and its actions",
        ["Install Semantic Search", "Install", "Not now"]
            .into_iter()
            .all(|label| {
                inspect_displayed_text(&harness)
                    .iter()
                    .any(|text| text == label)
            }),
    );
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
    let displayed = inspect_displayed_text(&harness);
    scenario.assertion(
        "downloading displays installation progress and Cancel",
        ["Installing Semantic Search", "42%", "Cancel"]
            .into_iter()
            .all(|label| displayed.iter().any(|text| text == label)),
    );
    scenario.fact("composer.download-text", serde_json::json!(displayed));

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
    scenario.assertion(
        "failed install displays its error and Retry",
        ["Install failed", "network: timeout", "Retry"]
            .into_iter()
            .all(|label| {
                inspect_displayed_text(&harness)
                    .iter()
                    .any(|text| text == label)
            }),
    );

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
    scenario.finish();
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

    harness
        .state_mut()
        .canvas
        .apply_viewport_state(&crate::data::ViewportState {
            camera_pan_x: 0.0,
            camera_pan_y: 0.0,
            camera_zoom: 1.0,
            ..Default::default()
        });
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

    let dropped = harness
        .state()
        .canvas
        .committed_placements()
        .into_iter()
        .filter(|p| p.id() != blue.id())
        .collect::<Vec<_>>();
    let mut dropped_assets = dropped
        .iter()
        .map(|p| p.require_loaded_asset().id)
        .collect::<Vec<_>>();
    dropped_assets.sort_unstable();
    let mut expected_assets = result_placements
        .iter()
        .map(|p| p.require_loaded_asset().id)
        .collect::<Vec<_>>();
    expected_assets.sort_unstable();
    scenario.assertion(
        "drop keeps exactly the three selected asset identities",
        dropped_assets == expected_assets,
    );
    let pointer_world = egui::pos2(290.0, 210.0);
    scenario.assertion(
        "dropped batch stays centered at the drop pointer",
        harness
            .state()
            .canvas
            .selected_bounds_world()
            .is_some_and(|r| r.center().distance(pointer_world) < 0.1),
    );
    scenario.assertion(
        "dropped results keep their source dimensions",
        dropped.iter().all(|p| {
            result_placements.iter().any(|source| {
                source.require_loaded_asset().id == p.require_loaded_asset().id
                    && source.state.geometry.size == p.state.geometry.size
            })
        }),
    );
    scenario.assertion(
        "dropped results do not overlap",
        dropped.iter().enumerate().all(|(i, a)| {
            dropped[i + 1..].iter().all(|b| {
                let r = a.visual_bounds().intersect(b.visual_bounds());
                r.width() <= 0.01 || r.height() <= 0.01
            })
        }),
    );
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

    scenario.finish();
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
    scenario.assertion(
        "press frame shows the selection border",
        harness
            .state()
            .canvas
            .selection_border_visible(harness.ctx.input(|input| input.time)),
    );
    scenario.capture_click_hold_fact("screen-001-pressed-selected", &harness, placement_id);
    scenario.capture_screen("screen-001-pressed-selected", &mut harness);

    for index in 2..=8 {
        harness.step();
        let name = format!("screen-{index:03}-held");
        scenario.assertion(
            "each held frame keeps the selection border visible",
            harness
                .state()
                .canvas
                .selection_border_visible(harness.ctx.input(|input| input.time)),
        );
        scenario.assertion(
            "each held frame keeps the blue placement selected and stationary",
            harness.state().canvas.selected_placement_ids() == vec![placement_id]
                && harness
                    .state()
                    .canvas
                    .placement_for_id(placement_id)
                    .unwrap()
                    .state
                    .geometry
                    .center
                    == egui::Pos2::ZERO,
        );
        scenario.capture_click_hold_fact(&name, &harness, placement_id);
        scenario.capture_screen(&name, &mut harness);
    }

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

    scenario.finish();
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

    scenario.finish();
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
    scenario.assertion(
        "pointer rectangle crops to 100 by 70 pixels",
        (
            cropped.require_loaded_asset().width,
            cropped.require_loaded_asset().height,
        ) == (100, 70),
    );
    let pixels = image::open(&cropped.require_loaded_asset().original_path)
        .unwrap()
        .to_rgba8();
    scenario.assertion(
        "crop keeps the source rectangle pixels",
        pixels.get_pixel(0, 0).0 == [39, 96, 42, 255]
            && pixels.get_pixel(99, 69).0 == [197, 96, 189, 255],
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
    let durable = library.load_placements().unwrap();
    scenario.assertion(
        "explicit flush preserves exactly the loaded cropped Asset relation",
        durable.iter().all(Placement::is_loaded)
            && durable.iter().map(|p| p.asset_id()).collect::<Vec<_>>()
                == vec![Some(cropped_asset_id)],
    );

    scenario.finish();
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

    scenario.finish();
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
        scenario.finish();
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

    scenario.finish();
}

#[test]
fn keyboard_search_placement_keeps_an_acquired_snap_until_click() {
    let root = TempRoot::new("snappyref-keyboard-snap-retention");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let path = library
        .store_rgba_image("snap", &solid_rgba([60, 120, 200, 255], 100, 100), 100, 100)
        .unwrap();
    let asset = library
        .register_original(&path, "image/png", 100, 100, 40000, None)
        .unwrap();
    let anchor = library
        .fixture_placement(
            library.load_user_session().unwrap().active_board_id,
            Placement::loaded(
                crate::data::PlacementState::new(
                    library.allocate_document_id(),
                    egui::Pos2::ZERO,
                    1.0,
                    0.0,
                    0,
                    false,
                ),
                asset.clone(),
            ),
        )
        .unwrap();
    let anchor = inspect_prepared_loaded_placement(
        anchor,
        [60, 120, 200, 255],
        100,
        100,
        None,
        crate::image_preparation::ImportOrigin::Existing,
    );
    let mut app = test_app(Some(library.clone()));
    app.settings.semantic_search_first_launch_prompt_seen = true;
    app.settings.placement_gap = 24.0;
    app.settings.placement_target_area = 10_000.0;
    app.canvas.set_placement_gap(24.0);
    app.search
        .set_local_model_store_status_for_test(LocalModelStoreStatus::Installed);
    let mut harness = app_full_harness_with_startup(1.0 / 60.0, move |ctx| {
        app.canvas.add_prepared_placement_image(ctx, anchor);
        app
    });
    harness.state_mut().search.overlay.open = true;
    harness.state_mut().search.overlay.focus_requested = true;
    harness.step();
    harness.state_mut().search.overlay.results = vec![SearchResultAssetEntry {
        asset_id: asset.id,
        asset_preview_path: Some(path),
        asset_width: 100,
        asset_height: 100,
    }];
    harness.state_mut().search.overlay.status = crate::search::palette::SearchOverlayStatus::Ready;
    harness
        .state_mut()
        .search
        .overlay
        .result_selection
        .replace(0);
    harness.step();
    // The test screen maps world (0, 0) to screen (600, 400) at zoom 1.
    harness.hover_at(egui::pos2(722.0, 402.0));
    harness.key_press(egui::Key::Enter);
    harness.step();
    harness.step();
    let center = |harness: &Harness<'_, SnappyRefApp>| {
        harness
            .state()
            .canvas
            .selected_bounds_world()
            .unwrap()
            .center()
    };
    assert_eq!(center(&harness), egui::pos2(124.0, 0.0));
    // 10px away is past the 8px acquire distance but within the 12px release distance.
    harness.hover_at(egui::pos2(734.0, 410.0));
    harness.step();
    harness.step();
    assert_eq!(center(&harness), egui::pos2(124.0, 0.0));
    inspect_click_at(&harness, egui::pos2(734.0, 410.0), egui::Modifiers::NONE);
    harness.step();
    harness.step();
    assert_eq!(center(&harness), egui::pos2(124.0, 0.0));
    assert_eq!(harness.state().canvas.guide_snap_indicator_count(), 0);
}

#[test]
fn inspect_search_keyboard_placement_writes_artifact() {
    let mut scenario = InspectScenario::new("search-keyboard-placement");
    let root = TempRoot::new("snappyref-search-keyboard-placement");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let entries = (0..6)
        .map(|index| {
            let path = library
                .store_rgba_image(
                    &format!("keyboard-{index}"),
                    &solid_rgba([40 + index * 25, 100, 180, 255], 80, 60),
                    80,
                    60,
                )
                .unwrap();
            let asset = library
                .register_original(&path, "image/png", 80, 60, 19200, None)
                .unwrap();
            SearchResultAssetEntry {
                asset_id: asset.id,
                asset_preview_path: Some(path),
                asset_width: 80,
                asset_height: 60,
            }
        })
        .collect::<Vec<_>>();
    let mut app = test_app(Some(library.clone()));
    app.settings.semantic_search_first_launch_prompt_seen = true;
    app.search
        .set_local_model_store_status_for_test(LocalModelStoreStatus::Installed);
    app.canvas
        .apply_viewport_state(&crate::data::ViewportState {
            camera_pan_x: 0.0,
            camera_pan_y: 0.0,
            camera_zoom: 1.0,
            ..Default::default()
        });
    let mut harness = app_full_harness(app);
    harness.state_mut().search.overlay.open = true;
    harness.state_mut().search.overlay.focus_requested = true;
    harness.step();
    harness.state_mut().search.overlay.results = entries.clone();
    harness.state_mut().search.overlay.status = crate::search::palette::SearchOverlayStatus::Ready;
    harness
        .state_mut()
        .search
        .overlay
        .result_selection
        .replace(0);
    harness.step();
    scenario.capture_screen("screen-000-results", &mut harness);
    harness.key_press(egui::Key::ArrowRight);
    harness.step();
    scenario.assertion(
        "Right selects the second result",
        harness.state().search.overlay.result_selection.selected() == [1],
    );
    harness.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::ArrowRight);
    harness.step();
    scenario.assertion(
        "Shift-Right extends selection to second and third results",
        harness.state().search.overlay.result_selection.selected() == [1, 2],
    );
    harness.key_press(egui::Key::ArrowDown);
    harness.step();
    scenario.assertion(
        "Down selects the final result on the second row",
        harness.state().search.overlay.result_selection.selected() == [5],
    );
    harness.key_press(egui::Key::ArrowUp);
    harness.step();
    scenario.assertion(
        "Up returns to the first row",
        harness.state().search.overlay.result_selection.selected() == [0],
    );
    harness.key_press(egui::Key::ArrowRight);
    harness.step();
    harness.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::ArrowRight);
    harness.step();
    scenario.capture_screen("screen-001-keyboard-selection", &mut harness);
    harness.hover_at(egui::pos2(900.0, 600.0));
    harness.key_press(egui::Key::Enter);
    harness.step();
    let provisional = harness
        .state()
        .canvas
        .provisional_placements_in_selection_order();
    scenario.assertion(
        "Enter picks up the selected second and third assets",
        provisional.iter().map(|p| p.asset_id()).collect::<Vec<_>>()
            == vec![Some(entries[1].asset_id), Some(entries[2].asset_id)],
    );
    scenario.capture_screen("screen-002-enter-provisional", &mut harness);
    harness.hover_at(egui::pos2(820.0, 520.0));
    harness.step();
    harness.step();
    scenario.assertion(
        "Enter preview follows the moved cursor before a click",
        harness
            .state()
            .canvas
            .selected_bounds_world()
            .is_some_and(|b| b.center().distance(egui::pos2(220.0, 120.0)) < 0.1),
    );
    scenario.capture_screen("screen-002-enter-following-cursor", &mut harness);
    harness.key_press(egui::Key::Escape);
    harness.step();
    harness.state_mut().flush_persistence();
    scenario.assertion(
        "Escape cancels the picked-up batch without adding content",
        harness.state().canvas.all_placement_ids().is_empty()
            && library.load_placements().unwrap().is_empty(),
    );
    scenario.capture_screen("screen-003-canceled", &mut harness);

    harness.state_mut().search.overlay.open = true;
    harness.state_mut().search.overlay.focus_requested = true;
    harness.step();
    harness.state_mut().search.overlay.results = entries.clone();
    harness.state_mut().search.overlay.status = crate::search::palette::SearchOverlayStatus::Ready;
    harness
        .state_mut()
        .search
        .overlay
        .result_selection
        .replace(1);
    harness
        .state_mut()
        .search
        .overlay
        .result_selection
        .toggle(2);
    harness.step();
    harness.key_press(egui::Key::Enter);
    harness.step();
    harness.hover_at(egui::pos2(860.0, 560.0));
    harness.step();
    harness.step();
    scenario.assertion(
        "selected keyboard preview follows the cursor before commit",
        harness
            .state()
            .canvas
            .selected_bounds_world()
            .is_some_and(|b| b.center().distance(egui::pos2(260.0, 160.0)) < 0.1),
    );
    inspect_click_at(&harness, egui::pos2(900.0, 600.0), egui::Modifiers::NONE);
    harness.step();
    harness.state_mut().flush_persistence();
    let stored = library.load_placements().unwrap();
    scenario.assertion(
        "click places exactly the selected image identities",
        stored.iter().all(Placement::is_loaded)
            && stored.iter().map(|p| p.asset_id()).collect::<Vec<_>>()
                == vec![Some(entries[1].asset_id), Some(entries[2].asset_id)],
    );
    scenario.assertion(
        "keyboard drop has the literal pointer-centered geometry",
        harness
            .state()
            .canvas
            .selected_bounds_world()
            .is_some_and(|b| b.center().distance(egui::pos2(300.0, 200.0)) < 0.1),
    );
    scenario.fact(
        "keyboard.stored-placements",
        serde_json::json!(stored.iter().map(inspect_placement).collect::<Vec<_>>()),
    );
    scenario.capture_screen("screen-004-placed", &mut harness);
    harness.key_press_modifiers(inspect_command_modifier(), egui::Key::Z);
    harness.step();
    scenario.assertion(
        "one Undo removes the committed batch",
        harness.state().canvas.all_placement_ids().is_empty(),
    );

    harness.state_mut().search.overlay.open = true;
    harness.state_mut().search.overlay.focus_requested = true;
    harness.step();
    harness.state_mut().search.overlay.results = entries.clone();
    harness.state_mut().search.overlay.status = crate::search::palette::SearchOverlayStatus::Ready;
    harness
        .state_mut()
        .search
        .overlay
        .result_selection
        .replace(0);
    harness.step();
    harness.key_press_modifiers(inspect_command_modifier(), egui::Key::Enter);
    harness.step();
    scenario.assertion(
        "Mod-Enter picks up all six assets",
        harness
            .state()
            .canvas
            .provisional_placements_in_selection_order()
            .iter()
            .map(|p| p.asset_id())
            .collect::<Vec<_>>()
            == entries.iter().map(|e| Some(e.asset_id)).collect::<Vec<_>>(),
    );
    harness.hover_at(egui::pos2(780.0, 500.0));
    harness.step();
    harness.step();
    scenario.assertion(
        "Mod-Enter preview follows the moved cursor before a click",
        harness
            .state()
            .canvas
            .selected_bounds_world()
            .is_some_and(|b| b.center().distance(egui::pos2(180.0, 100.0)) < 0.1),
    );
    scenario.capture_screen("screen-005-all-results-following-cursor", &mut harness);
    harness.key_press(egui::Key::Escape);
    harness.step();
    harness.key_press_modifiers(
        egui::Modifiers {
            shift: true,
            ..inspect_command_modifier()
        },
        egui::Key::Z,
    );
    harness.step();
    scenario.assertion(
        "cancel adds no history entry and Redo restores the earlier committed batch",
        harness
            .state()
            .canvas
            .committed_placements()
            .iter()
            .map(|p| p.asset_id())
            .collect::<Vec<_>>()
            == vec![Some(entries[1].asset_id), Some(entries[2].asset_id)],
    );
    scenario.capture_screen("screen-005-redone-after-cancel", &mut harness);
    harness.key_press_modifiers(inspect_command_modifier(), egui::Key::Z);
    harness.step();
    harness.state_mut().search.overlay.open = true;
    harness.state_mut().search.overlay.focus_requested = true;
    harness.step();
    harness.state_mut().search.overlay.results = entries.clone();
    harness.state_mut().search.overlay.status = crate::search::palette::SearchOverlayStatus::Ready;
    harness.step();
    harness.key_press_modifiers(inspect_command_modifier(), egui::Key::Enter);
    harness.step();
    inspect_click_at(&harness, egui::pos2(850.0, 580.0), egui::Modifiers::NONE);
    harness.step();
    harness.state_mut().flush_persistence();
    scenario.assertion(
        "all-results placements are durable loaded content",
        library
            .load_placements()
            .unwrap()
            .iter()
            .all(Placement::is_loaded),
    );
    scenario.assertion(
        "Mod-Enter and click commit each result once",
        library
            .load_placements()
            .unwrap()
            .iter()
            .map(|p| p.asset_id())
            .collect::<Vec<_>>()
            == vec![Some(1), Some(2), Some(3), Some(4), Some(5), Some(6)],
    );
    scenario.assertion(
        "all-results drop follows its literal pointer center",
        harness
            .state()
            .canvas
            .selected_bounds_world()
            .is_some_and(|b| b.center().distance(egui::pos2(250.0, 180.0)) < 0.1),
    );
    scenario.capture_screen("screen-006-all-results-placed", &mut harness);
    harness.key_press_modifiers(inspect_command_modifier(), egui::Key::Z);
    harness.step();
    harness.state_mut().flush_persistence();
    drop(harness);
    let mut app = test_app(Some(library.clone()));
    app.settings.semantic_search_first_launch_prompt_seen = true;
    app.search
        .set_local_model_store_status_for_test(LocalModelStoreStatus::Installed);
    app.search
        .set_recent_searches(vec!["stone arches".into(), "blue chairs".into()]);
    app.search.overlay.open = true;
    app.search.overlay.focus_requested = true;
    let mut harness = app_full_harness(app);
    harness.step();
    scenario.capture_screen("screen-007-recents-before-keyboard", &mut harness);
    harness.key_press(egui::Key::ArrowDown);
    harness.step();
    harness.key_press(egui::Key::Enter);
    harness.step();
    scenario.assertion(
        "recent arrows and Enter restore blue chairs",
        harness.state().search.overlay.query == "blue chairs",
    );
    scenario.capture_screen("screen-007-keyboard-recent", &mut harness);
    harness.key_press(egui::Key::Escape);
    harness.step();
    harness.state_mut().search.overlay.open = true;
    harness.state_mut().search.overlay.focus_requested = true;
    harness.step();
    inspect_click_at(&harness, egui::pos2(350.0, 192.0), egui::Modifiers::NONE);
    harness.step();
    harness.state_mut().flush_persistence();
    scenario.assertion(
        "clicking the second recent restores stone arches and persists recency",
        harness.state().search.overlay.query == "stone arches"
            && library.load_user_session().unwrap().recent_searches
                == vec!["stone arches", "blue chairs"],
    );
    scenario.capture_screen("screen-008-clicked-recent", &mut harness);
    scenario.finish();
}

#[test]
fn inspect_sidebar_board_shortcuts_writes_artifact() {
    fn board_key(harness: &mut Harness<'_, SnappyRefApp>, key: egui::Key) {
        let modifiers = inspect_command_modifier();
        for pressed in [true, false] {
            harness.event_modifiers(
                egui::Event::Key {
                    key,
                    physical_key: Some(key),
                    pressed,
                    repeat: false,
                    modifiers,
                },
                modifiers,
            );
        }
        harness.step();
        harness.event(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
        harness.step();
    }

    let mut scenario = InspectScenario::new("sidebar-board-shortcuts");
    let root = TempRoot::new("snappyref-inspect-sidebar-board-shortcuts");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let first = library.list_boards().unwrap()[0].clone();
    let second = library.fixture_board("References").unwrap();
    for name in [
        "Third", "Fourth", "Fifth", "Sixth", "Seventh", "Eighth", "Ninth", "Tenth",
    ] {
        library.fixture_board(name).unwrap();
    }
    let mut app = test_app(Some(library.clone()));
    app.sidebar_open = true;
    app.settings.semantic_search_first_launch_prompt_seen = true;
    let mut harness = app_full_harness_with_step_dt(app, 1.0 / 60.0);
    harness.run_steps(12);
    scenario.capture_screen("screen-000-open", &mut harness);

    // Clicking a board gives its button focus. Navigation must still work.
    inspect_click_at(
        &harness,
        harness.get_by_label("Board References").rect().center(),
        egui::Modifiers::NONE,
    );
    harness.run_steps(4);
    scenario.assertion(
        "clicking References opens that board",
        harness.state().active_board_id == second.id,
    );
    harness.key_press(egui::Key::Tab);
    harness.step();
    scenario.fact("focused-board-button", serde_json::json!({"keyboard_input": harness.ctx.egui_wants_keyboard_input(), "text_edit": harness.ctx.text_edit_focused()}));
    board_key(&mut harness, egui::Key::Num1);
    harness.run_steps(4);
    scenario.capture_screen("screen-001-command-1", &mut harness);
    scenario.assertion(
        "Command-1 switches with a keyboard-focused board button",
        harness.state().active_board_id == first.id && harness.state().sidebar_open,
    );

    for (key, name) in [
        (egui::Key::Num2, "References"),
        (egui::Key::Num3, "Third"),
        (egui::Key::Num4, "Fourth"),
        (egui::Key::Num5, "Fifth"),
        (egui::Key::Num6, "Sixth"),
        (egui::Key::Num7, "Seventh"),
        (egui::Key::Num8, "Eighth"),
        (egui::Key::Num9, "Ninth"),
        (egui::Key::Num0, "Tenth"),
    ] {
        board_key(&mut harness, key);
        harness.run_steps(4);
        let active = harness
            .state()
            .board_summaries()
            .iter()
            .find(|s| s.board.id == harness.state().active_board_id)
            .unwrap()
            .board
            .name
            .clone();
        scenario.fact(
            "digit-switch",
            serde_json::json!({"key": format!("{key:?}"), "expected": name, "active": active}),
        );
        scenario.assertion(
            "each digit opens its board with the sidebar open",
            active == name,
        );
    }

    inspect_click_at(
        &harness,
        harness.get_by_label("Settings").rect().center(),
        egui::Modifiers::NONE,
    );
    harness.run_steps(12);
    board_key(&mut harness, egui::Key::Num2);
    harness.run_steps(4);
    scenario.capture_screen("screen-002-settings-switch", &mut harness);
    scenario.assertion(
        "Command-2 switches after a Settings tab click",
        harness.state().active_board_id == second.id
            && harness.state().sidebar_tab == SidebarTab::Settings,
    );
    inspect_click_at(
        &harness,
        harness.get_by_label("Keys").rect().center(),
        egui::Modifiers::NONE,
    );
    harness.run_steps(12);
    board_key(&mut harness, egui::Key::Num1);
    harness.run_steps(4);
    scenario.assertion(
        "Command-1 switches after a Keys tab click",
        harness.state().active_board_id == first.id,
    );
    inspect_click_at(
        &harness,
        harness.get_by_label("Boards").rect().center(),
        egui::Modifiers::NONE,
    );
    harness.run_steps(12);

    inspect_click_at(
        &harness,
        harness.get_by_label("Board References").rect().center(),
        egui::Modifiers::NONE,
    );
    let pos = harness.get_by_label("Board References").rect().center();
    inspect_double_click_at(&mut harness, pos);
    harness.step();
    harness.get_by_label("Board name");
    harness.key_press_modifiers(inspect_command_modifier(), egui::Key::A);
    harness.step();
    harness.event(egui::Event::Text("Renamed references".to_owned()));
    harness.step();
    let rename_board = harness.state().active_board_id;
    board_key(&mut harness, egui::Key::Num1);
    harness.step();
    scenario.capture_screen("screen-003-rename-keeps-keys", &mut harness);
    scenario.assertion(
        "the rename field keeps Command-1 without switching",
        harness.state().active_board_id == rename_board
            && harness.get_by_label("Board name").value() == Some("Renamed references".to_owned()),
    );
    harness.key_press(egui::Key::Enter);
    harness.run_steps(4);
    scenario.assertion(
        "Enter commits the typed board name",
        harness
            .state()
            .board_summaries()
            .iter()
            .find(|s| s.board.id == second.id)
            .unwrap()
            .board
            .name
            == "Renamed references",
    );
    board_key(&mut harness, egui::Key::Num1);
    harness.run_steps(4);
    scenario.assertion(
        "board switching resumes after the rename",
        harness.state().active_board_id == first.id,
    );
    scenario.capture_screen("screen-004-renamed", &mut harness);
    scenario.finish();
}
