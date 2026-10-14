use super::*;
use crate::library::storage::Library;
use crate::test_support::TempRoot;
use std::thread::JoinHandle;

fn app(root: &TempRoot) -> (SnappyRefApp, egui::Context) {
    let ctx = egui::Context::default();
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let mut app = SnappyRefApp::detached(None, 8192, crate::updater::UpdateConfig::for_tests());
    app.board_editing.persistence =
        crate::active_board::persistence::DocumentPersistence::queued(library.clone());
    app.library = Some(library);
    app.refresh_board_summaries();
    app.settings.ai_control = AiControlAccess::Full;
    app.start_control(&ctx, root.to_path_buf());
    (app, ctx)
}

fn pending(
    app: &mut SnappyRefApp,
    ctx: &egui::Context,
    root: &TempRoot,
    tool: &str,
    args: Value,
) -> (Waiting, JoinHandle<Option<Value>>) {
    let directory = root.to_path_buf();
    let call = json!({"tool":tool,"arguments":args});
    let client = std::thread::spawn(move || crate::control::call_running_app(&directory, &call));
    let deadline = Instant::now() + Duration::from_secs(5);
    let request = loop {
        if let Some(request) = app.control.server.as_ref().unwrap().try_recv() {
            break request;
        }
        assert!(Instant::now() < deadline, "control request did not arrive");
        std::thread::sleep(Duration::from_millis(2));
    };
    let Step::Wait(timeout, on) = app
        .control_call(ctx, &request.tool, request.arguments)
        .unwrap()
    else {
        panic!("expected pending call")
    };
    (
        Waiting {
            responder: request.responder,
            deadline: Instant::now() + timeout,
            on,
        },
        client,
    )
}

fn reply(client: JoinHandle<Option<Value>>) -> Value {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !client.is_finished() {
        assert!(Instant::now() < deadline, "control response did not arrive");
        std::thread::sleep(Duration::from_millis(2));
    }
    client.join().unwrap().unwrap()
}

fn search_request(waiting: &Waiting) -> u64 {
    match waiting.on {
        WaitingOn::Search { request_id, .. } => request_id,
        _ => unreachable!(),
    }
}

fn take_search_result(app: &mut SnappyRefApp, request: u64) -> SearchQueryResult {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        app.search.handle_results();
        if let Some(result) = app.search.take_direct_search_result(request) {
            return result;
        }
        assert!(Instant::now() < deadline, "search result did not arrive");
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn assert_unindexed_result(result: SearchQueryResult, expected_request: u64) {
    let SearchQueryResult::Failed { request_id, error } = result else {
        panic!("unindexed pivot must fail")
    };
    assert_eq!(request_id, expected_request);
    assert_eq!(error, "Source image not indexed yet");
}

#[test]
fn search_calls_complete_fail_retry_model_pending_and_expire() {
    let root = TempRoot::new("snappyref-control-search-results");
    let (mut app, ctx) = app(&root);
    app.search
        .set_local_model_store_status_for_test(LocalModelStoreStatus::Installed);
    app.search
        .sync_service(app.library.as_ref(), &mut app.status, 8192);
    let library = app.library.clone().unwrap();
    let path = library
        .store_rgba_image("blue", &[0, 0, 255, 255], 1, 1)
        .unwrap();
    let asset = library
        .register_original(&path, "image/png", 1, 1, 4, None)
        .unwrap();
    let path = library
        .store_rgba_image("green", &[0, 255, 0, 255], 1, 1)
        .unwrap();
    let pivot = library
        .register_original(&path, "image/png", 1, 1, 4, None)
        .unwrap()
        .id;
    // An unindexed image pivot never loads a real model. Controlled service
    // results below exercise the app's continuation, independently of ranking.
    let args = json!({"similar_to":pivot,"limit":1});
    let (mut waiting, client) = pending(&mut app, &ctx, &root, "search_library", args.clone());
    let request = search_request(&waiting);
    app.search.forget_direct_search(request); // The service delivery consumes its entry.
    let result = app
        .poll_control_search(
            &mut waiting,
            Some(SearchQueryResult::Completed {
                request_id: request,
                results: vec![crate::search::semantic::SearchResultAssetEntry {
                    asset_id: asset.id,
                    asset_preview_path: None,
                    asset_width: 1,
                    asset_height: 1,
                }],
            }),
            false,
        )
        .unwrap();
    waiting.responder.send(ControlReply::from_result(result));
    let answered = reply(client);
    let value: Value =
        serde_json::from_str(answered["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(
        value,
        json!({"assets":[{"asset":asset.id,"width":1,"height":1,"file":asset.filename}]})
    );
    let (mut waiting, client) = pending(&mut app, &ctx, &root, "search_library", args.clone());
    let request = search_request(&waiting);
    app.search.forget_direct_search(request);
    let result = app
        .poll_control_search(
            &mut waiting,
            Some(SearchQueryResult::Failed {
                request_id: request,
                error: "fixture search failed".to_owned(),
            }),
            false,
        )
        .unwrap();
    waiting.responder.send(ControlReply::from_result(result));
    assert_eq!(
        reply(client),
        json!({"content":[{"type":"text","text":"fixture search failed"}],"isError":true})
    );
    let (mut waiting, client) = pending(&mut app, &ctx, &root, "search_library", args);
    let previous = search_request(&waiting);
    app.search.forget_direct_search(previous);
    assert!(
        app.poll_control_search(
            &mut waiting,
            Some(SearchQueryResult::ModelPending {
                request_id: previous
            }),
            false
        )
        .is_none()
    );
    let renewed = search_request(&waiting);
    let result = take_search_result(&mut app, renewed);
    assert_unindexed_result(result, renewed);
    assert!(!client.is_finished());
    assert!(
        app.poll_control_search(
            &mut waiting,
            Some(SearchQueryResult::ModelPending {
                request_id: renewed
            }),
            false
        )
        .is_none()
    );
    let expired = search_request(&waiting);
    app.control.waiting.push(waiting);
    app.drive_control_at(&ctx, Instant::now() + Duration::from_secs(31));
    assert_eq!(
        reply(client),
        json!({"content":[{"type":"text","text":"The search model is still loading. Try again in a moment."}],"isError":true})
    );
    assert!(app.control.waiting.is_empty());
    // The search worker answers in request order. Receiving this later request
    // proves the expired result has arrived before checking that it was forgotten.
    let barrier = app
        .search
        .start_direct_search(None, Some(pivot), 1)
        .unwrap();
    let result = take_search_result(&mut app, barrier);
    assert_unindexed_result(result, barrier);
    assert!(
        app.search.take_direct_search_result(expired).is_none(),
        "expired search is still tracked after its late result arrived"
    );
}

#[test]
fn cancelling_ask_preserves_the_asset_and_revoking_access_answers_waiting_calls() {
    let root = TempRoot::new("snappyref-control-cancel-and-revoke");
    let (mut app, ctx) = app(&root);
    let library = app.library.clone().unwrap();
    let path = library
        .store_rgba_image("keep", &[0, 255, 0, 255], 1, 1)
        .unwrap();
    let asset = library
        .register_original(&path, "image/png", 1, 1, 4, None)
        .unwrap();
    app.settings.ai_control = AiControlAccess::Ask;
    let (waiting, client) = pending(
        &mut app,
        &ctx,
        &root,
        "remove_from_library",
        json!({"assets":[asset.id]}),
    );
    assert_eq!(
        app.pending_library_remove.as_ref().unwrap().asset_ids,
        vec![asset.id]
    );
    crate::ui::test_frame::run_ui(
        &ctx,
        egui::RawInput {
            events: vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
            ..Default::default()
        },
        |ui| app.show_library_remove_confirmation(ui.ctx()),
    );
    assert!(app.pending_library_remove.is_none());
    assert!(app.poll_waiting(waiting, Instant::now()).is_none());
    assert_eq!(
        reply(client),
        json!({"content":[{"type":"text","text":"The user declined in SnappyRef."}],"isError":true})
    );
    assert_eq!(
        library.load_asset(asset.id).unwrap().filename,
        asset.filename
    );
    let (waiting, client) = pending(&mut app, &ctx, &root, "screenshot", json!({}));
    app.control.waiting.push(waiting);
    app.search
        .set_local_model_store_status_for_test(LocalModelStoreStatus::Installed);
    app.search
        .sync_service(app.library.as_ref(), &mut app.status, 8192);
    let (search_waiting, search_client) = pending(
        &mut app,
        &ctx,
        &root,
        "search_library",
        json!({"similar_to":asset.id}),
    );
    let revoked_search = search_request(&search_waiting);
    app.control.waiting.push(search_waiting);
    app.settings.ai_control = AiControlAccess::Off;
    app.sync_control_server(&ctx);
    let refused = reply(client);
    assert_eq!(
        refused,
        json!({"content":[{"type":"text","text":"SnappyRef dropped the request."}],"isError":true})
    );
    assert_eq!(
        reply(search_client),
        json!({"content":[{"type":"text","text":"SnappyRef dropped the request."}],"isError":true})
    );
    let barrier = app
        .search
        .start_direct_search(None, Some(asset.id), 1)
        .unwrap();
    let result = take_search_result(&mut app, barrier);
    assert_unindexed_result(result, barrier);
    assert!(
        app.search
            .take_direct_search_result(revoked_search)
            .is_none(),
        "revoked search is still tracked after its late result arrived"
    );
    assert!(app.control.waiting.is_empty());
    assert!(!root.join("control.json").exists());
    assert_eq!(
        library.load_asset(asset.id).unwrap().filename,
        asset.filename
    );
}

#[test]
fn screenshot_events_answer_matching_calls_out_of_order_with_decodable_pngs() {
    let root = TempRoot::new("snappyref-control-screenshot-order");
    let (mut app, ctx) = app(&root);
    let (first, one) = pending(&mut app, &ctx, &root, "screenshot", json!({}));
    let first_data = screenshot_command_data(&ctx);
    let (second, two) = pending(&mut app, &ctx, &root, "screenshot", json!({}));
    let second_data = screenshot_command_data(&ctx);
    app.control.waiting.extend([first, second]);
    for (user_data, size, expected, client) in [
        (second_data, [2000, 1000], (1568, 784), two),
        (first_data, [80, 40], (80, 40), one),
    ] {
        let mut input = egui::RawInput {
            events: vec![egui::Event::Screenshot {
                viewport_id: egui::ViewportId::ROOT,
                user_data,
                image: std::sync::Arc::new(egui::ColorImage::filled(size, egui::Color32::BLUE)),
            }],
            ..Default::default()
        };
        app.answer_screenshots(&mut input);
        assert!(input.events.is_empty());
        let result = reply(client);
        use base64::Engine as _;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(result["content"][1]["data"].as_str().unwrap())
            .unwrap();
        let image = image::load_from_memory(&bytes).unwrap();
        assert_eq!((image.width(), image.height()), expected);
        let dimensions: Value =
            serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(dimensions, json!({"width":expected.0,"height":expected.1}));
        assert_eq!(image.to_rgba8().get_pixel(0, 0).0, [0, 0, 255, 255]);
    }
    assert!(app.control.waiting.is_empty());
}

fn screenshot_command_data(ctx: &egui::Context) -> egui::UserData {
    let output = crate::ui::test_frame::run_ui(ctx, Default::default(), |_| {});
    output.viewport_output[&egui::ViewportId::ROOT]
        .commands
        .iter()
        .find_map(|command| match command {
            egui::ViewportCommand::Screenshot(data) => Some(data.clone()),
            _ => None,
        })
        .expect("screenshot command was emitted")
}
