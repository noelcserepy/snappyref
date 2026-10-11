//! Inspect scenarios for the main user operations: canvas editing, navigation
//! and Board switching, session restore across a restart, and recovery from a
//! failed durable write.

use super::support::INSPECT_DELETE_KEY;
use super::*;
use crate::test_support::TempRoot;

const CMD: egui::Modifiers = egui::Modifiers {
    command: true,
    ..egui::Modifiers::NONE
};
const CMD_ALT: egui::Modifiers = egui::Modifiers {
    command: true,
    alt: true,
    ..egui::Modifiers::NONE
};

fn seed_loaded_placement(
    library: &Library,
    harness: &mut Harness<'_, SnappyRefApp>,
    name: &str,
    color: [u8; 4],
    size: (u32, u32),
    center: egui::Pos2,
) -> Placement {
    let path = library
        .store_rgba_image(name, &solid_rgba(color, size.0, size.1), size.0, size.1)
        .unwrap();
    let placement = library
        .fixture_placement(
            library.load_user_session().unwrap().active_board_id,
            crate::data::Placement::loaded(
                crate::data::PlacementState::new(
                    library.allocate_document_id(),
                    center,
                    1.0,
                    0.0,
                    0,
                    false,
                ),
                library
                    .register_original(
                        &path,
                        "image/png",
                        size.0,
                        size.1,
                        size.0 as u64 * size.1 as u64 * 4,
                        None,
                    )
                    .unwrap(),
            ),
        )
        .unwrap();
    let board_id = harness.state().active_board_id;
    harness
        .state_mut()
        .board_editing
        .documents
        .restore_document(&library.load_board_document(board_id).unwrap());
    let ctx = harness.ctx.clone();
    harness.state_mut().canvas.add_prepared_placement_image(
        &ctx,
        inspect_prepared_loaded_placement(
            placement.clone(),
            color,
            size.0,
            size.1,
            None,
            crate::image_preparation::ImportOrigin::Existing,
        ),
    );
    harness.step();
    placement
}

/// Presses at `from`, waits out the click-to-select move debounce, drags to
/// `to`, and releases while `modifiers` stay held.
fn drag_with_modifiers(
    harness: &mut Harness<'_, SnappyRefApp>,
    from: egui::Pos2,
    to: egui::Pos2,
    modifiers: egui::Modifiers,
    button: egui::PointerButton,
) {
    harness.event(egui::Event::ModifiersChanged(modifiers));
    harness.event(egui::Event::PointerMoved(from));
    harness.event(egui::Event::PointerButton {
        pos: from,
        button,
        pressed: true,
        modifiers,
    });
    for _ in 0..8 {
        harness.step();
    }
    std::thread::sleep(Duration::from_millis(120));
    let mid = from + (to - from) * 0.5;
    harness.event(egui::Event::PointerMoved(mid));
    harness.step();
    harness.event(egui::Event::PointerMoved(to));
    harness.step();
    harness.event(egui::Event::PointerButton {
        pos: to,
        button,
        pressed: false,
        modifiers,
    });
    harness.step();
    harness.event(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
    harness.step();
}

fn shortcut(harness: &mut Harness<'_, SnappyRefApp>, modifiers: egui::Modifiers, key: egui::Key) {
    for pressed in [true, false] {
        harness.event_modifiers(
            egui::Event::Key {
                key,
                physical_key: Some(key),
                pressed,
                repeat: false,
                modifiers,
            },
            modifiers,
        );
    }
    harness.step();
}

fn center_of(harness: &Harness<'_, SnappyRefApp>, placement_id: i64) -> egui::Pos2 {
    harness
        .state()
        .canvas
        .placement_for_id(placement_id)
        .expect("placement should be on canvas")
        .state
        .geometry
        .center
}

fn approx(a: egui::Pos2, b: egui::Pos2, tolerance: f32) -> bool {
    a.distance(b) <= tolerance
}

/// Steps the app until the import worker has loaded `placement_id`. The
/// budget is wall-clock so a busy parallel test run cannot starve it.
fn step_until_placement_loaded(harness: &mut Harness<'_, SnappyRefApp>, placement_id: i64) -> bool {
    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline {
        harness.step();
        if harness
            .state()
            .canvas
            .placement_for_id(placement_id)
            .is_some_and(|placement| placement.is_loaded())
        {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    false
}

fn finish(scenario: InspectScenario) {
    let report = scenario.finish();
    assert!(
        report.passed,
        "inspect scenario failed; artifact={}",
        report.artifact_dir.display()
    );
}

#[test]
fn inspect_canvas_edit_operations_writes_artifact() {
    let mut scenario = InspectScenario::new("canvas-edit-operations");
    scenario.event(
        "seed",
        serde_json::json!({
            "seed": "two-loaded-placements",
            "purpose": "select, move, duplicate, undo, arrange, stack order, rotate, scale, textbox, delete, durability",
        }),
    );

    let root = TempRoot::new("snappyref-inspect-canvas-edit-operations");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let mut app = test_app(Some(library.clone()));
    app.board_editing.persistence =
        crate::active_board::persistence::DocumentPersistence::queued(library.clone());
    let mut harness = app_full_harness(app);
    let blue = seed_loaded_placement(
        &library,
        &mut harness,
        "edit-blue",
        [78, 126, 226, 255],
        (140, 100),
        egui::pos2(-150.0, 0.0),
    );
    let orange = seed_loaded_placement(
        &library,
        &mut harness,
        "edit-orange",
        [226, 160, 78, 255],
        (140, 100),
        egui::pos2(150.0, 0.0),
    );
    let screen = test_screen_rect();
    let blue_id = blue.state.id;
    let orange_id = orange.state.id;
    scenario.capture_screen("screen-000-seeded", &mut harness);

    // Select + move.
    let blue_screen = inspect_world_to_screen(blue.state.geometry.center, screen);
    scenario.action("click blue placement", blue_screen);
    inspect_click_at(&harness, blue_screen, egui::Modifiers::NONE);
    harness.step();
    scenario.assertion(
        "click selects blue placement",
        harness.state().canvas.selected_placement_ids() == vec![blue_id],
    );
    let move_target = blue_screen + egui::vec2(0.0, 150.0);
    scenario.action("drag blue placement down", move_target);
    drag_with_modifiers(
        &mut harness,
        blue_screen,
        move_target,
        egui::Modifiers::NONE,
        egui::PointerButton::Primary,
    );
    let moved_center = center_of(&harness, blue_id);
    scenario.fact(
        "move.center",
        serde_json::json!({"x": moved_center.x, "y": moved_center.y}),
    );
    scenario.assertion(
        "drag moves blue placement by the pointer delta",
        approx(moved_center, egui::pos2(-150.0, 150.0), 4.0),
    );
    scenario.capture_screen("screen-001-moved", &mut harness);

    // Duplicate + undo.
    scenario.event("action", serde_json::json!({"name": "press Command-D"}));
    shortcut(&mut harness, CMD, egui::Key::D);
    let after_duplicate = harness.state().canvas.canvas_stack_order().len();
    scenario.assertion(
        "Command-D adds one duplicate to the canvas",
        after_duplicate == 3,
    );
    scenario.capture_screen("screen-002-duplicated", &mut harness);
    shortcut(&mut harness, CMD, egui::Key::Z);
    scenario.assertion(
        "Undo removes the duplicate",
        harness.state().canvas.canvas_stack_order().len() == 2,
    );

    // Select all + row arrange.
    shortcut(&mut harness, CMD, egui::Key::A);
    scenario.assertion(
        "Command-A selects both placements",
        harness.state().canvas.selected_placement_ids().len() == 2,
    );
    scenario.event(
        "action",
        serde_json::json!({"name": "press W (row arrange)"}),
    );
    harness.key_press(egui::Key::W);
    harness.step();
    let (b, o) = (center_of(&harness, blue_id), center_of(&harness, orange_id));
    scenario.fact(
        "arrange.row",
        serde_json::json!({"blue": [b.x, b.y], "orange": [o.x, o.y]}),
    );
    scenario.assertion(
        "row arrange aligns both placements on one row without overlap",
        (b.y - o.y).abs() < 1.0 && (b.x - o.x).abs() >= 140.0,
    );
    scenario.capture_screen("screen-003-row-arranged", &mut harness);

    // Stack order. Clicking a member of a multi-selection keeps the whole
    // selection, so clear it on empty canvas first.
    inspect_click_at(
        &harness,
        screen.center() + egui::vec2(0.0, 300.0),
        egui::Modifiers::NONE,
    );
    harness.step();
    let orange_screen = inspect_world_to_screen(center_of(&harness, orange_id), screen);
    inspect_click_at(&harness, orange_screen, egui::Modifiers::NONE);
    harness.step();
    scenario.fact(
        "stack.selection",
        serde_json::json!(harness.state().canvas.selected_placement_ids()),
    );
    shortcut(&mut harness, CMD, egui::Key::Comma);
    let stack = harness.state().canvas.canvas_stack_order();
    scenario.fact("stack.after-back", serde_json::json!(format!("{stack:?}")));
    scenario.assertion(
        "Command-, sends orange to the back",
        stack.first() == Some(&CanvasStackItem::Placement(orange_id)),
    );
    shortcut(&mut harness, CMD, egui::Key::Period);
    let stack = harness.state().canvas.canvas_stack_order();
    scenario.assertion(
        "Command-. brings orange to the front",
        stack.last() == Some(&CanvasStackItem::Placement(orange_id)),
    );

    // Rotate with R + drag on the handle above the selection.
    let orange_center = center_of(&harness, orange_id);
    let orange_screen = inspect_world_to_screen(orange_center, screen);
    let handle = orange_screen - egui::vec2(0.0, 50.0 + 18.0);
    scenario.target(
        "orange rotation handle",
        "handle",
        egui::Rect::from_center_size(handle, egui::vec2(26.0, 26.0)),
    );
    scenario.action(
        "hold R and drag rotation handle",
        handle + egui::vec2(60.0, 40.0),
    );
    harness.key_down(egui::Key::R);
    harness.step();
    drag_with_modifiers(
        &mut harness,
        handle,
        handle + egui::vec2(60.0, 40.0),
        egui::Modifiers::NONE,
        egui::PointerButton::Primary,
    );
    harness.key_up(egui::Key::R);
    harness.step();
    let rotation = harness
        .state()
        .canvas
        .placement_for_id(orange_id)
        .unwrap()
        .state
        .geometry
        .rotation;
    scenario.fact("rotate.radians", serde_json::json!(rotation));
    scenario.assertion("R + drag rotates the selection", rotation.abs() > 0.05);
    scenario.capture_screen("screen-004-rotated", &mut harness);

    // Scale with Command-Option drag on the placement.
    let blue_center = center_of(&harness, blue_id);
    let blue_screen = inspect_world_to_screen(blue_center, screen);
    inspect_click_at(&harness, blue_screen, egui::Modifiers::NONE);
    harness.step();
    let before = harness
        .state()
        .canvas
        .placement_for_id(blue_id)
        .unwrap()
        .state
        .geometry;
    scenario.action(
        "Command-Option drag scales blue",
        blue_screen + egui::vec2(80.0, 60.0),
    );
    drag_with_modifiers(
        &mut harness,
        blue_screen,
        blue_screen + egui::vec2(80.0, 60.0),
        CMD_ALT,
        egui::PointerButton::Primary,
    );
    let after = harness
        .state()
        .canvas
        .placement_for_id(blue_id)
        .unwrap()
        .state
        .geometry;
    scenario.fact(
        "scale.before-after",
        serde_json::json!({"before": [before.size.x, before.size.y, before.scale], "after": [after.size.x, after.size.y, after.scale]}),
    );
    scenario.assertion(
        "Command-Option drag scales the selection uniformly",
        after.scale > before.scale * 1.05
            && ((after.size.x / after.size.y) - (before.size.x / before.size.y)).abs() < 0.01,
    );
    scenario.capture_screen("screen-005-scaled", &mut harness);

    // Textbox: T over empty canvas, type, Escape.
    let textbox_screen = screen.center() + egui::vec2(0.0, -250.0);
    scenario.action("press T over empty canvas", textbox_screen);
    harness.hover_at(textbox_screen);
    harness.step();
    shortcut(&mut harness, egui::Modifiers::NONE, egui::Key::T);
    harness.step();
    harness
        .input_mut()
        .events
        .push(egui::Event::Text("Note".to_owned()));
    harness.step();
    harness.key_press(egui::Key::Escape);
    harness.step();
    let annotations: Vec<_> = harness
        .state()
        .canvas
        .all_annotation_ids()
        .into_iter()
        .filter_map(|id| harness.state().canvas.annotation_for_id(id))
        .collect();
    scenario.fact(
        "textbox",
        serde_json::json!({"count": annotations.len(), "text": annotations.first().map(|a| a.text.clone())}),
    );
    scenario.assertion(
        "T + click creates one textbox holding the typed text",
        annotations.len() == 1 && annotations[0].text == "Note",
    );
    scenario.capture_screen("screen-006-textbox", &mut harness);

    // Delete + durability.
    let blue_screen = inspect_world_to_screen(center_of(&harness, blue_id), screen);
    inspect_click_at(&harness, blue_screen, egui::Modifiers::NONE);
    harness.step();
    harness.key_press(INSPECT_DELETE_KEY);
    harness.step();
    scenario.assertion(
        "Delete shortcut removes the selected placement",
        harness.state().canvas.placement_for_id(blue_id).is_none(),
    );
    scenario.capture_screen("screen-007-deleted", &mut harness);

    harness.state_mut().persist_canvas_changes();
    harness.state_mut().flush_persistence();
    let stored = library.load_placements().unwrap();
    let stored_orange = stored.iter().find(|p| p.state.id == orange_id);
    let stored_annotations = library.load_canvas_annotations().unwrap();
    scenario.fact(
        "durable",
        serde_json::json!({
            "placements": stored.len(),
            "orange_rotation": stored_orange.map(|p| p.state.geometry.rotation),
            "annotations": stored_annotations.len(),
            "status": harness.state().status,
        }),
    );
    scenario.assertion(
        "flush persists the edited Board: one rotated orange, no blue, one textbox",
        stored.len() == 1
            && stored_orange.is_some_and(|p| (p.state.geometry.rotation - rotation).abs() < 1e-4)
            && stored_annotations.len() == 1
            && stored_annotations[0].text == "Note",
    );

    finish(scenario);
}

#[test]
fn inspect_canvas_navigation_and_boards_writes_artifact() {
    let mut scenario = InspectScenario::new("canvas-navigation-boards");
    scenario.event(
        "seed",
        serde_json::json!({
            "seed": "one-loaded-placement",
            "purpose": "wheel zoom, middle-drag pan, Space focus, sidebar toggle, Command-N / Command-1 Board switching",
        }),
    );

    let root = TempRoot::new("snappyref-inspect-canvas-navigation");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let mut app = test_app(Some(library.clone()));
    app.board_editing.persistence =
        crate::active_board::persistence::DocumentPersistence::queued(library.clone());
    let mut harness = app_full_harness(app);
    let ctx = harness.ctx.clone();
    harness.state_mut().import_worker = Some(crate::image_preparation::ImagePrepWorker::new(
        library.clone(),
        ctx,
        None,
    ));
    let green = seed_loaded_placement(
        &library,
        &mut harness,
        "nav-green",
        [74, 178, 102, 255],
        (180, 180),
        egui::pos2(0.0, 0.0),
    );
    let screen = test_screen_rect();
    scenario.capture_screen("screen-000-seeded", &mut harness);

    // Wheel zoom in (mouse pointer mode).
    let zoom_before = harness.state().canvas.zoom;
    harness.hover_at(screen.center() + egui::vec2(200.0, 100.0));
    harness.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Line,
        delta: egui::vec2(0.0, 1.0),
        phase: egui::TouchPhase::Move,
        modifiers: egui::Modifiers::NONE,
    });
    for _ in 0..30 {
        harness.step();
    }
    let zoom_after = harness.state().canvas.zoom;
    scenario.fact(
        "zoom",
        serde_json::json!({"before": zoom_before, "after": zoom_after}),
    );
    scenario.assertion("wheel up zooms in", zoom_after > zoom_before * 1.2);
    scenario.capture_screen("screen-001-zoomed-in", &mut harness);

    // Middle-drag pan.
    let pan_before = harness.state().canvas.viewport_state();
    drag_with_modifiers(
        &mut harness,
        screen.center() + egui::vec2(300.0, 200.0),
        screen.center() + egui::vec2(100.0, 100.0),
        egui::Modifiers::NONE,
        egui::PointerButton::Middle,
    );
    let pan_after = harness.state().canvas.viewport_state();
    scenario.fact(
        "pan",
        serde_json::json!({"before": [pan_before.camera_pan_x, pan_before.camera_pan_y], "after": [pan_after.camera_pan_x, pan_after.camera_pan_y]}),
    );
    scenario.assertion(
        "middle drag pans the view",
        (pan_after.camera_pan_x - pan_before.camera_pan_x).abs() > 100.0,
    );
    scenario.capture_screen("screen-002-panned", &mut harness);

    let item_before_pan = harness
        .state()
        .canvas
        .placement_for_id(green.state.id)
        .unwrap()
        .state
        .geometry;
    let before_alt = harness.state().canvas.viewport_state();
    drag_with_modifiers(
        &mut harness,
        screen.center() + egui::vec2(300.0, 200.0),
        screen.center() + egui::vec2(360.0, 260.0),
        egui::Modifiers::ALT,
        egui::PointerButton::Primary,
    );
    let after_alt = harness.state().canvas.viewport_state();
    scenario.assertion(
        "Alt-primary drag pans without moving content",
        after_alt != before_alt
            && harness
                .state()
                .canvas
                .placement_for_id(green.state.id)
                .unwrap()
                .state
                .geometry
                == item_before_pan,
    );
    harness.hover_at(screen.center() + egui::vec2(400.0, 300.0));
    harness.step();
    scenario.assertion(
        "releasing Alt-primary drag ends panning",
        harness.state().canvas.viewport_state() == after_alt,
    );

    // Space focuses the selection.
    harness
        .state_mut()
        .canvas
        .select_placements(&[green.state.id]);
    harness.key_press(egui::Key::Space);
    for _ in 0..30 {
        harness.step();
    }
    let focused = harness.state().canvas.viewport_state();
    scenario.fact(
        "focus",
        serde_json::json!({"pan": [focused.camera_pan_x, focused.camera_pan_y], "zoom": focused.camera_zoom}),
    );
    scenario.assertion(
        "Space recenters the selected placement",
        focused.camera_pan_x.abs() < 2.0 && focused.camera_pan_y.abs() < 2.0,
    );
    scenario.capture_screen("screen-003-focused", &mut harness);

    // Sidebar toggle.
    shortcut(&mut harness, CMD, egui::Key::B);
    scenario.assertion("Command-B opens the sidebar", harness.state().sidebar_open);
    scenario.capture_screen("screen-004-sidebar-open", &mut harness);

    // Board create and switch by shortcut.
    let first_board = harness.state().active_board_id;
    shortcut(&mut harness, CMD, egui::Key::N);
    let created_board = harness.state().active_board_id;
    scenario.fact(
        "boards.after-create",
        serde_json::json!({"active": created_board, "count": harness.state().board_summaries().len(), "canvas_items": harness.state().canvas.canvas_stack_order().len()}),
    );
    scenario.assertion(
        "Command-N creates and activates an empty new Board",
        created_board != first_board
            && harness.state().board_summaries().len() == 2
            && harness.state().canvas.canvas_stack_order().is_empty(),
    );
    scenario.capture_screen("screen-005-new-board", &mut harness);

    shortcut(&mut harness, CMD, egui::Key::Num1);
    step_until_placement_loaded(&mut harness, green.state.id);
    scenario.fact(
        "boards.after-switch-back",
        serde_json::json!({"active": harness.state().active_board_id, "canvas_items": harness.state().canvas.canvas_stack_order().len(), "status": harness.state().status}),
    );
    scenario.assertion(
        "Command-1 switches back to the first Board with its placement",
        harness.state().active_board_id == first_board
            && harness
                .state()
                .canvas
                .placement_for_id(green.state.id)
                .is_some(),
    );
    scenario.assertion(
        "returning after Board creation preserves the focused view",
        harness.state().canvas.viewport_state() == focused,
    );
    scenario.capture_screen("screen-006-back-on-first-board", &mut harness);

    harness.state_mut().flush_persistence();
    scenario.assertion(
        "both Boards are durable",
        library.list_boards().unwrap().len() == 2,
    );

    finish(scenario);
}

#[test]
fn inspect_session_restart_restore_writes_artifact() {
    let mut scenario = InspectScenario::new("session-restart-restore");
    scenario.event(
        "seed",
        serde_json::json!({
            "seed": "moved-placement-zoomed-view-open-sidebar",
            "purpose": "a second app instance on the same library restores view, sidebar, and placement geometry through the startup load path",
        }),
    );

    let root = TempRoot::new("snappyref-inspect-session-restart");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let mut app = test_app(Some(library.clone()));
    app.board_editing.persistence =
        crate::active_board::persistence::DocumentPersistence::queued(library.clone());
    let mut harness = app_full_harness(app);
    let red = seed_loaded_placement(
        &library,
        &mut harness,
        "restart-red",
        [224, 68, 76, 255],
        (200, 120),
        egui::pos2(0.0, 0.0),
    );
    let screen = test_screen_rect();
    let red_screen = inspect_world_to_screen(egui::pos2(0.0, 0.0), screen);
    inspect_click_at(&harness, red_screen, egui::Modifiers::NONE);
    harness.step();
    drag_with_modifiers(
        &mut harness,
        red_screen,
        red_screen + egui::vec2(-120.0, 90.0),
        egui::Modifiers::NONE,
        egui::PointerButton::Primary,
    );
    let moved = center_of(&harness, red.state.id);
    harness.hover_at(screen.center());
    harness.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Line,
        delta: egui::vec2(0.0, 1.0),
        phase: egui::TouchPhase::Move,
        modifiers: egui::Modifiers::NONE,
    });
    for _ in 0..30 {
        harness.step();
    }
    shortcut(&mut harness, CMD, egui::Key::B);
    let saved_view = harness.state().canvas.viewport_state();
    scenario.fact(
        "before-restart",
        serde_json::json!({"center": [moved.x, moved.y], "zoom": saved_view.camera_zoom, "sidebar_open": harness.state().sidebar_open}),
    );
    scenario.capture_screen("screen-000-before-restart", &mut harness);
    harness.state_mut().persist_canvas_changes();
    harness.state_mut().save_user_session();
    harness.state_mut().flush_persistence();
    drop(harness);

    // Second instance: the real startup sequence (worker, session load,
    // summaries, existing-item load) before the first frame, so the first
    // frame's own session save cannot race the load.
    let startup_library = library.clone();
    let mut harness = app_full_harness_with_startup(1.0 / 4.0, move |ctx| {
        let mut app = test_app(Some(startup_library.clone()));
        app.board_editing.persistence =
            crate::active_board::persistence::DocumentPersistence::queued(startup_library.clone());
        app.import_worker = Some(crate::image_preparation::ImagePrepWorker::new(
            startup_library,
            ctx.clone(),
            None,
        ));
        app.load_user_session();
        app.refresh_board_summaries();
        app.board_lifecycle().restore().unwrap();
        app
    });
    let loaded = step_until_placement_loaded(&mut harness, red.state.id);
    let restored_view = harness.state().canvas.viewport_state();
    let restored_center = harness
        .state()
        .canvas
        .placement_for_id(red.state.id)
        .map(|placement| placement.state.geometry.center);
    scenario.fact(
        "after-restart",
        serde_json::json!({
            "loaded": loaded,
            "center": restored_center.map(|c| [c.x, c.y]),
            "zoom": restored_view.camera_zoom,
            "sidebar_open": harness.state().sidebar_open,
            "status": harness.state().status,
        }),
    );
    scenario.assertion("restart loads the placement through the worker", loaded);
    scenario.assertion(
        "restart restores the moved placement geometry",
        restored_center.is_some_and(|c| approx(c, moved, 0.5)),
    );
    scenario.assertion(
        "restart restores zoom and pan",
        (restored_view.camera_zoom - saved_view.camera_zoom).abs() < 1e-4
            && (restored_view.camera_pan_x - saved_view.camera_pan_x).abs() < 0.5
            && (restored_view.camera_pan_y - saved_view.camera_pan_y).abs() < 0.5,
    );
    scenario.assertion(
        "restart restores the open sidebar",
        harness.state().sidebar_open,
    );
    scenario.capture_screen("screen-001-after-restart", &mut harness);

    finish(scenario);
}

#[test]
fn inspect_persistence_write_retry_writes_artifact() {
    let mut scenario = InspectScenario::new("persistence-write-retry");
    scenario.event(
        "seed",
        serde_json::json!({
            "seed": "loaded-placement-with-failing-history-table",
            "purpose": "a failed durable write remains queued after bounded retries, status reports it, and retry persists it once storage recovers",
        }),
    );

    let root = TempRoot::new("snappyref-inspect-write-retry");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let mut app = test_app(Some(library.clone()));
    app.board_editing.persistence =
        crate::active_board::persistence::DocumentPersistence::queued(library.clone());
    let mut harness = app_full_harness(app);
    let blue = seed_loaded_placement(
        &library,
        &mut harness,
        "retry-blue",
        [78, 126, 226, 255],
        (140, 100),
        egui::pos2(0.0, 0.0),
    );
    let screen = test_screen_rect();

    let trigger_connection = rusqlite::Connection::open(root.join("library.db")).unwrap();
    trigger_connection
        .execute_batch(
            "CREATE TRIGGER fail_history_insert BEFORE INSERT ON board_history
             BEGIN SELECT RAISE(ABORT, 'injected history failure'); END;
             CREATE TRIGGER fail_history_update BEFORE UPDATE ON board_history
             BEGIN SELECT RAISE(ABORT, 'injected history failure'); END;",
        )
        .unwrap();

    let blue_screen = inspect_world_to_screen(egui::pos2(0.0, 0.0), screen);
    inspect_click_at(&harness, blue_screen, egui::Modifiers::NONE);
    harness.step();
    drag_with_modifiers(
        &mut harness,
        blue_screen,
        blue_screen + egui::vec2(100.0, 0.0),
        egui::Modifiers::NONE,
        egui::PointerButton::Primary,
    );
    harness.state_mut().persist_canvas_changes();
    let first_move = center_of(&harness, blue.state.id);
    scenario.capture_screen("screen-000-moved-while-storage-fails", &mut harness);

    let started = Instant::now();
    let mut failure_status = None;
    while started.elapsed() < Duration::from_secs(15) {
        harness.step();
        if harness.state().status.starts_with("durable write failed") {
            failure_status = Some(harness.state().status.clone());
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    scenario.fact(
        "failure",
        serde_json::json!({"status": failure_status, "elapsed_ms": started.elapsed().as_millis()}),
    );
    scenario.assertion(
        "bounded retries report write failure within 15s",
        failure_status.is_some(),
    );
    scenario.assertion(
        "the canvas keeps the accepted move while the write waits for retry",
        approx(center_of(&harness, blue.state.id), first_move, 0.1),
    );
    // Let the status pill finish its entrance before capturing it.
    for _ in 0..4 {
        std::thread::sleep(Duration::from_millis(50));
        harness.step();
    }
    scenario.capture_screen("screen-001-write-failure-status", &mut harness);

    trigger_connection
        .execute_batch("DROP TRIGGER fail_history_insert; DROP TRIGGER fail_history_update;")
        .unwrap();
    harness.state_mut().flush_persistence();
    scenario.assertion(
        "the original failed write becomes durable without a replacement edit",
        library.load_placements().unwrap().iter().any(|placement| {
            placement.id() == blue.id() && approx(placement.state.geometry.center, first_move, 0.1)
        }),
    );
    let blue_screen = inspect_world_to_screen(first_move, screen);
    inspect_click_at(&harness, blue_screen, egui::Modifiers::NONE);
    harness.step();
    drag_with_modifiers(
        &mut harness,
        blue_screen,
        blue_screen + egui::vec2(0.0, 80.0),
        egui::Modifiers::NONE,
        egui::PointerButton::Primary,
    );
    harness.state_mut().persist_canvas_changes();
    harness.state_mut().flush_persistence();
    let second_move = center_of(&harness, blue.state.id);
    let stored = library
        .load_placements()
        .unwrap()
        .into_iter()
        .find(|placement| placement.state.id == blue.state.id)
        .map(|placement| placement.state.geometry.center);
    scenario.fact(
        "recovered",
        serde_json::json!({"canvas": [second_move.x, second_move.y], "stored": stored.map(|c| [c.x, c.y]), "status": harness.state().status}),
    );
    scenario.assertion(
        "after storage recovers the next write lands with the full current geometry",
        stored.is_some_and(|c| approx(c, second_move, 0.1)),
    );
    scenario.capture_screen("screen-002-recovered", &mut harness);

    finish(scenario);
}

#[test]
fn canvas_selection_gestures_preserve_content_geometry() {
    let root = TempRoot::new("snappyref-selection-gestures");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let mut harness = app_full_harness(test_app(Some(library.clone())));
    let left = seed_loaded_placement(
        &library,
        &mut harness,
        "left",
        [60, 90, 180, 255],
        (100, 100),
        egui::pos2(-150.0, 0.0),
    );
    let right = seed_loaded_placement(
        &library,
        &mut harness,
        "right",
        [180, 90, 60, 255],
        (100, 100),
        egui::pos2(150.0, 0.0),
    );
    let screen = test_screen_rect();
    let point = |p| inspect_world_to_screen(p, screen);
    inspect_click_at(
        &harness,
        point(left.state.geometry.center),
        egui::Modifiers::NONE,
    );
    harness.step();
    assert_eq!(
        harness.state().canvas.selected_placement_ids(),
        vec![left.state.id]
    );
    inspect_click_at(
        &harness,
        point(right.state.geometry.center),
        egui::Modifiers::SHIFT,
    );
    harness.step();
    assert_eq!(harness.state().canvas.selected_placement_ids().len(), 2);
    inspect_click_at(
        &harness,
        point(left.state.geometry.center),
        egui::Modifiers::SHIFT,
    );
    harness.step();
    assert_eq!(
        harness.state().canvas.selected_placement_ids(),
        vec![right.state.id]
    );
    inspect_click_at(
        &harness,
        point(egui::pos2(0.0, 250.0)),
        egui::Modifiers::NONE,
    );
    harness.step();
    assert!(harness.state().canvas.selected_placement_ids().is_empty());
    for (min, max, modifiers, count) in [
        (
            egui::pos2(-350.0, -250.0),
            egui::pos2(-10.0, 250.0),
            egui::Modifiers::NONE,
            1,
        ),
        (
            egui::pos2(10.0, -250.0),
            egui::pos2(350.0, 250.0),
            egui::Modifiers::SHIFT,
            2,
        ),
    ] {
        drag_with_modifiers(
            &mut harness,
            point(min),
            point(max),
            modifiers,
            egui::PointerButton::Primary,
        );
        assert_eq!(harness.state().canvas.selected_placement_ids().len(), count);
    }
    for placement in [left, right] {
        assert_eq!(
            harness
                .state()
                .canvas
                .placement_for_id(placement.state.id)
                .unwrap()
                .state
                .geometry,
            placement.state.geometry
        );
    }
    assert_eq!(harness.state().canvas.canvas_stack_order().len(), 2);
}

#[test]
fn space_focus_fits_multiple_images_textbox_and_mixed_selection() {
    let root = TempRoot::new("snappyref-focus-kinds");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let mut harness = app_full_harness(test_app(Some(library.clone())));
    let left = seed_loaded_placement(
        &library,
        &mut harness,
        "focus-left",
        [30, 60, 90, 255],
        (100, 150),
        egui::pos2(-150.0, -80.0),
    );
    let right = seed_loaded_placement(
        &library,
        &mut harness,
        "focus-right",
        [90, 60, 30, 255],
        (180, 100),
        egui::pos2(220.0, 110.0),
    );
    let annotation = crate::data::CanvasAnnotation::new(
        901,
        CanvasAnnotationKind::Textbox,
        egui::pos2(-50.0, -200.0),
        egui::pos2(190.0, -100.0),
        "Focus note".to_owned(),
        CanvasTextSize::M,
        CanvasStrokeWidth::M,
        crate::data::CanvasAnnotationColor::Sky,
        false,
    );
    harness.state_mut().canvas.add_annotation(annotation);
    for (images, notes) in [
        (vec![left.id(), right.id()], vec![]),
        (vec![], vec![901]),
        (vec![left.id()], vec![901]),
    ] {
        harness.state_mut().canvas.select_content(&images, &notes);
        harness.key_press(egui::Key::Space);
        for _ in 0..30 {
            harness.step();
        }
        let selected = harness
            .state()
            .canvas
            .selection_screen_rect(test_screen_rect())
            .unwrap();
        assert!(
            test_screen_rect().shrink(20.0).contains_rect(selected),
            "{selected:?}"
        );
        assert_eq!(harness.state().canvas.selected_placement_ids(), images);
    }
}

#[test]
fn abandoned_external_hover_leaves_no_content_or_history() {
    let root = TempRoot::new("snappyref-abandoned-hover");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let source = library
        .store_rgba_image("hover", &solid_rgba([50, 90, 140, 255], 40, 30), 40, 30)
        .unwrap();
    let (worker, _) = crate::image_preparation::ImagePrepWorker::controlled();
    let mut app = test_app(Some(library.clone()));
    app.import_worker = Some(worker);
    let mut harness = app_full_harness(app);
    harness.hover_at(egui::pos2(700.0, 400.0));
    harness.input_mut().hovered_files = vec![egui::HoveredFile {
        path: Some(source),
        ..Default::default()
    }];
    harness.step();
    assert_eq!(
        harness
            .state()
            .canvas
            .provisional_placements_in_selection_order()
            .len(),
        1
    );
    harness.input_mut().hovered_files.clear();
    harness.event(egui::Event::PointerGone);
    harness.step();
    assert!(harness.state().canvas.canvas_stack_order().is_empty());
    assert_eq!(harness.state().placement_creation.pending_len(), 0);
    harness.state_mut().flush_persistence();
    assert!(library.load_placements().unwrap().is_empty());
    let board = harness.state().active_board_id;
    assert!(
        library
            .load_document_history()
            .unwrap()
            .undo(board)
            .is_none()
    );
}

#[test]
fn inspect_slow_url_and_inactive_mixed_batch_writes_artifact() {
    use std::io::{Read, Write};
    let mut scenario = InspectScenario::new("slow-url-inactive-mixed-batch");
    let root = TempRoot::new("snappyref-slow-url-batch");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let source = root.join("valid.png");
    image::RgbaImage::from_pixel(80, 50, image::Rgba([55, 120, 200, 255]))
        .save(&source)
        .unwrap();
    let corrupt = root.join("corrupt.png");
    std::fs::write(&corrupt, b"corrupt image").unwrap();
    let png = std::fs::read(&source).unwrap();
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let address = listener.local_addr().unwrap();
    let (release, wait) = std::sync::mpsc::channel();
    let server = std::thread::spawn(move || {
        for _ in 0..2 {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(10)))
                .unwrap();
            let mut request = [0; 2048];
            let n = stream.read(&mut request).unwrap();
            let slow = String::from_utf8_lossy(&request[..n]).contains("/slow.png");
            if slow {
                wait.recv_timeout(Duration::from_secs(15)).unwrap();
                write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", png.len()).unwrap();
                stream.write_all(&png).unwrap();
            } else {
                stream.write_all(b"HTTP/1.1 503 Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
            }
        }
    });
    let mut app = test_app(Some(library.clone()));
    app.import_worker = Some(crate::image_preparation::ImagePrepWorker::new(
        library.clone(),
        egui::Context::default(),
        None,
    ));
    let mut harness = app_full_harness(app);
    let screen = test_screen_rect();
    let view_before = harness.state().canvas.viewport_state();
    harness.hover_at(screen.center());
    harness.event(egui::Event::Paste(format!("http://{address}/slow.png")));
    harness.step();
    let slow_id = harness.state().canvas.selected_placement_ids()[0];
    assert_eq!(harness.state().canvas.viewport_state(), view_before);
    assert_eq!(harness.state().placement_creation.pending_len(), 1);
    scenario.capture_screen("screen-000-url-loading", &mut harness);
    drag_with_modifiers(
        &mut harness,
        screen.center(),
        screen.center() + egui::vec2(85.0, 65.0),
        egui::Modifiers::NONE,
        egui::PointerButton::Primary,
    );
    let moved = center_of(&harness, slow_id);
    assert!(moved.distance(egui::Pos2::ZERO) > 20.0);
    drag_with_modifiers(
        &mut harness,
        screen.center() + egui::vec2(300.0, 200.0),
        screen.center() + egui::vec2(360.0, 250.0),
        egui::Modifiers::ALT,
        egui::PointerButton::Primary,
    );
    let navigated = harness.state().canvas.viewport_state();
    assert_ne!(navigated, view_before);
    release.send(()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while harness.state().placement_creation.pending_len() != 0 && Instant::now() < deadline {
        harness.step();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(harness.state().placement_creation.pending_len(), 0);
    assert!(
        harness
            .state()
            .canvas
            .placement_for_id(slow_id)
            .unwrap()
            .is_loaded()
    );
    assert!(center_of(&harness, slow_id).distance(moved) < 0.01);
    assert_eq!(harness.state().canvas.viewport_state(), navigated);
    scenario.assertion("slow URL accepts without viewport movement; pending placement moves and pans; completion preserves identity, edited center and navigated viewport", true);
    scenario.capture_screen("screen-001-url-loaded", &mut harness);
    harness.input_mut().dropped_files = vec![
        dropped_file(source),
        dropped_file(corrupt),
        dropped_file(format!("http://{address}/fail.png")),
    ];
    harness.step();
    let ids = harness.state().canvas.selected_placement_ids();
    assert_eq!(ids.len(), 3);
    let board_a = harness.state().active_board_id;
    shortcut(&mut harness, CMD, egui::Key::N);
    let board_b = harness.state().active_board_id;
    assert_ne!(board_a, board_b);
    let deadline = Instant::now() + Duration::from_secs(10);
    while harness.state().placement_creation.pending_len() != 0 && Instant::now() < deadline {
        harness.step();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(harness.state().placement_creation.pending_len(), 0);
    assert!(harness.state().canvas.canvas_stack_order().is_empty());
    harness.state_mut().flush_persistence();
    let completed = library.load_placements_for_board(board_a).unwrap();
    assert_eq!(completed.len(), 4);
    assert_eq!(completed.iter().filter(|p| p.is_loaded()).count(), 2);
    assert_eq!(
        completed
            .iter()
            .filter(|p| matches!(p.content, crate::data::PlacementContent::Failed { .. }))
            .count(),
        2
    );
    assert!(
        library
            .load_placements_for_board(board_b)
            .unwrap()
            .is_empty()
    );
    let batch = completed
        .iter()
        .filter(|p| ids.contains(&p.id()))
        .collect::<Vec<_>>();
    for (i, a) in batch.iter().enumerate() {
        for b in &batch[i + 1..] {
            let overlap = a.visual_bounds().intersect(b.visual_bounds());
            assert!(
                overlap.width() <= 0.01 || overlap.height() <= 0.01,
                "batch bounds {:?} {:?}",
                a.visual_bounds(),
                b.visual_bounds()
            );
        }
    }
    scenario.fact(
        "completed-inactive-batch-layout",
        serde_json::json!(
            batch
                .iter()
                .map(|p| (
                    p.id(),
                    p.state.geometry.center.x,
                    p.state.geometry.center.y,
                    p.state.geometry.scale
                ))
                .collect::<Vec<_>>()
        ),
    );
    shortcut(&mut harness, CMD, egui::Key::Num1);
    for _ in 0..20 {
        harness.step();
    }
    assert_eq!(harness.state().active_board_id, board_a);
    assert_eq!(harness.state().canvas.canvas_stack_order().len(), 4);
    scenario.assertion("mixed valid, corrupt and failed HTTP batch completes only on inactive target board with stable IDs and nonoverlapping final layout", true);
    scenario.capture_screen("screen-002-return-to-completed-batch", &mut harness);
    harness.state_mut().canvas.select_placements(&ids);
    harness.key_press(INSPECT_DELETE_KEY);
    harness.step();
    assert_eq!(harness.state().canvas.canvas_stack_order().len(), 1);
    scenario.assertion(
        "failed placements leave no endless loading and delete with the mixed batch",
        true,
    );
    server.join().unwrap();
    finish(scenario);
}

#[test]
fn overlay_scroll_routing_releases_canvas_for_next_drag() {
    let root = TempRoot::new("snappyref-overlay-routing");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let mut app = test_app(Some(library.clone()));
    app.settings.semantic_search_first_launch_prompt_seen = true;
    let mut harness = app_full_harness(app);
    let placement = seed_loaded_placement(
        &library,
        &mut harness,
        "routing",
        [60, 150, 90, 255],
        (100, 80),
        egui::pos2(200.0, 180.0),
    );
    shortcut(&mut harness, CMD, egui::Key::B);
    let before = harness.state().canvas.viewport_state();
    harness.hover_at(sidebar_rect(test_screen_rect()).center());
    harness.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Line,
        delta: egui::vec2(0.0, 2.0),
        phase: egui::TouchPhase::Move,
        modifiers: egui::Modifiers::NONE,
    });
    for _ in 0..20 {
        harness.step();
    }
    assert_eq!(harness.state().canvas.viewport_state(), before);
    shortcut(&mut harness, CMD, egui::Key::F);
    assert!(harness.state().search.overlay.open);
    let overlay = harness
        .state()
        .search
        .overlay
        .last_expanded_overlay_rect
        .unwrap();
    harness.hover_at(overlay.center());
    harness.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0.0, 50.0),
        phase: egui::TouchPhase::Move,
        modifiers: egui::Modifiers::NONE,
    });
    for _ in 0..20 {
        harness.step();
    }
    assert_eq!(harness.state().canvas.viewport_state(), before);
    harness.key_press(egui::Key::Escape);
    harness.step();
    assert!(!harness.state().search.overlay.open);
    shortcut(&mut harness, CMD, egui::Key::B);
    let from = inspect_world_to_screen(placement.state.geometry.center, test_screen_rect());
    drag_with_modifiers(
        &mut harness,
        from,
        from + egui::vec2(90.0, 60.0),
        egui::Modifiers::NONE,
        egui::PointerButton::Primary,
    );
    assert!(center_of(&harness, placement.id()).distance(placement.state.geometry.center) > 30.0);
    let before = harness.state().canvas.viewport_state();
    harness.hover_at(test_screen_rect().center());
    harness.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Line,
        delta: egui::vec2(0.0, 1.0),
        phase: egui::TouchPhase::Move,
        modifiers: egui::Modifiers::NONE,
    });
    for _ in 0..20 {
        harness.step();
    }
    assert!(harness.state().canvas.viewport_state().camera_zoom > before.camera_zoom);
}

#[test]
fn inspect_multi_transform_and_layout_contracts_writes_artifact() {
    let mut scenario = InspectScenario::new("multi-transform-layout-contracts");
    let root = TempRoot::new("snappyref-transform-layout");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let mut harness = app_full_harness(test_app(Some(library.clone())));
    let a = seed_loaded_placement(
        &library,
        &mut harness,
        "transform-a",
        [60, 100, 180, 255],
        (140, 100),
        egui::pos2(-180.0, -80.0),
    );
    let b = seed_loaded_placement(
        &library,
        &mut harness,
        "transform-b",
        [180, 100, 60, 255],
        (80, 160),
        egui::pos2(180.0, 80.0),
    );
    let other = seed_loaded_placement(
        &library,
        &mut harness,
        "unselected",
        [60, 180, 100, 255],
        (90, 90),
        egui::pos2(650.0, 250.0),
    );
    let ids = [a.id(), b.id()];
    let snapshot = |h: &Harness<'_, SnappyRefApp>| {
        ids.map(|id| {
            h.state()
                .canvas
                .placement_for_id(id)
                .unwrap()
                .state
                .geometry
        })
    };
    harness.state_mut().canvas.select_placements(&ids);
    let screen = test_screen_rect();
    let before = snapshot(&harness);
    let start = inspect_world_to_screen(before[0].center, screen);
    drag_with_modifiers(
        &mut harness,
        start,
        start + egui::vec2(90.0, 55.0),
        CMD,
        egui::PointerButton::Primary,
    );
    let moved = snapshot(&harness);
    assert!((moved[0].center - before[0].center).length() > 50.0);
    assert!(
        (moved[0].center - before[0].center - (moved[1].center - before[1].center)).length() < 0.01
    );
    assert_eq!(
        harness
            .state()
            .canvas
            .placement_for_id(other.id())
            .unwrap()
            .state
            .geometry,
        other.state.geometry
    );
    scenario.assertion(
        "multi-selection drag preserves relative geometry and unselected image",
        true,
    );
    shortcut(&mut harness, CMD, egui::Key::Z);
    assert_eq!(snapshot(&harness), before);
    harness.state_mut().canvas.select_placements(&ids);
    let view = harness.state().canvas.viewport_state();
    drag_with_modifiers(
        &mut harness,
        start,
        start + egui::vec2(90.0, 0.0),
        CMD_ALT,
        egui::PointerButton::Primary,
    );
    let scaled = snapshot(&harness);
    let factor = scaled[0].scale / before[0].scale;
    assert!(factor > 1.05);
    assert!((scaled[1].scale / before[1].scale - factor).abs() < 0.001);
    assert!(
        (scaled[0].center.distance(scaled[1].center) / before[0].center.distance(before[1].center)
            - factor)
            .abs()
            < 0.001
    );
    assert_eq!(harness.state().canvas.viewport_state(), view);
    scenario.assertion(
        "Mod-Alt scales sizes and spacing uniformly without panning",
        true,
    );
    shortcut(&mut harness, CMD, egui::Key::Z);
    harness.state_mut().canvas.select_placements(&ids);
    harness.state_mut().settings.rotation_snap_degrees = 15.0;
    harness.key_down(egui::Key::R);
    harness.step();
    let bounds = harness
        .state()
        .canvas
        .selection_screen_rect(screen)
        .unwrap();
    let handle = bounds.center_top() - egui::vec2(0.0, 18.0);
    drag_with_modifiers(
        &mut harness,
        handle,
        bounds.center() + egui::vec2(80.0, -130.0),
        egui::Modifiers::NONE,
        egui::PointerButton::Primary,
    );
    harness.key_up(egui::Key::R);
    harness.step();
    let rotated = snapshot(&harness);
    let steps = rotated[0].rotation / 15.0_f32.to_radians();
    assert!(steps.abs() >= 1.0 && (steps - steps.round()).abs() < 0.001);
    assert!((rotated[0].rotation - rotated[1].rotation).abs() < 0.001);
    assert!(
        (rotated[0].center.distance(rotated[1].center)
            - before[0].center.distance(before[1].center))
        .abs()
            < 0.01
    );
    scenario.assertion(
        "R-handle rotation snaps both images in15degree steps and preserves separation",
        true,
    );
    shortcut(&mut harness, CMD, egui::Key::Z);
    harness.state_mut().canvas.select_placements(&ids);
    harness.key_down(egui::Key::R);
    harness.step();
    let bounds = harness
        .state()
        .canvas
        .selection_screen_rect(screen)
        .unwrap();
    let handle = bounds.center_top() - egui::vec2(0.0, 18.0);
    harness.event(egui::Event::PointerMoved(handle));
    harness.event(egui::Event::PointerButton {
        pos: handle,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::NONE,
    });
    harness.step();
    harness.event(egui::Event::ModifiersChanged(egui::Modifiers::ALT));
    let target = bounds.center() + egui::vec2(80.0, -130.0);
    harness.event(egui::Event::PointerMoved(target));
    harness.step();
    harness.event(egui::Event::PointerButton {
        pos: target,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::ALT,
    });
    harness.step();
    harness.event(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
    harness.key_up(egui::Key::R);
    harness.step();
    let precise = snapshot(&harness)[0].rotation / 15.0_f32.to_radians();
    assert!((precise - precise.round()).abs() > 0.05);
    scenario.assertion("Alt added during active rotation bypasses snap", true);
    shortcut(&mut harness, CMD, egui::Key::Z);
    harness.state_mut().canvas.select_placements(&ids);
    drag_with_modifiers(
        &mut harness,
        start,
        egui::pos2(screen.right() + 80.0, screen.center().y),
        CMD_ALT,
        egui::PointerButton::Primary,
    );
    shortcut(&mut harness, CMD, egui::Key::Z);
    harness.state_mut().canvas.select_placements(&ids);
    let before_normal = snapshot(&harness);
    let start = inspect_world_to_screen(before_normal[0].center, screen);
    drag_with_modifiers(
        &mut harness,
        start,
        start + egui::vec2(30.0, 35.0),
        CMD,
        egui::PointerButton::Primary,
    );
    let after_normal = snapshot(&harness);
    assert_eq!(after_normal[0].scale, before_normal[0].scale);
    assert!(after_normal[0].center.distance(before_normal[0].center) > 20.0);
    scenario.assertion(
        "release outside canvas ends scaling; next ordinary drag moves without scaling",
        true,
    );
    shortcut(&mut harness, CMD, egui::Key::Z);
    scenario.capture_screen("screen-000-after-transforms", &mut harness);
    for gap in [0.0, 24.0] {
        let before_setting = snapshot(&harness);
        harness.state_mut().settings.placement_gap = gap;
        harness.step();
        assert_eq!(snapshot(&harness), before_setting);
        for key in [egui::Key::W, egui::Key::E] {
            harness.state_mut().canvas.select_placements(&ids);
            let original = snapshot(&harness);
            harness.key_press(key);
            harness.step();
            let arranged = snapshot(&harness);
            let ra = harness
                .state()
                .canvas
                .placement_for_id(ids[0])
                .unwrap()
                .visual_bounds();
            let rb = harness
                .state()
                .canvas
                .placement_for_id(ids[1])
                .unwrap()
                .visual_bounds();
            let actual = if key == egui::Key::W {
                (ra.left().max(rb.left()) - ra.right().min(rb.right())).abs()
            } else {
                (ra.top().max(rb.top()) - ra.bottom().min(rb.bottom())).abs()
            };
            assert!((actual - gap).abs() < 0.01, "gap {gap} actual {actual}");
            assert_eq!(
                [arranged[0].size, arranged[1].size],
                [original[0].size, original[1].size]
            );
            assert_eq!(
                [arranged[0].scale, arranged[1].scale],
                [original[0].scale, original[1].scale]
            );
            shortcut(&mut harness, CMD, egui::Key::Z);
            assert_eq!(snapshot(&harness), original);
        }
    }
    scenario.assertion("row and column respect gap0/24 and proportions, undo restores, setting alone does not rearrange", true);
    for key in [
        egui::Key::ArrowLeft,
        egui::Key::ArrowRight,
        egui::Key::ArrowUp,
        egui::Key::ArrowDown,
    ] {
        harness.state_mut().canvas.select_placements(&ids);
        let original = snapshot(&harness);
        harness.key_press(key);
        harness.step();
        assert_eq!(
            harness
                .state()
                .canvas
                .placement_for_id(other.id())
                .unwrap()
                .state
                .geometry,
            other.state.geometry
        );
        let ra = harness
            .state()
            .canvas
            .placement_for_id(ids[0])
            .unwrap()
            .visual_bounds();
        let rb = harness
            .state()
            .canvas
            .placement_for_id(ids[1])
            .unwrap()
            .visual_bounds();
        let overlap = ra.intersect(rb);
        assert!(overlap.width() <= 0.01 || overlap.height() <= 0.01);
        shortcut(&mut harness, CMD, egui::Key::Z);
        assert_eq!(snapshot(&harness), original);
    }
    scenario.assertion(
        "four arrow settle commands preserve unselected image, avoid overlap and undo exactly",
        true,
    );
    for modifiers in [CMD, egui::Modifiers { shift: true, ..CMD }] {
        harness.state_mut().canvas.select_placements(&ids);
        let original = snapshot(&harness);
        let target = if modifiers.shift {
            original
                .iter()
                .map(|g| g.size.x * g.size.y * g.scale * g.scale)
                .sum::<f32>()
                / 2.0
        } else {
            crate::placement::DEFAULT_PLACEMENT_TARGET_AREA
        };
        shortcut(&mut harness, modifiers, egui::Key::L);
        for (after, before) in snapshot(&harness).iter().zip(original) {
            assert!((after.size.x * after.size.y * after.scale * after.scale - target).abs() < 1.0);
            assert_eq!(after.size, before.size);
        }
        shortcut(&mut harness, CMD, egui::Key::Z);
        assert_eq!(snapshot(&harness), original);
    }
    scenario.assertion("default and selection average-area shortcuts hit their targets, preserve aspect ratios and undo", true);
    scenario.capture_screen("screen-001-after-layouts", &mut harness);
    finish(scenario);
}
