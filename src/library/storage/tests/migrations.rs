use super::*;
use crate::active_board::history::{
    BoardLifecycleHistoryAction, DocumentHistory, HistoryAction, HistoryChange,
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
