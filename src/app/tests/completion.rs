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
    let id = reservation.placement_id.unwrap();
    let mut loaded = persisted_loaded_item(&library, id, app.active_board_id);
    loaded.queue_id = Some(reservation.queue_id);
    loaded.origin = crate::image_preparation::ImportOrigin::Path;
    loaded.trace = None;
    loaded.existing_load_diagnostics = None;
    let asset = loaded.placement.asset_id();
    app.accept_preparation_result(
        &ctx,
        crate::image_preparation::ImagePrepResult::Loaded(loaded),
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
fn cancelled_reservation_rejects_its_late_result() {
    let ctx = egui::Context::default();
    let mut app = test_app(None);
    let reservation =
        app.placement_creation
            .reserve_placement(app.active_board_id, -1, egui::Pos2::ZERO);
    app.placement_creation.cancel(reservation);
    let mut loaded = test_loaded_item(-1, app.active_board_id);
    loaded.queue_id = Some(reservation.queue_id);
    app.accept_preparation_result(
        &ctx,
        crate::image_preparation::ImagePrepResult::Loaded(loaded),
    );
    assert!(app.canvas.placement_for_id(-1).is_none());
    assert_eq!(app.placement_creation.pending_len(), 0);
}

#[test]
fn batch_completion_persists_empty_textbox_cleanup_without_an_extra_history_entry() {
    let root = TempRoot::new("snappyref-batch-textbox-cleanup");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let ctx = egui::Context::default();
    let annotation = library
        .create_canvas_annotation(
            CanvasAnnotationKind::Textbox,
            egui::pos2(40.0, 60.0),
            egui::pos2(140.0, 100.0),
            "",
            CanvasTextSize::M,
            CanvasStrokeWidth::M,
            crate::data::CanvasAnnotationColor::Sky,
            true,
        )
        .unwrap();
    let mut app = test_app(Some(library.clone()));
    app.board_editing.persistence = crate::app::DocumentPersistence::queued(library.clone());
    app.canvas.add_annotation(annotation.clone());
    let mut prepared = Vec::new();
    for id in [1, 2] {
        let mut loaded = persisted_loaded_item(&library, id, app.active_board_id);
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
    app.register_test_batch(app.active_board_id, egui::Pos2::ZERO, vec![1, 2]);
    app.canvas.take_document_changes();
    app.canvas.begin_textbox_edit(annotation.id);
    for loaded in prepared {
        app.install_test_prepared_image(&ctx, loaded);
    }
    assert!(app.canvas.annotation_for_id(annotation.id).is_none());
    app.flush_persistence();
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
    let mut history = library.load_document_history().unwrap();
    assert_eq!(
        history.undo(app.active_board_id).unwrap().action.label(),
        "Import images"
    );
    assert!(
        history.undo(app.active_board_id).is_none(),
        "completion must retain one batch entry"
    );
}
