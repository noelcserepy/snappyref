use super::*;

#[test]
fn board_unit_persists_failed_resolution_state_with_matching_history() {
    let root = TempRoot::new("snappyref-failed-resolution-board-unit-test");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let loading = library
        .fixture_placement(
            library.load_user_session().unwrap().active_board_id,
            library.new_committed_loading_placement(
                egui::pos2(10.0, 20.0),
                1.0,
                eframe::egui::vec2(100.0, 80.0),
                ContentResolutionSource::ExternalPath {
                    path: "/tmp/missing.png".to_owned(),
                },
                Some("Loading".to_owned()),
            ),
        )
        .unwrap();
    let mut history = crate::active_board::history::DocumentHistory::default();
    history.record_applied(
        1,
        crate::active_board::history::HistoryAction::document(
            "Added image".to_owned(),
            vec![crate::active_board::history::HistoryChange {
                before: None,
                after: Some(loading.clone()),
            }],
            Vec::new(),
            None,
        ),
    );
    let failed = history
        .fail_accepted_placement(1, loading.state.id, "decode failed".to_owned())
        .unwrap();

    library
        .persist_board_unit(
            &crate::library::storage::BoardDurabilityOperation::Document {
                board_id: 1,
                changes: crate::canvas::CanvasDocumentChanges {
                    updated_placements: vec![failed],
                    ..Default::default()
                },
            },
            &history,
        )
        .unwrap();

    let stored_history = library.load_document_history().unwrap();
    assert!(matches!(
        stored_history
            .accepted_placement(1, loading.state.id)
            .unwrap()
            .content,
        PlacementContent::Failed { .. }
    ));
    let stored = library.load_placements().unwrap();
    assert!(matches!(stored[0].content, PlacementContent::Failed { .. }));
}

#[test]
fn board_unit_persists_undone_placement_state_with_redo_history() {
    let root = TempRoot::new("snappyref-undone-placement-board-unit-test");
    let source = root.join("source.png");
    fs::write(&source, [255, 255, 255, 255]).unwrap();
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let original = library
        .fixture_placement(
            library.load_user_session().unwrap().active_board_id,
            crate::data::Placement::loaded(
                crate::data::PlacementState::new(
                    library.allocate_document_id(),
                    egui::pos2(3.0, 4.0),
                    1.0,
                    0.0,
                    0,
                    false,
                ),
                library
                    .register_original(&source, "image/png", 10, 20, 4, None)
                    .unwrap(),
            ),
        )
        .unwrap();
    let mut moved = original.clone();
    moved.state.geometry.center = egui::pos2(30.0, 40.0);
    let mut history = crate::active_board::history::DocumentHistory::default();
    history.record_applied(
        1,
        crate::active_board::history::HistoryAction::document(
            "Move image".to_owned(),
            vec![crate::active_board::history::HistoryChange {
                before: Some(original.clone()),
                after: Some(moved.clone()),
            }],
            Vec::new(),
            None,
        ),
    );
    library
        .persist_board_unit(
            &crate::library::storage::BoardDurabilityOperation::Document {
                board_id: 1,
                changes: crate::canvas::CanvasDocumentChanges {
                    updated_placements: vec![moved],
                    ..Default::default()
                },
            },
            &history,
        )
        .unwrap();

    history.undo(1).unwrap();
    library
        .persist_board_unit(
            &crate::library::storage::BoardDurabilityOperation::Document {
                board_id: 1,
                changes: crate::canvas::CanvasDocumentChanges {
                    updated_placements: vec![original.clone()],
                    ..Default::default()
                },
            },
            &history,
        )
        .unwrap();

    assert_eq!(
        library.load_placements().unwrap()[0].state.geometry.center,
        original.state.geometry.center
    );
    assert!(library.load_document_history().unwrap().redo(1).is_some());
}

#[test]
fn embedding_retryable_failures_get_one_immediate_and_one_delayed_retry() {
    let root = TempRoot::new("snappyref-embedding-retry-schedule-test");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let path = library
        .store_rgba_image("retry", &[255, 0, 0, 255], 1, 1)
        .unwrap();
    let placement = library
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
                    .register_original(&path, "image/png", 1, 1, 4, None)
                    .unwrap(),
            ),
        )
        .unwrap();

    library
        .record_retryable_asset_embedding_failure(
            placement.require_loaded_asset().id,
            "mobileclip_s0",
            768,
            "temporary failure",
            1_000,
        )
        .unwrap();
    assert!(
        library
            .next_embedding_backfill_job_for_model("mobileclip_s0", 1_000)
            .unwrap()
            .is_some()
    );
    assert_eq!(
        library.next_embedding_retry_at_for_model("mobileclip_s0"),
        Ok(Some(1_000))
    );

    library
        .record_retryable_asset_embedding_failure(
            placement.require_loaded_asset().id,
            "mobileclip_s0",
            768,
            "temporary failure again",
            1_010,
        )
        .unwrap();
    assert!(
        library
            .next_embedding_backfill_job_for_model("mobileclip_s0", 1_309)
            .unwrap()
            .is_none()
    );
    assert_eq!(
        library.next_embedding_retry_at_for_model("mobileclip_s0"),
        Ok(Some(1_310))
    );
    assert!(
        library
            .next_embedding_backfill_job_for_model("mobileclip_s0", 1_310)
            .unwrap()
            .is_some()
    );

    library
        .record_retryable_asset_embedding_failure(
            placement.require_loaded_asset().id,
            "mobileclip_s0",
            768,
            "temporary failure exhausted",
            1_310,
        )
        .unwrap();
    assert!(
        library
            .next_embedding_backfill_job_for_model("mobileclip_s0", 2_000)
            .unwrap()
            .is_none()
    );
    assert_eq!(
        library.next_embedding_retry_at_for_model("mobileclip_s0"),
        Ok(None)
    );
}

#[test]
fn pending_retry_preserves_embedding_failure_count_until_success() {
    let root = TempRoot::new("snappyref-pending-retry-count-test");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let path = library
        .store_rgba_image("retry", &[255, 0, 0, 255], 1, 1)
        .unwrap();
    let placement = library
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
                    .register_original(&path, "image/png", 1, 1, 4, None)
                    .unwrap(),
            ),
        )
        .unwrap();

    library
        .record_retryable_asset_embedding_failure(
            placement.require_loaded_asset().id,
            "mobileclip_s0",
            768,
            "temporary failure",
            1_000,
        )
        .unwrap();
    library
        .upsert_asset_embedding_status(
            placement.require_loaded_asset().id,
            "mobileclip_s0",
            768,
            EmbeddingStatus::Pending,
            None,
        )
        .unwrap();
    library
        .record_retryable_asset_embedding_failure(
            placement.require_loaded_asset().id,
            "mobileclip_s0",
            768,
            "temporary failure again",
            1_010,
        )
        .unwrap();

    assert!(
        library
            .next_embedding_backfill_job_for_model("mobileclip_s0", 1_309)
            .unwrap()
            .is_none()
    );
    assert!(
        library
            .next_embedding_backfill_job_for_model("mobileclip_s0", 1_310)
            .unwrap()
            .is_some()
    );

    library
        .upsert_asset_embedding_blob(
            placement.require_loaded_asset().id,
            "mobileclip_s0",
            &[1.0, 0.0],
        )
        .unwrap();
    assert!(
        library
            .next_embedding_backfill_job_for_model("mobileclip_s0", 2_000)
            .unwrap()
            .is_none()
    );
}

#[test]
fn terminal_embedding_failures_are_not_automatic_retry_jobs() {
    let root = TempRoot::new("snappyref-terminal-embedding-failure-test");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let path = library
        .store_rgba_image("terminal", &[255, 0, 0, 255], 1, 1)
        .unwrap();
    let placement = library
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
                    .register_original(&path, "image/png", 1, 1, 4, None)
                    .unwrap(),
            ),
        )
        .unwrap();

    library
        .record_terminal_asset_embedding_failure(
            placement.require_loaded_asset().id,
            "mobileclip_s0",
            768,
            "failed to open image",
        )
        .unwrap();

    assert!(
        library
            .next_embedding_backfill_job_for_model("mobileclip_s0", 10_000)
            .unwrap()
            .is_none()
    );
}

#[test]
fn interrupted_pending_embedding_is_backfilled_after_reopen() {
    let root = TempRoot::new("snappyref-interrupted-embedding");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let path = library
        .store_rgba_image("pending", &[255, 0, 0, 255], 1, 1)
        .unwrap();
    let asset = library
        .register_original(&path, "image/png", 1, 1, 4, None)
        .unwrap();
    library
        .upsert_asset_embedding_status(asset.id, "model", 2, EmbeddingStatus::Pending, None)
        .unwrap();
    assert!(
        library
            .next_embedding_backfill_job_for_model("model", i64::MAX)
            .unwrap()
            .is_none()
    );
    drop(library);

    let reopened = Library::initialize(root.to_path_buf()).unwrap();
    let job = reopened
        .next_embedding_backfill_job_for_model("model", i64::MAX)
        .unwrap()
        .unwrap();
    assert_eq!(job.asset_id, asset.id);
    assert_eq!(job.original_path, path);
    reopened
        .upsert_asset_embedding_blob(asset.id, "model", &[1.0, 0.0])
        .unwrap();
    assert_eq!(
        reopened.load_asset_embedding(asset.id, "model").unwrap(),
        Some(vec![0, 0, 128, 63, 0, 0, 0, 0])
    );
    assert!(
        reopened
            .next_embedding_backfill_job_for_model("model", i64::MAX)
            .unwrap()
            .is_none()
    );
}
