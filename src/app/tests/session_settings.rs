use super::*;
use crate::test_support::TempRoot;

#[test]
fn persist_settings_if_changed_true_calls_save_once() {
    let mut calls = 0;
    SnappyRefApp::persist_settings_if_changed_with(true, || {
        calls += 1;
    });
    assert_eq!(calls, 1);
}

#[test]
fn persist_settings_if_changed_false_skips_save() {
    let mut calls = 0;
    SnappyRefApp::persist_settings_if_changed_with(false, || {
        calls += 1;
    });
    assert_eq!(calls, 0);
}

#[test]
fn save_user_session_round_trips_placement_gap() {
    let root = TempRoot::new("snappyref-app-gap");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let mut app = test_app(Some(library.clone()));
    app.settings.auto_image_scaling = false;
    app.settings.placement_gap = 37.0;
    app.settings.placement_target_area = 45_000.0;
    app.settings.canvas_image_max_side = 4096;
    app.canvas
        .set_background(crate::data::CanvasBackground::Grid);

    app.save_user_session();

    let session = library.load_user_session().unwrap();
    assert!(!session.settings.auto_image_scaling);
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

    assert!(
        config
            .restored_workspace_settings()
            .is_some_and(|settings| settings.always_on_top)
    );
}
