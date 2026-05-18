use super::{Library, embeddings::EmbeddingStatus};
use crate::data::{
    CanvasAnnotationKind, CanvasBackground, CanvasStackItem, CanvasTextSize,
    ContentResolutionSource, PlacementContent, PlacementLifecycle, UserSession,
};
use eframe::egui;
use rusqlite::Connection;
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_root(prefix: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!(
        "{prefix}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&root).unwrap();
    root
}

#[test]
fn loaded_placements_load_as_committed_loaded_content() {
    let root = temp_root("snappyref-loaded-placement-content-test");
    let source = root.join("source.png");
    fs::write(&source, [255, 255, 255, 255]).unwrap();
    let library = Library::initialize(root.clone()).unwrap();
    let record = library
        .persist_imported_image(&source, "image/png", 10, 20, 4, egui::pos2(3.0, 4.0), 2.0)
        .unwrap();

    let reloaded = library.load_placements().unwrap();
    assert_eq!(reloaded.len(), 1);
    assert_eq!(reloaded[0].state.id, record.state.id);
    assert_eq!(
        reloaded[0].content,
        PlacementContent::Loaded {
            asset_id: record.asset.as_ref().expect("loaded placement asset").id
        }
    );
    assert!(reloaded[0].asset.is_some());
    assert_eq!(reloaded[0].lifecycle, PlacementLifecycle::Committed);
    assert_eq!(reloaded[0].visual_size, egui::vec2(10.0, 20.0));
    assert_eq!(
        library.load_placement_layout_items().unwrap()[0],
        crate::data::PlacementLayoutItem {
            id: record.state.id,
            center: egui::pos2(3.0, 4.0),
            width: 20.0,
            height: 40.0,
        }
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn committed_loading_and_failed_placements_persist_load_and_stack() {
    let root = temp_root("snappyref-unloaded-placement-content-test");
    let library = Library::initialize(root.clone()).unwrap();
    let loading = library
        .persist_committed_loading_placement(
            egui::pos2(12.0, 24.0),
            2.0,
            320.0,
            180.0,
            ContentResolutionSource::ExternalPath {
                path: "/tmp/source.png".to_owned(),
            },
            Some("Queued".to_owned()),
        )
        .unwrap();
    let failed = library
        .persist_committed_failed_placement(
            egui::pos2(48.0, 96.0),
            1.0,
            120.0,
            90.0,
            Some(ContentResolutionSource::Unknown {
                description: "clipboard".to_owned(),
            }),
            "Could not read image".to_owned(),
            true,
        )
        .unwrap();

    let reloaded = library.load_placements().unwrap();
    assert_eq!(reloaded.len(), 2);
    assert!(matches!(
        reloaded[0].content,
        PlacementContent::Loading { .. }
    ));
    assert!(reloaded[0].asset.is_none());
    assert!(matches!(
        reloaded[1].content,
        PlacementContent::Failed { .. }
    ));
    assert!(reloaded[1].asset.is_none());
    assert_eq!(reloaded[0].visual_size.x, 320.0);
    assert_eq!(reloaded[1].visual_size.y, 90.0);
    assert_eq!(
        library.load_placement_layout_items().unwrap(),
        vec![
            crate::data::PlacementLayoutItem {
                id: loading.state.id,
                center: egui::pos2(12.0, 24.0),
                width: 640.0,
                height: 360.0,
            },
            crate::data::PlacementLayoutItem {
                id: failed.state.id,
                center: egui::pos2(48.0, 96.0),
                width: 120.0,
                height: 90.0,
            },
        ]
    );
    assert_eq!(
        library.load_canvas_stack_order().unwrap(),
        vec![
            CanvasStackItem::Placement(loading.state.id),
            CanvasStackItem::Placement(failed.state.id),
        ]
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn legacy_board_items_asset_id_not_null_migrates_for_loading_placements() {
    let root = temp_root("snappyref-board-item-asset-nullability-test");
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

    let library = Library::initialize(root.clone()).unwrap();
    let migrated = Connection::open(&db_path).unwrap();
    let not_null = board_items_column_not_null(&migrated, "asset_id");
    drop(migrated);

    assert!(!not_null);
    let loading = library
        .persist_committed_loading_placement(
            egui::pos2(12.0, 24.0),
            1.0,
            320.0,
            180.0,
            ContentResolutionSource::ExternalPath {
                path: "C:/Users/Admin/Pictures/drop.png".to_owned(),
            },
            Some("Loading".to_owned()),
        )
        .unwrap();

    assert_eq!(loading.state.asset_id, None);
    assert!(matches!(loading.content, PlacementContent::Loading { .. }));

    let _ = fs::remove_dir_all(root);
}

fn board_items_column_not_null(connection: &Connection, column: &str) -> bool {
    let mut statement = connection
        .prepare("PRAGMA table_info(board_items)")
        .unwrap();
    let mut rows = statement.query([]).unwrap();
    while let Some(row) = rows.next().unwrap() {
        let name: String = row.get(1).unwrap();
        if name == column {
            let not_null: i64 = row.get(3).unwrap();
            return not_null != 0;
        }
    }
    panic!("missing board_items column {column}");
}

#[test]
fn imported_asset_resolves_existing_loading_placement_without_geometry_or_stack_changes() {
    let root = temp_root("snappyref-resolve-loading-placement-test");
    let source = root.join("source.png");
    fs::write(&source, [255, 255, 255, 255]).unwrap();
    let library = Library::initialize(root.clone()).unwrap();
    let loading = library
        .persist_committed_loading_placement(
            egui::pos2(12.0, 24.0),
            1.5,
            320.0,
            180.0,
            ContentResolutionSource::ExternalPath {
                path: source.display().to_string(),
            },
            Some("Queued".to_owned()),
        )
        .unwrap();
    let stack_before = library.load_canvas_stack_order().unwrap();

    let loaded = library
        .persist_imported_image_resolving_placement(
            1,
            Some(loading.state.id),
            &source,
            "image/png",
            640,
            480,
            4,
            egui::pos2(999.0, 999.0),
            0.25,
        )
        .unwrap();

    assert_eq!(loaded.state.id, loading.state.id);
    assert_eq!(loaded.state.center.x, 12.0);
    assert_eq!(loaded.state.center.y, 24.0);
    assert_eq!(loaded.state.scale, 1.5);
    assert_eq!(loaded.visual_size, egui::vec2(320.0, 180.0));
    assert!(matches!(loaded.content, PlacementContent::Loaded { .. }));
    assert_eq!(library.load_canvas_stack_order().unwrap(), stack_before);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn import_failure_marks_loading_placement_failed_and_archived_completion_stays_stale() {
    let root = temp_root("snappyref-failed-loading-placement-test");
    let source = root.join("source.png");
    fs::write(&source, [255, 255, 255, 255]).unwrap();
    let library = Library::initialize(root.clone()).unwrap();
    let loading = library
        .persist_committed_loading_placement(
            egui::pos2(10.0, 20.0),
            1.0,
            100.0,
            80.0,
            ContentResolutionSource::Unknown {
                description: "clipboard".to_owned(),
            },
            Some("Queued".to_owned()),
        )
        .unwrap();

    let failed = library
        .mark_placement_failed(1, loading.state.id, "decode failed".to_owned())
        .unwrap()
        .unwrap();
    assert!(matches!(failed.content, PlacementContent::Failed { .. }));
    assert_eq!(failed.state.center.x, 10.0);
    assert_eq!(failed.visual_size.x, 100.0);

    library.archive_placements(&[loading.state.id]).unwrap();
    assert!(
        library
            .persist_imported_image_resolving_placement(
                1,
                Some(loading.state.id),
                &source,
                "image/png",
                100,
                80,
                4,
                egui::Pos2::ZERO,
                1.0,
            )
            .is_err()
    );
    assert!(library.load_placements().unwrap().is_empty());

    let _ = fs::remove_dir_all(root);
}

#[test]
fn update_preservation_smoke() {
    let app_root = temp_root("snappyref-update-preservation-test");
    let library_root = app_root.join("library");
    fs::create_dir_all(&library_root).unwrap();
    fs::write(app_root.join("SnappyRef.app"), b"old app bundle").unwrap();
    fs::write(app_root.join("UPDATE.txt"), b"old update notes").unwrap();

    let original_bytes = [255, 0, 0, 255];
    let library = Library::initialize(library_root.clone()).unwrap();
    let first_board = library.list_boards().unwrap()[0].clone();
    let second_board = library.create_board("Second").unwrap();
    library.set_active_board(second_board.id).unwrap();
    let original_path = library
        .store_rgba_image("preserved-original", &original_bytes, 1, 1)
        .unwrap();
    let imported = library
        .persist_imported_image(
            &original_path,
            "image/png",
            1,
            1,
            original_bytes.len() as u64,
            egui::pos2(42.0, 24.0),
            1.5,
        )
        .unwrap();
    let mut session = UserSession {
        active_board_id: second_board.id,
        ..UserSession::default()
    };
    session.viewport.camera_pan_x = 123.0;
    session.viewport.camera_pan_y = 456.0;
    session.viewport.camera_zoom = 1.75;
    session.viewport.canvas_background = CanvasBackground::Grid;
    session.settings.auto_image_scaling = false;
    session.settings.import_gap = 36.0;
    session.settings.import_target_canvas_area = 54_321.0;
    session.settings.semantic_search_enabled = false;
    session.settings.canvas_image_max_side = 4096;
    session.settings.rotation_snap_degrees = 30.0;
    session.settings.default_textbox_background = true;
    session.settings.always_on_top = true;
    session.sidebar_open = true;
    session.sidebar_tab = "boards".to_owned();
    library.save_user_session(&session).unwrap();
    let stored_original_bytes = fs::read(
        &imported
            .asset
            .as_ref()
            .expect("loaded placement asset")
            .original_path,
    )
    .unwrap();
    drop(library);

    fs::remove_file(app_root.join("SnappyRef.app")).unwrap();
    fs::remove_file(app_root.join("UPDATE.txt")).unwrap();
    fs::write(app_root.join("SnappyRef.app"), b"new app bundle").unwrap();
    fs::write(app_root.join("UPDATE.txt"), b"new update notes").unwrap();

    let reopened = Library::initialize(library_root.clone()).unwrap();
    let boards = reopened.list_boards().unwrap();
    let reloaded_session = reopened.load_user_session().unwrap();
    let items = reopened.load_placements().unwrap();

    assert_eq!(
        boards.iter().map(|board| board.id).collect::<Vec<_>>(),
        vec![first_board.id, second_board.id]
    );
    assert_eq!(reloaded_session.active_board_id, second_board.id);
    assert_eq!(reloaded_session.viewport.camera_pan_x, 123.0);
    assert_eq!(reloaded_session.viewport.camera_pan_y, 456.0);
    assert_eq!(reloaded_session.viewport.camera_zoom, 1.75);
    assert_eq!(
        reloaded_session.viewport.canvas_background,
        CanvasBackground::Grid
    );
    assert!(!reloaded_session.settings.auto_image_scaling);
    assert_eq!(reloaded_session.settings.import_gap, 36.0);
    assert_eq!(
        reloaded_session.settings.import_target_canvas_area,
        54_321.0
    );
    assert!(!reloaded_session.settings.semantic_search_enabled);
    assert_eq!(reloaded_session.settings.canvas_image_max_side, 4096);
    assert_eq!(reloaded_session.settings.rotation_snap_degrees, 30.0);
    assert!(reloaded_session.settings.default_textbox_background);
    assert!(reloaded_session.settings.always_on_top);
    assert!(reloaded_session.sidebar_open);
    assert_eq!(reloaded_session.sidebar_tab, "boards");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].state.id, imported.state.id);
    assert_eq!(
        items[0].asset.as_ref().expect("loaded placement asset").id,
        imported.asset.as_ref().expect("loaded placement asset").id
    );
    assert_eq!(items[0].state.center.x, 42.0);
    assert_eq!(items[0].state.center.y, 24.0);
    assert_eq!(items[0].state.scale, 1.5);
    assert_eq!(
        items[0]
            .asset
            .as_ref()
            .expect("loaded placement asset")
            .original_path,
        imported
            .asset
            .as_ref()
            .expect("loaded placement asset")
            .original_path
    );
    assert!(
        items[0]
            .asset
            .as_ref()
            .expect("loaded placement asset")
            .original_path
            .exists()
    );
    assert_eq!(
        fs::read(
            &items[0]
                .asset
                .as_ref()
                .expect("loaded placement asset")
                .original_path
        )
        .unwrap(),
        stored_original_bytes
    );

    let _ = fs::remove_dir_all(app_root);
}

#[test]
fn new_library_has_default_non_archived_active_board() {
    let root = temp_root("snappyref-default-board-test");
    let library = Library::initialize(root.clone()).unwrap();

    let boards = library.list_boards().unwrap();
    let session = library.load_user_session().unwrap();

    assert_eq!(boards.len(), 1);
    assert_eq!(boards[0].name, "My Board");
    assert!(!boards[0].archived);
    assert_eq!(session.active_board_id, boards[0].id);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn existing_single_canvas_data_migrates_to_my_board() {
    let root = temp_root("snappyref-board-migration-test");
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
                import_gap REAL NOT NULL DEFAULT 24.0,
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
                id, camera_pan_x, camera_pan_y, camera_zoom, auto_image_scaling, import_gap,
                canvas_background, semantic_search_enabled, canvas_image_mode,
                canvas_image_max_side, rotation_snap_degrees, default_textbox_background
            ) VALUES (1, 44.0, 55.0, 1.25, 0, 37.0, 'grid', 0, 'original', 4096, 30.0, 1);
            ",
        )
        .unwrap();
    drop(connection);

    let library = Library::initialize(root.clone()).unwrap();
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

    let _ = fs::remove_dir_all(root);
}

#[test]
fn existing_canvas_data_migrates_to_unified_stack_with_annotations_above_placements() {
    let root = temp_root("snappyref-stack-order-migration-test");
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

    let library = Library::initialize(root.clone()).unwrap();
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

    let _ = fs::remove_dir_all(root);
}

#[test]
fn created_annotations_round_trip_through_canvas_stack_order() {
    let root = temp_root("snappyref-stack-order-annotation-test");
    let library = Library::initialize(root.clone()).unwrap();
    let path = library
        .store_rgba_image("placed", &[255, 0, 0, 255], 1, 1)
        .unwrap();
    let placement = library
        .persist_imported_image(&path, "image/png", 1, 1, 4, egui::pos2(0.0, 0.0), 1.0)
        .unwrap();
    let annotation = library
        .create_canvas_annotation(
            CanvasAnnotationKind::Textbox,
            egui::pos2(10.0, 12.0),
            egui::pos2(110.0, 72.0),
            "hello",
            CanvasTextSize::default(),
            crate::data::CanvasStrokeWidth::default(),
            crate::data::CanvasAnnotationColor::default(),
            false,
        )
        .unwrap();

    drop(library);
    let library = Library::initialize(root.clone()).unwrap();
    let stack = library.load_canvas_stack_order().unwrap();

    assert_eq!(
        stack,
        vec![
            CanvasStackItem::Placement(placement.state.id),
            CanvasStackItem::Annotation(annotation.id),
        ]
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn saved_canvas_stack_order_round_trips_for_active_board() {
    let root = temp_root("snappyref-stack-order-save-test");
    let library = Library::initialize(root.clone()).unwrap();
    let first_path = library
        .store_rgba_image("first", &[255, 0, 0, 255], 1, 1)
        .unwrap();
    let second_path = library
        .store_rgba_image("second", &[0, 255, 0, 255], 1, 1)
        .unwrap();
    let first = library
        .persist_imported_image(&first_path, "image/png", 1, 1, 4, egui::pos2(0.0, 0.0), 1.0)
        .unwrap();
    let second = library
        .persist_imported_image(
            &second_path,
            "image/png",
            1,
            1,
            4,
            egui::pos2(10.0, 0.0),
            1.0,
        )
        .unwrap();
    let annotation = library
        .create_canvas_annotation(
            CanvasAnnotationKind::Textbox,
            egui::pos2(10.0, 12.0),
            egui::pos2(110.0, 72.0),
            "hello",
            CanvasTextSize::default(),
            crate::data::CanvasStrokeWidth::default(),
            crate::data::CanvasAnnotationColor::default(),
            false,
        )
        .unwrap();

    let saved = vec![
        CanvasStackItem::Annotation(annotation.id),
        CanvasStackItem::Placement(second.state.id),
        CanvasStackItem::Placement(first.state.id),
    ];
    library.save_canvas_stack_order(&saved).unwrap();

    drop(library);
    let library = Library::initialize(root.clone()).unwrap();
    assert_eq!(library.load_canvas_stack_order().unwrap(), saved);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn active_board_persists_and_scopes_items_annotations_and_viewport() {
    let root = temp_root("snappyref-board-scope-test");
    let library = Library::initialize(root.clone()).unwrap();
    let first_board = library.list_boards().unwrap()[0].clone();
    let first_path = library
        .store_rgba_image("first", &[255, 0, 0, 255], 1, 1)
        .unwrap();
    let first_item = library
        .persist_imported_image(
            &first_path,
            "image/png",
            1,
            1,
            4,
            egui::pos2(10.0, 0.0),
            1.0,
        )
        .unwrap();
    let first_annotation = library
        .create_canvas_annotation(
            CanvasAnnotationKind::Textbox,
            egui::pos2(0.0, 0.0),
            egui::pos2(50.0, 20.0),
            "first board",
            CanvasTextSize::default(),
            crate::data::CanvasStrokeWidth::default(),
            crate::data::CanvasAnnotationColor::default(),
            false,
        )
        .unwrap();

    let second_board = library.create_board("Second").unwrap();
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
        .persist_imported_image(
            &second_path,
            "image/png",
            1,
            1,
            4,
            egui::pos2(20.0, 0.0),
            1.0,
        )
        .unwrap();
    let second_annotation = library
        .create_canvas_annotation(
            CanvasAnnotationKind::Textbox,
            egui::pos2(100.0, 0.0),
            egui::pos2(150.0, 20.0),
            "second board",
            CanvasTextSize::default(),
            crate::data::CanvasStrokeWidth::default(),
            crate::data::CanvasAnnotationColor::default(),
            true,
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

    let _ = fs::remove_dir_all(root);
}

#[test]
fn board_rename_allows_duplicates_and_preserves_name_for_empty_edits() {
    let root = temp_root("snappyref-board-rename-test");
    let library = Library::initialize(root.clone()).unwrap();
    let first = library.list_boards().unwrap()[0].clone();
    let second = library.create_board("Second").unwrap();

    library.rename_board(first.id, "Second").unwrap();
    library.rename_board(second.id, "   ").unwrap();

    let boards = library.list_boards().unwrap();
    assert_eq!(boards[0].name, "Second");
    assert_eq!(boards[1].name, "Second");

    let _ = fs::remove_dir_all(root);
}

#[test]
fn delete_board_archives_board_preserves_assets_and_archives_board_content() {
    let root = temp_root("snappyref-board-delete-test");
    let library = Library::initialize(root.clone()).unwrap();
    let first = library.list_boards().unwrap()[0].clone();
    let second = library.create_board("Second").unwrap();
    let third = library.create_board("Third").unwrap();
    library.set_active_board(second.id).unwrap();
    let path = library
        .store_rgba_image("delete-board", &[255, 0, 0, 255], 1, 1)
        .unwrap();
    let item = library
        .persist_imported_image(&path, "image/png", 1, 1, 4, egui::pos2(10.0, 20.0), 1.0)
        .unwrap();
    let annotation = library
        .create_canvas_annotation(
            CanvasAnnotationKind::Textbox,
            egui::pos2(0.0, 0.0),
            egui::pos2(40.0, 20.0),
            "delete me",
            CanvasTextSize::default(),
            crate::data::CanvasStrokeWidth::default(),
            crate::data::CanvasAnnotationColor::default(),
            false,
        )
        .unwrap();

    let remaining_active = library.delete_board(second.id).unwrap();

    assert_eq!(remaining_active, first.id);
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
            .asset_original_paths_for_assets(&[item
                .asset
                .as_ref()
                .expect("loaded placement asset")
                .id])
            .unwrap()
            .contains_key(&item.asset.as_ref().expect("loaded placement asset").id)
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

    assert!(library.delete_board(first.id).is_ok());
    assert!(library.delete_board(third.id).is_err());

    let _ = fs::remove_dir_all(root);
}

#[test]
fn archive_assets_removes_their_placements_from_all_boards_and_preserves_files() {
    let root = temp_root("snappyref-archive-assets-test");
    let library = Library::initialize(root.clone()).unwrap();
    let second = library.create_board("Second").unwrap();
    let first_path = library
        .store_rgba_image("first", &[255, 0, 0, 255], 1, 1)
        .unwrap();
    let second_path = library
        .store_rgba_image("second", &[0, 255, 0, 255], 1, 1)
        .unwrap();
    let first_item = library
        .persist_imported_image(
            &first_path,
            "image/png",
            1,
            1,
            4,
            egui::pos2(10.0, 20.0),
            1.0,
        )
        .unwrap();
    let kept_item = library
        .persist_imported_image(
            &second_path,
            "image/png",
            1,
            1,
            4,
            egui::pos2(30.0, 40.0),
            1.0,
        )
        .unwrap();
    library.set_active_board(second.id).unwrap();
    let second_placement = library
        .create_placement_from_asset(
            first_item
                .asset
                .as_ref()
                .expect("loaded placement asset")
                .id,
            egui::pos2(50.0, 60.0),
            1.0,
            0.0,
            0,
        )
        .unwrap();
    library
        .upsert_asset_embedding_blob(
            first_item
                .asset
                .as_ref()
                .expect("loaded placement asset")
                .id,
            "test-model",
            &[1.0, 0.0],
        )
        .unwrap();
    library
        .upsert_asset_embedding_blob(
            kept_item.asset.as_ref().expect("loaded placement asset").id,
            "test-model",
            &[0.0, 1.0],
        )
        .unwrap();

    library
        .archive_assets(&[first_item
            .asset
            .as_ref()
            .expect("loaded placement asset")
            .id])
        .unwrap();

    assert!(
        first_item
            .asset
            .as_ref()
            .expect("loaded placement asset")
            .original_path
            .exists()
    );
    assert!(
        second_placement
            .asset
            .as_ref()
            .expect("loaded placement asset")
            .original_path
            .exists()
    );
    library.set_active_board(1).unwrap();
    let first_placements = library.load_placements().unwrap();
    assert_eq!(first_placements.len(), 1);
    assert_eq!(
        first_placements[0]
            .asset
            .as_ref()
            .expect("loaded placement asset")
            .id,
        kept_item.asset.as_ref().expect("loaded placement asset").id
    );
    library.set_active_board(second.id).unwrap();
    assert!(library.load_placements().unwrap().is_empty());
    let embeddings = library.load_library_embeddings("test-model").unwrap();
    assert_eq!(
        embeddings
            .iter()
            .map(|embedding| embedding.asset_id)
            .collect::<Vec<_>>(),
        vec![kept_item.asset.as_ref().expect("loaded placement asset").id]
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn workspace_settings_remain_global_across_active_boards() {
    let root = temp_root("snappyref-global-settings-test");
    let library = Library::initialize(root.clone()).unwrap();
    let first_board = library.list_boards().unwrap()[0].clone();
    let initial_session = library.load_user_session().unwrap();
    assert_eq!(
        initial_session.viewport.camera_zoom,
        crate::data::DEFAULT_VIEWPORT_ZOOM
    );
    let second_board = library.create_board("Second").unwrap();
    library.set_active_board(second_board.id).unwrap();

    let mut session = library.load_user_session().unwrap();
    session.settings.auto_image_scaling = false;
    session.settings.import_gap = 48.0;
    session.settings.import_target_canvas_area = 45_000.0;
    session.settings.semantic_search_enabled = false;
    session.settings.semantic_search_first_launch_prompt_seen = true;
    session.settings.canvas_image_max_side = 4096;
    session.settings.rotation_snap_degrees = 45.0;
    session.settings.default_textbox_background = true;
    session.viewport.camera_pan_x = 90.0;
    session.viewport.canvas_background = CanvasBackground::Grid;
    library.save_user_session(&session).unwrap();

    library.set_active_board(first_board.id).unwrap();
    let reopened = library.load_user_session().unwrap();

    assert_eq!(reopened.active_board_id, first_board.id);
    assert!(!reopened.settings.auto_image_scaling);
    assert_eq!(reopened.settings.import_gap, 48.0);
    assert_eq!(reopened.settings.import_target_canvas_area, 45_000.0);
    assert!(!reopened.settings.semantic_search_enabled);
    assert!(reopened.settings.semantic_search_first_launch_prompt_seen);
    assert_eq!(reopened.settings.canvas_image_max_side, 4096);
    assert_eq!(reopened.settings.rotation_snap_degrees, 45.0);
    assert!(reopened.settings.default_textbox_background);
    assert_eq!(reopened.viewport.camera_pan_x, 0.0);
    assert_eq!(reopened.viewport.canvas_background, CanvasBackground::None);

    library.set_active_board(second_board.id).unwrap();
    let second_reopened = library.load_user_session().unwrap();
    assert_eq!(second_reopened.viewport.camera_pan_x, 90.0);
    assert_eq!(
        second_reopened.viewport.canvas_background,
        CanvasBackground::Grid
    );
    assert_eq!(second_reopened.settings.import_gap, 48.0);
    assert_eq!(second_reopened.settings.import_target_canvas_area, 45_000.0);

    drop(library);
    let library = Library::initialize(root.clone()).unwrap();
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

    let _ = fs::remove_dir_all(root);
}

#[test]
fn user_session_payload_takes_precedence_over_legacy_preference_columns() {
    let root = temp_root("snappyref-session-payload-precedence");
    let library = Library::initialize(root.clone()).unwrap();

    let mut session = library.load_user_session().unwrap();
    session.settings.auto_image_scaling = false;
    session.settings.import_gap = 48.0;
    session.settings.import_target_canvas_area = 45_000.0;
    session.settings.semantic_search_enabled = false;
    session.settings.canvas_image_max_side = 4096;
    session.settings.rotation_snap_degrees = 45.0;
    session.settings.default_textbox_background = true;
    session.sidebar_open = true;
    session.sidebar_tab = "boards".to_owned();
    library.save_user_session(&session).unwrap();
    drop(library);

    let connection = Connection::open(root.join("library.db")).unwrap();
    connection
        .execute(
            "UPDATE user_session
             SET auto_image_scaling = 1,
                 import_gap = 12.0,
                 import_target_canvas_area = 12.0,
                 semantic_search_enabled = 1,
                 canvas_image_max_side = 100,
                 rotation_snap_degrees = 5.0,
                 default_textbox_background = 0,
                 sidebar_open = 0,
                 sidebar_tab = 'settings'
             WHERE id = 1",
            [],
        )
        .unwrap();
    drop(connection);

    let library = Library::initialize(root.clone()).unwrap();
    let reloaded = library.load_user_session().unwrap();

    assert!(!reloaded.settings.auto_image_scaling);
    assert_eq!(reloaded.settings.import_gap, 48.0);
    assert_eq!(reloaded.settings.import_target_canvas_area, 45_000.0);
    assert!(!reloaded.settings.semantic_search_enabled);
    assert_eq!(reloaded.settings.canvas_image_max_side, 4096);
    assert_eq!(reloaded.settings.rotation_snap_degrees, 45.0);
    assert!(reloaded.settings.default_textbox_background);
    assert!(reloaded.sidebar_open);
    assert_eq!(reloaded.sidebar_tab, "boards");

    let _ = fs::remove_dir_all(root);
}

#[test]
fn existing_legacy_user_session_columns_backfill_payload() {
    let root = temp_root("snappyref-session-payload-backfill");
    let db_path = root.join("library.db");
    let connection = Connection::open(&db_path).unwrap();
    connection
        .execute_batch(
            "
            CREATE TABLE user_session (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                camera_pan_x REAL NOT NULL,
                camera_pan_y REAL NOT NULL,
                camera_zoom REAL NOT NULL,
                auto_image_scaling INTEGER NOT NULL DEFAULT 1,
                import_gap REAL NOT NULL DEFAULT 24.0,
                import_target_canvas_area REAL NOT NULL DEFAULT 90000.0,
                canvas_background TEXT NOT NULL DEFAULT 'none',
                semantic_search_enabled INTEGER NOT NULL DEFAULT 1,
                canvas_image_mode TEXT NOT NULL DEFAULT 'downscale',
                canvas_image_max_side INTEGER NOT NULL DEFAULT 2000,
                rotation_snap_degrees REAL NOT NULL DEFAULT 15.0,
                default_textbox_background INTEGER NOT NULL DEFAULT 0,
                sidebar_open INTEGER NOT NULL DEFAULT 0,
                sidebar_tab TEXT NOT NULL DEFAULT 'settings',
                active_board_id INTEGER NOT NULL DEFAULT 1
            );

            INSERT INTO user_session (
                id,
                camera_pan_x,
                camera_pan_y,
                camera_zoom,
                auto_image_scaling,
                import_gap,
                import_target_canvas_area,
                canvas_background,
                semantic_search_enabled,
                canvas_image_max_side,
                rotation_snap_degrees,
                default_textbox_background,
                sidebar_open,
                sidebar_tab,
                active_board_id
            )
            VALUES (
                1,
                10.0,
                20.0,
                1.5,
                0,
                48.0,
                45000.0,
                'grid',
                0,
                4096,
                45.0,
                1,
                1,
                'boards',
                1
            );
            ",
        )
        .unwrap();
    drop(connection);

    let library = Library::initialize(root.clone()).unwrap();
    let session = library.load_user_session().unwrap();

    assert_eq!(session.viewport.camera_pan_x, 10.0);
    assert_eq!(session.viewport.camera_pan_y, 20.0);
    assert_eq!(session.viewport.camera_zoom, 1.5);
    assert_eq!(session.viewport.canvas_background, CanvasBackground::Grid);
    assert!(!session.settings.auto_image_scaling);
    assert_eq!(session.settings.import_gap, 48.0);
    assert_eq!(session.settings.import_target_canvas_area, 45_000.0);
    assert!(!session.settings.semantic_search_enabled);
    assert_eq!(session.settings.canvas_image_max_side, 4096);
    assert_eq!(session.settings.rotation_snap_degrees, 45.0);
    assert!(session.settings.default_textbox_background);
    assert!(session.sidebar_open);
    assert_eq!(session.sidebar_tab, "boards");
    drop(library);

    let connection = Connection::open(root.join("library.db")).unwrap();
    let payload: Option<String> = connection
        .query_row(
            "SELECT session_payload FROM user_session WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(payload.as_deref().is_some_and(|value| !value.is_empty()));
    connection
        .execute(
            "UPDATE user_session
             SET import_gap = 12.0,
                 sidebar_open = 0,
                 sidebar_tab = 'settings'
             WHERE id = 1",
            [],
        )
        .unwrap();
    drop(connection);

    let library = Library::initialize(root.clone()).unwrap();
    let reloaded = library.load_user_session().unwrap();
    assert_eq!(reloaded.settings.import_gap, 48.0);
    assert!(reloaded.sidebar_open);
    assert_eq!(reloaded.sidebar_tab, "boards");

    let _ = fs::remove_dir_all(root);
}

#[test]
fn existing_user_session_rows_migrate_to_default_preferences() {
    let root = std::env::temp_dir().join(format!(
        "snappyref-storage-test-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&root).unwrap();

    let db_path = root.join("library.db");
    let connection = Connection::open(&db_path).unwrap();
    connection
        .execute_batch(
            "
            CREATE TABLE user_session (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                camera_pan_x REAL NOT NULL,
                camera_pan_y REAL NOT NULL,
                camera_zoom REAL NOT NULL
            );

            INSERT INTO user_session (id, camera_pan_x, camera_pan_y, camera_zoom)
            VALUES (1, 10.0, 20.0, 1.5);
            ",
        )
        .unwrap();
    drop(connection);

    let library = Library::initialize(root.clone()).unwrap();
    let session = library.load_user_session().unwrap();

    assert_eq!(session.viewport.camera_pan_x, 10.0);
    assert_eq!(session.viewport.camera_pan_y, 20.0);
    assert_eq!(session.viewport.camera_zoom, 1.5);
    assert!(session.settings.auto_image_scaling);
    assert_eq!(session.settings.import_gap, crate::data::DEFAULT_IMPORT_GAP);
    assert_eq!(
        session.settings.import_target_canvas_area,
        crate::data::DEFAULT_IMPORT_TARGET_CANVAS_AREA
    );
    assert_eq!(
        session.settings.canvas_image_max_side,
        crate::data::DEFAULT_CANVAS_IMAGE_MAX_SIDE
    );
    assert_eq!(session.viewport.canvas_background, CanvasBackground::None);
    assert!(session.settings.semantic_search_enabled);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn canvas_image_cache_path_is_deterministic() {
    let root = std::env::temp_dir().join(format!(
        "snappyref-thumb-path-test-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&root).unwrap();

    let library = Library::initialize(root.clone()).unwrap();
    let path = library
        .store_canvas_image_cache(42, 2000, &[255, 0, 0, 255], 1, 1)
        .unwrap();
    let path = path.unwrap();

    assert_eq!(
        path,
        root.join("thumbs")
            .join("asset-42-canvas-v3-downscale-working-2000.jpg")
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn clipboard_images_get_unique_destination_paths() {
    let root = std::env::temp_dir().join(format!(
        "snappyref-clipboard-path-test-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&root).unwrap();

    let library = Library::initialize(root.clone()).unwrap();
    let first = library
        .store_clipboard_image(&[255, 0, 0, 255], 1, 1)
        .unwrap();
    let second = library
        .store_clipboard_image(&[0, 255, 0, 255], 1, 1)
        .unwrap();

    assert_ne!(first, second);
    assert!(first.exists());
    assert!(second.exists());

    let _ = fs::remove_dir_all(root);
}

#[test]
fn store_and_remove_canvas_image_cache_round_trips_thumb_file() {
    let root = std::env::temp_dir().join(format!(
        "snappyref-thumb-store-test-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&root).unwrap();

    let library = Library::initialize(root.clone()).unwrap();
    let preview_path = library
        .store_canvas_image_cache(7, 2000, &[255, 0, 0, 255], 1, 1)
        .unwrap()
        .unwrap();

    assert!(preview_path.is_file());

    library.remove_asset_canvas_caches(7).unwrap();
    assert!(!preview_path.exists());

    let _ = fs::remove_dir_all(root);
}

#[test]
fn remove_asset_canvas_caches_removes_all_variants_for_asset() {
    let root = std::env::temp_dir().join(format!(
        "snappyref-thumb-remove-variants-test-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&root).unwrap();

    let library = Library::initialize(root.clone()).unwrap();
    let first = library
        .store_canvas_image_cache(7, 2000, &[255, 0, 0, 255], 1, 1)
        .unwrap()
        .unwrap();
    let second = library
        .store_canvas_image_cache(7, 4000, &[255, 0, 0, 255], 1, 1)
        .unwrap()
        .unwrap();

    library.remove_asset_canvas_caches(7).unwrap();

    assert!(!first.exists());
    assert!(!second.exists());

    let _ = fs::remove_dir_all(root);
}

#[test]
fn embedding_backfill_includes_unplaced_library_assets() {
    let root = std::env::temp_dir().join(format!(
        "snappyref-library-embedding-test-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&root).unwrap();

    let library = Library::initialize(root.clone()).unwrap();
    let ready_path = library
        .store_rgba_image("ready", &[255, 0, 0, 255], 1, 1)
        .unwrap();
    let pending_path = library
        .store_rgba_image("pending", &[0, 255, 0, 255], 1, 1)
        .unwrap();
    let unplaced_path = library
        .store_rgba_image("unplaced", &[0, 0, 255, 255], 1, 1)
        .unwrap();

    let ready = library
        .persist_imported_image(&ready_path, "image/png", 1, 1, 4, egui::pos2(0.0, 0.0), 1.0)
        .unwrap();
    let pending = library
        .persist_imported_image(
            &pending_path,
            "image/png",
            1,
            1,
            4,
            egui::pos2(10.0, 0.0),
            1.0,
        )
        .unwrap();
    let unplaced = library
        .persist_imported_image(
            &unplaced_path,
            "image/png",
            1,
            1,
            4,
            egui::pos2(20.0, 0.0),
            1.0,
        )
        .unwrap();

    library.archive_placements(&[unplaced.state.id]).unwrap();
    library
        .upsert_asset_embedding_blob(
            ready.asset.as_ref().expect("loaded placement asset").id,
            "mobileclip_s0",
            &[1.0, 0.0],
        )
        .unwrap();

    let jobs = library
        .missing_live_canvas_asset_jobs_for_model("mobileclip_s0")
        .unwrap();

    let job_asset_ids = jobs.iter().map(|job| job.asset_id).collect::<Vec<_>>();
    assert_eq!(
        job_asset_ids,
        vec![
            pending.asset.as_ref().expect("loaded placement asset").id,
            unplaced.asset.as_ref().expect("loaded placement asset").id
        ]
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn embedding_backfill_skips_svg_assets() {
    let root = temp_root("snappyref-svg-embedding-backfill-test");
    let library = Library::initialize(root.clone()).unwrap();
    let raster_path = library
        .store_rgba_image("raster", &[255, 0, 0, 255], 1, 1)
        .unwrap();
    let svg_path = root.join("icon.svg");
    fs::write(
        &svg_path,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="1" height="1"></svg>"#,
    )
    .unwrap();

    let raster = library
        .persist_imported_image(
            &raster_path,
            "image/png",
            1,
            1,
            4,
            egui::pos2(0.0, 0.0),
            1.0,
        )
        .unwrap();
    let svg = library
        .persist_imported_image(
            &svg_path,
            "image/svg+xml",
            1,
            1,
            fs::metadata(&svg_path).unwrap().len(),
            egui::pos2(1.0, 0.0),
            1.0,
        )
        .unwrap();

    let active_jobs = library
        .missing_active_board_asset_jobs_for_model("mobileclip_s0")
        .unwrap();
    let live_jobs = library
        .missing_live_canvas_asset_jobs_for_model("mobileclip_s0")
        .unwrap();

    assert_eq!(
        active_jobs
            .iter()
            .map(|job| job.asset_id)
            .collect::<Vec<_>>(),
        vec![raster.asset.as_ref().expect("loaded placement asset").id]
    );
    assert_eq!(
        live_jobs.iter().map(|job| job.asset_id).collect::<Vec<_>>(),
        vec![raster.asset.as_ref().expect("loaded placement asset").id]
    );
    assert!(
        !active_jobs
            .iter()
            .any(|job| job.asset_id == svg.asset.as_ref().expect("loaded placement asset").id)
    );
    assert!(
        !live_jobs
            .iter()
            .any(|job| job.asset_id == svg.asset.as_ref().expect("loaded placement asset").id)
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn active_board_embedding_backfill_jobs_include_active_placements_only() {
    let root = temp_root("snappyref-active-board-embedding-backfill-test");
    let library = Library::initialize(root.clone()).unwrap();
    let second = library.create_board("Second").unwrap();
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
        .persist_imported_image(
            &active_path,
            "image/png",
            1,
            1,
            4,
            egui::pos2(0.0, 0.0),
            1.0,
        )
        .unwrap();
    let ready = library
        .persist_imported_image(&ready_path, "image/png", 1, 1, 4, egui::pos2(1.0, 0.0), 1.0)
        .unwrap();
    library
        .upsert_asset_embedding_blob(
            ready.asset.as_ref().expect("loaded placement asset").id,
            "mobileclip_s0",
            &[1.0, 0.0],
        )
        .unwrap();

    library.set_active_board(second.id).unwrap();
    let inactive = library
        .persist_imported_image(
            &inactive_path,
            "image/png",
            1,
            1,
            4,
            egui::pos2(2.0, 0.0),
            1.0,
        )
        .unwrap();
    library.set_active_board(1).unwrap();

    let jobs = library
        .missing_active_board_asset_jobs_for_model("mobileclip_s0")
        .unwrap();

    let job_asset_ids = jobs.iter().map(|job| job.asset_id).collect::<Vec<_>>();
    assert_eq!(
        job_asset_ids,
        vec![active.asset.as_ref().expect("loaded placement asset").id]
    );
    assert!(!job_asset_ids.contains(&ready.asset.as_ref().expect("loaded placement asset").id));
    assert!(!job_asset_ids.contains(&inactive.asset.as_ref().expect("loaded placement asset").id));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn embedding_backfill_retries_failed_assets_after_never_tried_assets() {
    let root = temp_root("snappyref-embedding-retry-order-test");
    let library = Library::initialize(root.clone()).unwrap();
    let retry_path = library
        .store_rgba_image("retry", &[255, 0, 0, 255], 1, 1)
        .unwrap();
    let fresh_path = library
        .store_rgba_image("fresh", &[0, 255, 0, 255], 1, 1)
        .unwrap();

    let retry = library
        .persist_imported_image(&retry_path, "image/png", 1, 1, 4, egui::pos2(0.0, 0.0), 1.0)
        .unwrap();
    let fresh = library
        .persist_imported_image(&fresh_path, "image/png", 1, 1, 4, egui::pos2(1.0, 0.0), 1.0)
        .unwrap();

    library
        .record_retryable_asset_embedding_failure(
            retry.asset.as_ref().expect("loaded placement asset").id,
            "mobileclip_s0",
            768,
            "temporary failure",
            1_000,
        )
        .unwrap();

    let fresh_jobs = library
        .missing_live_canvas_asset_jobs_for_model("mobileclip_s0")
        .unwrap();
    assert_eq!(
        fresh_jobs
            .iter()
            .map(|job| job.asset_id)
            .collect::<Vec<_>>(),
        vec![fresh.asset.as_ref().expect("loaded placement asset").id]
    );

    let retry_jobs = library
        .retryable_live_canvas_asset_jobs_for_model("mobileclip_s0", 1_000)
        .unwrap();
    assert_eq!(
        retry_jobs
            .iter()
            .map(|job| job.asset_id)
            .collect::<Vec<_>>(),
        vec![retry.asset.as_ref().expect("loaded placement asset").id]
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn embedding_retryable_failures_get_one_immediate_and_one_delayed_retry() {
    let root = temp_root("snappyref-embedding-retry-schedule-test");
    let library = Library::initialize(root.clone()).unwrap();
    let path = library
        .store_rgba_image("retry", &[255, 0, 0, 255], 1, 1)
        .unwrap();
    let record = library
        .persist_imported_image(&path, "image/png", 1, 1, 4, egui::pos2(0.0, 0.0), 1.0)
        .unwrap();

    library
        .record_retryable_asset_embedding_failure(
            record.asset.as_ref().expect("loaded placement asset").id,
            "mobileclip_s0",
            768,
            "temporary failure",
            1_000,
        )
        .unwrap();
    assert_eq!(
        library
            .retryable_live_canvas_asset_jobs_for_model("mobileclip_s0", 1_000)
            .unwrap()
            .len(),
        1
    );

    library
        .record_retryable_asset_embedding_failure(
            record.asset.as_ref().expect("loaded placement asset").id,
            "mobileclip_s0",
            768,
            "temporary failure again",
            1_010,
        )
        .unwrap();
    assert!(
        library
            .retryable_live_canvas_asset_jobs_for_model("mobileclip_s0", 1_309)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        library
            .retryable_live_canvas_asset_jobs_for_model("mobileclip_s0", 1_310)
            .unwrap()
            .len(),
        1
    );

    library
        .record_retryable_asset_embedding_failure(
            record.asset.as_ref().expect("loaded placement asset").id,
            "mobileclip_s0",
            768,
            "temporary failure exhausted",
            1_310,
        )
        .unwrap();
    assert!(
        library
            .retryable_live_canvas_asset_jobs_for_model("mobileclip_s0", 2_000)
            .unwrap()
            .is_empty()
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn pending_retry_preserves_embedding_failure_count_until_success() {
    let root = temp_root("snappyref-pending-retry-count-test");
    let library = Library::initialize(root.clone()).unwrap();
    let path = library
        .store_rgba_image("retry", &[255, 0, 0, 255], 1, 1)
        .unwrap();
    let record = library
        .persist_imported_image(&path, "image/png", 1, 1, 4, egui::pos2(0.0, 0.0), 1.0)
        .unwrap();

    library
        .record_retryable_asset_embedding_failure(
            record.asset.as_ref().expect("loaded placement asset").id,
            "mobileclip_s0",
            768,
            "temporary failure",
            1_000,
        )
        .unwrap();
    library
        .upsert_asset_embedding_status(
            record.asset.as_ref().expect("loaded placement asset").id,
            "mobileclip_s0",
            768,
            EmbeddingStatus::Pending,
            None,
        )
        .unwrap();
    library
        .record_retryable_asset_embedding_failure(
            record.asset.as_ref().expect("loaded placement asset").id,
            "mobileclip_s0",
            768,
            "temporary failure again",
            1_010,
        )
        .unwrap();

    assert!(
        library
            .retryable_live_canvas_asset_jobs_for_model("mobileclip_s0", 1_309)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        library
            .retryable_live_canvas_asset_jobs_for_model("mobileclip_s0", 1_310)
            .unwrap()
            .len(),
        1
    );

    library
        .upsert_asset_embedding_blob(
            record.asset.as_ref().expect("loaded placement asset").id,
            "mobileclip_s0",
            &[1.0, 0.0],
        )
        .unwrap();
    assert!(
        library
            .retryable_live_canvas_asset_jobs_for_model("mobileclip_s0", 2_000)
            .unwrap()
            .is_empty()
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn terminal_embedding_failures_are_not_automatic_retry_jobs() {
    let root = temp_root("snappyref-terminal-embedding-failure-test");
    let library = Library::initialize(root.clone()).unwrap();
    let path = library
        .store_rgba_image("terminal", &[255, 0, 0, 255], 1, 1)
        .unwrap();
    let record = library
        .persist_imported_image(&path, "image/png", 1, 1, 4, egui::pos2(0.0, 0.0), 1.0)
        .unwrap();

    library
        .record_terminal_asset_embedding_failure(
            record.asset.as_ref().expect("loaded placement asset").id,
            "mobileclip_s0",
            768,
            "failed to open image",
        )
        .unwrap();

    assert!(
        library
            .missing_live_canvas_asset_jobs_for_model("mobileclip_s0")
            .unwrap()
            .is_empty()
    );
    assert!(
        library
            .retryable_live_canvas_asset_jobs_for_model("mobileclip_s0", 10_000)
            .unwrap()
            .is_empty()
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn next_embedding_backfill_job_prefers_never_tried_assets_before_retries() {
    let root = temp_root("snappyref-next-embedding-backfill-job-test");
    let library = Library::initialize(root.clone()).unwrap();
    let retry_path = library
        .store_rgba_image("retry", &[255, 0, 0, 255], 1, 1)
        .unwrap();
    let fresh_path = library
        .store_rgba_image("fresh", &[0, 255, 0, 255], 1, 1)
        .unwrap();

    let retry = library
        .persist_imported_image(&retry_path, "image/png", 1, 1, 4, egui::pos2(0.0, 0.0), 1.0)
        .unwrap();
    let fresh = library
        .persist_imported_image(&fresh_path, "image/png", 1, 1, 4, egui::pos2(1.0, 0.0), 1.0)
        .unwrap();
    library
        .record_retryable_asset_embedding_failure(
            retry.asset.as_ref().expect("loaded placement asset").id,
            "mobileclip_s0",
            768,
            "temporary failure",
            1_000,
        )
        .unwrap();

    let job = library
        .next_embedding_backfill_job_for_model("mobileclip_s0", 1_000)
        .unwrap()
        .unwrap();
    assert_eq!(
        job.asset_id,
        fresh.asset.as_ref().expect("loaded placement asset").id
    );

    library
        .upsert_asset_embedding_blob(
            fresh.asset.as_ref().expect("loaded placement asset").id,
            "mobileclip_s0",
            &[1.0, 0.0],
        )
        .unwrap();
    let retry_job = library
        .next_embedding_backfill_job_for_model("mobileclip_s0", 1_000)
        .unwrap()
        .unwrap();
    assert_eq!(
        retry_job.asset_id,
        retry.asset.as_ref().expect("loaded placement asset").id
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn cropped_relink_requires_new_asset_embedding() {
    let root = std::env::temp_dir().join(format!(
        "snappyref-crop-embedding-relink-test-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&root).unwrap();

    let library = Library::initialize(root.clone()).unwrap();
    let original_path = library
        .store_rgba_image("original", &[255, 0, 0, 255], 1, 1)
        .unwrap();
    let cropped_path = library
        .store_rgba_image("cropped", &[0, 255, 0, 255], 1, 1)
        .unwrap();

    let record = library
        .persist_imported_image(
            &original_path,
            "image/png",
            1,
            1,
            4,
            egui::pos2(0.0, 0.0),
            1.0,
        )
        .unwrap();
    library
        .upsert_asset_embedding_blob(
            record.asset.as_ref().expect("loaded placement asset").id,
            "mobileclip_s0",
            &[1.0, 0.0],
        )
        .unwrap();

    let relinked_asset = library
        .create_asset_and_relink_placement(
            record.state.id,
            &cropped_path,
            "image/png",
            1,
            1,
            4,
            egui::pos2(0.0, 0.0),
        )
        .unwrap();

    assert_eq!(
        library
            .load_asset_embedding(relinked_asset.id, "mobileclip_s0")
            .unwrap(),
        None
    );
    assert!(
        library
            .load_active_canvas_embeddings("mobileclip_s0")
            .unwrap()
            .is_empty()
    );

    library
        .upsert_asset_embedding_blob(relinked_asset.id, "mobileclip_s0", &[0.0, 1.0])
        .unwrap();
    let embeddings = library
        .load_active_canvas_embeddings("mobileclip_s0")
        .unwrap();

    assert_eq!(embeddings.len(), 1);
    assert_eq!(embeddings[0].asset_id, relinked_asset.id);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn embedding_helpers_scope_to_requested_model_key() {
    let root = std::env::temp_dir().join(format!(
        "snappyref-embedding-model-key-test-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&root).unwrap();

    let library = Library::initialize(root.clone()).unwrap();
    let first_path = library
        .store_rgba_image("first", &[255, 0, 0, 255], 1, 1)
        .unwrap();
    let second_path = library
        .store_rgba_image("second", &[0, 255, 0, 255], 1, 1)
        .unwrap();

    let first = library
        .persist_imported_image(&first_path, "image/png", 1, 1, 4, egui::pos2(0.0, 0.0), 1.0)
        .unwrap();
    let second = library
        .persist_imported_image(
            &second_path,
            "image/png",
            1,
            1,
            4,
            egui::pos2(10.0, 0.0),
            1.0,
        )
        .unwrap();

    library
        .upsert_asset_embedding_blob(
            first.asset.as_ref().expect("loaded placement asset").id,
            "clip_key",
            &[1.0, 0.0],
        )
        .unwrap();
    library
        .upsert_asset_embedding_blob(
            second.asset.as_ref().expect("loaded placement asset").id,
            "clip_key",
            &[0.0, 1.0],
        )
        .unwrap();
    library
        .upsert_asset_embedding_blob(
            first.asset.as_ref().expect("loaded placement asset").id,
            "siglip_key",
            &[0.5, 0.5],
        )
        .unwrap();

    let jobs = library
        .missing_live_canvas_asset_jobs_for_model("siglip_key")
        .unwrap();
    assert_eq!(jobs.len(), 1);
    assert_eq!(
        jobs[0].asset_id,
        second.asset.as_ref().expect("loaded placement asset").id
    );

    let embeddings = library.load_active_canvas_embeddings("siglip_key").unwrap();
    assert_eq!(embeddings.len(), 1);
    assert_eq!(
        embeddings[0].asset_id,
        first.asset.as_ref().expect("loaded placement asset").id
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn canvas_annotations_round_trip_and_archive() {
    let root = std::env::temp_dir().join(format!(
        "snappyref-annotation-test-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&root).unwrap();

    let library = Library::initialize(root.clone()).unwrap();
    let created = library
        .create_canvas_annotation(
            CanvasAnnotationKind::Textbox,
            egui::pos2(10.0, 12.0),
            egui::pos2(110.0, 72.0),
            "hello",
            CanvasTextSize::default(),
            crate::data::CanvasStrokeWidth::default(),
            crate::data::CanvasAnnotationColor::default(),
            false,
        )
        .unwrap();

    let loaded = library.load_canvas_annotations().unwrap();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0], created);
    assert_eq!(loaded[0].text_size, CanvasTextSize::default());

    let mut updated = loaded[0].clone();
    updated.text = "updated".to_owned();
    library.update_canvas_annotation(&updated).unwrap();

    let loaded = library.load_canvas_annotations().unwrap();
    assert_eq!(loaded[0].text, "updated");

    library.archive_canvas_annotations(&[created.id]).unwrap();
    assert!(library.load_canvas_annotations().unwrap().is_empty());

    library.restore_canvas_annotations(&[created.id]).unwrap();
    let restored = library.load_canvas_annotations().unwrap();
    assert_eq!(restored.len(), 1);
    assert_eq!(restored[0].id, created.id);
    assert_eq!(restored[0].text, "updated");
    assert_eq!(
        library.load_canvas_stack_order().unwrap(),
        vec![CanvasStackItem::Annotation(created.id)]
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn existing_canvas_annotations_rows_migrate_text_size_to_default_m() {
    let root = std::env::temp_dir().join(format!(
        "snappyref-annotation-migration-test-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&root).unwrap();

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

    let library = Library::initialize(root.clone()).unwrap();
    let loaded = library.load_canvas_annotations().unwrap();

    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].text_size, CanvasTextSize::M);

    let _ = fs::remove_dir_all(root);
}
