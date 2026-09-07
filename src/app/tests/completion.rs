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
