use super::*;

#[test]
fn perf_idle_hold_starts_when_idle_is_observed() {
    let root = TempRoot::new("snappyref-perf-delayed-idle-frame");
    let config = AppConfig {
        library_root: Ok(root.to_path_buf()),
        no_startup_focus: true,
        perf_log: false,
        perf_decode_path: None,
        perf_scenario: Some("startup".to_owned()),
        perf_corpus: Some(root.to_path_buf()),
        perf_run_dir: Some(root.to_path_buf()),
        update_smoke_install: false,
        update_smoke_status_path: None,
    };
    let mut scenario = crate::app::PerfScenarioState::from_config(&config).unwrap();
    scenario.queued = true;
    // A background window can miss callbacks long after its work drains.
    scenario.idle_since = Some(Instant::now() - Duration::from_secs(60));
    let mut app = test_app(None);
    app.perf_scenario = Some(scenario);
    assert!(!app.finish_perf_scenario_if_idle(&egui::Context::default()));
    let summary: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("summary.json")).unwrap()).unwrap();
    assert_eq!(summary["scenario"], "startup");
    assert_eq!(summary["expected_imports"], 0);
    assert_eq!(summary["loaded_imports"], 0);
    assert_eq!(summary["visible_imports"], 0);
    assert_eq!(
        summary["import_worker"],
        serde_json::json!({"queued_startup":0,"queued_interactive":0,"queued_background":0,"in_flight":0})
    );
    std::thread::sleep(Duration::from_millis(5_050));
    let ctx = egui::Context::default();
    let mut finished = false;
    let output = crate::ui::test_frame::run_ui(&ctx, Default::default(), |ui| {
        finished = app.finish_perf_scenario_if_idle(ui.ctx());
    });
    assert!(finished);
    assert!(
        output.viewport_output[&egui::ViewportId::ROOT]
            .commands
            .iter()
            .any(|command| matches!(command, egui::ViewportCommand::Close))
    );
}
