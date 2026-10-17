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
    assert_eq!(edited["refs"], json!({"title":2,"frame":3,"faces":4}));
    assert_eq!(edited["items"][0]["text"], "Heads");
    assert_eq!(
        edited["items"][1],
        json!({"id":3,"type":"rectangle","group":4,"x":0.0,"y":31.9,"width":200.0,"height":100.0,"color":"default","fill":"tint","stroke":"m"})
    );
    assert_eq!(
        edited["items"][2],
        json!({"id":5,"type":"arrow","x1":300.0,"y1":0.0,"x2":350.0,"y2":50.0,"color":"default","stroke":"m"})
    );
    let before_delete = call(&mut app, &ctx, "get_board", json!({})).unwrap();
    app.flush_persistence();
    let library = app.library.clone().unwrap();
    let persisted = library.load_board_document(app.active_board_id).unwrap();
    assert_eq!(
        persisted
            .annotations
            .iter()
            .map(|a| a.id)
            .collect::<Vec<_>>(),
        vec![2, 3, 5]
    );
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
    call(&mut app, &ctx, "redo", json!({})).unwrap();
    let redone = call(&mut app, &ctx, "get_board", json!({})).unwrap();
    assert_eq!(redone["items"], before_delete["items"]);
    assert_eq!(redone["groups"], before_delete["groups"]);
    assert_eq!(redone["selection"], json!([2]));
    app.flush_persistence();
    assert_eq!(
        library
            .load_board_document(app.active_board_id)
            .unwrap()
            .annotations,
        persisted.annotations
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
    assert!(
        call.is_finished(),
        "AI call did not answer within 2,000 frames"
    );
    let result = call.join().unwrap().ok_or("no connection")?;
    let text = result["content"][0]["text"].as_str().unwrap_or_default();
    if result["isError"] == true {
        return Err(text.to_owned());
    }
    serde_json::from_str(text).map_err(|error| error.to_string())
}

#[test]
fn add_images_answers_from_its_board_after_the_user_opens_another() {
    let root = TempRoot::new("snappyref-control-import-board-switch");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let source = library
        .store_rgba_image("ai-import", &solid_rgba([40, 120, 200, 255], 8, 8), 8, 8)
        .unwrap();
    let first = library.list_boards().unwrap().remove(0);
    let second = library.fixture_board("Second").unwrap();
    let mut app = test_app(Some(library.clone()));
    app.board_lifecycle().restore().unwrap();
    app.board_editing.persistence = crate::app::DocumentPersistence::queued(library.clone());
    app.settings.ai_control = AiControlAccess::Full;
    let library_root = root.to_path_buf();
    let mut harness = app_full_harness_with_startup(1.0 / 4.0, move |ctx| {
        app.import_worker = Some(crate::image_preparation::ImagePrepWorker::new(
            library,
            ctx.clone(),
            None,
        ));
        app.start_control(ctx, library_root);
        app
    });

    let call = start_ai_call(&root, "add_images", json!({"sources": [source]}));
    // The frame that takes the call reserves the image, before its worker
    // can have decoded it.
    for _ in 0..2000 {
        if !harness.state().canvas.all_placement_ids().is_empty() {
            break;
        }
        harness.step();
        std::thread::sleep(Duration::from_millis(2));
    }
    let ids = harness.state().canvas.all_placement_ids();
    assert_eq!(
        ids.len(),
        1,
        "AI call must reserve its placement within 2,000 frames"
    );
    let placement_id = ids[0];
    harness.state_mut().switch_to_board(second.id);

    let added = finish_ai_call(&mut harness, call).unwrap();
    assert_eq!(added["items"][0]["id"], placement_id);
    assert_eq!(added["items"][0]["type"], "image");
    let library = harness.state().library.clone().unwrap();
    harness.state_mut().flush_persistence();
    let stored = library.load_placements_for_board(first.id).unwrap();
    assert_eq!(
        stored.iter().map(|p| p.id()).collect::<Vec<_>>(),
        vec![placement_id]
    );
    assert_eq!(
        added["items"][0]["asset"],
        stored[0].require_loaded_asset().id
    );
    assert_eq!(added["items"][0]["width"], 8.0);
    assert_eq!(added["items"][0]["height"], 8.0);
    assert!(
        library
            .load_placements_for_board(second.id)
            .unwrap()
            .is_empty()
    );
    assert_eq!(added["items"][0].get("status"), None);
    assert_eq!(
        added["note"],
        format!(
            "The open board changed. These images are on board {}.",
            first.id
        )
    );
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

    harness.state_mut().flush_persistence();
    let reopened = Library::initialize(root.to_path_buf()).unwrap();
    let document = reopened
        .load_board_document(harness.state().active_board_id)
        .unwrap();
    scenario.fact(
        "ai-control.durable-document",
        json!({"annotations":document.annotations.iter().map(|a| (a.id,format!("{:?}",a.kind),&a.text)).collect::<Vec<_>>(), "groups":document.groups.iter().map(|g| (g.id,&g.name,&g.members)).collect::<Vec<_>>()}),
    );
    scenario.assertion(
        "AI edit survives a flush and independent library readback",
        document
            .annotations
            .iter()
            .map(|a| (a.id, a.kind, a.text.as_str()))
            .collect::<Vec<_>>()
            == vec![
                (2, CanvasAnnotationKind::Textbox, "Warm palette"),
                (3, CanvasAnnotationKind::Rectangle, ""),
                (4, CanvasAnnotationKind::Rectangle, ""),
                (6, CanvasAnnotationKind::Arrow, ""),
            ]
            && document
                .groups
                .iter()
                .map(|g| (g.id, g.name.as_str(), g.members.clone()))
                .collect::<Vec<_>>()
                == vec![(5, "Palette", vec![2, 3, 4])],
    );

    scenario.assertion(
        "AI arrow endpoints survive durable readback",
        document
            .annotations
            .iter()
            .find(|a| a.id == 6)
            .is_some_and(|a| {
                a.start() == egui::pos2(420.0, 40.0) && a.end() == egui::pos2(260.0, 120.0)
            }),
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

    scenario.finish();
}

#[test]
fn board_tool_calls_keep_identity_and_restore_deleted_content() {
    let root = TempRoot::new("snappyref-control-board-lifecycle");
    let (mut app, ctx) = control_app(&root);
    let library = app.library.clone().unwrap();
    assert_eq!(
        call(&mut app, &ctx, "create_board", json!({"name":" Faces "})).unwrap(),
        json!({"board":{"id":2,"name":"Faces"}})
    );
    assert_eq!(
        call(
            &mut app,
            &ctx,
            "rename_board",
            json!({"board":2,"name":" Heads "})
        )
        .unwrap(),
        json!({"board":{"id":2,"name":"Heads"}})
    );
    call(
        &mut app,
        &ctx,
        "edit_board",
        json!({"edits":[{"action":"add","type":"rectangle","x":10,"y":20,"width":80,"height":40}]}),
    )
    .unwrap();
    app.flush_persistence();
    let before = library.load_board_document(2).unwrap();
    assert_eq!(before.board.name, "Heads");
    assert_eq!(before.annotations[0].id, 3);
    assert_eq!(
        call(&mut app, &ctx, "open_board", json!({"board":1})).unwrap(),
        json!({"board":{"id":1,"name":"My Board"}})
    );
    assert_eq!(
        call(&mut app, &ctx, "open_board", json!({"board":2})).unwrap(),
        json!({"board":{"id":2,"name":"Heads"}})
    );
    assert_eq!(
        call(&mut app, &ctx, "delete_board", json!({"board":2})).unwrap(),
        json!({"deleted":2,"open":1})
    );
    app.flush_persistence();
    assert_eq!(
        library
            .list_boards()
            .unwrap()
            .iter()
            .map(|b| b.id)
            .collect::<Vec<_>>(),
        vec![1]
    );
    let undone = call(&mut app, &ctx, "undo", json!({})).unwrap();
    assert_eq!(undone["open"], 2);
    app.flush_persistence();
    let restored = library.load_board_document(2).unwrap();
    assert_eq!(restored.board.name, "Heads");
    assert_eq!(restored.annotations, before.annotations);
    assert_eq!(restored.stack_order, before.stack_order);
}

#[test]
fn settings_tool_clamps_persists_and_refuses_unknown_fields_atomically() {
    let root = TempRoot::new("snappyref-control-settings");
    let (mut app, ctx) = control_app(&root);
    let settings = call(&mut app,&ctx,"set_settings",json!({"placement_gap":999,"rotation_snap":-10,"pointer":"trackpad","always_on_top":true,"sidebar_open":true})).unwrap();
    assert_eq!(settings["placement_gap"], 128.0);
    assert_eq!(settings["rotation_snap"], 0.0);
    assert_eq!(settings["pointer"], "trackpad");
    assert_eq!(settings["always_on_top"], true);
    assert_eq!(settings["sidebar_open"], true);
    app.flush_persistence();
    let library = app.library.clone().unwrap();
    let mut reopened = test_app(Some(library.clone()));
    reopened.apply_user_session(library.load_user_session());
    assert_eq!(
        call(&mut reopened, &ctx, "get_settings", json!({})).unwrap(),
        settings
    );
    let error = call(
        &mut app,
        &ctx,
        "set_settings",
        json!({"placement_gap":3,"unsupported":true}),
    )
    .unwrap_err();
    assert_eq!(
        error,
        "unknown field `unsupported`, expected one of `placement_gap`, `rotation_snap`, `pointer`, `always_on_top`, `sidebar_open`."
    );
    assert_eq!(
        call(&mut app, &ctx, "get_settings", json!({})).unwrap(),
        settings
    );
    app.flush_persistence();
    assert_eq!(
        library.load_user_session().unwrap().settings.placement_gap,
        128.0
    );
}

#[test]
fn view_tool_uses_visible_canvas_and_selection_refusal_keeps_selection() {
    let root = TempRoot::new("snappyref-control-view");
    let (mut app, ctx) = control_app(&root);
    crate::ui::test_frame::run_ui(
        &ctx,
        egui::RawInput {
            screen_rect: Some(test_screen_rect()),
            ..Default::default()
        },
        |_| {},
    );
    for (open, width) in [(false, 1200.0), (true, 908.0)] {
        app.sidebar_open = open;
        let view = call(
            &mut app,
            &ctx,
            "set_view",
            json!({"x":-40,"y":20,"zoom":2,"background":"grid"}),
        )
        .unwrap();
        assert_eq!(
            view,
            json!({"view":{"x":-40.0,"y":20.0,"width":width/2.0,"height":400.0,"zoom":2.0},"background":"grid"})
        );
    }
    app.sidebar_open = false;
    let added=call(&mut app,&ctx,"edit_board",json!({"edits":[{"action":"add","type":"rectangle","x":10,"y":20,"width":200,"height":100,"ref":"box"}]})).unwrap();
    assert_eq!(added["refs"]["box"], 2);
    assert_eq!(
        call(&mut app, &ctx, "select", json!({"ids":[2]})).unwrap(),
        json!({"selection":[2]})
    );
    assert_eq!(
        call(&mut app, &ctx, "select", json!({"ids":[2,999]})).unwrap_err(),
        "No item or group has id 999."
    );
    assert_eq!(
        call(&mut app, &ctx, "get_board", json!({})).unwrap()["selection"],
        json!([2])
    );
    let fitted = call(&mut app, &ctx, "set_view", json!({"fit":[2]})).unwrap();
    assert_eq!(
        fitted,
        json!({"background":"grid","view":{"x":1.3,"y":-2.5,"width":217.4,"height":144.9,"zoom":5.52}})
    );
}

#[test]
fn image_edit_sizing_transfer_arrangement_and_order_have_concrete_effects() {
    let root = TempRoot::new("snappyref-control-edit-variants");
    let (mut app, ctx) = control_app(&root);
    let library = app.library.clone().unwrap();
    let path = library
        .store_rgba_image("wide", &solid_rgba([1, 2, 3, 255], 80, 40), 80, 40)
        .unwrap();
    let asset = library
        .register_original(&path, "image/png", 80, 40, 12800, None)
        .unwrap()
        .id;
    app.import_worker = Some(crate::image_preparation::ImagePrepWorker::controlled().0);
    let made = call(
        &mut app,
        &ctx,
        "edit_board",
        json!({"edits":[
            {"action":"add","type":"image","asset":asset,"x":10,"y":20,"width":200,"ref":"image"},
            {"action":"add","type":"image","asset":asset,"x":400,"y":200,"width":40,"ref":"box"},
            {"action":"group","ids":["$image"],"flow":"free","name":"First","ref":"first"},
            {"action":"group","ids":["$box"],"flow":"free","name":"Second","ref":"second"}
        ]}),
    )
    .unwrap();
    assert_eq!(
        made["refs"],
        json!({"image":2,"box":3,"first":4,"second":5})
    );
    assert_eq!(made["items"][0]["width"], 200.0);
    assert_eq!(made["items"][0]["height"], 100.0);
    let transfer = call(
        &mut app,
        &ctx,
        "edit_board",
        json!({"edits":[{"action":"add_to_group","ids":[2],"group":5}]}),
    )
    .unwrap();
    assert_eq!(transfer["groups"][0]["members"], json!([]));
    assert_eq!(transfer["groups"][1]["members"], json!([3, 2]));
    let grouped = call(&mut app, &ctx, "get_board", json!({})).unwrap();
    assert_eq!(grouped["groups"][0]["members"], json!([]));
    assert_eq!(grouped["groups"][1]["members"], json!([3, 2]));
    let changed = call(
        &mut app,
        &ctx,
        "edit_board",
        json!({"edits":[
            {"action":"set","id":2,"height":60,"x":30,"y":50},
            {"action":"ungroup","ids":[5]},
            {"action":"arrange","ids":[2,3],"layout":"row","gap":10},
            {"action":"order","ids":[2],"to":"front"}
        ]}),
    )
    .unwrap();
    assert_eq!(changed["items"][0]["x"], 170.0);
    assert_eq!(changed["items"][0]["y"], 115.0);
    assert_eq!(changed["items"][1]["x"], 300.0);
    assert_eq!(changed["items"][1]["y"], 155.0);
    assert_eq!(changed["deleted"], json!([5]));
    let board = call(&mut app, &ctx, "get_board", json!({})).unwrap();
    assert_eq!(board["items"][1]["id"], 2);
    assert_eq!(board["items"][1]["width"], 120.0);
    assert_eq!(board["items"][1]["height"], 60.0);
    let error=call(&mut app,&ctx,"edit_board",json!({"edits":[{"action":"set","id":2,"width":70},{"action":"set","id":"$missing","x":0}]})).unwrap_err();
    assert!(error.contains("$missing"), "{error}");
    assert!(error.ends_with("Nothing was applied."), "{error}");
    assert_eq!(call(&mut app, &ctx, "get_board", json!({})).unwrap(), board);
    app.flush_persistence();
    let stored = library.load_board_document(1).unwrap();
    assert_eq!(
        stored.stack_order,
        vec![
            crate::data::CanvasStackItem::Placement(3),
            crate::data::CanvasStackItem::Placement(2)
        ]
    );
    let wide = stored.placements.iter().find(|p| p.id() == 2).unwrap();
    assert_eq!(wide.asset_id(), Some(asset));
    assert_eq!(wide.state.geometry.scaled_size(), egui::vec2(120.0, 60.0));
}
