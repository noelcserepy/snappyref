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
    fn tool(
        harness: &mut Harness<'_, SnappyRefApp>,
        name: &str,
        args: serde_json::Value,
    ) -> serde_json::Value {
        let ctx = harness.ctx.clone();
        let result = harness.state_mut().control_call(&ctx, name, args).unwrap();
        let crate::app::control::Step::Done(value) = result else {
            panic!("tool waits")
        };
        value
    }
    fn rectangle(x: f64, y: f64) -> serde_json::Value {
        serde_json::json!([{"id":3,"type":"rectangle","x":x,"y":y,"width":80.0,"height":40.0,"stroke":"m","color":"default","fill":"tint"}])
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
    assert_eq!(
        tool(&mut harness, "get_board", serde_json::json!({}))["board"],
        serde_json::json!({"id":2,"name":"Saved"})
    );
    let settings = tool(&mut harness, "get_settings", serde_json::json!({}));
    assert_eq!(settings["placement_gap"], 37.0);
    assert_eq!(settings["sidebar_open"], true);
    let made = tool(
        &mut harness,
        "edit_board",
        serde_json::json!({"edits":[{"action":"add","type":"rectangle","x":10,"y":20,"width":80,"height":40,"ref":"box"}]}),
    );
    assert_eq!(made["refs"], serde_json::json!({"box":3}));
    let id = 3;
    harness.state_mut().flush_persistence();
    assert_eq!(
        library.load_board_document(board.id).unwrap().annotations[0].start(),
        egui::pos2(10.0, 20.0)
    );
    // Seed an unrecorded UI edit. The action under test is exit, not dragging.
    harness.state_mut().canvas.set_rect_item_geometry(
        id,
        crate::data::RectGeometry::new(
            egui::pos2(250.0, 140.0),
            egui::vec2(80.0, 40.0),
            1.0,
            0.0,
            0,
        ),
    );
    harness.state_mut().settings.placement_gap = 51.0;
    harness.state_mut().sidebar_open = false;
    // Do not render or explicitly flush the edit. Exit owns the pending edit.
    eframe::App::on_exit(harness.state_mut());
    drop(harness);
    let mut reopened = start(root.to_path_buf());
    let restored = tool(&mut reopened, "get_board", serde_json::json!({}));
    assert_eq!(
        restored["board"],
        serde_json::json!({"id":2,"name":"Saved"})
    );
    assert_eq!(restored["items"], rectangle(210.0, 120.0));
    let settings = tool(&mut reopened, "get_settings", serde_json::json!({}));
    assert_eq!(settings["placement_gap"], 51.0);
    assert_eq!(settings["sidebar_open"], false);
    tool(&mut reopened, "undo", serde_json::json!({}));
    assert_eq!(
        tool(&mut reopened, "get_board", serde_json::json!({}))["items"],
        rectangle(10.0, 20.0)
    );
    tool(&mut reopened, "redo", serde_json::json!({}));
    assert_eq!(
        tool(&mut reopened, "get_board", serde_json::json!({}))["items"],
        rectangle(210.0, 120.0)
    );
    eframe::App::on_exit(reopened.state_mut());
    assert_eq!(
        library.load_board_document(board.id).unwrap().annotations[0].start(),
        egui::pos2(210.0, 120.0)
    );
}
