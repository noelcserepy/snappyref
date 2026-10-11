use super::*;

#[test]
fn committed_loading_and_failed_placements_persist_load_and_stack() {
    let root = TempRoot::new("snappyref-unloaded-placement-content-test");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let loading = library
        .fixture_placement(
            library.load_user_session().unwrap().active_board_id,
            library.new_committed_loading_placement(
                egui::pos2(12.0, 24.0),
                2.0,
                eframe::egui::vec2(320.0, 180.0),
                ContentResolutionSource::ExternalPath {
                    path: "/tmp/source.png".to_owned(),
                },
                Some("Queued".to_owned()),
            ),
        )
        .unwrap();
    let failed = library
        .fixture_placement(
            library.load_user_session().unwrap().active_board_id,
            crate::data::Placement {
                state: crate::data::PlacementState::with_display_size(
                    library.allocate_document_id(),
                    egui::pos2(48.0, 96.0),
                    eframe::egui::vec2(120.0, 90.0),
                    1.0,
                    0.0,
                    0,
                    false,
                ),
                content: crate::data::PlacementContent::Failed {
                    source: Some(ContentResolutionSource::Unknown {
                        description: "clipboard".to_owned(),
                    }),
                    message: "Could not read image".to_owned(),
                    recoverable: true,
                },
                lifecycle: crate::data::PlacementLifecycle::Committed,
            },
        )
        .unwrap();

    let reloaded = library.load_placements().unwrap();
    assert_eq!(reloaded.len(), 2);
    assert!(matches!(
        reloaded[0].content,
        PlacementContent::Loading { .. }
    ));
    assert!(matches!(
        reloaded[1].content,
        PlacementContent::Failed { .. }
    ));
    assert_eq!(reloaded[0].state.geometry.size.x, 320.0);
    assert_eq!(reloaded[1].state.geometry.size.y, 90.0);

    assert_eq!(
        library.load_canvas_stack_order().unwrap(),
        vec![
            CanvasStackItem::Placement(loading.state.id),
            CanvasStackItem::Placement(failed.state.id),
        ]
    );
}

#[test]
fn legacy_board_items_asset_id_not_null_migrates_for_loading_placements() {
    let root = TempRoot::new("snappyref-board-item-asset-nullability-test");
    let db_path = root.join("library.db");
    let connection = Connection::open(&db_path).unwrap();
    connection
        .execute_batch(
            "
            CREATE TABLE assets (
                id INTEGER PRIMARY KEY,
                original_path TEXT NOT NULL UNIQUE,
                filename TEXT NOT NULL,
                mime_type TEXT NOT NULL,
                width INTEGER NOT NULL,
                height INTEGER NOT NULL,
                file_size INTEGER NOT NULL,
                imported_at INTEGER NOT NULL
            );
            CREATE TABLE board_items (
                id INTEGER PRIMARY KEY,
                asset_id INTEGER NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
                x REAL NOT NULL,
                y REAL NOT NULL,
                scale REAL NOT NULL,
                rotation REAL NOT NULL,
                z_index INTEGER NOT NULL,
                archived INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE user_session (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                camera_pan_x REAL NOT NULL,
                camera_pan_y REAL NOT NULL,
                camera_zoom REAL NOT NULL
            );
            INSERT INTO user_session (id, camera_pan_x, camera_pan_y, camera_zoom)
            VALUES (1, 0.0, 0.0, 1.0);
            ",
        )
        .unwrap();
    drop(connection);

    let library = Library::initialize(root.to_path_buf()).unwrap();
    let migrated = Connection::open(&db_path).unwrap();
    let not_null = board_items_column_not_null(&migrated, "asset_id");
    drop(migrated);

    assert!(!not_null);
    let loading = library
        .fixture_placement(
            library.load_user_session().unwrap().active_board_id,
            library.new_committed_loading_placement(
                egui::pos2(12.0, 24.0),
                1.0,
                eframe::egui::vec2(320.0, 180.0),
                ContentResolutionSource::ExternalPath {
                    path: "C:/Users/Admin/Pictures/drop.png".to_owned(),
                },
                Some("Loading".to_owned()),
            ),
        )
        .unwrap();

    assert_eq!(loading.asset_id(), None);
    assert!(matches!(loading.content, PlacementContent::Loading { .. }));
}

#[test]
fn new_library_has_default_non_archived_active_board() {
    let root = TempRoot::new("snappyref-default-board-test");
    let library = Library::initialize(root.to_path_buf()).unwrap();

    let boards = library.list_boards().unwrap();
    let session = library.load_user_session().unwrap();

    assert_eq!(boards.len(), 1);
    assert_eq!(boards[0].name, "My Board");
    assert_eq!(session.active_board_id, boards[0].id);
}

#[test]
fn existing_single_canvas_data_migrates_to_my_board() {
    let root = TempRoot::new("snappyref-board-migration-test");
    let db_path = root.join("library.db");
    let connection = Connection::open(&db_path).unwrap();
    connection
        .execute_batch(
            "
            CREATE TABLE assets (
                id INTEGER PRIMARY KEY,
                original_path TEXT NOT NULL UNIQUE,
                filename TEXT NOT NULL,
                mime_type TEXT NOT NULL,
                width INTEGER NOT NULL,
                height INTEGER NOT NULL,
                file_size INTEGER NOT NULL,
                imported_at INTEGER NOT NULL
            );
            CREATE TABLE board_items (
                id INTEGER PRIMARY KEY,
                asset_id INTEGER NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
                x REAL NOT NULL,
                y REAL NOT NULL,
                scale REAL NOT NULL,
                rotation REAL NOT NULL,
                z_index INTEGER NOT NULL,
                archived INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE canvas_annotations (
                id INTEGER PRIMARY KEY,
                kind TEXT NOT NULL,
                start_x REAL NOT NULL,
                start_y REAL NOT NULL,
                end_x REAL NOT NULL,
                end_y REAL NOT NULL,
                text TEXT NOT NULL DEFAULT '',
                text_size TEXT NOT NULL DEFAULT 'm',
                stroke_width TEXT NOT NULL DEFAULT 'm',
                color TEXT NOT NULL DEFAULT 'default',
                background_enabled INTEGER NOT NULL DEFAULT 0,
                archived INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE user_session (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                camera_pan_x REAL NOT NULL,
                camera_pan_y REAL NOT NULL,
                camera_zoom REAL NOT NULL,
                auto_image_scaling INTEGER NOT NULL DEFAULT 1,
                placement_gap REAL NOT NULL DEFAULT 24.0,
                canvas_background TEXT NOT NULL DEFAULT 'none',
                semantic_search_enabled INTEGER NOT NULL DEFAULT 1,
                canvas_image_mode TEXT NOT NULL DEFAULT 'downscale',
                canvas_image_max_side INTEGER NOT NULL DEFAULT 2000,
                rotation_snap_degrees REAL NOT NULL DEFAULT 15.0,
                default_textbox_background INTEGER NOT NULL DEFAULT 0
            );
            INSERT INTO assets (
                id, original_path, filename, mime_type, width, height, file_size, imported_at
            ) VALUES (11, '/tmp/legacy.png', 'legacy.png', 'image/png', 100, 80, 4, 123);
            INSERT INTO board_items (
                id, asset_id, x, y, scale, rotation, z_index, archived
            ) VALUES (21, 11, 10.0, 20.0, 0.5, 12.0, 3, 0);
            INSERT INTO canvas_annotations (
                id, kind, start_x, start_y, end_x, end_y, text, text_size, stroke_width, color, background_enabled, archived
            ) VALUES (31, 'textbox', 1.0, 2.0, 101.0, 52.0, 'note', 'l', 's', 'sky', 1, 0);
            INSERT INTO user_session (
                id, camera_pan_x, camera_pan_y, camera_zoom, auto_image_scaling, placement_gap,
                canvas_background, semantic_search_enabled, canvas_image_mode,
                canvas_image_max_side, rotation_snap_degrees, default_textbox_background
            ) VALUES (1, 44.0, 55.0, 1.25, 0, 37.0, 'grid', 0, 'original', 4096, 30.0, 1);
            ",
        )
        .unwrap();
    drop(connection);

    let library = Library::initialize(root.to_path_buf()).unwrap();
    let boards = library.list_boards().unwrap();
    let session = library.load_user_session().unwrap();
    let items = library.load_placements().unwrap();
    let annotations = library.load_canvas_annotations().unwrap();

    assert_eq!(boards.len(), 1);
    assert_eq!(boards[0].name, "My Board");
    assert_eq!(session.active_board_id, boards[0].id);
    assert_eq!(session.viewport.camera_pan_x, 44.0);
    assert_eq!(session.viewport.camera_pan_y, 55.0);
    assert_eq!(session.viewport.camera_zoom, 1.25);
    assert_eq!(session.viewport.canvas_background, CanvasBackground::Grid);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].state.id, 21);
    assert_eq!(annotations.len(), 1);
    assert_eq!(annotations[0].id, 31);
    assert_eq!(annotations[0].text, "note");
}

#[test]
fn existing_canvas_data_migrates_to_unified_stack_with_annotations_above_placements() {
    let root = TempRoot::new("snappyref-stack-order-migration-test");
    let db_path = root.join("library.db");
    let connection = Connection::open(&db_path).unwrap();
    connection
        .execute_batch(
            "
            CREATE TABLE assets (
                id INTEGER PRIMARY KEY,
                original_path TEXT NOT NULL UNIQUE,
                filename TEXT NOT NULL,
                mime_type TEXT NOT NULL,
                width INTEGER NOT NULL,
                height INTEGER NOT NULL,
                file_size INTEGER NOT NULL,
                imported_at INTEGER NOT NULL
            );
            CREATE TABLE board_items (
                id INTEGER PRIMARY KEY,
                asset_id INTEGER NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
                x REAL NOT NULL,
                y REAL NOT NULL,
                scale REAL NOT NULL,
                rotation REAL NOT NULL,
                z_index INTEGER NOT NULL,
                archived INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE canvas_annotations (
                id INTEGER PRIMARY KEY,
                kind TEXT NOT NULL,
                start_x REAL NOT NULL,
                start_y REAL NOT NULL,
                end_x REAL NOT NULL,
                end_y REAL NOT NULL,
                text TEXT NOT NULL DEFAULT '',
                text_size TEXT NOT NULL DEFAULT 'm',
                stroke_width TEXT NOT NULL DEFAULT 'm',
                color TEXT NOT NULL DEFAULT 'default',
                background_enabled INTEGER NOT NULL DEFAULT 0,
                archived INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE user_session (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                camera_pan_x REAL NOT NULL,
                camera_pan_y REAL NOT NULL,
                camera_zoom REAL NOT NULL
            );
            INSERT INTO assets (
                id, original_path, filename, mime_type, width, height, file_size, imported_at
            ) VALUES
                (11, '/tmp/first.png', 'first.png', 'image/png', 100, 80, 4, 123),
                (12, '/tmp/second.png', 'second.png', 'image/png', 100, 80, 4, 124);
            INSERT INTO board_items (
                id, asset_id, x, y, scale, rotation, z_index, archived
            ) VALUES
                (21, 11, 10.0, 20.0, 1.0, 0.0, 7, 0),
                (22, 12, 20.0, 20.0, 1.0, 0.0, 3, 0);
            INSERT INTO canvas_annotations (
                id, kind, start_x, start_y, end_x, end_y, text, archived
            ) VALUES
                (31, 'textbox', 1.0, 2.0, 101.0, 52.0, 'first note', 0),
                (32, 'arrow', 3.0, 4.0, 103.0, 54.0, '', 0);
            INSERT INTO user_session (id, camera_pan_x, camera_pan_y, camera_zoom)
            VALUES (1, 0.0, 0.0, 1.0);
            ",
        )
        .unwrap();
    drop(connection);

    let library = Library::initialize(root.to_path_buf()).unwrap();
    let stack = library.load_canvas_stack_order().unwrap();

    assert_eq!(
        stack,
        vec![
            CanvasStackItem::Placement(22),
            CanvasStackItem::Placement(21),
            CanvasStackItem::Annotation(31),
            CanvasStackItem::Annotation(32),
        ]
    );
}

#[test]
fn created_annotations_round_trip_through_canvas_stack_order() {
    let root = TempRoot::new("snappyref-stack-order-annotation-test");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let path = library
        .store_rgba_image("placed", &[255, 0, 0, 255], 1, 1)
        .unwrap();
    let placement = library
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
                    .register_original(&path, "image/png", 1, 1, 4, None)
                    .unwrap(),
            ),
        )
        .unwrap();
    let annotation = library
        .fixture_annotation(
            CanvasAnnotationKind::Textbox,
            egui::pos2(10.0, 12.0),
            egui::pos2(110.0, 72.0),
            "hello",
            CanvasTextSize::default(),
            crate::data::CanvasStrokeWidth::default(),
            crate::data::CanvasAnnotationColor::default(),
        )
        .unwrap();

    drop(library);
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let stack = library.load_canvas_stack_order().unwrap();

    assert_eq!(
        stack,
        vec![
            CanvasStackItem::Placement(placement.state.id),
            CanvasStackItem::Annotation(annotation.id),
        ]
    );
}

#[test]
fn saved_canvas_stack_order_round_trips_for_active_board() {
    let root = TempRoot::new("snappyref-stack-order-save-test");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let first_path = library
        .store_rgba_image("first", &[255, 0, 0, 255], 1, 1)
        .unwrap();
    let second_path = library
        .store_rgba_image("second", &[0, 255, 0, 255], 1, 1)
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
                    .register_original(&first_path, "image/png", 1, 1, 4, None)
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
                    egui::pos2(10.0, 0.0),
                    1.0,
                    0.0,
                    0,
                    false,
                ),
                library
                    .register_original(&second_path, "image/png", 1, 1, 4, None)
                    .unwrap(),
            ),
        )
        .unwrap();
    let annotation = library
        .fixture_annotation(
            CanvasAnnotationKind::Textbox,
            egui::pos2(10.0, 12.0),
            egui::pos2(110.0, 72.0),
            "hello",
            CanvasTextSize::default(),
            crate::data::CanvasStrokeWidth::default(),
            crate::data::CanvasAnnotationColor::default(),
        )
        .unwrap();

    let saved = vec![
        CanvasStackItem::Annotation(annotation.id),
        CanvasStackItem::Placement(second.state.id),
        CanvasStackItem::Placement(first.state.id),
    ];
    library
        .fixture_board_document(
            library.load_user_session().unwrap().active_board_id,
            crate::canvas::CanvasDocumentChanges {
                stack_order: Some(saved.to_vec()),
                ..Default::default()
            },
        )
        .unwrap();

    drop(library);
    let library = Library::initialize(root.to_path_buf()).unwrap();
    assert_eq!(library.load_canvas_stack_order().unwrap(), saved);
}

#[test]
fn active_board_persists_and_scopes_items_annotations_and_viewport() {
    let root = TempRoot::new("snappyref-board-scope-test");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let first_board = library.list_boards().unwrap()[0].clone();
    let first_path = library
        .store_rgba_image("first", &[255, 0, 0, 255], 1, 1)
        .unwrap();
    let first_item = library
        .fixture_placement(
            library.load_user_session().unwrap().active_board_id,
            crate::data::Placement::loaded(
                crate::data::PlacementState::new(
                    library.allocate_document_id(),
                    egui::pos2(10.0, 0.0),
                    1.0,
                    0.0,
                    0,
                    false,
                ),
                library
                    .register_original(&first_path, "image/png", 1, 1, 4, None)
                    .unwrap(),
            ),
        )
        .unwrap();
    let first_annotation = library
        .fixture_annotation(
            CanvasAnnotationKind::Textbox,
            egui::pos2(0.0, 0.0),
            egui::pos2(50.0, 20.0),
            "first board",
            CanvasTextSize::default(),
            crate::data::CanvasStrokeWidth::default(),
            crate::data::CanvasAnnotationColor::default(),
        )
        .unwrap();

    let second_board = library.fixture_board("Second").unwrap();
    let second_session_before_save = library.load_user_session().unwrap();
    assert_eq!(
        second_session_before_save.viewport.camera_zoom,
        crate::data::DEFAULT_VIEWPORT_ZOOM
    );
    library.set_active_board(second_board.id).unwrap();
    let second_path = library
        .store_rgba_image("second", &[0, 255, 0, 255], 1, 1)
        .unwrap();
    let second_item = library
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
                    .register_original(&second_path, "image/png", 1, 1, 4, None)
                    .unwrap(),
            ),
        )
        .unwrap();
    let second_annotation = library
        .fixture_annotation(
            CanvasAnnotationKind::Textbox,
            egui::pos2(100.0, 0.0),
            egui::pos2(150.0, 20.0),
            "second board",
            CanvasTextSize::default(),
            crate::data::CanvasStrokeWidth::default(),
            crate::data::CanvasAnnotationColor::default(),
        )
        .unwrap();
    let mut session = library.load_user_session().unwrap();
    session.viewport.camera_pan_x = 200.0;
    session.viewport.camera_pan_y = 300.0;
    session.viewport.camera_zoom = 2.0;
    session.viewport.canvas_background = CanvasBackground::Dots;
    library.save_user_session(&session).unwrap();

    let second_items = library.load_placements().unwrap();
    let second_annotations = library.load_canvas_annotations().unwrap();
    let reopened = library.load_user_session().unwrap();
    assert_eq!(reopened.active_board_id, second_board.id);
    assert_eq!(reopened.viewport.camera_pan_x, 200.0);
    assert_eq!(reopened.viewport.canvas_background, CanvasBackground::Dots);
    assert_eq!(second_items.len(), 1);
    assert_eq!(second_items[0].state.id, second_item.state.id);
    assert_eq!(second_annotations.len(), 1);
    assert_eq!(second_annotations[0].id, second_annotation.id);

    library.set_active_board(first_board.id).unwrap();
    let first_items = library.load_placements().unwrap();
    let first_annotations = library.load_canvas_annotations().unwrap();
    let reopened_first = library.load_user_session().unwrap();
    assert_eq!(reopened_first.active_board_id, first_board.id);
    assert_eq!(reopened_first.viewport.camera_pan_x, 0.0);
    assert_eq!(
        reopened_first.viewport.camera_zoom,
        crate::data::DEFAULT_VIEWPORT_ZOOM
    );
    assert_eq!(
        reopened_first.viewport.canvas_background,
        CanvasBackground::None
    );
    assert_eq!(first_items.len(), 1);
    assert_eq!(first_items[0].state.id, first_item.state.id);
    assert_eq!(first_annotations.len(), 1);
    assert_eq!(first_annotations[0].id, first_annotation.id);
}

#[test]
fn board_rename_allows_duplicates_and_preserves_name_for_empty_edits() {
    let root = TempRoot::new("snappyref-board-rename-test");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let first = library.list_boards().unwrap()[0].clone();
    let second = library.fixture_board("Second").unwrap();

    library
        .persist_board_unit(
            &crate::library::storage::BoardDurabilityOperation::Lifecycle(
                crate::active_board::persistence::BoardLifecyclePersistence::Rename {
                    board_id: first.id,
                    name: "Second".to_owned(),
                },
            ),
            &Default::default(),
        )
        .unwrap();
    library
        .persist_board_unit(
            &crate::library::storage::BoardDurabilityOperation::Lifecycle(
                crate::active_board::persistence::BoardLifecyclePersistence::Rename {
                    board_id: second.id,
                    name: "   ".to_owned(),
                },
            ),
            &Default::default(),
        )
        .unwrap();

    let boards = library.list_boards().unwrap();
    assert_eq!(boards[0].name, "Second");
    assert_eq!(boards[1].name, "Second");
}

#[test]
fn delete_board_archives_board_preserves_assets_and_archives_board_content() {
    let root = TempRoot::new("snappyref-board-delete-test");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let first = library.list_boards().unwrap()[0].clone();
    let second = library.fixture_board("Second").unwrap();
    let third = library.fixture_board("Third").unwrap();
    library.set_active_board(second.id).unwrap();
    let path = library
        .store_rgba_image("delete-board", &[255, 0, 0, 255], 1, 1)
        .unwrap();
    let item = library
        .fixture_placement(
            library.load_user_session().unwrap().active_board_id,
            crate::data::Placement::loaded(
                crate::data::PlacementState::new(
                    library.allocate_document_id(),
                    egui::pos2(10.0, 20.0),
                    1.0,
                    0.0,
                    0,
                    false,
                ),
                library
                    .register_original(&path, "image/png", 1, 1, 4, None)
                    .unwrap(),
            ),
        )
        .unwrap();
    let annotation = library
        .fixture_annotation(
            CanvasAnnotationKind::Textbox,
            egui::pos2(0.0, 0.0),
            egui::pos2(40.0, 20.0),
            "delete me",
            CanvasTextSize::default(),
            crate::data::CanvasStrokeWidth::default(),
            crate::data::CanvasAnnotationColor::default(),
        )
        .unwrap();

    library
        .persist_board_unit(
            &crate::library::storage::BoardDurabilityOperation::Lifecycle(
                crate::active_board::persistence::BoardLifecyclePersistence::Archive {
                    board_id: second.id,
                    active_board_id: first.id,
                },
            ),
            &Default::default(),
        )
        .unwrap();

    assert_eq!(
        library.load_user_session().unwrap().active_board_id,
        first.id
    );
    assert_eq!(
        library
            .list_boards()
            .unwrap()
            .iter()
            .map(|board| board.id)
            .collect::<Vec<_>>(),
        vec![first.id, third.id]
    );
    assert!(library.load_placements_for_board(second.id).is_err());
    assert!(
        library
            .asset_original_paths_for_assets(&[item.require_loaded_asset().id])
            .unwrap()
            .contains_key(&item.require_loaded_asset().id)
    );
    {
        let connection = Connection::open(root.join("library.db")).unwrap();
        let annotation_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM canvas_annotations WHERE id = ?1 AND archived = 1",
                [annotation.id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(annotation_count, 1);
    }

    let remaining_active = third.id;
    library
        .persist_board_unit(
            &crate::library::storage::BoardDurabilityOperation::Lifecycle(
                crate::active_board::persistence::BoardLifecyclePersistence::Archive {
                    board_id: first.id,
                    active_board_id: remaining_active,
                },
            ),
            &Default::default(),
        )
        .unwrap();
}

#[test]
fn restoring_board_snapshot_restores_loading_placement_content_exactly() {
    let root = TempRoot::new("snappyref-board-restore-loading-content");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let first = library.list_boards().unwrap()[0].clone();
    let second = library.fixture_board("Second").unwrap();
    library.set_active_board(second.id).unwrap();
    let source = root.join("source.png");
    fs::write(&source, [255, 0, 0, 255]).unwrap();
    let loading = library
        .fixture_placement(
            library.load_user_session().unwrap().active_board_id,
            library.new_committed_loading_placement(
                egui::pos2(12.0, 24.0),
                1.5,
                eframe::egui::vec2(320.0, 180.0),
                ContentResolutionSource::ExternalPath {
                    path: source.to_string_lossy().into_owned(),
                },
                Some("Queued".to_owned()),
            ),
        )
        .unwrap();
    let snapshot = library.load_board_document(second.id).unwrap();

    library
        .persist_board_unit(
            &crate::library::storage::BoardDurabilityOperation::Lifecycle(
                crate::active_board::persistence::BoardLifecyclePersistence::Archive {
                    board_id: second.id,
                    active_board_id: first.id,
                },
            ),
            &Default::default(),
        )
        .unwrap();
    library
        .fixture_board_snapshot(&snapshot, second.id)
        .unwrap();

    let restored = library.load_placements().unwrap();
    assert_eq!(restored.len(), 1);
    assert_eq!(restored[0].state.geometry, loading.state.geometry);
    assert_eq!(restored[0].content, loading.content);
    assert_eq!(restored[0].lifecycle, loading.lifecycle);
}

#[test]
fn archive_assets_removes_their_placements_from_all_boards_and_preserves_files() {
    let root = TempRoot::new("snappyref-archive-assets-test");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let second = library.fixture_board("Second").unwrap();
    let first_path = library
        .store_rgba_image("first", &[255, 0, 0, 255], 1, 1)
        .unwrap();
    let second_path = library
        .store_rgba_image("second", &[0, 255, 0, 255], 1, 1)
        .unwrap();
    let first_item = library
        .fixture_placement(
            library.load_user_session().unwrap().active_board_id,
            crate::data::Placement::loaded(
                crate::data::PlacementState::new(
                    library.allocate_document_id(),
                    egui::pos2(10.0, 20.0),
                    1.0,
                    0.0,
                    0,
                    false,
                ),
                library
                    .register_original(&first_path, "image/png", 1, 1, 4, None)
                    .unwrap(),
            ),
        )
        .unwrap();
    let kept_item = library
        .fixture_placement(
            library.load_user_session().unwrap().active_board_id,
            crate::data::Placement::loaded(
                crate::data::PlacementState::new(
                    library.allocate_document_id(),
                    egui::pos2(30.0, 40.0),
                    1.0,
                    0.0,
                    0,
                    false,
                ),
                library
                    .register_original(&second_path, "image/png", 1, 1, 4, None)
                    .unwrap(),
            ),
        )
        .unwrap();
    library.set_active_board(second.id).unwrap();
    let second_placement = library
        .fixture_placement(
            library.load_user_session().unwrap().active_board_id,
            library
                .new_placement_from_asset(
                    first_item.require_loaded_asset().id,
                    egui::pos2(50.0, 60.0),
                    1.0,
                    0.0,
                    0,
                )
                .unwrap(),
        )
        .unwrap();
    library
        .upsert_asset_embedding_blob(
            first_item.require_loaded_asset().id,
            "test-model",
            &[1.0, 0.0],
        )
        .unwrap();
    library
        .upsert_asset_embedding_blob(
            kept_item.require_loaded_asset().id,
            "test-model",
            &[0.0, 1.0],
        )
        .unwrap();

    library
        .archive_assets(&[first_item.require_loaded_asset().id])
        .unwrap();

    assert!(first_item.require_loaded_asset().original_path.exists());
    assert!(
        second_placement
            .require_loaded_asset()
            .original_path
            .exists()
    );
    library.set_active_board(1).unwrap();
    let first_placements = library.load_placements().unwrap();
    assert_eq!(first_placements.len(), 1);
    assert_eq!(
        first_placements[0].require_loaded_asset().id,
        kept_item.require_loaded_asset().id
    );
    library.set_active_board(second.id).unwrap();
    assert!(library.load_placements().unwrap().is_empty());
    let embeddings = library.load_library_embeddings("test-model").unwrap();
    assert_eq!(
        embeddings
            .iter()
            .map(|embedding| embedding.asset_id)
            .collect::<Vec<_>>(),
        vec![kept_item.require_loaded_asset().id]
    );
}

#[test]
fn workspace_settings_remain_global_across_active_boards() {
    let root = TempRoot::new("snappyref-global-settings-test");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let first_board = library.list_boards().unwrap()[0].clone();
    let initial_session = library.load_user_session().unwrap();
    assert_eq!(
        initial_session.viewport.camera_zoom,
        crate::data::DEFAULT_VIEWPORT_ZOOM
    );
    let second_board = library.fixture_board("Second").unwrap();
    library.set_active_board(second_board.id).unwrap();

    let mut session = library.load_user_session().unwrap();
    session.settings.placement_gap = 48.0;
    session.settings.placement_target_area = 45_000.0;
    session.settings.semantic_search_first_launch_prompt_seen = true;
    session.settings.canvas_image_max_side = 4096;
    session.settings.rotation_snap_degrees = 45.0;
    session.viewport.camera_pan_x = 90.0;
    session.viewport.canvas_background = CanvasBackground::Grid;
    library.save_user_session(&session).unwrap();

    library.set_active_board(first_board.id).unwrap();
    let reopened = library.load_user_session().unwrap();

    assert_eq!(reopened.active_board_id, first_board.id);
    assert_eq!(reopened.settings.placement_gap, 48.0);
    assert_eq!(reopened.settings.placement_target_area, 45_000.0);
    assert!(reopened.settings.semantic_search_first_launch_prompt_seen);
    assert_eq!(reopened.settings.canvas_image_max_side, 4096);
    assert_eq!(reopened.settings.rotation_snap_degrees, 45.0);
    assert_eq!(reopened.viewport.camera_pan_x, 0.0);
    assert_eq!(reopened.viewport.canvas_background, CanvasBackground::None);

    library.set_active_board(second_board.id).unwrap();
    let second_reopened = library.load_user_session().unwrap();
    assert_eq!(second_reopened.viewport.camera_pan_x, 90.0);
    assert_eq!(
        second_reopened.viewport.canvas_background,
        CanvasBackground::Grid
    );
    assert_eq!(second_reopened.settings.placement_gap, 48.0);
    assert_eq!(second_reopened.settings.placement_target_area, 45_000.0);

    drop(library);
    let library = Library::initialize(root.to_path_buf()).unwrap();
    library.set_active_board(first_board.id).unwrap();
    let first_after_reopen = library.load_user_session().unwrap();
    assert_eq!(first_after_reopen.viewport.camera_pan_x, 0.0);
    assert_eq!(
        first_after_reopen.viewport.canvas_background,
        CanvasBackground::None
    );
    library.set_active_board(second_board.id).unwrap();
    let second_after_reopen = library.load_user_session().unwrap();
    assert_eq!(second_after_reopen.viewport.camera_pan_x, 90.0);
    assert_eq!(
        second_after_reopen.viewport.canvas_background,
        CanvasBackground::Grid
    );
}

#[test]
fn clipboard_images_get_unique_destination_paths() {
    let root = TempRoot::new("snappyref-clipboard-path-test");

    let library = Library::initialize(root.to_path_buf()).unwrap();
    let first = library
        .store_clipboard_image(&[255, 0, 0, 255], 1, 1)
        .unwrap();
    let second = library
        .store_clipboard_image(&[0, 255, 0, 255], 1, 1)
        .unwrap();

    assert_ne!(first, second);
    assert!(first.exists());
    assert!(second.exists());
}

#[test]
fn embedding_backfill_prioritizes_active_board_placements() {
    let root = TempRoot::new("snappyref-active-board-embedding-backfill-test");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let second = library.fixture_board("Second").unwrap();
    let active_path = library
        .store_rgba_image("active", &[255, 0, 0, 255], 1, 1)
        .unwrap();
    let ready_path = library
        .store_rgba_image("ready", &[0, 255, 0, 255], 1, 1)
        .unwrap();
    let inactive_path = library
        .store_rgba_image("inactive", &[0, 0, 255, 255], 1, 1)
        .unwrap();

    let active = library
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
                    .register_original(&active_path, "image/png", 1, 1, 4, None)
                    .unwrap(),
            ),
        )
        .unwrap();
    let ready = library
        .fixture_placement(
            library.load_user_session().unwrap().active_board_id,
            crate::data::Placement::loaded(
                crate::data::PlacementState::new(
                    library.allocate_document_id(),
                    egui::pos2(1.0, 0.0),
                    1.0,
                    0.0,
                    0,
                    false,
                ),
                library
                    .register_original(&ready_path, "image/png", 1, 1, 4, None)
                    .unwrap(),
            ),
        )
        .unwrap();
    library
        .upsert_asset_embedding_blob(
            ready.require_loaded_asset().id,
            "mobileclip_s0",
            &[1.0, 0.0],
        )
        .unwrap();

    library.set_active_board(second.id).unwrap();
    let inactive = library
        .fixture_placement(
            library.load_user_session().unwrap().active_board_id,
            crate::data::Placement::loaded(
                crate::data::PlacementState::new(
                    library.allocate_document_id(),
                    egui::pos2(2.0, 0.0),
                    1.0,
                    0.0,
                    0,
                    false,
                ),
                library
                    .register_original(&inactive_path, "image/png", 1, 1, 4, None)
                    .unwrap(),
            ),
        )
        .unwrap();
    library.set_active_board(1).unwrap();

    let job = library
        .next_embedding_backfill_job_for_model("mobileclip_s0", 0)
        .unwrap()
        .unwrap();
    assert_eq!(job.asset_id, active.require_loaded_asset().id);
    assert_ne!(job.asset_id, ready.require_loaded_asset().id);
    assert_ne!(job.asset_id, inactive.require_loaded_asset().id);
}

#[test]
fn canvas_annotations_round_trip_and_archive() {
    let root = TempRoot::new("snappyref-annotation-test");

    let library = Library::initialize(root.to_path_buf()).unwrap();
    let created = library
        .fixture_annotation(
            CanvasAnnotationKind::Textbox,
            egui::pos2(10.0, 12.0),
            egui::pos2(110.0, 72.0),
            "hello",
            CanvasTextSize::default(),
            crate::data::CanvasStrokeWidth::default(),
            crate::data::CanvasAnnotationColor::default(),
        )
        .unwrap();

    let loaded = library.load_canvas_annotations().unwrap();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0], created);
    assert_eq!(loaded[0].text_size, CanvasTextSize::default());

    let mut updated = loaded[0].clone();
    updated.text = "updated".to_owned();
    library
        .fixture_board_document(
            1,
            crate::canvas::CanvasDocumentChanges {
                updated_annotations: vec![updated.clone()],
                ..Default::default()
            },
        )
        .unwrap();

    let loaded = library.load_canvas_annotations().unwrap();
    assert_eq!(loaded[0].text, "updated");

    library
        .fixture_board_document(
            1,
            crate::canvas::CanvasDocumentChanges {
                archived_annotation_ids: vec![created.id],
                stack_order: Some(vec![]),
                ..Default::default()
            },
        )
        .unwrap();
    assert!(library.load_canvas_annotations().unwrap().is_empty());
}

#[test]
fn existing_canvas_annotations_rows_migrate_text_size_to_default_m() {
    let root = TempRoot::new("snappyref-annotation-migration-test");

    let db_path = root.join("library.db");
    let connection = Connection::open(&db_path).unwrap();
    connection
        .execute_batch(
            "
            CREATE TABLE canvas_annotations (
                id INTEGER PRIMARY KEY,
                kind TEXT NOT NULL,
                start_x REAL NOT NULL,
                start_y REAL NOT NULL,
                end_x REAL NOT NULL,
                end_y REAL NOT NULL,
                text TEXT NOT NULL DEFAULT '',
                archived INTEGER NOT NULL DEFAULT 0
            );
            INSERT INTO canvas_annotations (
                id, kind, start_x, start_y, end_x, end_y, text, archived
            ) VALUES (1, 'textbox', 10.0, 12.0, 110.0, 72.0, 'hello', 0);
            ",
        )
        .unwrap();
    drop(connection);

    let library = Library::initialize(root.to_path_buf()).unwrap();
    let loaded = library.load_canvas_annotations().unwrap();

    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].text_size, CanvasTextSize::M);
}
