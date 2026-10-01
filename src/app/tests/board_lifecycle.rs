use super::*;

#[test]
fn save_user_session_round_trips_sidebar_state() {
    let root = TempRoot::new("snappyref-app-sidebar-session");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let mut app = test_app(Some(library.clone()));
    app.sidebar_open = true;
    app.sidebar_tab = SidebarTab::Boards;

    app.save_user_session();
    app.flush_persistence();

    let mut reopened = test_app(Some(library));
    reopened.load_user_session();
    assert!(reopened.sidebar_open);
    assert_eq!(reopened.sidebar_tab, SidebarTab::Boards);
}

#[test]
fn board_summaries_are_served_from_cache_until_refreshed() {
    let root = TempRoot::new("snappyref-board-summary-cache");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let mut app = test_app(Some(library.clone()));

    app.refresh_board_summaries();
    assert_eq!(app.board_summaries().len(), 1);

    library.fixture_board("Second").unwrap();
    assert_eq!(
        app.board_summaries().len(),
        1,
        "rendering should use cached Board Summaries, not query storage every frame"
    );

    app.refresh_board_summaries();
    assert_eq!(app.board_summaries().len(), 2);
}

#[test]
fn persist_canvas_changes_saves_viewport_changes() {
    let root = TempRoot::new("snappyref-app-viewport-session");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let mut app = test_app(Some(library.clone()));
    app.canvas
        .apply_viewport_state(&crate::data::ViewportState {
            camera_pan_x: 10.0,
            camera_pan_y: 20.0,
            camera_zoom: 1.75,
            canvas_background: crate::data::CanvasBackground::Dots,
        });

    app.persist_canvas_changes();

    app.flush_persistence();
    let session = library.load_user_session().unwrap();
    assert_eq!(session.viewport.camera_pan_x, 10.0);
    assert_eq!(session.viewport.camera_pan_y, 20.0);
    assert_eq!(session.viewport.camera_zoom, 1.75);
    assert_eq!(
        session.viewport.canvas_background,
        crate::data::CanvasBackground::Dots
    );
}

#[test]
fn debounced_canvas_persistence_defers_session_only_viewport_changes() {
    let ctx = egui::Context::default();
    let root = TempRoot::new("snappyref-app-viewport-session-debounce");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let mut app = test_app(Some(library.clone()));
    app.canvas
        .apply_viewport_state(&crate::data::ViewportState {
            camera_pan_x: 10.0,
            camera_pan_y: 20.0,
            camera_zoom: 1.75,
            canvas_background: crate::data::CanvasBackground::Dots,
        });

    app.persist_canvas_changes_debounced(&ctx);

    app.flush_persistence();
    let session = library.load_user_session().unwrap();
    assert_ne!(session.viewport.camera_pan_x, 10.0);
    assert!(app.pending_user_session.is_some());

    app.pending_user_session_save_at = Some(Instant::now() - Duration::from_millis(1));
    app.persist_canvas_changes_debounced(&ctx);

    app.flush_persistence();
    let session = library.load_user_session().unwrap();
    assert_eq!(session.viewport.camera_pan_x, 10.0);
    assert_eq!(session.viewport.camera_pan_y, 20.0);
    assert_eq!(session.viewport.camera_zoom, 1.75);
    assert!(app.pending_user_session.is_none());
}

#[test]
fn cross_board_paste_creates_new_images_and_annotations_on_active_board() {
    let ctx = egui::Context::default();
    let root = TempRoot::new("snappyref-cross-board-paste");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let first = library.list_boards().unwrap()[0].clone();
    let second = library.fixture_board("Second").unwrap();
    let image_path = library
        .store_rgba_image("source", &[255, 0, 0, 255], 1, 1)
        .unwrap();
    let source_item = library
        .fixture_placement(
            library.load_user_session().unwrap().active_board_id,
            crate::data::Placement::loaded(
                crate::data::PlacementState::new(
                    library.allocate_document_id(),
                    egui::pos2(10.0, 20.0),
                    1.0,
                    0.0,
                    0,
                    false,
                ),
                library
                    .register_original(&image_path, "image/png", 1, 1, 4, None)
                    .unwrap(),
            ),
        )
        .unwrap();
    let source_annotation = library
        .fixture_annotation(
            CanvasAnnotationKind::Textbox,
            egui::pos2(40.0, 60.0),
            egui::pos2(140.0, 100.0),
            "note",
            CanvasTextSize::M,
            CanvasStrokeWidth::M,
            crate::data::CanvasAnnotationColor::Sky,
            true,
        )
        .unwrap();
    let mut app = test_app(Some(library.clone()));
    app.canvas.add_prepared_placement_image(
        &ctx,
        PreparedPlacementImage {
            target_board_id: first.id,
            ..loaded_record(source_item.clone())
        },
    );
    app.canvas.add_annotation(source_annotation.clone());
    let source_stack_order = vec![
        CanvasStackItem::Annotation(source_annotation.id),
        CanvasStackItem::Placement(source_item.state.id),
    ];
    app.canvas
        .set_canvas_stack_order(source_stack_order.clone());
    library
        .fixture_board_document(
            library.load_user_session().unwrap().active_board_id,
            crate::canvas::CanvasDocumentChanges {
                stack_order: Some(source_stack_order.to_vec()),
                ..Default::default()
            },
        )
        .unwrap();
    app.canvas
        .select_content(&[source_item.state.id], &[source_annotation.id]);

    let payload = app.copy_selected_board_payload().unwrap();
    library.set_active_board(second.id).unwrap();
    app.active_board_id = second.id;
    app.refresh_board_summaries();
    app.paste_board_payload_at(&payload, egui::pos2(300.0, 300.0))
        .unwrap();

    app.flush_persistence();
    let pasted_items = library.load_placements_for_board(second.id).unwrap();
    let pasted_annotations = library.load_canvas_annotations().unwrap();
    assert_eq!(pasted_items.len(), 1);
    assert_eq!(pasted_annotations.len(), 1);
    assert_ne!(pasted_items[0].state.id, source_item.state.id);
    assert_ne!(pasted_annotations[0].id, source_annotation.id);
    assert_eq!(
        library.load_placements_for_board(first.id).unwrap().len(),
        1
    );

    let pasted_delta = egui::vec2(
        pasted_annotations[0].start().x - pasted_items[0].state.geometry.center.x,
        pasted_annotations[0].start().y - pasted_items[0].state.geometry.center.y,
    );
    let source_delta = egui::vec2(
        source_annotation.start().x - source_item.state.geometry.center.x,
        source_annotation.start().y - source_item.state.geometry.center.y,
    );
    assert_eq!(pasted_delta, source_delta);
    assert_eq!(
        app.canvas.selected_placement_ids(),
        vec![pasted_items[0].state.id]
    );
    assert_eq!(
        app.canvas.selected_annotation_ids(),
        vec![pasted_annotations[0].id]
    );
    let pasted_stack_order = vec![
        CanvasStackItem::Annotation(pasted_annotations[0].id),
        CanvasStackItem::Placement(pasted_items[0].state.id),
    ];
    app.flush_persistence();
    assert_eq!(
        library.load_canvas_stack_order().unwrap(),
        pasted_stack_order
    );
    library.set_active_board(first.id).unwrap();
    assert_eq!(
        library.load_canvas_stack_order().unwrap(),
        source_stack_order
    );
}

#[test]
fn duplicate_offsets_mixed_selection_on_same_board_and_selects_new_content() {
    let ctx = egui::Context::default();
    let root = TempRoot::new("snappyref-mixed-duplicate");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let image_path = library
        .store_rgba_image("source", &[255, 0, 0, 255], 1, 1)
        .unwrap();
    let source_item = library
        .fixture_placement(
            library.load_user_session().unwrap().active_board_id,
            crate::data::Placement::loaded(
                crate::data::PlacementState::new(
                    library.allocate_document_id(),
                    egui::pos2(10.0, 20.0),
                    1.0,
                    0.0,
                    0,
                    false,
                ),
                library
                    .register_original(&image_path, "image/png", 1, 1, 4, None)
                    .unwrap(),
            ),
        )
        .unwrap();
    let source_annotation = library
        .fixture_annotation(
            CanvasAnnotationKind::Line,
            egui::pos2(40.0, 60.0),
            egui::pos2(80.0, 90.0),
            "",
            CanvasTextSize::M,
            CanvasStrokeWidth::M,
            crate::data::CanvasAnnotationColor::Gold,
            false,
        )
        .unwrap();
    let mut app = test_app(Some(library.clone()));
    app.settings.placement_gap = 24.0;
    app.canvas
        .add_prepared_placement_image(&ctx, loaded_record(source_item.clone()));
    app.canvas.add_annotation(source_annotation.clone());
    app.canvas.set_canvas_stack_order(vec![
        CanvasStackItem::Annotation(source_annotation.id),
        CanvasStackItem::Placement(source_item.state.id),
    ]);
    app.canvas
        .select_content(&[source_item.state.id], &[source_annotation.id]);

    app.duplicate_selection_with_gap();

    app.flush_persistence();
    let items = library.load_placements().unwrap();
    let annotations = library.load_canvas_annotations().unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(annotations.len(), 2);
    let duplicated_item = items
        .iter()
        .find(|item| item.state.id != source_item.state.id)
        .unwrap();
    let duplicated_annotation = annotations
        .iter()
        .find(|annotation| annotation.id != source_annotation.id)
        .unwrap();
    let item_offset = egui::vec2(
        duplicated_item.state.geometry.center.x - source_item.state.geometry.center.x,
        duplicated_item.state.geometry.center.y - source_item.state.geometry.center.y,
    );
    let annotation_offset = egui::vec2(
        duplicated_annotation.start().x - source_annotation.start().x,
        duplicated_annotation.start().y - source_annotation.start().y,
    );
    assert_eq!(annotation_offset, item_offset);
    assert_ne!(item_offset, egui::Vec2::ZERO);
    assert_eq!(
        app.canvas.selected_placement_ids(),
        vec![duplicated_item.state.id]
    );
    assert_eq!(
        app.canvas.selected_annotation_ids(),
        vec![duplicated_annotation.id]
    );
    let expected = vec![
        CanvasStackItem::Annotation(source_annotation.id),
        CanvasStackItem::Placement(source_item.state.id),
        CanvasStackItem::Annotation(duplicated_annotation.id),
        CanvasStackItem::Placement(duplicated_item.state.id),
    ];
    assert_eq!(app.canvas.canvas_stack_order(), expected);
    app.flush_persistence();
    assert_eq!(library.load_canvas_stack_order().unwrap(), expected);
}

#[test]
fn command_a_selects_all_active_board_images_and_annotations() {
    let ctx = egui::Context::default();
    let mut app = test_app(None);
    for id in [1_i64, 2] {
        app.canvas
            .add_prepared_placement_image(&ctx, test_loaded_item(id, 1));
    }
    app.canvas
        .add_annotation(crate::data::CanvasAnnotation::new(
            10,
            CanvasAnnotationKind::Textbox,
            eframe::egui::pos2(0.0, 0.0),
            eframe::egui::pos2(100.0, 40.0),
            "one".to_owned(),
            CanvasTextSize::M,
            CanvasStrokeWidth::M,
            crate::data::CanvasAnnotationColor::Default,
            false,
            false,
        ));
    app.canvas
        .add_annotation(crate::data::CanvasAnnotation::new(
            11,
            CanvasAnnotationKind::Line,
            eframe::egui::pos2(20.0, 20.0),
            eframe::egui::pos2(80.0, 80.0),
            String::new(),
            CanvasTextSize::M,
            CanvasStrokeWidth::M,
            crate::data::CanvasAnnotationColor::Default,
            false,
            false,
        ));
    app.canvas.select_content(&[], &[]);

    let _ = crate::ui::test_frame::run_ui(
        &ctx,
        key_press_input(egui::Modifiers::COMMAND, egui::Key::A),
        |ui| {
            app.handle_selection_shortcuts(
                ui.ctx(),
                egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(960.0, 640.0)),
            )
        },
    );

    assert_eq!(app.canvas.selected_placement_ids(), vec![1, 2]);
    assert_eq!(app.canvas.selected_annotation_ids(), vec![10, 11]);
}

#[test]
fn delete_from_board_archives_image_placements_and_annotations() {
    let ctx = egui::Context::default();
    let root = TempRoot::new("snappyref-delete-from-board");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let image_path = library
        .store_rgba_image("source", &[255, 0, 0, 255], 1, 1)
        .unwrap();
    let source_item = library
        .fixture_placement(
            library.load_user_session().unwrap().active_board_id,
            crate::data::Placement::loaded(
                crate::data::PlacementState::new(
                    library.allocate_document_id(),
                    egui::pos2(10.0, 20.0),
                    1.0,
                    0.0,
                    0,
                    false,
                ),
                library
                    .register_original(&image_path, "image/png", 1, 1, 4, None)
                    .unwrap(),
            ),
        )
        .unwrap();
    let annotation = library
        .fixture_annotation(
            CanvasAnnotationKind::Line,
            egui::pos2(0.0, 0.0),
            egui::pos2(20.0, 20.0),
            "",
            CanvasTextSize::M,
            CanvasStrokeWidth::M,
            crate::data::CanvasAnnotationColor::Default,
            false,
        )
        .unwrap();
    let mut app = test_app(Some(library.clone()));
    app.canvas
        .add_prepared_placement_image(&ctx, loaded_record(source_item.clone()));
    app.canvas.add_annotation(annotation.clone());
    app.canvas
        .select_content(&[source_item.state.id], &[annotation.id]);

    app.handle_context_menu_action(
        crate::app::shortcuts::ContextMenuAction::DeleteFromBoard,
        &ctx,
        egui::Rect::EVERYTHING,
    );

    app.flush_persistence();
    assert!(library.load_placements().unwrap().is_empty());
    assert!(
        library
            .asset_original_paths_for_assets(&[source_item.require_loaded_asset().id])
            .unwrap()
            .contains_key(&source_item.require_loaded_asset().id)
    );
    let connection = rusqlite::Connection::open(root.join("library.db")).unwrap();
    let annotation_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM canvas_annotations WHERE id = ?1 AND archived = 1",
            [annotation.id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(annotation_count, 1);
}

#[test]
fn board_create_undo_and_redo_restore_the_same_identity_once() {
    let ctx = egui::Context::default();
    let root = TempRoot::new("snappyref-create-board-history");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let original_id = library.list_boards().unwrap()[0].id;
    let other = library.fixture_board("Other").unwrap();
    let mut app = test_app(Some(library.clone()));

    app.create_and_switch_board(false);
    app.flush_persistence();
    let created = library
        .list_boards()
        .unwrap()
        .into_iter()
        .find(|board| board.id != original_id && board.id != other.id)
        .unwrap();

    app.board_editing.persistence = crate::app::DocumentPersistence::queued(library.clone());
    let storage_lock = rusqlite::Connection::open(root.join("library.db")).unwrap();
    storage_lock.execute_batch("BEGIN IMMEDIATE").unwrap();

    app.undo_document_history(&ctx);
    assert_eq!(app.active_board_id, original_id);
    assert_eq!(
        app.board_summaries()
            .iter()
            .map(|summary| summary.board.id)
            .collect::<Vec<_>>(),
        vec![original_id, other.id]
    );
    let archived_before_flush: i64 = storage_lock
        .query_row(
            "SELECT archived FROM boards WHERE id = ?1",
            [created.id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(archived_before_flush, 0);

    storage_lock.execute_batch("COMMIT").unwrap();
    app.flush_persistence();
    assert!(!app.status.contains("failed"), "{}", app.status);
    assert_eq!(
        storage_lock
            .query_row(
                "SELECT archived FROM boards WHERE id = ?1",
                [created.id],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        1,
        "queued create Undo must archive the created Board"
    );
    assert_eq!(
        library
            .list_boards()
            .unwrap()
            .iter()
            .map(|board| board.id)
            .collect::<Vec<_>>(),
        vec![original_id, other.id]
    );
    assert_eq!(app.active_board_id, original_id);

    let status_after_undo = app.status.clone();
    app.undo_document_history(&ctx);
    assert_eq!(app.status, status_after_undo, "create records one entry");

    app.redo_document_history(&ctx);
    app.flush_persistence();
    assert_eq!(
        library.list_boards().unwrap(),
        vec![
            crate::data::Board {
                id: original_id,
                name: "My Board".to_owned(),
            },
            other,
            created.clone(),
        ]
    );
    assert_eq!(app.active_board_id, created.id);
}

#[test]
fn board_rename_undo_and_redo_restore_exact_names_once() {
    let ctx = egui::Context::default();
    let root = TempRoot::new("snappyref-rename-board-history");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let board = library.list_boards().unwrap()[0].clone();
    let mut app = test_app(Some(library.clone()));
    app.board_editing.persistence = crate::app::DocumentPersistence::queued(library.clone());
    let storage_lock = rusqlite::Connection::open(root.join("library.db")).unwrap();
    storage_lock.execute_batch("BEGIN IMMEDIATE").unwrap();

    app.handle_board_action(Some(crate::ui::sidebar::SidebarBoardAction::Rename(
        board.id,
        "  Reference Board  ".to_owned(),
    )));
    assert_eq!(app.board_summaries()[0].board.name, "Reference Board");
    app.handle_board_action(Some(crate::ui::sidebar::SidebarBoardAction::Rename(
        board.id,
        "Final Board".to_owned(),
    )));
    assert_eq!(app.board_summaries()[0].board.name, "Final Board");
    assert_eq!(
        storage_lock
            .query_row("SELECT name FROM boards WHERE id = ?1", [board.id], |row| {
                row.get::<_, String>(0)
            })
            .unwrap(),
        "My Board"
    );
    storage_lock.execute_batch("COMMIT").unwrap();
    app.flush_persistence();
    assert_eq!(library.list_boards().unwrap()[0].name, "Final Board");

    storage_lock.execute_batch("BEGIN IMMEDIATE").unwrap();
    app.undo_document_history(&ctx);
    assert_eq!(app.board_summaries()[0].board.name, "Reference Board");
    assert_eq!(
        storage_lock
            .query_row("SELECT name FROM boards WHERE id = ?1", [board.id], |row| {
                row.get::<_, String>(0)
            })
            .unwrap(),
        "Final Board"
    );
    storage_lock.execute_batch("COMMIT").unwrap();
    app.flush_persistence();
    assert_eq!(library.list_boards().unwrap()[0].name, "Reference Board");

    app.undo_document_history(&ctx);
    app.flush_persistence();
    assert_eq!(library.list_boards().unwrap()[0].name, "My Board");

    let status_after_undo = app.status.clone();
    app.undo_document_history(&ctx);
    assert_eq!(app.status, status_after_undo, "rename records one entry");

    app.redo_document_history(&ctx);
    app.flush_persistence();
    assert_eq!(library.list_boards().unwrap()[0].name, "Reference Board");
    app.redo_document_history(&ctx);
    app.flush_persistence();
    assert_eq!(library.list_boards().unwrap()[0].name, "Final Board");
}

#[test]
fn deleting_inactive_board_after_queued_resolution_restores_resolved_placement() {
    let ctx = egui::Context::default();
    let root = TempRoot::new("snappyref-delete-after-queued-resolution");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let first = library.list_boards().unwrap()[0].clone();
    let second = library.fixture_board("Second").unwrap();
    library.set_active_board(second.id).unwrap();
    let source = library
        .store_rgba_image("resolved", &[255, 0, 0, 255], 1, 1)
        .unwrap();
    let asset = library.create_asset(&source, "image/png", 1, 1, 4).unwrap();
    let loading = library
        .fixture_placement(
            library.load_user_session().unwrap().active_board_id,
            library.new_committed_loading_placement(
                egui::Pos2::ZERO,
                1.0,
                eframe::egui::vec2(1.0, 1.0),
                crate::data::ContentResolutionSource::ExternalPath {
                    path: source.to_string_lossy().into_owned(),
                },
                Some("Loading".to_owned()),
            ),
        )
        .unwrap();
    library.set_active_board(first.id).unwrap();

    let mut app = test_app(Some(library.clone()));
    app.active_board_id = first.id;
    app.refresh_board_summaries();
    app.board_editing
        .accepted_board_document(&library, second.id)
        .unwrap();
    app.board_editing.history.record_applied(
        second.id,
        crate::app::HistoryAction::document(
            "Added image".to_owned(),
            vec![crate::app::HistoryChange {
                before: None,
                after: Some(loading.clone()),
            }],
            Vec::new(),
            None,
        ),
    );
    app.board_editing.persistence = crate::app::DocumentPersistence::queued(library.clone());
    app.board_editing
        .persistence
        .enqueue_document_unit(
            second.id,
            crate::canvas::CanvasDocumentChanges {
                updated_placements: vec![loading.clone()],
                ..Default::default()
            },
            &app.board_editing.history,
        )
        .unwrap();
    app.flush_persistence();

    let trigger_connection = rusqlite::Connection::open(root.join("library.db")).unwrap();
    trigger_connection
        .execute_batch(
            "CREATE TRIGGER fail_resolution_history
             BEFORE UPDATE ON board_history
             BEGIN
                 SELECT RAISE(ABORT, 'injected resolution-history failure');
             END;",
        )
        .unwrap();

    let mut resolved = loading.clone();
    resolved.content = crate::data::PlacementContent::Loaded {
        asset: asset.clone(),
    };
    assert!(
        app.board_editing
            .resolve_placements(second.id, vec![resolved.clone()])
            .unwrap()
    );

    app.request_board_delete(second.id, second.name.clone());
    app.confirm_pending_board_delete(&ctx);
    trigger_connection
        .execute_batch("DROP TRIGGER fail_resolution_history")
        .unwrap();
    app.flush_persistence();

    app.undo_document_history(&ctx);
    app.flush_persistence();
    let reopened = Library::initialize(root.to_path_buf()).unwrap();
    let restored = reopened.load_board_document(second.id).unwrap();
    assert_eq!(restored.placements.len(), 1);
    assert_eq!(restored.placements[0].state.id, resolved.state.id);
    assert_eq!(restored.placements[0].content, resolved.content);
}

#[test]
fn queued_board_identities_remain_visible_while_persistence_retries() {
    let root = TempRoot::new("snappyref-queued-board-identity");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let trigger_connection = rusqlite::Connection::open(root.join("library.db")).unwrap();
    trigger_connection
        .execute_batch(
            "CREATE TRIGGER fail_board_history
             BEFORE INSERT ON board_history
             BEGIN
                 SELECT RAISE(ABORT, 'injected Board-history failure');
             END;",
        )
        .unwrap();
    let mut app = test_app(Some(library.clone()));
    app.board_editing.persistence = crate::app::DocumentPersistence::queued(library);

    app.create_and_switch_board(false);
    app.create_and_switch_board(false);
    assert_eq!(app.board_summaries().len(), 3);

    app.refresh_board_summaries();
    let accepted_board_count = app.board_summaries().len();

    trigger_connection
        .execute_batch("DROP TRIGGER fail_board_history")
        .unwrap();
    app.flush_persistence();
    drop(app);

    assert_eq!(accepted_board_count, 3);
}

#[test]
fn deleting_active_queued_board_uses_accepted_queued_fallback() {
    let ctx = egui::Context::default();
    let root = TempRoot::new("snappyref-delete-queued-active-board");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let trigger_connection = rusqlite::Connection::open(root.join("library.db")).unwrap();
    trigger_connection
        .execute_batch(
            "CREATE TRIGGER fail_board_history
             BEFORE INSERT ON board_history
             BEGIN
                 SELECT RAISE(ABORT, 'injected Board-history failure');
             END;",
        )
        .unwrap();
    let mut app = test_app(Some(library.clone()));
    app.board_editing.persistence = crate::app::DocumentPersistence::queued(library);

    app.create_and_switch_board(false);
    let fallback_id = app.active_board_id;
    app.create_and_switch_board(false);
    let deleted_id = app.active_board_id;

    app.request_board_delete(deleted_id, "Board 3".to_owned());
    app.confirm_pending_board_delete(&ctx);

    let active_after_delete = app.active_board_id;
    let deleted_identity_visible = app
        .board_summaries()
        .iter()
        .any(|summary| summary.board.id == deleted_id);

    trigger_connection
        .execute_batch("DROP TRIGGER fail_board_history")
        .unwrap();
    app.flush_persistence();
    drop(app);

    assert_eq!(active_after_delete, fallback_id);
    assert!(
        !deleted_identity_visible,
        "the accepted delete should update memory even while both Board creations await retry"
    );
}

#[test]
fn deleting_active_board_restores_accepted_fallback_content_while_persistence_retries() {
    let ctx = egui::Context::default();
    let root = TempRoot::new("snappyref-delete-accepted-fallback-content");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let fallback = library.list_boards().unwrap()[0].clone();
    let trigger_connection = rusqlite::Connection::open(root.join("library.db")).unwrap();
    trigger_connection
        .execute_batch(
            "CREATE TRIGGER fail_board_history
             BEFORE INSERT ON board_history
             BEGIN
                 SELECT RAISE(ABORT, 'injected Board-history failure');
             END;",
        )
        .unwrap();
    let mut app = test_app(Some(library.clone()));
    app.board_editing.persistence = crate::app::DocumentPersistence::queued(library.clone());

    app.handle_board_action(Some(crate::ui::sidebar::SidebarBoardAction::Rename(
        fallback.id,
        "Accepted fallback".to_owned(),
    )));
    let stack_before = app.canvas.canvas_stack_order();
    let annotation = library.new_canvas_annotation(
        CanvasAnnotationKind::Textbox,
        egui::pos2(40.0, 60.0),
        egui::pos2(140.0, 100.0),
        "accepted note".to_owned(),
        CanvasTextSize::M,
        CanvasStrokeWidth::M,
        crate::data::CanvasAnnotationColor::Sky,
        true,
    );
    app.canvas.add_annotation(annotation.clone());
    app.canvas.mark_annotation_for_persistence(annotation.id);
    app.board_editing.history.record_applied(
        fallback.id,
        crate::app::HistoryAction::document(
            "Added textbox".to_owned(),
            Vec::new(),
            vec![crate::app::HistoryChange {
                before: None,
                after: Some(annotation.clone()),
            }],
            Some(crate::app::HistoryChange {
                before: Some(stack_before),
                after: Some(app.canvas.canvas_stack_order()),
            }),
        ),
    );
    app.board_editing
        .accept_created_batch(&mut app.canvas, app.active_board_id, None)
        .unwrap();
    app.create_and_switch_board(false);
    let deleted_id = app.active_board_id;

    app.request_board_delete(deleted_id, "Board 2".to_owned());
    app.confirm_pending_board_delete(&ctx);

    let active_after_delete = app.active_board_id;
    let fallback_name_after_delete = app.board_summaries()[0].board.name.clone();
    let fallback_annotations_after_delete = app.canvas.annotations();

    trigger_connection
        .execute_batch("DROP TRIGGER fail_board_history")
        .unwrap();
    app.flush_persistence();
    drop(app);

    assert_eq!(active_after_delete, fallback.id);
    assert_eq!(fallback_name_after_delete, "Accepted fallback");
    assert_eq!(fallback_annotations_after_delete, vec![annotation]);
}

#[test]
fn undoing_board_create_reopens_accepted_previous_board_while_persistence_retries() {
    let ctx = egui::Context::default();
    let root = TempRoot::new("snappyref-undo-create-accepted-content");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let previous = library.list_boards().unwrap()[0].clone();
    let trigger_connection = rusqlite::Connection::open(root.join("library.db")).unwrap();
    trigger_connection
        .execute_batch(
            "CREATE TRIGGER fail_board_history
             BEFORE INSERT ON board_history
             BEGIN
                 SELECT RAISE(ABORT, 'injected Board-history failure');
             END;",
        )
        .unwrap();
    let mut app = test_app(Some(library.clone()));
    app.board_editing.persistence = crate::app::DocumentPersistence::queued(library.clone());

    app.handle_board_action(Some(crate::ui::sidebar::SidebarBoardAction::Rename(
        previous.id,
        "Accepted previous".to_owned(),
    )));
    let stack_before = app.canvas.canvas_stack_order();
    let annotation = library.new_canvas_annotation(
        CanvasAnnotationKind::Textbox,
        egui::pos2(40.0, 60.0),
        egui::pos2(140.0, 100.0),
        "accepted note".to_owned(),
        CanvasTextSize::M,
        CanvasStrokeWidth::M,
        crate::data::CanvasAnnotationColor::Sky,
        true,
    );
    app.canvas.add_annotation(annotation.clone());
    app.canvas.mark_annotation_for_persistence(annotation.id);
    app.board_editing.history.record_applied(
        previous.id,
        crate::app::HistoryAction::document(
            "Added textbox".to_owned(),
            Vec::new(),
            vec![crate::app::HistoryChange {
                before: None,
                after: Some(annotation.clone()),
            }],
            Some(crate::app::HistoryChange {
                before: Some(stack_before),
                after: Some(app.canvas.canvas_stack_order()),
            }),
        ),
    );
    app.board_editing
        .accept_created_batch(&mut app.canvas, app.active_board_id, None)
        .unwrap();
    app.create_and_switch_board(false);
    assert_ne!(app.active_board_id, previous.id);

    app.undo_document_history(&ctx);

    let active_after_undo = app.active_board_id;
    let previous_name_after_undo = app.board_summaries()[0].board.name.clone();
    let previous_annotations_after_undo = app.canvas.annotations();

    trigger_connection
        .execute_batch("DROP TRIGGER fail_board_history")
        .unwrap();
    app.flush_persistence();
    drop(app);

    assert_eq!(active_after_undo, previous.id);
    assert_eq!(previous_name_after_undo, "Accepted previous");
    assert_eq!(previous_annotations_after_undo, vec![annotation]);
}

#[test]
fn generated_board_names_fill_smallest_visible_gap() {
    let root = TempRoot::new("snappyref-board-name-gap");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let board_2 = library.fixture_board("Board 2").unwrap();
    library.fixture_board("Board 3").unwrap();
    {
        let connection = rusqlite::Connection::open(root.join("library.db")).unwrap();
        connection
            .execute("UPDATE boards SET archived = 1 WHERE id = ?1", [board_2.id])
            .unwrap();
    }
    let mut app = test_app(Some(library.clone()));

    app.create_and_switch_board(false);

    app.flush_persistence();
    let boards = library.list_boards().unwrap();
    assert!(boards.iter().any(|board| board.name == "Board 2"));
    assert_eq!(app.active_board_id, boards.last().unwrap().id);
    assert!(!app.sidebar_open);
}

#[test]
fn command_n_creates_board_without_opening_sidebar() {
    let ctx = egui::Context::default();
    let root = TempRoot::new("snappyref-cmd-n-board");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let mut app = test_app(Some(library.clone()));
    app.sidebar_open = false;

    let _ = crate::ui::test_frame::run_ui(
        &ctx,
        key_press_input(egui::Modifiers::COMMAND, egui::Key::N),
        |ui| app.handle_board_shortcuts(ui.ctx()),
    );

    app.flush_persistence();
    let boards = library.list_boards().unwrap();
    assert_eq!(boards.len(), 2);
    assert_eq!(boards[1].name, "Board 2");
    assert_eq!(app.active_board_id, boards[1].id);
    assert!(!app.sidebar_open);
}

#[test]
fn command_number_switches_visible_board() {
    let ctx = egui::Context::default();
    let root = TempRoot::new("snappyref-cmd-number-board");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    for index in 2..=10 {
        library.fixture_board(&format!("Board {index}")).unwrap();
    }
    let boards = library.list_boards().unwrap();
    let mut app = test_app(Some(library.clone()));

    for (key, index) in [
        (egui::Key::Num3, 2),
        (egui::Key::Num9, 8),
        (egui::Key::Num0, 9),
    ] {
        let _ = crate::ui::test_frame::run_ui(
            &ctx,
            key_press_input(egui::Modifiers::COMMAND, key),
            |ui| app.handle_board_shortcuts(ui.ctx()),
        );

        assert_eq!(app.active_board_id, boards[index].id);
    }
    app.flush_persistence();
    assert_eq!(
        library.load_user_session().unwrap().active_board_id,
        boards[9].id
    );
}

#[test]
fn board_shortcuts_are_suppressed_while_search_is_open() {
    let ctx = egui::Context::default();
    let root = TempRoot::new("snappyref-board-shortcut-search");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    library.fixture_board("Second").unwrap();
    let mut app = test_app(Some(library.clone()));
    app.search.overlay.open = true;

    let _ = crate::ui::test_frame::run_ui(
        &ctx,
        key_press_input(egui::Modifiers::COMMAND, egui::Key::Num2),
        |ui| app.handle_board_shortcuts(ui.ctx()),
    );

    assert_eq!(app.active_board_id, 1);
}

#[test]
fn switching_boards_saves_previous_viewport_and_loads_target_viewport() {
    let root = TempRoot::new("snappyref-board-switch-viewport");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let first = library.list_boards().unwrap()[0].clone();
    let second = library.fixture_board("Second").unwrap();
    library.set_active_board(second.id).unwrap();
    let mut second_session = library.load_user_session().unwrap();
    second_session.viewport.camera_pan_x = 75.0;
    second_session.viewport.canvas_background = crate::data::CanvasBackground::Dots;
    library.save_user_session(&second_session).unwrap();
    library.set_active_board(first.id).unwrap();
    let mut app = test_app(Some(library.clone()));
    app.active_board_id = first.id;
    app.refresh_board_summaries();
    app.canvas
        .apply_viewport_state(&crate::data::ViewportState {
            camera_pan_x: 22.0,
            camera_pan_y: 33.0,
            camera_zoom: 1.5,
            canvas_background: crate::data::CanvasBackground::Grid,
        });

    app.switch_to_board(second.id);

    let saved_first = {
        app.flush_persistence();
        library.set_active_board(first.id).unwrap();
        library.load_user_session().unwrap()
    };
    assert_eq!(saved_first.viewport.camera_pan_x, 22.0);
    assert_eq!(
        saved_first.viewport.canvas_background,
        crate::data::CanvasBackground::Grid
    );
    library.set_active_board(second.id).unwrap();
    assert_eq!(app.active_board_id, second.id);
    assert_eq!(app.canvas.background(), crate::data::CanvasBackground::Dots);
    let first_view = saved_first.viewport;
    app.sidebar_open = false;
    for _ in 0..3 {
        app.switch_to_board(first.id);
        assert_eq!(app.canvas.viewport_state(), first_view);
        app.switch_to_board(second.id);
        assert_eq!(app.canvas.viewport_state(), second_session.viewport);
    }
}

#[test]
fn active_board_load_applies_viewport_without_reloading_global_settings() {
    let root = TempRoot::new("snappyref-active-load-settings-scope");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let second = library.fixture_board("Second").unwrap();
    library.set_active_board(second.id).unwrap();
    let mut stored_session = library.load_user_session().unwrap();
    stored_session.settings.placement_gap = 99.0;
    stored_session.viewport.camera_pan_x = 75.0;
    stored_session.viewport.canvas_background = crate::data::CanvasBackground::Dots;
    library.save_user_session(&stored_session).unwrap();

    let mut app = test_app(Some(library.clone()));
    app.apply_user_session(Ok(stored_session));
    app.settings.placement_gap = 24.0;

    app.load_active_board();

    assert_eq!(app.active_board_id, second.id);
    assert_eq!(app.canvas.background(), crate::data::CanvasBackground::Dots);
    assert_eq!(app.canvas.viewport_state().camera_pan_x, 75.0);
    assert_eq!(app.settings.placement_gap, 24.0);
}

#[test]
fn board_delete_requires_confirmation_before_archiving_active_board() {
    let ctx = egui::Context::default();
    let root = TempRoot::new("snappyref-confirm-delete-board");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let first = library.list_boards().unwrap()[0].clone();
    let second = library.fixture_board("Second").unwrap();
    library.set_active_board(second.id).unwrap();
    let mut app = test_app(Some(library.clone()));
    app.active_board_id = second.id;
    app.refresh_board_summaries();
    app.sidebar_open = true;

    app.request_board_delete(second.id, second.name.clone());

    assert!(app.pending_board_delete.is_some());
    app.flush_persistence();
    assert_eq!(library.list_boards().unwrap().len(), 2);

    app.confirm_pending_board_delete(&ctx);

    assert!(app.pending_board_delete.is_none());
    assert!(app.sidebar_open);
    assert_eq!(app.active_board_id, first.id);
    app.flush_persistence();
    assert_eq!(
        library
            .list_boards()
            .unwrap()
            .iter()
            .map(|board| board.id)
            .collect::<Vec<_>>(),
        vec![first.id]
    );
}

#[test]
fn board_delete_confirmation_keeps_sidebar_open_and_removes_inactive_board() {
    let ctx = egui::Context::default();
    let root = TempRoot::new("snappyref-delete-inactive-board");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let first = library.list_boards().unwrap()[0].clone();
    let second = library.fixture_board("Second").unwrap();
    let mut app = test_app(Some(library.clone()));
    app.active_board_id = first.id;
    app.refresh_board_summaries();
    app.sidebar_open = true;

    app.request_board_delete(second.id, second.name.clone());
    app.confirm_pending_board_delete(&ctx);

    assert!(app.pending_board_delete.is_none());
    assert!(app.sidebar_open);
    assert_eq!(app.active_board_id, first.id);
    app.flush_persistence();
    assert_eq!(
        library
            .list_boards()
            .unwrap()
            .iter()
            .map(|board| board.id)
            .collect::<Vec<_>>(),
        vec![first.id]
    );
}

#[test]
fn board_delete_undo_restores_document_view_history_and_active_fallback() {
    let ctx = egui::Context::default();
    let root = TempRoot::new("snappyref-delete-board-history");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let first = library.list_boards().unwrap()[0].clone();
    let second = library.fixture_board("Reference").unwrap();
    library.set_active_board(second.id).unwrap();
    let image_path = library
        .store_rgba_image("board-history", &[255, 0, 0, 255], 1, 1)
        .unwrap();
    let placement = library
        .fixture_placement(
            library.load_user_session().unwrap().active_board_id,
            crate::data::Placement::loaded(
                crate::data::PlacementState::new(
                    library.allocate_document_id(),
                    egui::pos2(40.0, 60.0),
                    1.5,
                    0.0,
                    0,
                    false,
                ),
                library
                    .register_original(&image_path, "image/png", 1, 1, 4, None)
                    .unwrap(),
            ),
        )
        .unwrap();
    let annotation = library
        .fixture_annotation(
            CanvasAnnotationKind::Textbox,
            egui::pos2(10.0, 20.0),
            egui::pos2(110.0, 80.0),
            "kept note",
            CanvasTextSize::L,
            CanvasStrokeWidth::M,
            crate::data::CanvasAnnotationColor::Sky,
            true,
        )
        .unwrap();
    let mut other = placement.clone();
    other.state.id = library.allocate_document_id();
    other.state.geometry.center.x += 80.0;
    let other = library.fixture_placement(second.id, other).unwrap();
    let group = crate::data::Group {
        id: library.allocate_document_id(),
        name: "Kept group".to_owned(),
        members: vec![other.state.id, placement.state.id],
        container: placement.state.geometry,
        flow: crate::data::GroupFlow::Column,
        gap: 17.0,
        layout: crate::data::GroupLayoutSettings {
            row_gap: 17.0,
            ..Default::default()
        },
        padding: 23.0,
    };
    let mixed_stack = vec![
        CanvasStackItem::Annotation(annotation.id),
        CanvasStackItem::Placement(placement.state.id),
        CanvasStackItem::Placement(other.state.id),
    ];
    library
        .fixture_board_document(
            library.load_user_session().unwrap().active_board_id,
            crate::canvas::CanvasDocumentChanges {
                stack_order: Some(mixed_stack.to_vec()),
                groups: Some(vec![group.clone()]),
                ..Default::default()
            },
        )
        .unwrap();
    let mut session = library.load_user_session().unwrap();
    session.viewport = crate::data::ViewportState {
        camera_pan_x: 125.0,
        camera_pan_y: -48.0,
        camera_zoom: 1.75,
        canvas_background: CanvasBackground::Grid,
    };
    library.save_user_session(&session).unwrap();

    let mut app = test_app(Some(library.clone()));
    app.active_board_id = second.id;
    app.refresh_board_summaries();
    app.canvas.apply_viewport_state(&session.viewport);
    app.sidebar_open = true;
    app.context_menu_pos = Some(egui::pos2(12.0, 18.0));
    app.board_editing
        .accepted_board_document(&library, second.id)
        .unwrap();
    app.board_editing.history.record_applied(
        second.id,
        crate::active_board::history::HistoryAction::document(
            "Earlier document change".to_owned(),
            Vec::new(),
            Vec::new(),
            None,
        ),
    );

    app.request_board_delete(second.id, second.name.clone());
    app.confirm_pending_board_delete(&ctx);

    assert_eq!(app.active_board_id, first.id);
    app.flush_persistence();
    assert_eq!(library.list_boards().unwrap(), vec![first.clone()]);
    app.sidebar_open = false;
    app.window_state.inner_width = Some(777.0);
    app.context_menu_pos = None;
    app.board_editing.persistence = crate::app::DocumentPersistence::queued(library.clone());
    let storage_lock = rusqlite::Connection::open(root.join("library.db")).unwrap();
    storage_lock.execute_batch("BEGIN IMMEDIATE").unwrap();

    app.undo_document_history(&ctx);

    assert_eq!(app.active_board_id, second.id);
    assert_eq!(app.board_summaries().len(), 2);
    let archived_before_flush: i64 = storage_lock
        .query_row(
            "SELECT archived FROM boards WHERE id = ?1",
            [second.id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(archived_before_flush, 1);
    storage_lock.execute_batch("COMMIT").unwrap();
    app.flush_persistence();
    assert_eq!(
        library.list_boards().unwrap(),
        vec![first.clone(), second.clone()]
    );
    let restored_placements = library.load_placements().unwrap();
    assert_eq!(restored_placements.len(), 2);
    assert_eq!(library.load_groups().unwrap(), vec![group.clone()]);
    assert_eq!(restored_placements[0].state.id, placement.state.id);
    assert_eq!(
        restored_placements[0].state.geometry,
        placement.state.geometry
    );
    assert_eq!(restored_placements[0].content, placement.content);
    assert_eq!(library.load_canvas_annotations().unwrap(), vec![annotation]);
    assert_eq!(library.load_canvas_stack_order().unwrap(), mixed_stack);
    assert_eq!(
        library.load_user_session().unwrap().viewport,
        session.viewport
    );
    assert!(!app.sidebar_open, "sidebar state is not Board history");
    assert_eq!(app.window_state.inner_width, Some(777.0));
    assert!(
        app.context_menu_pos.is_none(),
        "Canvas transients are cleared"
    );

    let active_assets: i64 = storage_lock
        .query_row(
            "SELECT COUNT(*) FROM assets WHERE archived = 0",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(active_assets, 1);

    app.undo_document_history(&ctx);
    assert_eq!(app.status, "Undid Earlier document change");
    app.redo_document_history(&ctx);
    assert_eq!(app.status, "Redid Earlier document change");
    app.redo_document_history(&ctx);
    app.flush_persistence();

    assert_eq!(app.active_board_id, first.id);
    app.flush_persistence();
    assert_eq!(library.list_boards().unwrap(), vec![first]);
    let active_assets: i64 = storage_lock
        .query_row(
            "SELECT COUNT(*) FROM assets WHERE archived = 0",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(active_assets, 1);
    app.undo_document_history(&ctx);
    app.flush_persistence();
    assert_eq!(app.active_board_id, second.id);
    assert_eq!(library.load_groups().unwrap(), vec![group]);
    assert_eq!(library.load_canvas_stack_order().unwrap(), mixed_stack);
    assert_eq!(
        library.load_user_session().unwrap().viewport,
        session.viewport
    );
}

#[test]
fn board_delete_modal_click_routes_through_egui_layers() {
    let root = TempRoot::new("snappyref-delete-modal-click-routing");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let first = library.list_boards().unwrap()[0].clone();
    let second = library.fixture_board("Second").unwrap();
    let mut app = test_app(Some(library.clone()));
    app.active_board_id = second.id;
    app.refresh_board_summaries();
    app.sidebar_open = true;
    app.request_board_delete(second.id, second.name.clone());
    let mut harness = app_harness(app, |ctx, app| {
        let screen_rect = test_screen_rect();
        if app.should_block_sidebar_input() {
            block_sidebar_input(ctx, sidebar_rect(screen_rect));
        }
        app.show_board_delete_confirmation(ctx);
    });

    harness.get_by_label("Delete board").click();
    harness.run();

    assert!(
        harness.state().pending_board_delete.is_none(),
        "the modal should receive the delete-button click"
    );
    assert!(harness.state().sidebar_open);
    assert_eq!(harness.state().active_board_id, first.id);
    harness.state_mut().flush_persistence();
    assert_eq!(
        library
            .list_boards()
            .unwrap()
            .iter()
            .map(|board| board.id)
            .collect::<Vec<_>>(),
        vec![first.id]
    );
}

#[test]
fn board_sidebar_stays_clickable_after_delete_modal_confirms() {
    let root = TempRoot::new("snappyref-sidebar-click-after-delete");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let second = library.fixture_board("Second").unwrap();
    let mut app = test_app(Some(library.clone()));
    app.active_board_id = second.id;
    app.refresh_board_summaries();
    app.sidebar_open = true;
    app.sidebar_tab = SidebarTab::Boards;
    app.request_board_delete(second.id, second.name.clone());
    let mut harness = app_harness(app, |ctx, app| {
        render_sidebar_for_test(ctx, app);
        app.show_board_delete_confirmation(ctx);
    });

    harness.get_by_label("Delete board").click();
    harness.run();

    harness.get_by_label("New board").click();
    harness.run();

    harness.state_mut().flush_persistence();
    let boards = library.list_boards().unwrap();
    assert_eq!(
        boards
            .iter()
            .map(|board| board.name.as_str())
            .collect::<Vec<_>>(),
        vec!["My Board", "Board 2"]
    );
    assert_eq!(harness.state().active_board_id, boards[1].id);
    assert!(harness.state().sidebar_open);
}

#[test]
fn rapid_sidebar_board_name_clicks_keep_last_requested_board_active() {
    let root = TempRoot::new("snappyref-rapid-sidebar-board-clicks");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let first = library.list_boards().unwrap()[0].clone();
    let second = library.fixture_board("Second").unwrap();
    let _third = library.fixture_board("Third").unwrap();
    let mut app = test_app(Some(library.clone()));
    app.active_board_id = first.id;
    app.refresh_board_summaries();
    app.sidebar_open = true;
    app.sidebar_tab = SidebarTab::Boards;
    let mut harness = app_harness(app, |ctx, app| {
        render_sidebar_for_test(ctx, app);
    });

    for label in [
        "Board Second",
        "Board Third",
        "Board My Board",
        "Board Second",
    ] {
        harness.get_by_label(label).click();
        harness.run();
    }

    assert_eq!(harness.state().active_board_id, second.id);
    harness.state_mut().flush_persistence();
    assert_eq!(
        library.load_user_session().unwrap().active_board_id,
        second.id
    );
}

#[test]
fn board_delete_modal_blocks_sidebar_board_switches_until_closed() {
    let root = TempRoot::new("snappyref-delete-modal-blocks-sidebar");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let first = library.list_boards().unwrap()[0].clone();
    let second = library.fixture_board("Second").unwrap();
    let mut app = test_app(Some(library.clone()));
    app.active_board_id = first.id;
    app.refresh_board_summaries();
    app.sidebar_open = true;
    app.sidebar_tab = SidebarTab::Boards;
    app.request_board_delete(second.id, second.name.clone());
    let mut harness = app_harness(app, |ctx, app| {
        render_sidebar_for_test(ctx, app);
        app.show_board_delete_confirmation(ctx);
    });

    harness.get_by_label("Board Second").click();
    harness.run();

    assert_eq!(harness.state().active_board_id, first.id);
    assert!(harness.state().pending_board_delete.is_some());

    harness.get_by_label("Cancel").click();
    harness.run();

    assert!(harness.state().pending_board_delete.is_none());
    harness.get_by_label("Board Second").click();
    harness.run();

    assert_eq!(harness.state().active_board_id, second.id);
    harness.state_mut().flush_persistence();
    assert_eq!(
        library.load_user_session().unwrap().active_board_id,
        second.id
    );
}

#[test]
fn board_delete_modal_blocks_sidebar_tabs_and_settings() {
    let root = TempRoot::new("snappyref-delete-modal-blocks-sidebar-state");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let second = library.fixture_board("Second").unwrap();
    let mut app = test_app(Some(library.clone()));
    app.sidebar_open = true;
    app.sidebar_tab = SidebarTab::Boards;
    app.canvas.set_background(CanvasBackground::Grid);
    app.request_board_delete(second.id, second.name.clone());
    let mut harness = app_harness(app, |ctx, app| {
        render_sidebar_for_test(ctx, app);
        app.show_board_delete_confirmation(ctx);
    });

    harness.get_by_label("Settings").click();
    harness.run();

    assert_eq!(harness.state().sidebar_tab, SidebarTab::Boards);

    harness.state_mut().sidebar_tab = SidebarTab::Options;
    harness.run();
    let rotation_snap_rect = harness.get_by_label("Rotation snap").rect();
    harness.get_by_label("Dots").click();
    drag_test_pos(
        &harness,
        egui::pos2(
            rotation_snap_rect.left() + 28.0,
            rotation_snap_rect.bottom() + 34.0,
        ),
        egui::pos2(
            rotation_snap_rect.left() + 128.0,
            rotation_snap_rect.bottom() + 34.0,
        ),
    );
    harness.run();

    assert_eq!(harness.state().sidebar_tab, SidebarTab::Options);
    assert_eq!(harness.state().canvas.background(), CanvasBackground::Grid);
    assert_eq!(harness.state().settings.rotation_snap_degrees, 15.0);
    assert!(harness.state().pending_board_delete.is_some());
}

#[test]
fn sidebar_board_lifecycle_clicks_create_switch_and_persist_active_board() {
    let root = TempRoot::new("snappyref-sidebar-board-lifecycle");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let first = library.list_boards().unwrap()[0].clone();
    let mut app = test_app(Some(library.clone()));
    app.active_board_id = first.id;
    app.refresh_board_summaries();
    app.sidebar_open = true;
    app.sidebar_tab = SidebarTab::Boards;
    let mut harness = app_harness(app, |ctx, app| {
        render_sidebar_for_test(ctx, app);
    });

    harness.get_by_label("New board").click();
    harness.run();

    harness.state_mut().flush_persistence();
    let boards = library.list_boards().unwrap();
    assert_eq!(boards.len(), 2);
    assert_eq!(boards[1].name, "Board 2");
    assert_eq!(harness.state().active_board_id, boards[1].id);
    assert!(harness.state().sidebar_open);

    harness.get_by_label("Board My Board").click();
    harness.run();

    assert_eq!(harness.state().active_board_id, first.id);
    harness.state_mut().flush_persistence();
    assert_eq!(
        library.load_user_session().unwrap().active_board_id,
        first.id
    );
}

#[test]
fn switching_boards_clears_selection_and_search() {
    let ctx = egui::Context::default();
    let root = TempRoot::new("snappyref-board-switch-cleanup");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let second = library.fixture_board("Second").unwrap();
    let mut app = test_app(Some(library.clone()));
    let asset = Asset {
        id: 1,
        original_path: PathBuf::from("fixture.png"),
        filename: "fixture.png".to_owned(),
        mime_type: "image/png".to_owned(),
        width: 1,
        height: 1,
    };
    let placement = Placement::loaded(
        PlacementState::new(1, eframe::egui::pos2(0.0, 0.0), 1.0, 0.0, 0, false),
        asset,
    );
    app.canvas.add_prepared_placement_image(
        &ctx,
        PreparedPlacementImage {
            preview_rgba: vec![255, 255, 255, 255],
            ..loaded_record(placement.clone())
        },
    );
    app.canvas.select_placements(&[1]);
    app.search.overlay.open = true;
    app.switch_to_board(second.id);

    assert!(!app.canvas.has_selection());
    assert!(!app.search.is_open());
}

#[test]
fn search_keyboard_flow_owns_escape_through_app_render_path() {
    let root = TempRoot::new("snappyref-search-keyboard-routing");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let second = library.fixture_board("Second").unwrap();
    let mut app = test_app(Some(library.clone()));
    app.search
        .set_local_model_store_status_for_test(LocalModelStoreStatus::Installed);
    let mut harness = app_harness(app, |ctx, app| {
        let screen_rect = test_screen_rect();
        app.search.handle_shortcut(
            ctx,
            app.library.as_ref(),
            &mut app.canvas,
            &mut app.status,
            app.settings.canvas_image_max_side,
        );
        app.handle_board_shortcuts(ctx);
        app.show_search_overlay(ctx, screen_rect);
    });

    harness.key_press_modifiers(
        egui::Modifiers {
            command: true,
            ..Default::default()
        },
        egui::Key::F,
    );
    harness.step();

    assert!(harness.state().search.overlay.open);
    assert!(
        harness
            .ctx
            .memory(|memory| { memory.has_focus(egui::Id::new("semantic_search_input")) })
    );

    harness.key_press_modifiers(
        egui::Modifiers {
            command: true,
            ..Default::default()
        },
        egui::Key::Num2,
    );
    harness.step();

    assert_eq!(harness.state().active_board_id, 1);

    harness.state_mut().search.overlay.query = "cat".to_owned();
    harness.key_press(egui::Key::Escape);
    harness.step();

    assert!(!harness.state().search.overlay.open);
    assert!(harness.state().search.overlay.query.is_empty());
    harness.state_mut().flush_persistence();
    assert_eq!(library.load_user_session().unwrap().active_board_id, 1);
    assert_ne!(second.id, 1);
}

#[test]
fn enter_places_unplaced_library_search_result_on_empty_active_board() {
    let root = TempRoot::new("snappyref-library-search-enter-unplaced");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let first_path = library
        .store_rgba_image("library-only", &[255, 0, 0, 255], 1, 1)
        .unwrap();
    let library_item = library
        .fixture_placement(
            library.load_user_session().unwrap().active_board_id,
            crate::data::Placement::loaded(
                crate::data::PlacementState::new(
                    library.allocate_document_id(),
                    egui::pos2(0.0, 0.0),
                    1.0,
                    0.0,
                    0,
                    false,
                ),
                library
                    .register_original(&first_path, "image/png", 1, 1, 4, None)
                    .unwrap(),
            ),
        )
        .unwrap();
    let empty_board = library.fixture_board("Empty").unwrap();
    library.set_active_board(empty_board.id).unwrap();
    let mut app = test_app(Some(library.clone()));
    app.active_board_id = empty_board.id;
    app.refresh_board_summaries();
    app.search.overlay.open = true;
    app.search.overlay.results = vec![SearchResultAssetEntry {
        asset_id: library_item.require_loaded_asset().id,
        asset_preview_path: None,
        asset_width: 1,
        asset_height: 1,
    }];
    app.search.overlay.result_selection.replace(0);
    let mut harness = app_full_harness(app);

    harness.key_press(egui::Key::Enter);
    harness.step();
    harness.hover_at(egui::pos2(900.0, 600.0));
    harness.drag_at(egui::pos2(900.0, 600.0));
    harness.drop_at(egui::pos2(900.0, 600.0));
    harness.step();

    harness.state_mut().flush_persistence();
    let active_items = library.load_placements().unwrap();
    assert_eq!(active_items.len(), 1);
    assert_eq!(
        active_items[0].require_loaded_asset().id,
        library_item.require_loaded_asset().id
    );
    assert_eq!(
        harness.state().canvas.selected_placement_ids(),
        vec![active_items[0].state.id]
    );
    assert!(!harness.state().search.overlay.open);
}

#[test]
fn command_enter_places_all_unplaced_library_search_results_on_empty_active_board() {
    let root = TempRoot::new("snappyref-library-search-command-enter-unplaced");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let first_path = library
        .store_rgba_image("first", &[255, 0, 0, 255], 1, 1)
        .unwrap();
    let second_path = library
        .store_rgba_image("second", &[0, 255, 0, 255], 1, 1)
        .unwrap();
    let first = library
        .fixture_placement(
            library.load_user_session().unwrap().active_board_id,
            crate::data::Placement::loaded(
                crate::data::PlacementState::new(
                    library.allocate_document_id(),
                    egui::pos2(0.0, 0.0),
                    1.0,
                    0.0,
                    0,
                    false,
                ),
                library
                    .register_original(&first_path, "image/png", 1, 1, 4, None)
                    .unwrap(),
            ),
        )
        .unwrap();
    let second = library
        .fixture_placement(
            library.load_user_session().unwrap().active_board_id,
            crate::data::Placement::loaded(
                crate::data::PlacementState::new(
                    library.allocate_document_id(),
                    egui::pos2(20.0, 0.0),
                    1.0,
                    0.0,
                    0,
                    false,
                ),
                library
                    .register_original(&second_path, "image/png", 1, 1, 4, None)
                    .unwrap(),
            ),
        )
        .unwrap();
    let empty_board = library.fixture_board("Empty").unwrap();
    library.set_active_board(empty_board.id).unwrap();
    let mut app = test_app(Some(library.clone()));
    app.active_board_id = empty_board.id;
    app.refresh_board_summaries();
    app.search.overlay.open = true;
    app.search.overlay.results = vec![
        SearchResultAssetEntry {
            asset_id: first.require_loaded_asset().id,
            asset_preview_path: None,
            asset_width: 1,
            asset_height: 1,
        },
        SearchResultAssetEntry {
            asset_id: second.require_loaded_asset().id,
            asset_preview_path: None,
            asset_width: 1,
            asset_height: 1,
        },
    ];
    app.search.overlay.result_selection.replace(0);
    let mut harness = app_harness(app, |ctx, app| {
        app.show_search_overlay(ctx, test_screen_rect());
    });

    harness.key_press_modifiers(
        egui::Modifiers {
            command: true,
            ..Default::default()
        },
        egui::Key::Enter,
    );
    harness.step();
    harness.hover_at(egui::pos2(900.0, 600.0));
    harness.drag_at(egui::pos2(900.0, 600.0));
    harness.drop_at(egui::pos2(900.0, 600.0));
    harness.step();

    harness
        .state_mut()
        .board_editing
        .persistence
        .flush()
        .unwrap();
    let mut asset_ids = library
        .load_placements()
        .unwrap()
        .into_iter()
        .map(|placement| placement.require_loaded_asset().id)
        .collect::<Vec<_>>();
    asset_ids.sort_unstable();
    let mut expected_asset_ids = vec![
        first.require_loaded_asset().id,
        second.require_loaded_asset().id,
    ];
    expected_asset_ids.sort_unstable();
    assert_eq!(asset_ids, expected_asset_ids);
    assert_eq!(harness.state().canvas.selected_placement_ids().len(), 2);
    assert!(!harness.state().search.overlay.open);
}

#[test]
fn deleting_last_board_is_rejected_and_cancel_preserves_content() {
    let root = TempRoot::new("snappyref-last-board-delete");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let board = library.list_boards().unwrap()[0].clone();
    let mut app = test_app(Some(library.clone()));
    app.refresh_board_summaries();
    app.request_board_delete(board.id, board.name.clone());
    app.cancel_pending_board_delete();
    assert!(app.pending_board_delete.is_none());
    assert_eq!(library.list_boards().unwrap(), vec![board.clone()]);
    app.request_board_delete(board.id, board.name.clone());
    app.confirm_pending_board_delete(&egui::Context::default());
    app.flush_persistence();
    assert_eq!(app.status, "cannot delete the last board");
    assert_eq!(app.active_board_id, board.id);
    assert_eq!(library.list_boards().unwrap(), vec![board]);
    app.create_and_switch_board(false);
    assert_eq!(app.board_summaries().len(), 2);
}
