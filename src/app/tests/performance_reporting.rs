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
    assert!(root.join("summary.json").exists());
}
