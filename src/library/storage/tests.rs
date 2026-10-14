use super::{Library, embeddings::EmbeddingStatus};
use crate::data::{
    CanvasAnnotationKind, CanvasBackground, CanvasStackItem, CanvasTextSize,
    ContentResolutionSource, PlacementContent, PlacementLifecycle, UserSession,
};
use crate::test_support::TempRoot;
use eframe::egui;
use rusqlite::Connection;
use std::fs;
use std::sync::{Arc, Barrier};

#[test]
fn loaded_placements_load_as_committed_loaded_content() {
    let root = TempRoot::new("snappyref-loaded-placement-content-test");
    let source = root.join("source.png");
    fs::write(&source, [255, 255, 255, 255]).unwrap();
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let placement = library
        .fixture_placement(
            library.load_user_session().unwrap().active_board_id,
            crate::data::Placement::loaded(
                crate::data::PlacementState::new(
                    library.allocate_document_id(),
                    egui::pos2(3.0, 4.0),
                    2.0,
                    0.0,
                    0,
                    false,
                ),
                library
                    .register_original(&source, "image/png", 10, 20, 4, None)
                    .unwrap(),
            ),
        )
        .unwrap();

    let reloaded = library.load_placements().unwrap();
    assert_eq!(reloaded.len(), 1);
    assert_eq!(reloaded[0].state.id, placement.state.id);
    assert_eq!(
        reloaded[0].require_loaded_asset(),
        placement.require_loaded_asset()
    );
    assert_eq!(reloaded[0].lifecycle, PlacementLifecycle::Committed);
    assert_eq!(reloaded[0].state.geometry.size, egui::vec2(10.0, 20.0));
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
fn update_preservation_smoke() {
    let app_root = TempRoot::new("snappyref-update-preservation-test");
    let library_root = app_root.join("library");
    fs::create_dir_all(&library_root).unwrap();
    fs::write(app_root.join("SnappyRef.app"), b"old app bundle").unwrap();
    fs::write(app_root.join("UPDATE.txt"), b"old update notes").unwrap();

    let original_bytes = [255, 0, 0, 255];
    let library = Library::initialize(library_root.clone()).unwrap();
    let first_board = library.list_boards().unwrap()[0].clone();
    let second_board = library.fixture_board("Second").unwrap();
    library.set_active_board(second_board.id).unwrap();
    let original_path = library
        .store_rgba_image("preserved-original", &original_bytes, 1, 1)
        .unwrap();
    let imported = library
        .fixture_placement(
            library.load_user_session().unwrap().active_board_id,
            crate::data::Placement::loaded(
                crate::data::PlacementState::new(
                    library.allocate_document_id(),
                    egui::pos2(42.0, 24.0),
                    1.5,
                    0.0,
                    0,
                    false,
                ),
                library
                    .register_original(
                        &original_path,
                        "image/png",
                        1,
                        1,
                        original_bytes.len() as u64,
                        None,
                    )
                    .unwrap(),
            ),
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
    session.settings.placement_gap = 36.0;
    session.settings.placement_target_area = 54_321.0;
    session.settings.canvas_image_max_side = 4096;
    session.settings.rotation_snap_degrees = 30.0;
    session.settings.always_on_top = true;
    session.sidebar_open = true;
    session.sidebar_tab = "boards".to_owned();
    library.save_user_session(&session).unwrap();
    let stored_original_bytes = fs::read(&imported.require_loaded_asset().original_path).unwrap();
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
    assert_eq!(reloaded_session.settings.placement_gap, 36.0);
    assert_eq!(reloaded_session.settings.placement_target_area, 54_321.0);
    assert_eq!(reloaded_session.settings.canvas_image_max_side, 4096);
    assert_eq!(reloaded_session.settings.rotation_snap_degrees, 30.0);
    assert!(reloaded_session.settings.always_on_top);
    assert!(reloaded_session.sidebar_open);
    assert_eq!(reloaded_session.sidebar_tab, "boards");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].state.id, imported.state.id);
    assert_eq!(
        items[0].require_loaded_asset().id,
        imported.require_loaded_asset().id
    );
    assert_eq!(items[0].state.geometry.center.x, 42.0);
    assert_eq!(items[0].state.geometry.center.y, 24.0);
    assert_eq!(items[0].state.geometry.scale, 1.5);
    assert_eq!(
        items[0].require_loaded_asset().original_path,
        imported.require_loaded_asset().original_path
    );
    assert!(items[0].require_loaded_asset().original_path.exists());
    assert_eq!(
        fs::read(&items[0].require_loaded_asset().original_path).unwrap(),
        stored_original_bytes
    );
}

#[test]
fn user_session_payload_takes_precedence_over_legacy_preference_columns() {
    let root = TempRoot::new("snappyref-session-payload-precedence");
    let library = Library::initialize(root.to_path_buf()).unwrap();

    let mut session = library.load_user_session().unwrap();
    session.settings.placement_gap = 48.0;
    session.settings.placement_target_area = 45_000.0;
    session.settings.canvas_image_max_side = 4096;
    session.settings.rotation_snap_degrees = 45.0;
    session.sidebar_open = true;
    session.sidebar_tab = "boards".to_owned();
    library.save_user_session(&session).unwrap();
    drop(library);

    let connection = Connection::open(root.join("library.db")).unwrap();
    connection
        .execute_batch(
            "PRAGMA user_version = 0;
        ALTER TABLE user_session ADD COLUMN camera_pan_x REAL NOT NULL DEFAULT 0;
        ALTER TABLE user_session ADD COLUMN camera_pan_y REAL NOT NULL DEFAULT 0;
        ALTER TABLE user_session ADD COLUMN camera_zoom REAL NOT NULL DEFAULT 1;
        ALTER TABLE user_session ADD COLUMN auto_image_scaling INTEGER DEFAULT 1;
        ALTER TABLE user_session ADD COLUMN placement_gap REAL DEFAULT 12;
        ALTER TABLE user_session ADD COLUMN import_target_canvas_area REAL DEFAULT 12;
        ALTER TABLE user_session ADD COLUMN semantic_search_enabled INTEGER DEFAULT 1;
        ALTER TABLE user_session ADD COLUMN canvas_image_max_side INTEGER DEFAULT 100;
        ALTER TABLE user_session ADD COLUMN rotation_snap_degrees REAL DEFAULT 5;
        ALTER TABLE user_session ADD COLUMN default_textbox_background INTEGER DEFAULT 0;
        ALTER TABLE user_session ADD COLUMN sidebar_open INTEGER DEFAULT 0;
        ALTER TABLE user_session ADD COLUMN sidebar_tab TEXT DEFAULT 'settings';",
        )
        .unwrap();
    connection
        .execute(
            "UPDATE user_session
             SET auto_image_scaling = 1,
                 placement_gap = 12.0,
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

    let library = Library::initialize(root.to_path_buf()).unwrap();
    let reloaded = library.load_user_session().unwrap();

    assert_eq!(reloaded.settings.placement_gap, 48.0);
    assert_eq!(reloaded.settings.placement_target_area, 45_000.0);
    assert_eq!(reloaded.settings.canvas_image_max_side, 4096);
    assert_eq!(reloaded.settings.rotation_snap_degrees, 45.0);
    assert!(reloaded.sidebar_open);
    assert_eq!(reloaded.sidebar_tab, "boards");
}

#[test]
fn existing_legacy_user_session_columns_backfill_payload() {
    let root = TempRoot::new("snappyref-session-payload-backfill");
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
                placement_gap REAL NOT NULL DEFAULT 24.0,
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
                placement_gap,
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

    let library = Library::initialize(root.to_path_buf()).unwrap();
    let session = library.load_user_session().unwrap();

    assert_eq!(session.viewport.camera_pan_x, 10.0);
    assert_eq!(session.viewport.camera_pan_y, 20.0);
    assert_eq!(session.viewport.camera_zoom, 1.5);
    assert_eq!(session.viewport.canvas_background, CanvasBackground::Grid);
    assert_eq!(session.settings.placement_gap, 48.0);
    assert_eq!(session.settings.placement_target_area, 45_000.0);
    assert_eq!(session.settings.canvas_image_max_side, 4096);
    assert_eq!(session.settings.rotation_snap_degrees, 45.0);
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
    let columns: Vec<String> = connection
        .prepare("PRAGMA table_info(user_session)")
        .unwrap()
        .query_map([], |row| row.get(1))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(columns, ["id", "session_payload", "active_board_id"]);
    drop(connection);

    let library = Library::initialize(root.to_path_buf()).unwrap();
    let reloaded = library.load_user_session().unwrap();
    assert_eq!(reloaded.settings.placement_gap, 48.0);
    assert!(reloaded.sidebar_open);
    assert_eq!(reloaded.sidebar_tab, "boards");
}

#[test]
fn existing_user_session_rows_migrate_to_default_preferences() {
    let root = TempRoot::new("snappyref-storage-test");

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

    let library = Library::initialize(root.to_path_buf()).unwrap();
    let session = library.load_user_session().unwrap();

    assert_eq!(session.viewport.camera_pan_x, 10.0);
    assert_eq!(session.viewport.camera_pan_y, 20.0);
    assert_eq!(session.viewport.camera_zoom, 1.5);
    assert_eq!(
        session.settings.placement_gap,
        crate::data::DEFAULT_PLACEMENT_GAP
    );
    assert_eq!(
        session.settings.placement_target_area,
        crate::data::DEFAULT_PLACEMENT_TARGET_AREA
    );
    assert_eq!(
        session.settings.canvas_image_max_side,
        crate::data::DEFAULT_CANVAS_IMAGE_MAX_SIDE
    );
    assert_eq!(session.viewport.canvas_background, CanvasBackground::None);
}

mod durability;

mod boards;

mod assets;

mod migrations;

mod session;
