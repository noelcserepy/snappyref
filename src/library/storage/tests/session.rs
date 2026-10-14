use super::*;

#[test]
fn rejected_viewport_save_preserves_the_whole_session_after_reopen() {
    let root = TempRoot::new("snappyref-atomic-session-save");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let next_board = library.fixture_board("Next board").unwrap();
    let mut original = UserSession::default();
    original.viewport.camera_pan_x = 12.0;
    original.viewport.camera_pan_y = 34.0;
    original.viewport.camera_zoom = 1.5;
    original.viewport.canvas_background = CanvasBackground::Grid;
    original.settings.placement_gap = 36.0;
    original.recent_searches = vec!["landscape".to_owned()];
    library.save_user_session(&original).unwrap();

    let mut replacement = original.clone();
    replacement.active_board_id = next_board.id;
    replacement.viewport.camera_pan_x = 56.0;
    replacement.viewport.camera_pan_y = 78.0;
    replacement.viewport.camera_zoom = 2.5;
    replacement.viewport.canvas_background = CanvasBackground::None;
    replacement.settings.placement_gap = 48.0;
    replacement.sidebar_open = true;
    replacement.sidebar_tab = "boards".to_owned();
    replacement.recent_searches = vec!["portrait".to_owned()];

    let connection = Connection::open(root.join("library.db")).unwrap();
    connection
        .execute_batch(
            "CREATE TRIGGER reject_viewport_save
             BEFORE UPDATE OF camera_zoom ON boards
             BEGIN SELECT RAISE(ABORT, 'viewport rejected'); END;",
        )
        .unwrap();
    assert!(
        library
            .save_user_session(&replacement)
            .unwrap_err()
            .contains("viewport rejected")
    );
    drop(library);

    let reopened = Library::initialize(root.to_path_buf()).unwrap();
    assert_eq!(reopened.load_user_session().unwrap(), original);
    assert_eq!(
        reopened
            .load_board_document(next_board.id)
            .unwrap()
            .viewport,
        crate::data::ViewportState {
            camera_pan_x: 0.0,
            camera_pan_y: 0.0,
            camera_zoom: 0.45,
            canvas_background: CanvasBackground::None,
        }
    );

    connection
        .execute_batch("DROP TRIGGER reject_viewport_save;")
        .unwrap();
    reopened.save_user_session(&replacement).unwrap();
    drop(reopened);
    let reopened = Library::initialize(root.to_path_buf()).unwrap();
    assert_eq!(reopened.load_user_session().unwrap(), replacement);
}
