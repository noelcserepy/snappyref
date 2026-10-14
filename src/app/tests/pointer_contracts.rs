//! Pointer gestures cross canvas coordinates, preview state, and board history.
use super::*;

#[test]
fn inspect_rotated_corner_resize_outside_canvas_writes_artifact() {
    let mut scenario = InspectScenario::new("rotated-corner-resize");
    let root = TempRoot::new("snappyref-rotated-corner-resize");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let mut harness = app_full_harness(test_app(Some(library.clone())));
    let image = seed_loaded_placement(
        &library,
        &mut harness,
        "resize",
        [60, 100, 180, 255],
        (100, 60),
        egui::Pos2::ZERO,
    );
    let id = image.id();
    let mut initial = image.state.geometry;
    initial.rotation = std::f32::consts::FRAC_PI_2;
    harness
        .state_mut()
        .canvas
        .set_rect_item_geometry(id, initial);
    harness
        .state_mut()
        .canvas
        .apply_viewport_state(&crate::data::ViewportState {
            camera_pan_x: 50.0,
            camera_pan_y: 30.0,
            camera_zoom: 1.5,
            canvas_background: crate::data::CanvasBackground::default(),
        });
    harness.state_mut().canvas.select_placements(&[id]);
    harness.step();
    harness.state_mut().persist_canvas_changes();
    harness.state_mut().flush_persistence();
    scenario.capture_screen("screen-000-before-resize", &mut harness);

    // At 90 degrees the bottom-right corner is (-30, 50). The opposite
    // corner (30, -50) must remain fixed while the size grows sixfold.
    let screen = test_screen_rect();
    let origin = screen.center() + egui::vec2(50.0, 30.0);
    let from = origin + egui::vec2(-45.0, 75.0);
    let to = origin + egui::vec2(-495.0, 825.0);
    assert!(!screen.contains(to));
    scenario.action("resize rotated corner and release outside canvas", to);
    drag_with_modifiers(&mut harness, from, to, CMD, egui::PointerButton::Primary);
    let resized = harness
        .state()
        .canvas
        .placement_for_id(id)
        .unwrap()
        .state
        .geometry;
    scenario.fact("resize.geometry", serde_json::json!({"center": [resized.center.x, resized.center.y], "scale": resized.scale, "rotation": resized.rotation}));
    assert!(
        approx(resized.center, egui::pos2(-150.0, 250.0), 0.001),
        "{resized:?}"
    );
    assert!((resized.scale - 6.0).abs() < 0.001);
    assert_eq!(resized.rotation, std::f32::consts::FRAC_PI_2);
    assert!((resized.scaled_size() - egui::vec2(600.0, 360.0)).length() < 0.001);
    assert!(approx(
        resized.center + egui::vec2(180.0, -300.0),
        egui::pos2(30.0, -50.0),
        0.001
    ));
    scenario.assertion(
        "outside release preserves opposite corner and 5:3 aspect at center (-150,250), scale 6",
        true,
    );
    scenario.capture_screen("screen-001-resized", &mut harness);
    harness.state_mut().persist_canvas_changes();
    harness.state_mut().flush_persistence();
    let stored = library.load_placements().unwrap();
    assert_eq!(
        stored.iter().find(|p| p.id() == id).unwrap().state.geometry,
        resized
    );
    shortcut(&mut harness, CMD, egui::Key::Z);
    assert_eq!(
        harness
            .state()
            .canvas
            .placement_for_id(id)
            .unwrap()
            .state
            .geometry,
        initial
    );
    scenario.assertion(
        "committed geometry is durable and one Undo restores rotated seed",
        true,
    );
    scenario.capture_screen("screen-002-undone", &mut harness);
    finish(scenario);
}

#[test]
fn inspect_snapped_drag_release_and_command_bypass_writes_artifact() {
    let mut scenario = InspectScenario::new("snapped-drag-commit");
    let root = TempRoot::new("snappyref-snapped-drag-commit");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let mut harness = app_full_harness(test_app(Some(library.clone())));
    let moving = seed_loaded_placement(
        &library,
        &mut harness,
        "moving",
        [60, 100, 180, 255],
        (100, 80),
        egui::pos2(-180.0, 0.0),
    );
    let target = seed_loaded_placement(
        &library,
        &mut harness,
        "target",
        [180, 100, 60, 255],
        (100, 80),
        egui::Pos2::ZERO,
    );
    let id = moving.id();
    harness.state_mut().settings.placement_gap = 24.0;
    harness.state_mut().canvas.select_placements(&[id]);
    harness.step();
    let screen = test_screen_rect();
    let from = inspect_world_to_screen(egui::pos2(-180.0, 0.0), screen);
    scenario.capture_screen("screen-000-before-drag", &mut harness);
    harness.hover_at(from);
    harness.drag_at(from);
    for _ in 0..8 {
        harness.step();
    }
    std::thread::sleep(Duration::from_millis(120));
    for raw_x in [-122.0, -114.0] {
        let to = inspect_world_to_screen(egui::pos2(raw_x, 0.0), screen);
        scenario.action("acquire then retain gap snap", to);
        harness.hover_at(to);
        harness.step();
        let preview = center_of(&harness, id);
        scenario.fact(
            "snap.preview",
            serde_json::json!({"pointer_world": [raw_x, 0.0], "center": [preview.x, preview.y]}),
        );
        assert_eq!(preview, egui::pos2(-124.0, 0.0));
    }
    scenario.capture_screen("screen-001-preview", &mut harness);
    let release = inspect_world_to_screen(egui::pos2(-114.0, 0.0), screen);
    harness.drop_at(release);
    harness.step();
    assert_eq!(center_of(&harness, id), egui::pos2(-124.0, 0.0));
    assert_eq!(center_of(&harness, target.id()), egui::Pos2::ZERO);
    harness.state_mut().persist_canvas_changes();
    harness.state_mut().flush_persistence();
    assert_eq!(
        library
            .load_placements()
            .unwrap()
            .iter()
            .find(|p| p.id() == id)
            .unwrap()
            .state
            .geometry
            .center,
        egui::pos2(-124.0, 0.0)
    );
    scenario.assertion(
        "acquired snap survives 10px drift and release commits preview center (-124,0)",
        true,
    );
    shortcut(&mut harness, CMD, egui::Key::Z);
    assert_eq!(center_of(&harness, id), egui::pos2(-180.0, 0.0));
    let to = inspect_world_to_screen(egui::pos2(-122.0, 0.0), screen);
    scenario.action("Command drag bypasses nearby gap snap", to);
    drag_with_modifiers(&mut harness, from, to, CMD, egui::PointerButton::Primary);
    assert_eq!(center_of(&harness, id), egui::pos2(-122.0, 0.0));
    scenario.assertion("Command release commits unsnapped center (-122,0)", true);
    scenario.capture_screen("screen-002-bypassed", &mut harness);
    finish(scenario);
}

#[test]
fn inspect_marquee_clipped_group_and_shift_add_writes_artifact() {
    let mut scenario = InspectScenario::new("marquee-clipped-group");
    let root = TempRoot::new("snappyref-marquee-clipped-group");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let mut harness = app_full_harness(test_app(Some(library.clone())));
    let outside = seed_loaded_placement(
        &library,
        &mut harness,
        "outside",
        [60, 180, 100, 255],
        (100, 80),
        egui::pos2(-200.0, 0.0),
    );
    let partial = seed_loaded_placement(
        &library,
        &mut harness,
        "partial",
        [60, 100, 180, 255],
        (100, 80),
        egui::pos2(40.0, 0.0),
    );
    let hidden = seed_loaded_placement(
        &library,
        &mut harness,
        "hidden",
        [180, 100, 60, 255],
        (100, 80),
        egui::pos2(140.0, 0.0),
    );
    harness
        .state_mut()
        .canvas
        .restore_groups(vec![crate::data::Group {
            id: 1000,
            name: "Clipped references".into(),
            members: vec![partial.id(), hidden.id()],
            container: crate::data::RectGeometry::new(
                egui::Pos2::ZERO,
                egui::vec2(100.0, 100.0),
                1.0,
                0.0,
                0,
            ),
            flow: crate::data::GroupFlow::Free,
            gap: 0.0,
            padding: 0.0,
            layout: crate::data::GroupLayoutSettings {
                clip: true,
                ..Default::default()
            },
        }]);
    let screen = test_screen_rect();
    inspect_click_at(
        &harness,
        inspect_world_to_screen(egui::pos2(-200.0, 0.0), screen),
        egui::Modifiers::NONE,
    );
    harness.step();
    assert_eq!(
        harness.state().canvas.selected_placement_ids(),
        vec![outside.id()]
    );
    scenario.capture_screen("screen-000-clipped", &mut harness);
    for (min, max, expected) in [
        (
            egui::pos2(70.0, -90.0),
            egui::pos2(200.0, 60.0),
            vec![outside.id()],
        ),
        (
            egui::pos2(-20.0, -90.0),
            egui::pos2(20.0, 60.0),
            vec![outside.id(), partial.id(), hidden.id()],
        ),
    ] {
        scenario.action(
            "Shift marquee across hidden overflow then visible group pixels",
            inspect_world_to_screen(max, screen),
        );
        drag_with_modifiers(
            &mut harness,
            inspect_world_to_screen(min, screen),
            inspect_world_to_screen(max, screen),
            egui::Modifiers::SHIFT,
            egui::PointerButton::Primary,
        );
        let selected = harness.state().canvas.selected_placement_ids();
        scenario.fact(
            "marquee.selection",
            serde_json::json!({"selected": selected, "expected": expected}),
        );
        assert_eq!(selected, expected);
    }
    // Visible member contact selects the whole group, including its hidden member.
    assert_eq!(harness.state().canvas.selected_group_ids(), vec![1000]);
    scenario.assertion(
        "hidden pixels retain outside selection; visible pixels add exact group membership",
        true,
    );
    scenario.capture_screen("screen-001-shift-added", &mut harness);
    finish(scenario);
}
