use super::*;

#[test]
fn preparation_without_diagnostics_updates_the_accepted_edit_and_survives_restart() {
    let root = TempRoot::new("snappyref-completion-no-diagnostics");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let ctx = egui::Context::default();
    let mut app = test_app(Some(library.clone()));
    let reservation = app
        .reserve_import(
            Some((1, 1)),
            egui::pos2(40.0, 70.0),
            crate::data::ContentResolutionSource::Unknown {
                description: "accepted image".to_owned(),
            },
        )
        .unwrap();
    let id = reservation.placement_id;
    let mut loaded = persisted_loaded_item(&library, id, app.active_board_id);
    loaded.queue_id = Some(reservation.queue_id);
    loaded.origin = crate::image_preparation::ImportOrigin::Path;
    loaded.trace = None;
    loaded.existing_load_diagnostics = None;
    let asset = loaded.placement.asset_id();
    app.accept_preparation_result(
        &ctx,
        crate::image_preparation::ImagePrepResult::Loaded(Box::new(loaded)),
    );
    assert_eq!(app.placement_creation.pending_len(), 0);
    assert_eq!(app.canvas.placement_for_id(id).unwrap().asset_id(), asset);
    app.flush_persistence();
    assert_eq!(library.load_placements().unwrap()[0].asset_id(), asset);
    let mut history = library.load_document_history().unwrap();
    let action = history
        .undo(app.active_board_id)
        .expect("the accepted import remains undoable");
    assert_eq!(action.action.label(), "Import image");
    assert!(
        history.undo(app.active_board_id).is_none(),
        "resolution adds no history entry"
    );
}

#[test]
fn batch_completion_persists_empty_textbox_cleanup_without_an_extra_history_entry() {
    let root = TempRoot::new("snappyref-batch-textbox-cleanup");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let ctx = egui::Context::default();
    let annotation = library
        .fixture_annotation(
            CanvasAnnotationKind::Textbox,
            egui::pos2(40.0, 60.0),
            egui::pos2(140.0, 100.0),
            "",
            CanvasTextSize::M,
            CanvasStrokeWidth::M,
            crate::data::CanvasAnnotationColor::Sky,
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
                after: Some(app.canvas.canvas_stack_order()),
            }),
        ),
    );
    let mut prepared = Vec::new();
    for id in [1, 2] {
        let path = library
            .store_rgba_image(&format!("batch-{id}"), &[id as u8, 0, 0, 255], 1, 1)
            .unwrap();
        let placement = library
            .fixture_placement(
                library.load_user_session().unwrap().active_board_id,
                crate::data::Placement::loaded(
                    crate::data::PlacementState::new(
                        library.allocate_document_id(),
                        egui::Pos2::ZERO,
                        1.0,
                        0.0,
                        0,
                        false,
                    ),
                    library
                        .register_original(&path, "image/png", 1, 1, 4, None)
                        .unwrap(),
                ),
            )
            .unwrap();
        let mut loaded = loaded_record(placement);
        loaded.queue_id = Some(id as u64);
        loaded.origin = crate::image_preparation::ImportOrigin::Path;
        prepared.push(loaded);
    }
    app.board_editing.history.record_applied(
        app.active_board_id,
        crate::app::HistoryAction::document(
            "Import images".to_owned(),
            prepared
                .iter()
                .map(|loaded| crate::app::HistoryChange {
                    before: None,
                    after: Some(loaded.placement.clone()),
                })
                .collect(),
            Vec::new(),
            None,
        ),
    );
    app.placement_creation
        .batches
        .register_multi_item_batch(app.active_board_id, vec![1, 2]);
    app.canvas.take_document_changes();
    app.canvas.begin_textbox_edit(annotation.id);
    for loaded in prepared {
        app.install_test_prepared_image(&ctx, loaded);
    }
    assert!(app.canvas.annotation_for_id(annotation.id).is_none());
    app.board_editing.persistence.flush().unwrap();
    assert!(
        library.load_canvas_annotations().unwrap().is_empty(),
        "the empty textbox must not return after restart"
    );
    let document = library.load_board_document(app.active_board_id).unwrap();
    assert!(
        !document
            .stack_order
            .contains(&crate::data::CanvasStackItem::Annotation(annotation.id))
    );
    let board_id = app.active_board_id;
    let other = library.fixture_board("Other").unwrap();
    app.switch_to_board(other.id);
    app.switch_to_board(board_id);
    assert!(
        app.canvas.annotation_for_id(annotation.id).is_none(),
        "Board reopening must not resurrect the empty textbox"
    );
    app.undo_document_history(&ctx);
    assert!(app.canvas.committed_placements().is_empty());
    assert!(app.canvas.annotation_for_id(annotation.id).is_none());
    app.redo_document_history(&ctx);
    assert_eq!(app.canvas.committed_placements().len(), 2);
    assert!(app.canvas.annotation_for_id(annotation.id).is_none());
    app.undo_document_history(&ctx);
    app.undo_document_history(&ctx);
    assert!(app.canvas.annotation_for_id(annotation.id).is_none());
    app.redo_document_history(&ctx);
    app.redo_document_history(&ctx);
    assert_eq!(app.canvas.committed_placements().len(), 2);
    assert!(app.canvas.annotation_for_id(annotation.id).is_none());
    app.board_editing.persistence.flush().unwrap();
    let mut restarted = test_app(Some(library.clone()));
    restarted.board_editing.history = library.load_document_history().unwrap();
    let accepted = restarted
        .board_editing
        .accepted_board_document(&library, board_id)
        .unwrap();
    assert!(accepted.annotations.is_empty());
    assert!(
        !accepted
            .stack_order
            .contains(&crate::data::CanvasStackItem::Annotation(annotation.id))
    );
    let mut history = library.load_document_history().unwrap();
    assert_eq!(
        history.undo(board_id).unwrap().action.label(),
        "Import images"
    );
    assert_eq!(
        history.undo(board_id).unwrap().action.label(),
        "Added textbox"
    );
    assert!(
        history.undo(app.active_board_id).is_none(),
        "completion must retain one batch entry"
    );
}

#[test]
fn first_load_fits_latest_placeholder_area_and_rejects_deleted_placement() {
    let mut cancelled_app = test_app(None);
    let cancelled = cancelled_app
        .placement_creation
        .reserve_placement(1, -1, egui::Pos2::ZERO);
    cancelled_app.placement_creation.cancel(cancelled);
    let mut late = test_loaded_item(-1, 1);
    late.queue_id = Some(cancelled.queue_id);
    cancelled_app.accept_preparation_result(
        &egui::Context::default(),
        crate::image_preparation::ImagePrepResult::Loaded(Box::new(late)),
    );
    assert!(cancelled_app.canvas.placement_for_id(-1).is_none());
    assert_eq!(cancelled_app.placement_creation.pending_len(), 0);

    let root = TempRoot::new("snappyref-first-load-latest-geometry");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let ctx = egui::Context::default();
    let mut app = test_app(Some(library.clone()));
    let reservation = app
        .reserve_import(
            Some((640, 480)),
            egui::pos2(12.0, 24.0),
            crate::data::ContentResolutionSource::Clipboard,
        )
        .unwrap();
    let id = reservation.placement_id;
    let mut loaded = persisted_loaded_item(&library, id, app.active_board_id);
    loaded.placement.require_loaded_asset_mut().width = 640;
    loaded.placement.require_loaded_asset_mut().height = 480;
    loaded.queue_id = Some(reservation.queue_id);
    app.canvas.select_placements(&[id]);
    app.canvas.move_selected_placement_group_to_center(
        egui::pos2(120.0, 80.0),
        test_viewport_world(),
        0.0,
    );
    let mut edited = app.canvas.placement_for_id(id).unwrap();
    edited.state.geometry.size = egui::vec2(320.0, 180.0);
    edited.state.geometry.scale = 1.5;
    edited.state.geometry.rotation = 0.2;
    let current = edited.state.geometry;
    app.canvas.add_unresolved_placement(edited);
    app.accept_preparation_result(
        &ctx,
        crate::image_preparation::ImagePrepResult::Loaded(Box::new(loaded)),
    );
    let resolved = app.canvas.placement_for_id(id).unwrap().state.geometry;
    assert_eq!(resolved.center, current.center);
    assert_eq!(resolved.scale, current.scale);
    assert!((resolved.size.x * resolved.size.y - current.size.x * current.size.y).abs() < 0.1);
    assert!((resolved.size.x / resolved.size.y - 640.0 / 480.0).abs() < 0.001);

    let reservation = app
        .reserve_import(
            Some((1, 1)),
            egui::Pos2::ZERO,
            crate::data::ContentResolutionSource::Clipboard,
        )
        .unwrap();
    let mut late = persisted_loaded_item(&library, reservation.placement_id, app.active_board_id);
    late.queue_id = Some(reservation.queue_id);
    app.canvas.delete_placements(&[reservation.placement_id]);
    app.accept_preparation_result(
        &ctx,
        crate::image_preparation::ImagePrepResult::Loaded(Box::new(late)),
    );
    assert!(
        app.canvas
            .placement_for_id(reservation.placement_id)
            .is_none()
    );
}
