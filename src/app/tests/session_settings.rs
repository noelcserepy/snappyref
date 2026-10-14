use super::*;
use crate::test_support::TempRoot;

#[test]
fn save_user_session_round_trips_placement_gap() {
    let root = TempRoot::new("snappyref-app-gap");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let mut app = test_app(Some(library.clone()));
    app.settings.placement_gap = 37.0;
    app.settings.placement_target_area = 45_000.0;
    app.settings.canvas_image_max_side = 4096;
    app.canvas
        .set_background(crate::data::CanvasBackground::Grid);

    app.save_user_session();
    app.flush_persistence();

    let session = library.load_user_session().unwrap();
    assert_eq!(session.settings.placement_gap, 37.0);
    assert_eq!(session.settings.placement_target_area, 45_000.0);
    assert_eq!(session.settings.canvas_image_max_side, 4096);
    assert_eq!(
        session.viewport.canvas_background,
        crate::data::CanvasBackground::Grid
    );
}

#[test]
fn app_config_restores_always_on_top_workspace_setting() {
    let root = TempRoot::new("snappyref-always-on-top-startup");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let mut session = library.load_user_session().unwrap();
    session.settings.always_on_top = true;
    library.save_user_session(&session).unwrap();

    let config = AppConfig {
        library_root: Ok(root.to_path_buf()),
        no_startup_focus: false,
        perf_log: false,
        perf_decode_path: None,
        perf_scenario: None,
        perf_corpus: None,
        perf_run_dir: None,
        update_smoke_install: false,
        update_smoke_status_path: None,
    };

    assert!(config.prepare_startup().always_on_top());
}

#[test]
fn real_startup_and_exit_keep_pending_content_session_and_history() {
    fn config(path: PathBuf) -> AppConfig {
        AppConfig {
            library_root: Ok(path),
            no_startup_focus: true,
            perf_log: false,
            perf_decode_path: None,
            perf_scenario: None,
            perf_corpus: None,
            perf_run_dir: None,
            update_smoke_install: false,
            update_smoke_status_path: None,
        }
    }
    fn start(path: PathBuf) -> Harness<'static, SnappyRefApp> {
        Harness::builder()
            .with_size(egui::vec2(1200.0, 800.0))
            .with_max_steps(4)
            .build_eframe(move |cc| {
                crate::ui::theme::configure_theme(&cc.egui_ctx);
                let config = config(path);
                let startup = config.prepare_startup();
                SnappyRefApp::new(cc, config, startup)
            })
    }
    let root = TempRoot::new("snappyref-real-startup-exit-history");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let board = library.fixture_board("Saved").unwrap();
    let mut session = library.load_user_session().unwrap();
    session.active_board_id = board.id;
    session.sidebar_open = true;
    session.settings.placement_gap = 37.0;
    session.settings.semantic_search_first_launch_prompt_seen = true;
    library.save_user_session(&session).unwrap();
    let mut harness = start(root.to_path_buf());
    assert_eq!(harness.state().active_board_id, board.id);
    assert_eq!(harness.state().settings.placement_gap, 37.0);
    assert!(harness.state().sidebar_open);
    let ctx = harness.ctx.clone();
    let made=harness.state_mut().control_call(&ctx,"edit_board",serde_json::json!({"edits":[{"action":"add","type":"rectangle","x":10,"y":20,"width":80,"height":40,"ref":"box"}]})).unwrap();
    let crate::app::control::Step::Done(made) = made else {
        panic!("edit waits")
    };
    let id = made["refs"]["box"].as_i64().unwrap();
    harness.state_mut().flush_persistence();
    assert_eq!(
        library.load_board_document(board.id).unwrap().annotations[0].start(),
        egui::pos2(10.0, 20.0)
    );
    let mut geometry = harness.state().canvas.rect_item_geometry(id).unwrap();
    geometry.center = egui::pos2(250.0, 140.0);
    harness
        .state_mut()
        .canvas
        .set_rect_item_geometry(id, geometry);
    harness.state_mut().settings.placement_gap = 51.0;
    harness.state_mut().sidebar_open = false;
    // Do not render or explicitly flush the edit. Exit owns the pending edit.
    eframe::App::on_exit(harness.state_mut());
    drop(harness);
    let mut reopened = start(root.to_path_buf());
    let restored = reopened.state().canvas.annotation_for_id(id).unwrap();
    assert_eq!(restored.start(), egui::pos2(210.0, 120.0));
    assert_eq!(restored.end(), egui::pos2(290.0, 160.0));
    assert_eq!(reopened.state().active_board_id, board.id);
    assert_eq!(reopened.state().settings.placement_gap, 51.0);
    assert!(!reopened.state().sidebar_open);
    let ctx = reopened.ctx.clone();
    reopened.state_mut().undo_document_history(&ctx);
    let undone = reopened.state().canvas.annotation_for_id(id).unwrap();
    assert_eq!(undone.start(), egui::pos2(10.0, 20.0));
    assert_eq!(undone.end(), egui::pos2(90.0, 60.0));
    reopened.state_mut().redo_document_history(&ctx);
    let redone = reopened.state().canvas.annotation_for_id(id).unwrap();
    assert_eq!(redone.start(), egui::pos2(210.0, 120.0));
    assert_eq!(redone.end(), egui::pos2(290.0, 160.0));
    eframe::App::on_exit(reopened.state_mut());
    assert_eq!(
        library.load_board_document(board.id).unwrap().annotations[0].start(),
        egui::pos2(210.0, 120.0)
    );
}
