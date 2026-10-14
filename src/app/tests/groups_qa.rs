use super::*;

#[test]
fn inspect_group_picker_selection_writes_artifact() {
    let mut scenario = InspectScenario::new("group-picker-selection");
    let root = TempRoot::new("snappyref-group-picker-selection");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let items: Vec<_> = (0..3)
        .map(|i| {
            let rgba = solid_rgba([80 + i as u8 * 50, 100, 140, 255], 120, 90);
            let path = library
                .store_rgba_image(&format!("group-{i}"), &rgba, 120, 90)
                .unwrap();
            let asset = library
                .register_original(&path, "image/png", 120, 90, 43_200, None)
                .unwrap();
            let placement = library
                .fixture_placement(
                    1,
                    Placement::loaded(
                        PlacementState::new(
                            library.allocate_document_id(),
                            egui::pos2(i as f32 * 180.0 - 180.0, 90.0),
                            1.0,
                            0.0,
                            0,
                            false,
                        ),
                        asset,
                    ),
                )
                .unwrap();
            loaded_record(placement)
        })
        .collect();
    let ids: Vec<_> = items.iter().map(|item| item.placement.id()).collect();
    let arrow = library.new_canvas_annotation(
        CanvasAnnotationKind::Arrow,
        egui::pos2(-220.0, -160.0),
        egui::pos2(-60.0, -80.0),
        String::new(),
        CanvasTextSize::M,
        CanvasStrokeWidth::M,
        crate::data::CanvasAnnotationColor::Sky,
    );
    let arrow_id = arrow.id;
    let original_arrow = arrow.clone();
    let mut app = test_app(Some(library.clone()));
    app.settings.semantic_search_first_launch_prompt_seen = true;
    app.search
        .set_local_model_store_status_for_test(LocalModelStoreStatus::Installed);
    let mut harness = app_full_harness_with_startup(1.0 / 60.0, move |ctx| {
        for item in items {
            app.canvas.add_prepared_placement_image(ctx, item);
        }
        app.canvas.add_annotation(arrow);
        app
    });
    harness.state_mut().canvas.select_placements(&ids[..1]);
    shortcut(&mut harness, CMD, egui::Key::G);
    let destination = harness.state().canvas.groups()[0].id;
    let initial_groups = harness.state().canvas.groups().to_vec();
    harness.state_mut().canvas.select_content(&[], &[arrow_id]);
    scenario.capture_screen("arrow-selected", &mut harness);
    shortcut(&mut harness, egui::Modifiers::NONE, egui::Key::G);
    for _ in 0..12 {
        harness.step();
    }
    scenario.capture_screen("arrow-after-g", &mut harness);
    scenario.assertion(
        "arrow-only G leaves the picker closed",
        !harness.state().canvas.group_picker_open(),
    );
    scenario.assertion(
        "unsupported selection explains what can join a group",
        harness.state().status == "Only images, labels, and rectangles can join groups",
    );
    // On unfixed code, confirm both the existing and create no-op paths.
    if harness.state().canvas.group_picker_open() {
        shortcut(&mut harness, egui::Modifiers::NONE, egui::Key::Enter);
        scenario.capture_screen("arrow-existing-no-op", &mut harness);
        shortcut(&mut harness, egui::Modifiers::NONE, egui::Key::G);
        harness.step();
        harness.event(egui::Event::Text("Arrow references".into()));
        harness.step();
        shortcut(&mut harness, egui::Modifiers::NONE, egui::Key::Enter);
        scenario.capture_screen("arrow-create-no-op", &mut harness);
    }
    scenario.assertion(
        "unsupported selection leaves groups unchanged",
        harness.state().canvas.groups() == initial_groups,
    );
    harness
        .state_mut()
        .canvas
        .select_content(&ids[1..2], &[arrow_id]);
    shortcut(&mut harness, egui::Modifiers::NONE, egui::Key::G);
    harness.step();
    scenario.capture_screen("mixed-existing-picker", &mut harness);
    shortcut(&mut harness, egui::Modifiers::NONE, egui::Key::ArrowDown);
    shortcut(&mut harness, egui::Modifiers::NONE, egui::Key::Enter);
    scenario.assertion(
        "mixed selection adds only the image to an existing group",
        harness.state().canvas.groups()[0].id == destination
            && harness.state().canvas.groups()[0].members == ids[..2],
    );
    harness
        .state_mut()
        .canvas
        .select_content(&ids[2..], &[arrow_id]);
    shortcut(&mut harness, egui::Modifiers::NONE, egui::Key::G);
    harness.step();
    harness.event(egui::Event::Text("Mixed references".into()));
    harness.step();
    harness
        .get_by_label("Create new \"Mixed references\"")
        .click();
    for _ in 0..12 {
        harness.step();
    }
    scenario.capture_screen("mixed-created", &mut harness);
    let created = harness
        .state()
        .canvas
        .groups()
        .iter()
        .find(|g| g.id != destination)
        .unwrap();
    scenario.assertion(
        "mixed selection creates a named group containing only its image",
        created.name == "Mixed references" && created.members == ids[2..],
    );
    scenario.assertion(
        "grouping leaves the arrow geometry and style intact",
        harness.state().canvas.annotation_for_id(arrow_id) == Some(original_arrow),
    );
    harness.state_mut().flush_persistence();
    scenario.assertion(
        "both image memberships persist",
        library.load_groups().unwrap() == harness.state().canvas.groups(),
    );
    let report = scenario.finish();
    assert!(report.passed, "{}", report.artifact_dir.display());
}

#[test]
fn inspect_group_settings_gap_writes_artifact() {
    let mut scenario = InspectScenario::new("group-settings-gap");
    let root = TempRoot::new("snappyref-group-settings-gap");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let items: Vec<_> = (0..3)
        .map(|i| {
            let rgba = solid_rgba([80 + i as u8 * 50, 100, 140, 255], 120, 90);
            let path = library
                .store_rgba_image(&format!("group-{i}"), &rgba, 120, 90)
                .unwrap();
            let asset = library
                .register_original(&path, "image/png", 120, 90, 43_200, None)
                .unwrap();
            let placement = library
                .fixture_placement(
                    1,
                    Placement::loaded(
                        PlacementState::new(
                            library.allocate_document_id(),
                            egui::pos2(i as f32 * 180.0 - 180.0, 90.0),
                            1.0,
                            0.0,
                            0,
                            false,
                        ),
                        asset,
                    ),
                )
                .unwrap();
            loaded_record(placement)
        })
        .collect();
    let ids: Vec<_> = items.iter().map(|item| item.placement.id()).collect();
    let mut app = test_app(Some(library.clone()));
    app.settings.semantic_search_first_launch_prompt_seen = true;
    app.search
        .set_local_model_store_status_for_test(LocalModelStoreStatus::Installed);
    app.sidebar_open = true;
    app.sidebar_tab = SidebarTab::Settings;
    let mut harness = app_full_harness_with_startup(1.0 / 60.0, move |ctx| {
        for item in items {
            app.canvas.add_prepared_placement_image(ctx, item);
        }
        app
    });
    harness.state_mut().canvas.select_placements(&ids[..1]);
    shortcut(&mut harness, CMD, egui::Key::G);
    let existing_id = harness.state().canvas.groups()[0].id;
    let mut existing = harness.state().canvas.groups()[0].clone();
    existing.gap = 11.0;
    harness.state_mut().canvas.edit_group(existing).unwrap();
    harness.state_mut().canvas.select_content(&[], &[]);
    harness.step();
    scenario.capture_screen("settings-before", &mut harness);
    harness.get_by_label("Placement gap").click();
    harness.step();
    shortcut(&mut harness, CMD, egui::Key::A);
    harness.event(egui::Event::Text("30".into()));
    harness.step();
    shortcut(&mut harness, egui::Modifiers::NONE, egui::Key::Enter);
    harness.step();
    scenario.capture_screen("settings-gap-30", &mut harness);
    scenario.assertion(
        "Settings accepts a 30 px gap",
        harness.state().settings.placement_gap == 30.0,
    );
    harness.state_mut().canvas.select_placements(&ids[1..2]);
    shortcut(&mut harness, CMD, egui::Key::G);
    harness.step();
    scenario.capture_screen("shortcut-group-gap-30", &mut harness);
    harness.state_mut().canvas.select_placements(&ids[2..]);
    shortcut(&mut harness, egui::Modifiers::NONE, egui::Key::G);
    harness.step();
    harness.event(egui::Event::Text("Configured gap".into()));
    harness.step();
    shortcut(&mut harness, egui::Modifiers::NONE, egui::Key::Enter);
    harness.step();
    scenario.capture_screen("picker-group-gap-30", &mut harness);
    harness.state_mut().flush_persistence();
    let stored = library.load_groups().unwrap();
    scenario.fact(
        "stored-gaps",
        serde_json::json!(
            stored
                .iter()
                .map(|g| (&g.name, g.gap, g.layout.row_gap))
                .collect::<Vec<_>>()
        ),
    );
    scenario.assertion(
        "existing group keeps its stored 11 px gap",
        stored.iter().find(|g| g.id == existing_id).unwrap().gap == 11.0,
    );
    scenario.assertion(
        "shortcut and picker groups inherit the configured gap",
        stored
            .iter()
            .filter(|g| g.id != existing_id)
            .map(|g| (g.name.as_str(), g.gap, g.layout.row_gap))
            .collect::<Vec<_>>()
            == vec![("Group 2", 30.0, 30.0), ("Configured gap", 30.0, 30.0)],
    );
    let report = scenario.finish();
    assert!(report.passed, "{}", report.artifact_dir.display());
}
