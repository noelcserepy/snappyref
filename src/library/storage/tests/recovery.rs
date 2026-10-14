use super::*;

#[test]
fn corrupt_session_payload_restores_defaults_and_preserves_board_viewport() {
    let root = TempRoot::new("corrupt-session-payload");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let board = library.fixture_board("References").unwrap();
    let mut session = library.load_user_session().unwrap();
    session.active_board_id = board.id;
    session.viewport.camera_pan_x = 12.0;
    session.viewport.camera_pan_y = -30.0;
    session.viewport.camera_zoom = 2.0;
    session.viewport.canvas_background = CanvasBackground::Grid;
    session.sidebar_open = true;
    session.sidebar_tab = "boards".to_owned();
    session.recent_searches = vec!["mountains".to_owned()];
    session.settings.placement_gap = 75.0;
    library.save_user_session(&session).unwrap();
    assert_eq!(library.load_user_session().unwrap(), session);
    let boards = library.list_boards().unwrap();
    drop(library);
    let connection = Connection::open(root.join("library.db")).unwrap();
    connection
        .execute(
            "UPDATE user_session SET session_payload = '{broken' WHERE id = 1",
            [],
        )
        .unwrap();
    drop(connection);

    let reopened = Library::initialize(root.to_path_buf()).unwrap();
    let restored = reopened.load_user_session().unwrap();
    assert_eq!(
        restored,
        UserSession {
            active_board_id: board.id,
            viewport: crate::data::ViewportState {
                camera_pan_x: 12.0,
                camera_pan_y: -30.0,
                camera_zoom: 2.0,
                canvas_background: CanvasBackground::Grid,
            },
            window: crate::data::WindowState {
                outer_x: None,
                outer_y: None,
                inner_width: None,
                inner_height: None
            },
            settings: crate::data::WorkspaceSettings {
                placement_gap: 0.0,
                placement_target_area: 90_000.0,
                canvas_image_max_side: 2000,
                semantic_search_first_launch_prompt_seen: false,
                rotation_snap_degrees: 15.0,
                pointer_mode: crate::data::PointerMode::Mouse,
                always_on_top: false,
                ai_control: crate::data::AiControlAccess::Off,
                last_update_check_at: None,
            },
            sidebar_open: false,
            sidebar_tab: "settings".to_owned(),
            recent_searches: vec![],
        }
    );
    assert_eq!(reopened.list_boards().unwrap(), boards);
}

#[test]
fn corrupt_placement_payload_becomes_recoverable_failure_without_losing_other_content() {
    let root = TempRoot::new("corrupt-placement-payload");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let path = library
        .store_rgba_image("intact", &[255, 0, 0, 255], 1, 1)
        .unwrap();
    let asset = library
        .register_original(&path, "image/png", 1, 1, 4, None)
        .unwrap();
    let loaded = library
        .fixture_placement(
            1,
            crate::data::Placement::loaded(
                crate::data::PlacementState::new(
                    library.allocate_document_id(),
                    egui::pos2(10.0, 20.0),
                    2.0,
                    0.0,
                    0,
                    false,
                ),
                asset.clone(),
            ),
        )
        .unwrap();
    let loading = library
        .fixture_placement(
            1,
            library.new_committed_loading_placement(
                egui::pos2(-30.0, 40.0),
                1.5,
                egui::vec2(80.0, 60.0),
                ContentResolutionSource::ExternalPath {
                    path: "pending.png".to_owned(),
                },
                Some("Loading".to_owned()),
            ),
        )
        .unwrap();
    let original_bytes = fs::read(&path).unwrap();
    assert_eq!(
        library
            .load_placements()
            .unwrap()
            .iter()
            .map(placement_snapshot)
            .collect::<Vec<_>>(),
        vec![placement_snapshot(&loaded), placement_snapshot(&loading)]
    );
    drop(library);
    let connection = Connection::open(root.join("library.db")).unwrap();
    connection
        .execute(
            "UPDATE board_items SET content_resolution_intent = '{broken' WHERE id = ?1",
            [loading.state.id],
        )
        .unwrap();
    drop(connection);

    let reopened = Library::initialize(root.to_path_buf()).unwrap();
    let expected = crate::data::Placement {
        state: loading.state,
        lifecycle: PlacementLifecycle::Committed,
        content: PlacementContent::Failed {
            source: None,
            message: "Placement content could not be resumed after restart".to_owned(),
            recoverable: true,
        },
    };
    assert_eq!(
        reopened
            .load_placements()
            .unwrap()
            .iter()
            .map(placement_snapshot)
            .collect::<Vec<_>>(),
        vec![placement_snapshot(&loaded), placement_snapshot(&expected)]
    );
    assert_eq!(reopened.load_asset(asset.id).unwrap(), asset);
    assert_eq!(fs::read(path).unwrap(), original_bytes);
}
