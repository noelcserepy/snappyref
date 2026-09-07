//! Inspect scenarios for the main user operations: canvas editing, navigation
//! and Board switching, session restore across a restart, and recovery from a
//! dropped durable write.

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
        .persist_imported_image(
            &path,
            "image/png",
            size.0,
            size.1,
            size.0 as u64 * size.1 as u64 * 4,
            center,
            1.0,
        )
        .unwrap();
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
    harness.event_modifiers(egui::Event::PointerMoved(from), modifiers);
    harness.event_modifiers(
        egui::Event::PointerButton {
            pos: from,
            button,
            pressed: true,
            modifiers,
        },
        modifiers,
    );
    for _ in 0..8 {
        harness.step();
    }
    std::thread::sleep(Duration::from_millis(120));
    let mid = from + (to - from) * 0.5;
    harness.event_modifiers(egui::Event::PointerMoved(mid), modifiers);
    harness.step();
    harness.event_modifiers(egui::Event::PointerMoved(to), modifiers);
    harness.step();
    harness.event_modifiers(
        egui::Event::PointerButton {
            pos: to,
            button,
            pressed: false,
            modifiers,
        },
        modifiers,
    );
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

    // Textbox: T + click on empty canvas, type, Escape.
    let textbox_screen = screen.center() + egui::vec2(0.0, -250.0);
    scenario.action("hold T and click empty canvas", textbox_screen);
    harness.key_down(egui::Key::T);
    harness.step();
    inspect_click_at(&harness, textbox_screen, egui::Modifiers::NONE);
    harness.step();
    harness.key_up(egui::Key::T);
    harness.step();
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
    harness.key_press(egui::Key::Backspace);
    harness.step();
    scenario.assertion(
        "Backspace deletes the selected placement",
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
fn inspect_persistence_dropped_write_writes_artifact() {
    let mut scenario = InspectScenario::new("persistence-dropped-write");
    scenario.event(
        "seed",
        serde_json::json!({
            "seed": "loaded-placement-with-failing-history-table",
            "purpose": "a durable write that keeps failing is dropped after bounded retries, the status reports it, and the next write lands once storage recovers",
        }),
    );

    let root = TempRoot::new("snappyref-inspect-dropped-write");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let mut app = test_app(Some(library.clone()));
    app.board_editing.persistence =
        crate::active_board::persistence::DocumentPersistence::queued(library.clone());
    let mut harness = app_full_harness(app);
    let blue = seed_loaded_placement(
        &library,
        &mut harness,
        "dropped-blue",
        [78, 126, 226, 255],
        (140, 100),
        egui::pos2(0.0, 0.0),
    );
    let screen = test_screen_rect();

    let trigger_connection = rusqlite::Connection::open(root.join("library.db")).unwrap();
    trigger_connection
        .execute_batch(
            "CREATE TRIGGER fail_history_insert BEFORE INSERT ON board_document_history
             BEGIN SELECT RAISE(ABORT, 'injected history failure'); END;
             CREATE TRIGGER fail_history_update BEFORE UPDATE ON board_document_history
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
    let mut dropped_status = None;
    while started.elapsed() < Duration::from_secs(15) {
        harness.step();
        if harness.state().status.starts_with("dropped durable write") {
            dropped_status = Some(harness.state().status.clone());
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    scenario.fact(
        "dropped",
        serde_json::json!({"status": dropped_status, "elapsed_ms": started.elapsed().as_millis()}),
    );
    scenario.assertion(
        "bounded retries end in a dropped-write status within 15s",
        dropped_status.is_some(),
    );
    scenario.assertion(
        "the canvas keeps the accepted move while the write is dropped",
        approx(center_of(&harness, blue.state.id), first_move, 0.1),
    );
    // Let the status pill finish its entrance before capturing it.
    for _ in 0..4 {
        std::thread::sleep(Duration::from_millis(50));
        harness.step();
    }
    scenario.capture_screen("screen-001-dropped-write-status", &mut harness);

    trigger_connection
        .execute_batch("DROP TRIGGER fail_history_insert; DROP TRIGGER fail_history_update;")
        .unwrap();
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
