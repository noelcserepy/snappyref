use super::*;

#[test]
fn destructive_confirmation_shell_has_no_destructive_outline() {
    let ctx = egui::Context::default();
    crate::ui::theme::configure_theme(&ctx);

    let frame = crate::app::dialogs::dialog_frame(&ctx, 1.0);

    assert_eq!(frame.stroke, egui::Stroke::NONE);
    assert_eq!(frame.fill, crate::ui::theme::SURFACE);
}

#[test]
fn compact_arrangement_action_uses_dense_compact_layout() {
    let mut canvas = crate::canvas::CanvasState::default();
    let ctx = egui::Context::default();
    let fixture = [
        (1_i64, 300_u32, 520_u32),
        (2, 180, 260),
        (3, 160, 260),
        (4, 150, 220),
        (5, 150, 220),
        (6, 140, 180),
        (7, 140, 180),
    ];
    let gap = 12.0;

    for (index, (id, width, height)) in fixture.iter().enumerate() {
        let asset = Asset {
            id: *id,
            original_path: PathBuf::from(format!("fixture-{id}.png")),
            filename: format!("fixture-{id}.png"),
            mime_type: "image/png".to_owned(),
            width: *width,
            height: *height,
        };
        let placement = Placement::loaded(
            PlacementState::new(
                *id,
                eframe::egui::pos2(index as f32 * 500.0, index as f32 * 120.0),
                1.0,
                0.0,
                index as i32,
                false,
            ),
            asset,
        );
        let rgba = vec![255; (*width as usize) * (*height as usize) * 4];
        canvas.add_prepared_placement_image(
            &ctx,
            PreparedPlacementImage {
                preview_rgba: rgba,
                preview_width: *width,
                preview_height: *height,
                ..loaded_record(placement)
            },
        );
    }

    canvas.select_placements(&fixture.iter().map(|(id, _, _)| *id).collect::<Vec<_>>());
    let mut status = String::new();
    apply_arrangement_action(&mut canvas, ArrangementAction::Compact, gap, &mut status);

    let bounds = canvas.selected_bounds_world().unwrap();
    let arranged_area = bounds.width() * bounds.height();
    let item_area = fixture
        .iter()
        .map(|(_, width, height)| *width as f32 * *height as f32)
        .sum::<f32>();
    let occupancy = item_area / arranged_area;

    assert!(occupancy > 0.75);
    assert_eq!(status, format!("Compacted {} images", fixture.len()));
}

#[test]
fn delete_selected_annotation_records_undo_history() {
    let ctx = egui::Context::default();
    let root = TempRoot::new("snappyref-delete-annotation-history");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let annotation = library
        .create_canvas_annotation(
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
    app.board_editing.persistence = crate::app::DocumentPersistence::queued(library.clone());
    app.canvas.add_annotation(annotation.clone());
    app.canvas.select_content(&[], &[annotation.id]);

    app.delete_selected_items_from_board(test_screen_rect());
    app.flush_persistence();
    assert!(library.load_canvas_annotations().unwrap().is_empty());

    app.undo_document_history(&ctx);
    app.flush_persistence();

    assert_eq!(app.status, "Undid Deleted 1 annotation");
    assert_eq!(library.load_canvas_annotations().unwrap(), vec![annotation]);
}

#[test]
fn undo_delete_restores_last_placement_to_active_canvas() {
    let ctx = egui::Context::default();
    let root = TempRoot::new("snappyref-undo-delete-last-placement");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let source = library
        .store_rgba_image("source", &[255, 0, 0, 255], 1, 1)
        .unwrap();
    let placement = library
        .persist_imported_image(&source, "image/png", 1, 1, 4, egui::pos2(40.0, 60.0), 1.0)
        .unwrap();
    let mut app = test_app(Some(library.clone()));
    app.board_editing.persistence = crate::app::DocumentPersistence::queued(library.clone());
    add_loaded_record(&mut app, &ctx, placement.clone());
    app.canvas.select_placements(&[placement.state.id]);

    app.delete_selected_items_from_board(test_screen_rect());
    assert!(app.canvas.placement_for_id(placement.state.id).is_none());

    app.undo_document_history(&ctx);
    app.flush_persistence();

    assert_eq!(app.status, "Undid Deleted 1 image from board");
    assert!(
        app.canvas
            .placement_for_id(placement.state.id)
            .is_some_and(|placement| placement.is_loaded())
    );
    app.flush_persistence();
    assert_eq!(library.load_placements().unwrap().len(), 1);
}

#[test]
fn late_history_content_result_does_not_restore_redone_deletion() {
    let mut app = test_app(None);
    let reservation = app
        .placement_creation
        .reserve_history_placement(app.active_board_id, 7);
    let mut loaded = test_loaded_item(7, app.active_board_id);
    loaded.queue_id = Some(reservation.queue_id);

    app.accept_preparation_result(
        &egui::Context::default(),
        crate::image_preparation::ImagePrepResult::Loaded(loaded),
    );
    assert!(app.canvas.placement_for_id(7).is_none());
    assert_eq!(app.placement_creation.pending_len(), 0);
}

#[test]
fn late_undo_content_failure_is_ignored_after_newer_replay() {
    let ctx = egui::Context::default();
    let mut app = test_app(None);
    let placement_id = 7;
    let undo_placement = test_loaded_item(placement_id, app.active_board_id).placement;
    app.canvas.add_unresolved_placement(undo_placement.clone());
    let reservation = app
        .placement_creation
        .reserve_history_placement(app.active_board_id, placement_id);
    let mut redo_placement = undo_placement;
    redo_placement.require_loaded_asset_mut().id = 202;
    let action = crate::app::HistoryAction::document(
        "Crop image".to_owned(),
        vec![crate::app::HistoryChange {
            before: None,
            after: Some(redo_placement),
        }],
        Vec::new(),
        None,
    );
    let crate::app::HistoryAction::Document(action) = action else {
        panic!("expected document history action");
    };
    let _ = app.board_lifecycle().apply_history_action(
        &ctx,
        &crate::app::HistoryAction::Document(action),
        crate::active_board::history::HistoryDirection::Forward,
    );
    assert_eq!(
        app.canvas
            .placement_for_id(placement_id)
            .unwrap()
            .asset_id(),
        Some(202)
    );
    app.status = "Redid Crop image".to_owned();

    let result = crate::image_preparation::ImagePrepResult::Failed {
        target_board_id: Some(app.active_board_id),
        queue_id: Some(reservation.queue_id),
        placement_id: Some(placement_id),
        context: "fixture-7.png".to_owned(),
        error: "decode failed".to_owned(),
    };
    app.accept_preparation_result(&ctx, result);

    assert_eq!(app.status, "Redid Crop image");
    assert_eq!(
        app.canvas
            .placement_for_id(placement_id)
            .unwrap()
            .asset_id(),
        Some(202)
    );
    assert_eq!(app.placement_creation.pending_len(), 0);
}

#[test]
fn completed_history_content_replay_preserves_history_command_status() {
    let ctx = egui::Context::default();
    let mut app = test_app(None);
    let placement_id = 7;

    for (asset_id, command_status) in [(101, "Undid Crop image"), (202, "Redid Crop image")] {
        let mut loaded = test_loaded_item(placement_id, app.active_board_id);
        loaded.placement.require_loaded_asset_mut().id = asset_id;
        loaded.origin = crate::image_preparation::ImportOrigin::Existing;
        app.canvas
            .add_unresolved_placement(loaded.placement.clone());
        let reservation = app
            .placement_creation
            .reserve_history_placement(app.active_board_id, placement_id);
        loaded.queue_id = Some(reservation.queue_id);
        app.status = command_status.to_owned();

        let result = crate::image_preparation::ImagePrepResult::Loaded(loaded);
        app.accept_preparation_result(&ctx, result);

        assert_eq!(app.status, command_status);
        assert_eq!(
            app.canvas
                .placement_for_id(placement_id)
                .unwrap()
                .require_loaded_asset()
                .id,
            asset_id
        );
        assert_eq!(app.placement_creation.pending_len(), 0);

        let mut detail = test_loaded_item(placement_id, app.active_board_id);
        detail.placement.require_loaded_asset_mut().id = asset_id;
        detail.origin = crate::image_preparation::ImportOrigin::Existing;
        detail.queue_id = None;
        app.accept_preparation_result(
            &ctx,
            crate::image_preparation::ImagePrepResult::Loaded(detail),
        );

        assert_eq!(app.status, command_status);
    }
}

#[test]
fn undo_and_redo_defer_storage_until_explicit_flush() {
    let ctx = egui::Context::default();
    let root = TempRoot::new("snappyref-history-deferred-persistence");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let annotation = library
        .create_canvas_annotation(
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
    app.board_editing.persistence = crate::app::DocumentPersistence::queued(library.clone());
    app.canvas.add_annotation(annotation.clone());
    app.board_editing.history.record_applied(
        app.active_board_id,
        crate::app::HistoryAction::document(
            "Added textbox".to_owned(),
            Vec::new(),
            vec![crate::app::HistoryChange {
                before: None,
                after: Some(annotation.clone()),
            }],
            Some(crate::app::HistoryChange {
                before: Some(Vec::new()),
                after: Some(vec![CanvasStackItem::Annotation(annotation.id)]),
            }),
        ),
    );
    let storage_lock = rusqlite::Connection::open(root.join("library.db")).unwrap();

    storage_lock.execute_batch("BEGIN IMMEDIATE").unwrap();
    app.undo_document_history(&ctx);

    assert_eq!(app.status, "Undid Added textbox");
    assert!(app.canvas.annotation_for_id(annotation.id).is_none());
    let archived_before_flush: i64 = storage_lock
        .query_row(
            "SELECT archived FROM canvas_annotations WHERE id = ?1",
            [annotation.id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(archived_before_flush, 0);

    storage_lock.execute_batch("COMMIT").unwrap();
    app.flush_persistence();
    assert!(library.load_canvas_annotations().unwrap().is_empty());

    storage_lock.execute_batch("BEGIN IMMEDIATE").unwrap();
    app.redo_document_history(&ctx);

    assert_eq!(app.status, "Redid Added textbox");
    assert_eq!(
        app.canvas.annotation_for_id(annotation.id),
        Some(annotation.clone())
    );
    let archived_before_flush: i64 = storage_lock
        .query_row(
            "SELECT archived FROM canvas_annotations WHERE id = ?1",
            [annotation.id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(archived_before_flush, 1);

    storage_lock.execute_batch("COMMIT").unwrap();
    app.flush_persistence();
    assert_eq!(library.load_canvas_annotations().unwrap(), vec![annotation]);
}

#[test]
fn crop_can_be_undone_once_and_redone_with_its_asset_intact() {
    let ctx = egui::Context::default();
    let root = TempRoot::new("snappyref-crop-document-history");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let source_path = library
        .store_rgba_image("source", &[255; 2 * 2 * 4], 2, 2)
        .unwrap();
    let loading = library
        .persist_committed_loading_placement(
            egui::Pos2::ZERO,
            1.0,
            2.0,
            2.0,
            crate::data::ContentResolutionSource::ExternalPath {
                path: source_path.to_string_lossy().into_owned(),
            },
            Some("Loading".to_owned()),
        )
        .unwrap();
    let source = library
        .persist_imported_image_resolving_placement(
            library.load_user_session().unwrap().active_board_id,
            loading.state.id,
            &source_path,
            "image/png",
            2,
            2,
            2 * 2 * 4,
            egui::Pos2::ZERO,
            1.0,
        )
        .unwrap();
    let source_asset_id = source.require_loaded_asset().id;
    library
        .upsert_asset_embedding_blob(source_asset_id, "crop-test", &[1.0, 0.0])
        .unwrap();
    assert!(
        library
            .next_embedding_backfill_job_for_model("crop-test", 0)
            .unwrap()
            .is_none()
    );
    let cropped_path = library
        .store_cropped_rgba_image("source-crop", &[0, 255, 0, 255], 1, 1)
        .unwrap();
    let cropped_file_size = fs::metadata(&cropped_path).unwrap().len();
    let mut app = test_app(Some(library.clone()));
    app.board_editing.persistence = crate::app::DocumentPersistence::queued(library.clone());
    app.canvas.add_prepared_placement_image(
        &ctx,
        PreparedPlacementImage {
            target_board_id: app.active_board_id,
            preview_rgba: vec![255; 2 * 2 * 4],
            preview_width: 2,
            preview_height: 2,
            ..loaded_record(source.clone())
        },
    );
    app.canvas
        .warm_cached_pixels_for_item(&library, source.state.id)
        .unwrap();

    app.integrate_cropped_import_result(
        &ctx,
        crate::image_preparation::PreparedCropPreview {
            placement_id: source.state.id,
            stored_path: cropped_path.clone(),
            width: 1,
            height: 1,
            file_size: cropped_file_size,
            preview_rgba: vec![0, 255, 0, 255],
            preview_width: 1,
            preview_height: 1,
            preview_texture: crate::image_preparation::ImagePrepPreviewTexture::Cpu,
            loaded_lod: crate::data::CanvasImageLod::Working,
            crop_offset_local: egui::vec2(0.5, 0.5),
        },
    );
    let cropped_asset_id = app
        .canvas
        .placement_for_id(source.state.id)
        .unwrap()
        .require_loaded_asset()
        .id;
    assert_ne!(cropped_asset_id, source_asset_id);
    app.flush_persistence();
    assert_eq!(
        library
            .next_embedding_backfill_job_for_model("crop-test", 0)
            .unwrap()
            .unwrap()
            .asset_id,
        cropped_asset_id
    );
    let stored_after_crop = &library.load_placements().unwrap()[0];
    assert_eq!(
        stored_after_crop.require_loaded_asset().id,
        cropped_asset_id
    );

    app.undo_document_history(&ctx);
    app.flush_persistence();
    let status_after_undo = app.status.clone();
    assert_eq!(status_after_undo, "Undid Crop image");
    assert_eq!(
        app.canvas
            .placement_for_id(source.state.id)
            .unwrap()
            .require_loaded_asset()
            .id,
        source_asset_id
    );
    assert_eq!(
        app.canvas
            .preview_for_placement_id(source.state.id)
            .unwrap()
            .texture_size,
        egui::vec2(2.0, 2.0)
    );
    app.flush_persistence();
    let stored_after_undo = &library.load_placements().unwrap()[0];
    assert_eq!(stored_after_undo.require_loaded_asset().id, source_asset_id);

    app.undo_document_history(&ctx);
    assert_eq!(app.status, status_after_undo);
    assert_eq!(
        app.canvas
            .placement_for_id(source.state.id)
            .unwrap()
            .require_loaded_asset()
            .id,
        source_asset_id
    );

    app.redo_document_history(&ctx);
    app.flush_persistence();

    assert_eq!(app.status, "Redid Crop image");
    assert_eq!(
        app.canvas
            .placement_for_id(source.state.id)
            .unwrap()
            .require_loaded_asset()
            .id,
        cropped_asset_id
    );
    assert_eq!(
        app.canvas
            .preview_for_placement_id(source.state.id)
            .unwrap()
            .texture_size,
        egui::vec2(1.0, 1.0)
    );
    app.flush_persistence();
    let stored = &library.load_placements().unwrap()[0];
    assert_eq!(stored.require_loaded_asset().id, cropped_asset_id);
    assert_eq!(
        library.load_asset(cropped_asset_id).unwrap().id,
        cropped_asset_id
    );
    assert!(cropped_path.exists());
}

#[test]
fn paste_keeps_stack_order_and_places_group_at_front() {
    use StackContentKind::{Annotation, Placement};
    assert_copy_keeps_stack_order(
        [Placement, Annotation, Placement, Placement],
        paste_selection,
    );
    assert_copy_keeps_stack_order([Placement; 4], paste_selection);
    assert_copy_keeps_stack_order([Annotation; 4], paste_selection);
}

#[test]
fn duplicate_keeps_stack_order_and_places_group_at_front() {
    use StackContentKind::{Annotation, Placement};
    assert_copy_keeps_stack_order([Placement; 4], SnappyRefApp::duplicate_selection_with_gap);
    assert_copy_keeps_stack_order([Annotation; 4], SnappyRefApp::duplicate_selection_with_gap);
}

#[test]
fn undo_after_persisted_duplicate_removes_the_created_placement() {
    let ctx = egui::Context::default();
    let root = TempRoot::new("snappyref-duplicate-undo-durable");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let image_path = library
        .store_rgba_image("source", &[255, 0, 0, 255], 1, 1)
        .unwrap();
    let source_item = library
        .persist_imported_image(
            &image_path,
            "image/png",
            1,
            1,
            4,
            egui::pos2(10.0, 20.0),
            1.0,
        )
        .unwrap();
    let mut app = test_app(Some(library.clone()));
    app.board_editing.persistence = crate::app::DocumentPersistence::queued(library.clone());
    app.canvas
        .add_prepared_placement_image(&ctx, loaded_record(source_item.clone()));
    app.canvas
        .set_canvas_stack_order(vec![CanvasStackItem::Placement(source_item.state.id)]);
    app.canvas.select_placements(&[source_item.state.id]);

    app.duplicate_selection_with_gap();
    app.flush_persistence();
    assert_eq!(library.load_placements().unwrap().len(), 2);

    app.undo_document_history(&ctx);
    app.flush_persistence();

    let items = library.load_placements().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].state.id, source_item.state.id);
    assert_eq!(
        app.canvas
            .committed_placements()
            .iter()
            .map(|placement| placement.state.id)
            .collect::<Vec<_>>(),
        vec![source_item.state.id]
    );
    app.flush_persistence();
    assert_eq!(
        library.load_canvas_stack_order().unwrap(),
        vec![CanvasStackItem::Placement(source_item.state.id)]
    );
}

#[test]
fn qwe_arrange_shortcuts_apply_selected_image_layouts() {
    let ctx = egui::Context::default();
    for (key, expected_status) in [
        (egui::Key::Q, "Compacted 2 images"),
        (egui::Key::W, "Arranged 2 images in a row"),
        (egui::Key::E, "Arranged 2 images in a column"),
    ] {
        let mut app = test_app(None);
        for id in [1_i64, 2] {
            app.canvas
                .add_prepared_placement_image(&ctx, test_loaded_item(id, 1));
        }
        app.canvas.select_placements(&[1, 2]);

        let _ = ctx.run_ui(
            egui::RawInput {
                events: vec![egui::Event::Key {
                    key,
                    physical_key: Some(key),
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::default(),
                }],
                ..Default::default()
            },
            |ui| {
                app.handle_selection_shortcuts(
                    ui.ctx(),
                    egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(960.0, 640.0)),
                )
            },
        );

        assert_eq!(app.status, expected_status);
    }
}

#[test]
fn arrow_keys_apply_directional_settle_arrange() {
    let ctx = egui::Context::default();
    let mut app = test_app(None);
    app.settings.placement_gap = 20.0;

    for (id, x, y) in [(1_i64, 10.0, 50.0), (2, 170.0, 50.0), (3, 50.0, 160.0)] {
        let mut loaded = test_loaded_item(id, 1);
        loaded.placement.require_loaded_asset_mut().width = 100;
        loaded.placement.require_loaded_asset_mut().height = 40;
        loaded.placement.state.geometry.size = egui::vec2(100.0, 40.0);
        loaded.placement.state.geometry.center.x = x;
        loaded.placement.state.geometry.center.y = y;
        app.canvas.add_prepared_placement_image(&ctx, loaded);
    }
    app.canvas.select_placements(&[1, 2, 3]);

    let _ = ctx.run_ui(
        egui::RawInput {
            events: vec![egui::Event::Key {
                key: egui::Key::ArrowRight,
                physical_key: Some(egui::Key::ArrowRight),
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
            ..Default::default()
        },
        |ui| {
            app.handle_selection_shortcuts(
                ui.ctx(),
                egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(960.0, 640.0)),
            )
        },
    );

    let first = app.canvas.placement_for_id(1).unwrap();
    let second = app.canvas.placement_for_id(2).unwrap();
    let third = app.canvas.placement_for_id(3).unwrap();
    assert_eq!(second.state.geometry.center.x + 50.0, 220.0);
    assert_eq!(
        first.state.geometry.center.x + 50.0 + app.settings.placement_gap,
        second.state.geometry.center.x - 50.0
    );
    assert_eq!(third.state.geometry.center.x + 50.0, 220.0);
    assert_eq!(app.status, "Settled 2 images right");
}

#[test]
fn shortcuts_list_includes_directional_settle_arrange() {
    assert!(LAYOUT_AND_HISTORY_SHORTCUTS.iter().any(|shortcut| {
        shortcut.action == "Directional settle arrange" && shortcut.keys == "Arrow keys"
    }));
}

#[test]
fn always_on_top_shortcut_toggles_workspace_setting() {
    let ctx = egui::Context::default();
    let mut app = test_app(None);

    let output = ctx.run_ui(
        egui::RawInput {
            modifiers: egui::Modifiers {
                command: true,
                shift: true,
                ..Default::default()
            },
            events: vec![egui::Event::Key {
                key: egui::Key::T,
                physical_key: Some(egui::Key::T),
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers {
                    command: true,
                    shift: true,
                    ..Default::default()
                },
            }],
            ..Default::default()
        },
        |ui| {
            app.handle_selection_shortcuts(
                ui.ctx(),
                egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(960.0, 640.0)),
            )
        },
    );

    assert!(app.settings.always_on_top);
    let commands = &output
        .viewport_output
        .get(&egui::ViewportId::ROOT)
        .expect("root viewport output should exist")
        .commands;
    assert!(commands.iter().any(|command| {
        matches!(
            command,
            egui::ViewportCommand::WindowLevel(egui::viewport::WindowLevel::AlwaysOnTop)
        )
    }));
}

#[test]
fn copy_selection_payload_preserves_internal_json_for_mixed_selection() {
    let ctx = egui::Context::default();
    let mut app = test_app(None);
    app.canvas
        .add_prepared_placement_image(&ctx, test_loaded_item(1, 1));
    app.canvas
        .add_annotation(crate::data::CanvasAnnotation::new(
            20,
            CanvasAnnotationKind::Textbox,
            eframe::egui::pos2(10.0, 10.0),
            eframe::egui::pos2(70.0, 40.0),
            "caption".to_owned(),
            CanvasTextSize::M,
            CanvasStrokeWidth::M,
            crate::data::CanvasAnnotationColor::Default,
            false,
            false,
        ));
    app.canvas.select_content(&[1], &[20]);

    let copy_payload =
        crate::sources::clipboard::build_selection_copy_payload(&app.canvas, app.library.as_ref())
            .unwrap()
            .unwrap();
    assert!(copy_payload.external_image.is_none());
    let payload = serde_json::from_str::<crate::sources::clipboard::ClipboardBoardSelection>(
        &copy_payload.internal_json,
    )
    .unwrap();
    assert_eq!(payload.kind, crate::sources::clipboard::COPY_PAYLOAD_KIND);
    assert_eq!(payload.placements.len(), 1);
    assert_eq!(payload.annotations.len(), 1);
    assert_eq!(
        payload
            .stack_order
            .iter()
            .map(|entry| (entry.kind.as_str(), entry.index))
            .collect::<Vec<_>>(),
        vec![("placement", 0), ("annotation", 0)]
    );
}

#[test]
fn context_menu_items_reflect_mixed_selection_capabilities() {
    let ctx = egui::Context::default();
    let mut app = test_app(None);
    app.canvas
        .add_prepared_placement_image(&ctx, test_loaded_item(1, 1));
    app.canvas
        .add_annotation(crate::data::CanvasAnnotation::new(
            20,
            CanvasAnnotationKind::Textbox,
            eframe::egui::pos2(10.0, 10.0),
            eframe::egui::pos2(70.0, 40.0),
            "caption".to_owned(),
            CanvasTextSize::M,
            CanvasStrokeWidth::M,
            crate::data::CanvasAnnotationColor::Default,
            false,
            false,
        ));
    app.canvas.select_content(&[1], &[20]);

    let items = app.context_menu_items();

    assert_eq!(
        items.iter().map(|item| item.label).collect::<Vec<_>>(),
        vec![
            "Group",
            "Ungroup",
            "Add to group",
            "Copy",
            "Paste",
            "Duplicate",
            "Delete (from board)",
            "Move to Front",
            "Move to Back",
            "Search using image",
            "Remove from Library..."
        ]
    );
    assert_eq!(
        items
            .iter()
            .map(|item| (item.action, item.shortcut))
            .collect::<Vec<_>>(),
        vec![
            (
                crate::app::shortcuts::ContextMenuAction::Group,
                Some(crate::app::platform_shortcut_label("⌘G", "Ctrl+G"))
            ),
            (
                crate::app::shortcuts::ContextMenuAction::Ungroup,
                Some(crate::app::platform_shortcut_label("⌘⇧G", "Ctrl+Shift+G"))
            ),
            (
                crate::app::shortcuts::ContextMenuAction::AddToGroup,
                Some("G")
            ),
            (
                crate::app::shortcuts::ContextMenuAction::Copy,
                Some(crate::app::platform_shortcut_label("⌘ C", "Ctrl C")),
            ),
            (
                crate::app::shortcuts::ContextMenuAction::Paste,
                Some(crate::app::platform_shortcut_label("⌘ V", "Ctrl V")),
            ),
            (
                crate::app::shortcuts::ContextMenuAction::Duplicate,
                Some(crate::app::platform_shortcut_label("⌘ D", "Ctrl D")),
            ),
            (
                crate::app::shortcuts::ContextMenuAction::DeleteFromBoard,
                Some("del")
            ),
            (
                crate::app::shortcuts::ContextMenuAction::MoveToFront,
                Some(crate::app::platform_shortcut_label("⌘ .", "Ctrl .")),
            ),
            (
                crate::app::shortcuts::ContextMenuAction::MoveToBack,
                Some(crate::app::platform_shortcut_label("⌘ ,", "Ctrl ,")),
            ),
            (
                crate::app::shortcuts::ContextMenuAction::SearchUsingImage,
                Some(crate::app::platform_shortcut_label("⌘ F", "Ctrl F")),
            ),
            (
                crate::app::shortcuts::ContextMenuAction::RemoveFromLibrary,
                None
            ),
        ]
    );
    assert!(item_enabled(
        &items,
        crate::app::shortcuts::ContextMenuAction::Copy
    ));
    assert!(item_enabled(
        &items,
        crate::app::shortcuts::ContextMenuAction::Paste
    ));
    assert!(item_enabled(
        &items,
        crate::app::shortcuts::ContextMenuAction::Duplicate
    ));
    assert!(item_enabled(
        &items,
        crate::app::shortcuts::ContextMenuAction::DeleteFromBoard
    ));
    assert!(item_enabled(
        &items,
        crate::app::shortcuts::ContextMenuAction::MoveToFront
    ));
    assert!(item_enabled(
        &items,
        crate::app::shortcuts::ContextMenuAction::MoveToBack
    ));
    assert!(!item_enabled(
        &items,
        crate::app::shortcuts::ContextMenuAction::SearchUsingImage
    ));
    assert!(item_enabled(
        &items,
        crate::app::shortcuts::ContextMenuAction::RemoveFromLibrary
    ));
}

#[test]
fn context_menu_primary_press_closes_menu_only_outside_it() {
    let menu_rect = egui::Rect::from_min_size(egui::pos2(100.0, 120.0), egui::vec2(190.0, 240.0));
    let should_close = |pointer_pos: egui::Pos2| {
        crate::app::context_menu::context_menu_should_close(
            false,
            false,
            menu_rect,
            Some(pointer_pos),
            true,
            false,
            false,
        )
    };

    assert!(!should_close(menu_rect.center()));
    assert!(should_close(egui::pos2(
        menu_rect.right() + 8.0,
        menu_rect.center().y
    )));
}

#[test]
fn open_context_menu_blocks_canvas_input_behind_menu() {
    let ctx = egui::Context::default();
    let mut app = test_app(None);

    assert_eq!(
        app.canvas_input_policy(&ctx),
        crate::canvas::CanvasInputPolicy::Full
    );

    app.context_menu_pos = Some(egui::pos2(520.0, 260.0));

    assert_eq!(
        app.canvas_input_policy(&ctx),
        crate::canvas::CanvasInputPolicy::Blocked
    );
}

#[test]
fn stack_order_context_actions_are_disabled_without_canvas_item_selection() {
    let app = test_app(None);

    let items = app.context_menu_items();

    assert!(!item_enabled(
        &items,
        crate::app::shortcuts::ContextMenuAction::MoveToFront
    ));
    assert!(!item_enabled(
        &items,
        crate::app::shortcuts::ContextMenuAction::MoveToBack
    ));
}

#[test]
fn stack_order_context_actions_are_enabled_for_image_and_annotation_selections() {
    let ctx = egui::Context::default();
    let mut app = test_app(None);
    app.canvas
        .add_prepared_placement_image(&ctx, test_loaded_item(1, 1));
    app.canvas
        .add_annotation(crate::data::CanvasAnnotation::new(
            20,
            CanvasAnnotationKind::Textbox,
            eframe::egui::pos2(10.0, 10.0),
            eframe::egui::pos2(70.0, 40.0),
            "caption".to_owned(),
            CanvasTextSize::M,
            CanvasStrokeWidth::M,
            crate::data::CanvasAnnotationColor::Default,
            false,
            false,
        ));

    app.canvas.select_content(&[1], &[]);
    let image_only = app.context_menu_items();
    assert!(item_enabled(
        &image_only,
        crate::app::shortcuts::ContextMenuAction::MoveToFront
    ));
    assert!(item_enabled(
        &image_only,
        crate::app::shortcuts::ContextMenuAction::MoveToBack
    ));

    app.canvas.select_content(&[], &[20]);
    let annotation_only = app.context_menu_items();
    assert!(item_enabled(
        &annotation_only,
        crate::app::shortcuts::ContextMenuAction::MoveToFront
    ));
    assert!(item_enabled(
        &annotation_only,
        crate::app::shortcuts::ContextMenuAction::MoveToBack
    ));
}

#[test]
fn remove_from_library_context_action_waits_for_confirmation() {
    let ctx = egui::Context::default();
    let root = TempRoot::new("snappyref-context-remove-confirm");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let image_path = library
        .store_rgba_image("source", &[255, 0, 0, 255], 1, 1)
        .unwrap();
    let source_item = library
        .persist_imported_image(
            &image_path,
            "image/png",
            1,
            1,
            4,
            egui::pos2(10.0, 20.0),
            1.0,
        )
        .unwrap();
    let mut app = test_app(Some(library.clone()));
    app.canvas
        .add_prepared_placement_image(&ctx, loaded_record(source_item.clone()));
    app.canvas.select_placements(&[source_item.state.id]);

    app.handle_context_menu_action(
        crate::app::shortcuts::ContextMenuAction::RemoveFromLibrary,
        &ctx,
        egui::Rect::EVERYTHING,
    );

    assert!(app.pending_library_remove.is_some());
    app.flush_persistence();
    assert_eq!(library.load_placements().unwrap().len(), 1);

    app.confirm_pending_library_remove();

    assert!(app.pending_library_remove.is_none());
    app.flush_persistence();
    assert!(library.load_placements().unwrap().is_empty());
    assert!(source_item.require_loaded_asset().original_path.exists());
}

#[test]
fn move_to_front_context_action_persists_canvas_stack_order() {
    let ctx = egui::Context::default();
    let root = TempRoot::new("snappyref-context-move-front");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let first_path = library
        .store_rgba_image("first", &[255, 0, 0, 255], 1, 1)
        .unwrap();
    let second_path = library
        .store_rgba_image("second", &[0, 255, 0, 255], 1, 1)
        .unwrap();
    let first = library
        .persist_imported_image(&first_path, "image/png", 1, 1, 4, egui::pos2(0.0, 0.0), 1.0)
        .unwrap();
    let second = library
        .persist_imported_image(
            &second_path,
            "image/png",
            1,
            1,
            4,
            egui::pos2(10.0, 0.0),
            1.0,
        )
        .unwrap();
    let annotation = library
        .create_canvas_annotation(
            CanvasAnnotationKind::Textbox,
            egui::pos2(10.0, 12.0),
            egui::pos2(110.0, 72.0),
            "hello",
            CanvasTextSize::default(),
            CanvasStrokeWidth::default(),
            crate::data::CanvasAnnotationColor::default(),
            false,
        )
        .unwrap();
    let mut app = test_app(Some(library.clone()));
    app.canvas
        .add_prepared_placement_image(&ctx, test_loaded_item(first.state.id, 1));
    app.canvas
        .add_prepared_placement_image(&ctx, test_loaded_item(second.state.id, 1));
    app.canvas.add_annotation(annotation.clone());
    app.canvas.set_canvas_stack_order(vec![
        crate::data::CanvasStackItem::Placement(first.state.id),
        crate::data::CanvasStackItem::Annotation(annotation.id),
        crate::data::CanvasStackItem::Placement(second.state.id),
    ]);
    app.canvas
        .select_content(&[first.state.id], &[annotation.id]);

    app.handle_context_menu_action(
        crate::app::shortcuts::ContextMenuAction::MoveToFront,
        &ctx,
        egui::Rect::EVERYTHING,
    );

    app.flush_persistence();
    assert_eq!(
        library.load_canvas_stack_order().unwrap(),
        vec![
            crate::data::CanvasStackItem::Placement(second.state.id),
            crate::data::CanvasStackItem::Placement(first.state.id),
            crate::data::CanvasStackItem::Annotation(annotation.id),
        ]
    );

    app.handle_context_menu_action(
        crate::app::shortcuts::ContextMenuAction::MoveToBack,
        &ctx,
        egui::Rect::EVERYTHING,
    );

    app.flush_persistence();
    assert_eq!(
        library.load_canvas_stack_order().unwrap(),
        vec![
            crate::data::CanvasStackItem::Placement(first.state.id),
            crate::data::CanvasStackItem::Annotation(annotation.id),
            crate::data::CanvasStackItem::Placement(second.state.id),
        ]
    );
}

#[test]
fn create_undo_persists_reassigned_redo_history_with_the_archive() {
    let ctx = egui::Context::default();
    let root = TempRoot::new("snappyref-create-undo-atomic-history");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let original = library.list_boards().unwrap()[0].clone();

    let mut app = test_app(Some(library.clone()));
    app.active_board_id = original.id;
    app.board_editing.persistence = crate::app::DocumentPersistence::queued(library.clone());
    app.create_and_switch_board(false);
    app.flush_persistence();
    let created = library
        .list_boards()
        .unwrap()
        .into_iter()
        .find(|board| board.id != original.id)
        .unwrap();
    assert_eq!(app.active_board_id, created.id);

    let trigger_connection = rusqlite::Connection::open(root.join("library.db")).unwrap();
    trigger_connection
        .execute_batch(&format!(
            "CREATE TRIGGER fail_reassigned_history
             BEFORE UPDATE ON board_document_history
             WHEN json_array_length(json_extract(
                 NEW.payload,
                 '$.histories_by_board.\"{}\".redo_stack'
             )) > 0
             BEGIN
                 SELECT RAISE(ABORT, 'injected reassigned-history failure');
             END;",
            original.id
        ))
        .unwrap();

    app.undo_document_history(&ctx);
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(
        trigger_connection
            .query_row(
                "SELECT archived FROM boards WHERE id = ?1",
                [created.id],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        0,
        "a failed history write must roll back the Board archive"
    );
    assert!(
        library
            .load_document_history()
            .unwrap()
            .redo(original.id)
            .is_none(),
        "a failed Board archive must leave the prior durable history unchanged"
    );

    trigger_connection
        .execute_batch("DROP TRIGGER fail_reassigned_history")
        .unwrap();
    app.flush_persistence();
    assert_eq!(
        trigger_connection
            .query_row(
                "SELECT archived FROM boards WHERE id = ?1",
                [created.id],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        1
    );
    assert!(
        library
            .load_document_history()
            .unwrap()
            .redo(original.id)
            .is_some(),
        "the retried archive and reassigned Redo entry must commit together"
    );
    drop(app);
}

#[test]
fn update_action_check_requests_repaint() {
    let mut app = test_app(None);
    let ctx = egui::Context::default();
    let repaint_requests = Arc::new(AtomicUsize::new(0));
    let repaint_requests_for_callback = repaint_requests.clone();
    ctx.set_request_repaint_callback(move |_| {
        repaint_requests_for_callback.fetch_add(1, Ordering::SeqCst);
    });
    let baseline_repaints = repaint_requests.load(Ordering::SeqCst);

    app.handle_update_action_click(&ctx);

    assert!(
        app.last_manual_update_check.is_some(),
        "settings click must queue a manual update check"
    );
    assert!(
        repaint_requests.load(Ordering::SeqCst) > baseline_repaints,
        "manual update check must request a repaint so the queued check runs next frame"
    );
}

#[test]
fn clicked_multi_select_enter_places_all_selected_library_results() {
    let root = TempRoot::new("snappyref-library-search-click-multi-enter-drop");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let first_path = library
        .store_rgba_image("first", &vec![255; 10 * 10 * 4], 10, 10)
        .unwrap();
    let second_path = library
        .store_rgba_image("second", &vec![180; 20 * 10 * 4], 20, 10)
        .unwrap();
    let first = library
        .persist_imported_image(
            &first_path,
            "image/png",
            10,
            10,
            4,
            egui::pos2(0.0, 0.0),
            1.0,
        )
        .unwrap();
    let second = library
        .persist_imported_image(
            &second_path,
            "image/png",
            20,
            10,
            4,
            egui::pos2(20.0, 0.0),
            1.0,
        )
        .unwrap();
    library
        .archive_placements(&[first.state.id, second.state.id])
        .unwrap();
    let empty_board = library.create_board("Empty").unwrap();
    library.set_active_board(empty_board.id).unwrap();
    let mut app = test_app(Some(library.clone()));
    app.active_board_id = empty_board.id;
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

    harness.event_modifiers(
        egui::Event::PointerButton {
            pos: egui::pos2(472.0, 206.0),
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::CTRL,
        },
        egui::Modifiers::CTRL,
    );
    harness.event_modifiers(
        egui::Event::PointerButton {
            pos: egui::pos2(472.0, 206.0),
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::CTRL,
        },
        egui::Modifiers::CTRL,
    );
    harness.step();
    assert_eq!(
        harness
            .state()
            .search
            .overlay
            .result_selection
            .selected()
            .len(),
        2
    );

    harness.key_press(egui::Key::Enter);
    harness.step();
    assert_eq!(
        harness
            .state()
            .canvas
            .provisional_placements_in_selection_order()
            .len(),
        2
    );

    harness.hover_at(egui::pos2(900.0, 600.0));
    harness.drag_at(egui::pos2(900.0, 600.0));
    harness.drop_at(egui::pos2(900.0, 600.0));
    harness.step();

    // The drop commits in memory first; wait for its queued write before reading SQLite.
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
}

#[test]
fn context_menu_move_to_front_routes_through_rendered_menu_row() {
    let ctx = egui::Context::default();
    let root = TempRoot::new("snappyref-context-menu-move-front");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let image_path = library
        .store_rgba_image("source", &[255, 0, 0, 255], 1, 1)
        .unwrap();
    let back_item = library
        .persist_imported_image(&image_path, "image/png", 1, 1, 4, egui::pos2(0.0, 0.0), 1.0)
        .unwrap();
    let selected_item = library
        .create_placement_from_asset(
            back_item.require_loaded_asset().id,
            egui::pos2(10.0, 20.0),
            1.0,
            0.0,
            0,
        )
        .unwrap();
    let front_annotation = library
        .create_canvas_annotation(
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
    add_loaded_record(&mut app, &ctx, back_item.clone());
    add_loaded_record(&mut app, &ctx, selected_item.clone());
    app.canvas.add_annotation(front_annotation.clone());
    app.canvas.set_canvas_stack_order(vec![
        CanvasStackItem::Placement(back_item.state.id),
        CanvasStackItem::Placement(selected_item.state.id),
        CanvasStackItem::Annotation(front_annotation.id),
    ]);
    app.canvas.select_placements(&[selected_item.state.id]);
    app.context_menu_pos = Some(egui::pos2(300.0, 300.0));
    let mut harness = app_harness(app, |ctx, app| {
        app.show_context_menu(ctx, test_screen_rect(), false);
    });

    harness.get_by_label("Move to Front").click();
    harness.run();

    assert_eq!(
        harness.state().canvas.canvas_stack_order(),
        vec![
            CanvasStackItem::Placement(back_item.state.id),
            CanvasStackItem::Annotation(front_annotation.id),
            CanvasStackItem::Placement(selected_item.state.id),
        ]
    );
    harness.state_mut().flush_persistence();
    assert_eq!(
        library.load_canvas_stack_order().unwrap(),
        harness.state().canvas.canvas_stack_order()
    );
}

#[test]
fn accepted_loading_batch_is_one_history_action_despite_out_of_order_results() {
    let ctx = egui::Context::default();
    let root = TempRoot::new("snappyref-accepted-loading-batch-history");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let mut app = test_app(Some(library.clone()));
    app.board_editing.persistence = crate::app::DocumentPersistence::queued(library.clone());
    app.import_worker = Some(crate::image_preparation::ImagePrepWorker::new(
        library.clone(),
        ctx.clone(),
        None,
    ));
    app.enqueue_import_sources(
        (1..=3)
            .map(|index| {
                crate::image_preparation::ImportSource::Path(
                    root.join(format!("missing-{index}.png")),
                )
            })
            .collect(),
        test_screen_rect(),
        egui::pos2(120.0, 80.0),
    );
    let placement_ids = app.canvas.selected_placement_ids();
    assert_eq!(placement_ids.len(), 3);

    let mut loaded = persisted_loaded_item(&library, placement_ids[2], app.active_board_id);
    loaded.queue_id = Some(3);
    loaded.origin = crate::image_preparation::ImportOrigin::Path;
    let loaded_result = crate::image_preparation::ImagePrepResult::Loaded(loaded);
    app.accept_preparation_result(&ctx, loaded_result);

    let failed_result = crate::image_preparation::ImagePrepResult::Failed {
        target_board_id: Some(app.active_board_id),
        queue_id: Some(1),
        placement_id: Some(placement_ids[0]),
        context: "missing-1.png".to_owned(),
        error: "decode failed".to_owned(),
    };
    app.accept_preparation_result(&ctx, failed_result);

    app.undo_document_history(&ctx);
    let undo_status = app.status.clone();
    assert_eq!(undo_status, "Undid Import images");
    assert!(
        placement_ids
            .iter()
            .all(|placement_id| app.canvas.placement_for_id(*placement_id).is_none())
    );

    // Storage already rejects a load for an undone Placement, so the late
    // result never reaches the Library; model it in memory only.
    let mut late_loaded = test_loaded_item(placement_ids[1], app.active_board_id);
    late_loaded.queue_id = Some(2);
    late_loaded.origin = crate::image_preparation::ImportOrigin::Path;
    let late_result = crate::image_preparation::ImagePrepResult::Loaded(late_loaded);
    app.accept_preparation_result(&ctx, late_result);
    assert!(
        placement_ids
            .iter()
            .all(|placement_id| app.canvas.placement_for_id(*placement_id).is_none())
    );

    app.undo_document_history(&ctx);
    assert_eq!(app.status, undo_status);
    app.flush_persistence();
    assert!(library.load_placements().unwrap().is_empty());
}

#[test]
fn url_batch_previews_use_balanced_layout_for_approximate_dimensions() {
    let mut app = test_app(None);
    let sources = (0..13)
        .map(|index| {
            crate::image_preparation::ImportSource::Url(format!(
                "https://example.com/image-{index}.png"
            ))
        })
        .collect::<Vec<_>>();

    let intents = sources
        .iter()
        .map(|source| {
            crate::placement::creation::ProvisionalSourceIntent::SourceIntent(
                crate::placement::content_resolution::PlacementCreation::source_intent(source),
            )
        })
        .collect();
    let settings = app.placement_creation_settings();
    let batch = crate::placement::lifecycle::start_provisional_batch(
        &mut app.canvas,
        intents,
        egui::pos2(100.0, 200.0),
        test_screen_rect(),
        settings,
    );
    let previews = batch
        .placement_ids
        .iter()
        .map(|id| {
            app.canvas
                .placement_for_id(*id)
                .unwrap()
                .state
                .geometry
                .center
        })
        .collect::<Vec<_>>();
    let distinct_x = previews
        .iter()
        .fold(Vec::<i32>::new(), |mut values, center| {
            let rounded = center.x.round() as i32;
            if !values.contains(&rounded) {
                values.push(rounded);
            }
            values
        });
    let distinct_y = previews
        .iter()
        .fold(Vec::<i32>::new(), |mut values, center| {
            let rounded = center.y.round() as i32;
            if !values.contains(&rounded) {
                values.push(rounded);
            }
            values
        });

    assert_eq!(previews.len(), sources.len());
    assert!(distinct_x.len() > 1);
    assert!(distinct_y.len() > 1);
}
