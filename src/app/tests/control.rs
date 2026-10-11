use super::*;
use crate::app::control::Step;
use crate::data::AiControlAccess;
use serde_json::{Value, json};

fn control_app(root: &TempRoot) -> (SnappyRefApp, egui::Context) {
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let mut app = test_app(Some(library));
    app.board_lifecycle().restore().unwrap();
    let ctx = egui::Context::default();
    crate::ui::theme::configure_theme(&ctx);
    // Label sizes come from text layout, which needs one frame of fonts.
    crate::ui::test_frame::run_ui(&ctx, Default::default(), |_| {});
    (app, ctx)
}

fn call(
    app: &mut SnappyRefApp,
    ctx: &egui::Context,
    tool: &str,
    arguments: Value,
) -> Result<Value, String> {
    match app.control_call(ctx, tool, arguments)? {
        Step::Done(value) => Ok(value),
        Step::Wait(..) => panic!("{tool} waits"),
    }
}

fn item_count(app: &mut SnappyRefApp, ctx: &egui::Context) -> usize {
    let board = call(app, ctx, "get_board", json!({})).unwrap();
    board["items"].as_array().unwrap().len()
}

#[test]
fn every_advertised_tool_has_a_handler() {
    let root = TempRoot::new("snappyref-control-catalogue");
    let (mut app, ctx) = control_app(&root);
    for tool in crate::control::tool_names() {
        if let Err(error) = app.control_call(&ctx, tool, json!({})) {
            assert!(!error.contains("has no tool named"), "{error}");
        }
    }
}

#[test]
fn edit_board_call_is_one_undo_step() {
    let root = TempRoot::new("snappyref-control-edit");
    let (mut app, ctx) = control_app(&root);
    let edited = call(
        &mut app,
        &ctx,
        "edit_board",
        json!({"edits": [
            {"action": "add", "type": "label", "text": "Heads", "x": 0, "y": 0, "ref": "title"},
            {"action": "add", "type": "rectangle", "x": 0, "y": 80, "width": 200, "height": 100, "ref": "frame"},
            {"action": "group", "ids": ["$title", "$frame"], "name": "Faces", "flow": "column", "ref": "faces"},
            {"action": "add", "type": "arrow", "x1": 300, "y1": 0, "x2": 350, "y2": 50},
        ]}),
    )
    .unwrap();
    let refs = &edited["refs"];
    assert_eq!(
        edited["groups"][0]["members"],
        json!([refs["title"], refs["frame"]])
    );
    assert_eq!(edited["groups"][0]["flow"], "column");
    assert_eq!(item_count(&mut app, &ctx), 3);

    // Deleting a Group takes its members with it.
    let deleted = call(
        &mut app,
        &ctx,
        "edit_board",
        json!({"edits": [{"action": "delete", "ids": [refs["faces"]]}]}),
    )
    .unwrap();
    assert_eq!(deleted["deleted"].as_array().unwrap().len(), 3);
    assert_eq!(item_count(&mut app, &ctx), 1);

    call(&mut app, &ctx, "undo", json!({})).unwrap();
    let board = call(&mut app, &ctx, "get_board", json!({})).unwrap();
    assert_eq!(board["items"].as_array().unwrap().len(), 3);
    assert_eq!(board["groups"][0]["name"], "Faces");
    call(&mut app, &ctx, "undo", json!({})).unwrap();
    assert_eq!(item_count(&mut app, &ctx), 0);
    assert_eq!(
        call(&mut app, &ctx, "undo", json!({})).unwrap_err(),
        "Nothing to undo."
    );
}

#[test]
fn refused_edit_board_call_applies_nothing() {
    let root = TempRoot::new("snappyref-control-check");
    let (mut app, ctx) = control_app(&root);
    let error = call(
        &mut app,
        &ctx,
        "edit_board",
        json!({"edits": [
            {"action": "add", "type": "rectangle", "x": 0, "y": 0, "width": 10, "height": 10, "ref": "box"},
            {"action": "set", "id": "$box", "text": "no"},
        ]}),
    )
    .unwrap_err();
    assert!(
        error.starts_with("edits[1]: set on a rectangle does not take \"text\"."),
        "{error}"
    );
    assert!(error.ends_with("Nothing was applied."), "{error}");
    assert_eq!(item_count(&mut app, &ctx), 0);

    let made = call(
        &mut app,
        &ctx,
        "edit_board",
        json!({"edits": [
            {"action": "add", "type": "label", "text": "Heads", "x": 0, "y": 0, "ref": "title"},
            {"action": "add", "type": "rectangle", "x": 0, "y": 80, "width": 200, "height": 100, "ref": "frame"},
            {"action": "group", "ids": ["$title", "$frame"], "flow": "column"},
        ]}),
    )
    .unwrap();
    let before = call(&mut app, &ctx, "get_board", json!({})).unwrap();

    // The check passes this call. The canvas refuses its last edit, after the others applied.
    let error = call(
        &mut app,
        &ctx,
        "edit_board",
        json!({"edits": [
            {"action": "set", "id": made["refs"]["title"], "text": "Hands"},
            {"action": "delete", "ids": [made["refs"]["frame"]]},
            {"action": "add", "type": "rectangle", "x": 400, "y": 0, "width": 10, "height": 10, "ref": "loose"},
            {"action": "remove_from_group", "ids": ["$loose"]},
        ]}),
    )
    .unwrap_err();
    assert!(error.starts_with("edits[3] failed:"), "{error}");
    assert!(error.ends_with("Nothing was applied."), "{error}");
    assert_eq!(
        call(&mut app, &ctx, "get_board", json!({})).unwrap(),
        before
    );
    // The refused call left no Undo step either.
    call(&mut app, &ctx, "undo", json!({})).unwrap();
    assert_eq!(item_count(&mut app, &ctx), 0);
}

#[test]
fn library_removal_waits_for_the_user_unless_access_is_full() {
    let root = TempRoot::new("snappyref-control-confirm");
    let (mut app, ctx) = control_app(&root);
    let library = app.library.clone().unwrap();
    let path = library
        .store_rgba_image("confirm", &solid_rgba([200, 40, 40, 255], 8, 8), 8, 8)
        .unwrap();
    let asset = library
        .register_original(&path, "image/png", 8, 8, 256, None)
        .unwrap()
        .id;
    let remove = json!({"assets": [asset]});

    app.settings.ai_control = AiControlAccess::Ask;
    let asked = app.control_call(&ctx, "remove_from_library", remove.clone());
    assert!(matches!(asked, Ok(Step::Wait(..))));
    assert_eq!(library.load_assets(&[asset]).unwrap().len(), 1);

    app.settings.ai_control = AiControlAccess::Full;
    let removed = call(&mut app, &ctx, "remove_from_library", remove).unwrap();
    assert_eq!(removed, json!({"removed": [asset]}));
    assert!(library.load_assets(&[asset]).unwrap().is_empty());
}

#[test]
fn add_images_answers_from_its_board_after_the_user_opens_another() {
    let ctx = egui::Context::default();
    let root = TempRoot::new("snappyref-control-import-board-switch");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let first = library.list_boards().unwrap().remove(0);
    let second = library.fixture_board("Second").unwrap();
    let mut app = test_app(Some(library.clone()));
    app.board_editing.persistence = crate::app::DocumentPersistence::queued(library.clone());
    let reservation = app
        .reserve_import(
            Some((1, 1)),
            egui::Pos2::ZERO,
            crate::data::ContentResolutionSource::Unknown {
                description: "AI import".to_owned(),
            },
        )
        .unwrap();
    let ids = [reservation.placement_id];
    app.switch_to_board(second.id);
    assert!(app.added_images(first.id, &ids, false).is_none());

    let mut loaded = persisted_loaded_item(&library, reservation.placement_id, first.id);
    loaded.queue_id = Some(reservation.queue_id);
    loaded.origin = crate::image_preparation::ImportOrigin::Path;
    app.install_test_prepared_image(&ctx, loaded);

    let added = app.added_images(first.id, &ids, false).unwrap().unwrap();
    assert_eq!(added["items"][0]["id"], reservation.placement_id);
    assert_eq!(added["items"][0].get("status"), None);
}

type AiCall = std::thread::JoinHandle<Option<Value>>;

/// Sends one tool call the way `--mcp` does, from its own thread.
fn start_ai_call(root: &TempRoot, tool: &str, arguments: Value) -> AiCall {
    let root = root.to_path_buf();
    let call = json!({"tool": tool, "arguments": arguments});
    std::thread::spawn(move || crate::control::call_running_app(&root, &call))
}

/// Steps the app until it answers. Returns the tool's JSON, or its error text.
fn finish_ai_call(harness: &mut Harness<'_, SnappyRefApp>, call: AiCall) -> Result<Value, String> {
    for _ in 0..2000 {
        if call.is_finished() {
            break;
        }
        harness.step();
        std::thread::sleep(Duration::from_millis(2));
    }
    let result = call.join().unwrap().ok_or("no connection")?;
    let text = result["content"][0]["text"].as_str().unwrap_or_default();
    if result["isError"] == true {
        return Err(text.to_owned());
    }
    serde_json::from_str(text).map_err(|error| error.to_string())
}

#[test]
fn inspect_ai_control_writes_artifact() {
    let mut scenario = InspectScenario::new("ai-control");
    scenario.event(
        "seed",
        json!({
            "seed": "one-library-image-settings-open-access-off",
            "purpose": "turn AI control on in Settings, edit the board over the loopback listener, confirm a library removal in the app, turn access off",
        }),
    );

    let root = TempRoot::new("snappyref-inspect-ai-control");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let source = library
        .store_rgba_image(
            "control-swatch",
            &solid_rgba([224, 168, 72, 255], 8, 8),
            8,
            8,
        )
        .unwrap();
    let asset = library
        .register_original(&source, "image/png", 8, 8, 256, None)
        .unwrap()
        .id;
    let mut app = test_app(Some(library.clone()));
    app.board_lifecycle().restore().unwrap();
    app.sidebar_open = true;
    app.sidebar_tab = SidebarTab::Settings;
    app.settings.semantic_search_first_launch_prompt_seen = true;
    let library_root = root.to_path_buf();
    let mut harness = app_full_harness_with_startup(1.0 / 4.0, move |ctx| {
        app.start_control(ctx, library_root);
        app
    });
    harness.run_steps(4);
    let endpoint = root.join("control.json");
    scenario.capture_screen("screen-000-access-off", &mut harness);
    scenario.assertion(
        "access is off by default and publishes no endpoint",
        harness.state().settings.ai_control == AiControlAccess::Off && !endpoint.exists(),
    );

    let ask = harness.get_by_label("Ask").rect();
    scenario.action("choose Ask under AI control", ask.center());
    inspect_click_at(&harness, ask.center(), egui::Modifiers::NONE);
    harness.run_steps(4);
    scenario.capture_screen("screen-001-access-ask", &mut harness);
    scenario.assertion(
        "choosing Ask publishes the endpoint and offers the MCP config",
        endpoint.exists() && harness.query_by_label("Copy MCP config").is_some(),
    );

    let call = start_ai_call(&root, "set_settings", json!({"sidebar_open": false}));
    let settings = finish_ai_call(&mut harness, call);
    scenario.fact("ai-control.set-settings", json!(settings));
    let call = start_ai_call(
        &root,
        "edit_board",
        json!({"edits": [
            {"action": "add", "type": "label", "text": "Warm palette", "x": 0, "y": 0, "size": "l", "ref": "title"},
            {"action": "add", "type": "rectangle", "x": 0, "y": 0, "width": 220, "height": 140, "color": "gold", "fill": "tint", "ref": "warm"},
            {"action": "add", "type": "rectangle", "x": 0, "y": 0, "width": 220, "height": 140, "color": "sky", "fill": "tint", "ref": "cool"},
            {"action": "group", "ids": ["$title", "$warm", "$cool"], "name": "Palette", "flow": "column", "ref": "palette"},
            {"action": "add", "type": "arrow", "x1": 420, "y1": 40, "x2": 260, "y2": 120, "color": "coral"},
        ]}),
    );
    let edited = finish_ai_call(&mut harness, call);
    scenario.fact("ai-control.edit-board", json!(edited));
    let call = start_ai_call(&root, "set_view", json!({"fit": true}));
    let view = finish_ai_call(&mut harness, call);
    harness.run_steps(4);
    scenario.capture_screen("screen-002-ai-edited-board", &mut harness);
    scenario.assertion(
        "one edit_board call adds four items and a column Group",
        harness.state().canvas.annotations().len() == 4
            && harness.state().canvas.groups().len() == 1
            && view.is_ok()
            && !harness.state().sidebar_open,
    );

    let call = start_ai_call(&root, "undo", json!({}));
    let undone = finish_ai_call(&mut harness, call);
    let empty = harness.state().canvas.annotations().is_empty();
    let call = start_ai_call(&root, "redo", json!({}));
    let redone = finish_ai_call(&mut harness, call);
    scenario.assertion(
        "undo takes the whole call back and redo restores it",
        undone.is_ok() && empty && redone.is_ok() && harness.state().canvas.groups().len() == 1,
    );

    let call = start_ai_call(&root, "remove_from_library", json!({"assets": [asset]}));
    for _ in 0..200 {
        if harness.state().pending_library_remove.is_some() {
            break;
        }
        harness.step();
        std::thread::sleep(Duration::from_millis(2));
    }
    harness.run_steps(4);
    scenario.capture_screen("screen-003-user-confirms", &mut harness);
    scenario.assertion(
        "under Ask a library removal waits on the app's own dialog",
        harness.state().pending_library_remove.is_some()
            && !call.is_finished()
            && library.load_assets(&[asset]).unwrap().len() == 1,
    );
    let remove = harness.get_by_label("Remove").rect();
    scenario.action("confirm the removal", remove.center());
    inspect_click_at(&harness, remove.center(), egui::Modifiers::NONE);
    let removed = finish_ai_call(&mut harness, call);
    scenario.fact("ai-control.remove-from-library", json!(removed));
    scenario.assertion(
        "the call answers once the user confirms, and the asset is gone",
        removed == Ok(json!({"removed": [asset]}))
            && library.load_assets(&[asset]).unwrap().is_empty(),
    );

    harness.state_mut().sidebar_open = true;
    harness.run_steps(4);
    let off = harness.get_by_label("Off").rect();
    scenario.action("choose Off under AI control", off.center());
    inspect_click_at(&harness, off.center(), egui::Modifiers::NONE);
    harness.run_steps(4);
    scenario.capture_screen("screen-004-access-off-again", &mut harness);
    let call = start_ai_call(&root, "get_board", json!({}));
    scenario.assertion(
        "choosing Off withdraws the endpoint and refuses calls",
        !endpoint.exists() && finish_ai_call(&mut harness, call).is_err(),
    );

    let report = scenario.finish();
    assert!(
        report.passed,
        "inspect scenario failed; artifact={}",
        report.artifact_dir.display()
    );
}
