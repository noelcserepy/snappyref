use super::*;
use crate::active_board::history::{
    BoardLifecycleHistoryAction, DocumentHistory, HistoryAction, HistoryChange,
};
use crate::data::{
    CanvasAnnotation, CanvasAnnotationColor, CanvasStrokeWidth, PointerMode, RectGeometry,
    ViewportState, WindowState, WorkspaceSettings,
};

#[test]
fn content_upgrade_preserves_placements_undo_redo_and_archived_board_snapshots() {
    let root = TempRoot::new("stored-content-upgrade");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let loading = library
        .fixture_placement(
            library.load_user_session().unwrap().active_board_id,
            library.new_committed_loading_placement(
                egui::pos2(31.0, 42.0),
                2.0,
                eframe::egui::vec2(90.0, 70.0),
                ContentResolutionSource::ExternalPath {
                    path: "/tmp/original.png".into(),
                },
                Some("Queued".into()),
            ),
        )
        .unwrap();
    let mut failed = loading.clone();
    failed.content = PlacementContent::Failed {
        source: Some(ContentResolutionSource::Clipboard),
        message: "Cannot decode".into(),
        recoverable: true,
    };
    let mut history = DocumentHistory::default();
    history.record_applied(
        1,
        HistoryAction::document(
            "Import".into(),
            vec![HistoryChange {
                before: None,
                after: Some(loading.clone()),
            }],
            vec![],
            None,
        ),
    );
    history.record_applied(
        1,
        HistoryAction::document(
            "Fail".into(),
            vec![HistoryChange {
                before: Some(loading.clone()),
                after: Some(failed.clone()),
            }],
            vec![],
            None,
        ),
    );
    history.undo(1).unwrap();
    let board = library.fixture_board("Archived").unwrap();
    let mut snapshot = library.load_board_document(board.id).unwrap();
    snapshot.placements = vec![failed.clone()];
    history.record_applied(
        board.id,
        HistoryAction::Board(BoardLifecycleHistoryAction::Deleted {
            document: snapshot,
            active_before: board.id,
            active_after: 1,
        }),
    );
    library
        .persist_board_unit(
            &crate::library::storage::BoardDurabilityOperation::Lifecycle(
                crate::active_board::persistence::BoardLifecyclePersistence::Archive {
                    board_id: board.id,
                    active_board_id: 1,
                },
            ),
            &Default::default(),
        )
        .unwrap();
    library
        .persist_board_unit(
            &crate::library::storage::BoardDurabilityOperation::Document {
                board_id: 1,
                changes: crate::canvas::CanvasDocumentChanges::default(),
            },
            &history,
        )
        .unwrap();
    let expected_history: serde_json::Value =
        serde_json::from_str(&history.encode().unwrap()).unwrap();
    drop(library);
    let connection = Connection::open(root.join("library.db")).unwrap();
    connection.execute_batch("CREATE TABLE board_document_history (id INTEGER PRIMARY KEY, payload TEXT NOT NULL); DROP TABLE board_history;").unwrap();
    connection
        .execute(
            "INSERT INTO board_document_history VALUES (1, ?1)",
            [expected_history.to_string()],
        )
        .unwrap();
    connection.execute_batch("PRAGMA user_version = 0;
        UPDATE board_items SET content_resolution_intent = replace(content_resolution_intent, '\"source\":', '\"intent\":');
        UPDATE board_document_history SET payload = replace(payload, '\"source\":', '\"intent\":');").unwrap();
    drop(connection);
    let library = Library::initialize(root.to_path_buf()).unwrap();
    assert_eq!(
        library.load_placements().unwrap()[0].state.id,
        loading.state.id
    );
    assert_eq!(
        library.load_placements().unwrap()[0].state.geometry,
        loading.state.geometry
    );
    assert_eq!(
        library.load_placements().unwrap()[0].content,
        loading.content
    );
    let mut restored = library.load_document_history().unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&restored.encode().unwrap()).unwrap(),
        expected_history
    );
    let redo = restored.redo(1).unwrap();
    assert_eq!(redo.action.label(), "Fail");
    let undo = restored.undo(1).unwrap();
    assert_eq!(undo.action.label(), "Fail");
    let archived_undo = restored.undo(board.id).unwrap();
    let HistoryAction::Board(BoardLifecycleHistoryAction::Deleted { document, .. }) =
        archived_undo.action
    else {
        panic!("expected archived snapshot");
    };
    assert_eq!(document.placements.len(), 1);
    assert_eq!(document.placements[0].state.id, failed.state.id);
    assert_eq!(document.placements[0].state.geometry, failed.state.geometry);
    assert_eq!(document.placements[0].content, failed.content);
    drop(library);
    let reopened = Library::initialize(root.to_path_buf()).unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(
            &reopened.load_document_history().unwrap().encode().unwrap()
        )
        .unwrap(),
        expected_history
    );
}

#[test]
fn migration_failure_rolls_back_settings_schema_and_content_together() {
    let root = TempRoot::new("stored-upgrade-atomicity");
    let connection = Connection::open(root.join("library.db")).unwrap();
    connection.execute_batch("CREATE TABLE user_session (id INTEGER PRIMARY KEY, camera_pan_x REAL, camera_pan_y REAL, camera_zoom REAL, placement_gap REAL);
        INSERT INTO user_session VALUES (1, 12, 34, 1.5, 48);
        CREATE TABLE board_document_history (id INTEGER PRIMARY KEY, payload TEXT NOT NULL);
        INSERT INTO board_document_history VALUES (1, 'broken history');").unwrap();
    let schema_before: Vec<String> = connection
        .prepare("SELECT sql FROM sqlite_master WHERE sql IS NOT NULL ORDER BY name")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    drop(connection);
    assert!(
        Library::initialize(root.to_path_buf())
            .unwrap_err()
            .contains("cannot upgrade board_document_history")
    );
    let connection = Connection::open(root.join("library.db")).unwrap();
    let schema_after: Vec<String> = connection
        .prepare("SELECT sql FROM sqlite_master WHERE sql IS NOT NULL ORDER BY name")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(schema_after, schema_before);
    assert_eq!(
        connection
            .query_row("SELECT placement_gap FROM user_session", [], |row| row
                .get::<_, f64>(0))
            .unwrap(),
        48.0
    );
    assert_eq!(
        connection
            .query_row("SELECT payload FROM board_document_history", [], |row| {
                row.get::<_, String>(0)
            })
            .unwrap(),
        "broken history"
    );
    connection
        .execute("DELETE FROM board_document_history", [])
        .unwrap();
    drop(connection);
    let recovered = Library::initialize(root.to_path_buf()).unwrap();
    let session = recovered.load_user_session().unwrap();
    assert_eq!(session.settings.placement_gap, 48.0);
    assert_eq!(session.viewport.camera_pan_x, 12.0);
}

// The fixture was written by the shipped v0.2.0 build (122cd84) through its own
// storage API: imports resolved or failed, one crop, rotation, annotations,
// a reordered stack, one embedding and a saved session on each Board.
#[test]
fn library_written_by_v0_2_0_opens_with_boards_content_and_session() {
    let root = TempRoot::new("v0.2.0-library-upgrade");
    fs::copy(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/libraries/v0.2.0/library.db"),
        root.join("library.db"),
    )
    .unwrap();
    let library = Library::initialize(root.to_path_buf()).unwrap();

    assert_eq!(
        library
            .list_boards()
            .unwrap()
            .into_iter()
            .map(|board| (board.id, board.name))
            .collect::<Vec<_>>(),
        [(1, "Characters".to_owned()), (2, "Environments".to_owned())]
    );

    let characters = library.load_board_document(1).unwrap();
    assert_eq!(
        characters
            .placements
            .iter()
            .map(|placement| (placement.state.id, placement.state.geometry))
            .collect::<Vec<_>>(),
        [
            (
                1,
                RectGeometry::new(
                    egui::pos2(120.0, -40.0),
                    egui::vec2(40.0, 40.0),
                    1.5,
                    0.5,
                    0
                )
            ),
            (
                2,
                RectGeometry::new(
                    egui::pos2(210.0, 5.0),
                    egui::vec2(20.0, 30.0),
                    1.0,
                    -0.25,
                    0
                )
            ),
            (
                3,
                RectGeometry::new(
                    egui::pos2(-150.0, 60.0),
                    egui::vec2(50.0, 30.0),
                    1.0,
                    0.0,
                    0
                )
            ),
        ]
    );
    assert_eq!(characters.placements[0].require_loaded_asset().id, 1);
    // v0.2.0 crop relinked the asset_id column but left the stored content on
    // the uncropped Asset 2.
    let cropped = characters.placements[1].require_loaded_asset();
    assert_eq!((cropped.id, cropped.width, cropped.height), (3, 2, 3));
    assert_eq!(
        characters.placements[2].content,
        PlacementContent::Failed {
            source: Some(ContentResolutionSource::ExternalPath {
                path: "/refs/missing.png".to_owned(),
            }),
            message: "Cannot open /refs/missing.png".to_owned(),
            recoverable: true,
        }
    );
    assert_eq!(
        characters.annotations,
        [
            CanvasAnnotation::new(
                1,
                CanvasAnnotationKind::Textbox,
                egui::pos2(-100.0, -100.0),
                egui::pos2(60.0, -60.0),
                "Hero palette".to_owned(),
                CanvasTextSize::L,
                CanvasStrokeWidth::M,
                CanvasAnnotationColor::Coral,
                false,
            ),
            CanvasAnnotation::new(
                2,
                CanvasAnnotationKind::Line,
                egui::pos2(0.0, 80.0),
                egui::pos2(220.0, 90.0),
                String::new(),
                CanvasTextSize::M,
                CanvasStrokeWidth::L,
                CanvasAnnotationColor::Sky,
                false,
            ),
        ]
    );
    assert_eq!(
        characters.stack_order,
        [
            CanvasStackItem::Annotation(2),
            CanvasStackItem::Placement(2),
            CanvasStackItem::Placement(1),
            CanvasStackItem::Annotation(1),
            CanvasStackItem::Placement(3),
        ]
    );

    let environments = library.load_board_document(2).unwrap();
    assert_eq!(environments.placements[0].require_loaded_asset().id, 4);
    assert_eq!(
        environments.annotations,
        [CanvasAnnotation::new(
            3,
            CanvasAnnotationKind::Arrow,
            egui::pos2(-20.0, -20.0),
            egui::pos2(5.0, 15.0),
            String::new(),
            CanvasTextSize::M,
            CanvasStrokeWidth::S,
            CanvasAnnotationColor::Gold,
            false,
        )]
    );
    assert_eq!(
        environments.stack_order,
        [
            CanvasStackItem::Placement(4),
            CanvasStackItem::Annotation(3)
        ]
    );
    assert_eq!(
        environments.viewport,
        ViewportState {
            camera_pan_x: -50.0,
            camera_pan_y: 25.0,
            camera_zoom: 0.8,
            canvas_background: CanvasBackground::Dots,
        }
    );

    assert_eq!(
        library.load_user_session().unwrap(),
        UserSession {
            active_board_id: 1,
            viewport: ViewportState {
                camera_pan_x: 123.0,
                camera_pan_y: 456.0,
                camera_zoom: 1.75,
                canvas_background: CanvasBackground::Grid,
            },
            window: WindowState {
                outer_x: Some(40.0),
                outer_y: Some(60.0),
                inner_width: Some(1280.0),
                inner_height: Some(800.0),
            },
            settings: WorkspaceSettings {
                placement_gap: 36.0,
                placement_target_area: 54_321.0,
                canvas_image_max_side: 4096,
                semantic_search_first_launch_prompt_seen: true,
                rotation_snap_degrees: 30.0,
                pointer_mode: PointerMode::Trackpad,
                always_on_top: true,
                ai_control: Default::default(),
                last_update_check_at: Some(1_779_000_000_000),
            },
            sidebar_open: true,
            sidebar_tab: "boards".to_owned(),
            recent_searches: vec!["red hero".to_owned(), "forest".to_owned()],
        }
    );
    assert_eq!(
        library
            .load_asset_embedding(1, "siglip2_base_patch16_224_int8")
            .unwrap(),
        Some(
            [0.5_f32, -0.25, 0.125, 1.0]
                .iter()
                .flat_map(|value| value.to_ne_bytes())
                .collect()
        )
    );
}
