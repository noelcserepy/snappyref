use super::*;

#[test]
fn exact_dedupe_relinks_placements_to_first_asset_and_deletes_duplicates() {
    let root = TempRoot::new("snappyref-exact-dedupe-test");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let second_board = library.fixture_board("Second").unwrap();
    let first_path = library
        .store_rgba_image("first", &[255, 0, 0, 255], 1, 1)
        .unwrap();
    let duplicate_path = library
        .store_rgba_image("duplicate", &[255, 0, 0, 255], 1, 1)
        .unwrap();

    let first = library
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
                    .register_original(
                        &first_path,
                        "image/png",
                        1,
                        1,
                        fs::metadata(&first_path).unwrap().len(),
                        None,
                    )
                    .unwrap(),
            ),
        )
        .unwrap();
    library.set_active_board(second_board.id).unwrap();
    let duplicate = library
        .fixture_placement(
            library.load_user_session().unwrap().active_board_id,
            crate::data::Placement::loaded(
                crate::data::PlacementState::new(
                    library.allocate_document_id(),
                    egui::pos2(30.0, 40.0),
                    1.0,
                    0.0,
                    0,
                    false,
                ),
                library
                    .register_original(
                        &duplicate_path,
                        "image/png",
                        1,
                        1,
                        fs::metadata(&duplicate_path).unwrap().len(),
                        None,
                    )
                    .unwrap(),
            ),
        )
        .unwrap();
    let first_asset_id = first.require_loaded_asset().id;
    let duplicate_asset_id = duplicate.require_loaded_asset().id;
    let duplicate_original_path = duplicate.require_loaded_asset().original_path.clone();
    library
        .upsert_asset_embedding_blob(first_asset_id, "test-model", &[1.0, 0.0])
        .unwrap();
    library
        .upsert_asset_embedding_blob(duplicate_asset_id, "test-model", &[0.0, 1.0])
        .unwrap();
    library
        .store_canvas_image_cache(
            duplicate_asset_id,
            crate::data::DEFAULT_CANVAS_IMAGE_MAX_SIDE,
            &[255, 0, 0, 255],
            1,
            1,
        )
        .unwrap();
    assert!(
        library
            .canvas_image_cache_path(
                duplicate_asset_id,
                crate::data::DEFAULT_CANVAS_IMAGE_MAX_SIDE
            )
            .is_some()
    );

    let mut history = crate::active_board::history::DocumentHistory::default();
    history.record_applied(
        second_board.id,
        crate::active_board::history::HistoryAction::document(
            "Added duplicate".into(),
            vec![crate::active_board::history::HistoryChange {
                before: None,
                after: Some(duplicate.clone()),
            }],
            vec![],
            None,
        ),
    );
    library
        .persist_board_unit(
            &crate::library::storage::BoardDurabilityOperation::Document {
                board_id: second_board.id,
                changes: Default::default(),
            },
            &history,
        )
        .unwrap();
    let report = library.dedupe_exact_assets(usize::MAX, usize::MAX).unwrap();

    let replay = library
        .load_document_history()
        .unwrap()
        .undo(second_board.id)
        .unwrap();
    let crate::active_board::history::HistoryAction::Document(action) = replay.action else {
        panic!("expected placement history")
    };
    assert_eq!(
        action.placements[0].after.as_ref().unwrap().asset_id(),
        Some(first_asset_id)
    );
    assert_eq!(
        action.placements[0]
            .after
            .as_ref()
            .unwrap()
            .require_loaded_asset()
            .original_path,
        first.require_loaded_asset().original_path
    );
    assert_eq!(report.hashed_assets, 2);
    assert_eq!(report.duplicate_assets_removed, 1);
    assert_eq!(report.placements_relinked, 1);
    assert_eq!(
        report.asset_id_remaps,
        vec![(duplicate_asset_id, first_asset_id)]
    );
    assert!(!duplicate_original_path.exists());
    assert!(
        library
            .canvas_image_cache_path(
                duplicate_asset_id,
                crate::data::DEFAULT_CANVAS_IMAGE_MAX_SIDE
            )
            .is_none()
    );

    library.set_active_board(1).unwrap();
    let first_board_placements = library.load_placements().unwrap();
    assert_eq!(first_board_placements.len(), 1);
    assert_eq!(first_board_placements[0].asset_id(), Some(first_asset_id));
    library.set_active_board(second_board.id).unwrap();
    let second_board_placements = library.load_placements().unwrap();
    assert_eq!(second_board_placements.len(), 1);
    assert_eq!(second_board_placements[0].asset_id(), Some(first_asset_id));

    let embeddings = library.load_library_embeddings("test-model").unwrap();
    assert_eq!(embeddings.len(), 1);
    assert_eq!(embeddings[0].asset_id, first_asset_id);
    let connection = Connection::open(root.join("library.db")).unwrap();
    let live_assets: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM assets WHERE archived = 0",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(live_assets, 1);
    let duplicate_embedding_rows: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM asset_embeddings WHERE asset_id = ?1",
            [duplicate_asset_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(duplicate_embedding_rows, 0);
}

#[test]
fn parallel_hash_aware_imports_converge_on_one_exact_asset() {
    let root = TempRoot::new("snappyref-parallel-hash-aware-import-reuse-test");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let first_path = library
        .store_rgba_image("first", &[255, 0, 0, 255], 1, 1)
        .unwrap();
    let duplicate_path = library
        .store_rgba_image("duplicate", &[255, 0, 0, 255], 1, 1)
        .unwrap();
    let original_sha256 = crate::library::storage::sha256_file(&first_path).unwrap();
    let barrier = Arc::new(Barrier::new(2));

    let first_thread = {
        let library = library.clone();
        let barrier = Arc::clone(&barrier);
        let original_sha256 = original_sha256.clone();
        std::thread::spawn(move || {
            barrier.wait();
            library
                .register_original(
                    &first_path,
                    "image/png",
                    1,
                    1,
                    fs::metadata(&first_path).unwrap().len(),
                    Some(&original_sha256),
                )
                .unwrap()
                .id
        })
    };
    let duplicate_thread = {
        let library = library.clone();
        let barrier = Arc::clone(&barrier);
        std::thread::spawn(move || {
            barrier.wait();
            library
                .register_original(
                    &duplicate_path,
                    "image/png",
                    1,
                    1,
                    fs::metadata(&duplicate_path).unwrap().len(),
                    Some(&original_sha256),
                )
                .unwrap()
                .id
        })
    };

    let first_asset_id = first_thread.join().unwrap();
    let duplicate_asset_id = duplicate_thread.join().unwrap();
    assert_eq!(duplicate_asset_id, first_asset_id);

    let connection = Connection::open(root.join("library.db")).unwrap();
    let live_assets: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM assets WHERE archived = 0",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(live_assets, 1);
}

#[test]
fn remove_asset_canvas_caches_removes_every_variant_of_only_that_asset() {
    let root = TempRoot::new("snappyref-canvas-cache-remove-test");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let store = |asset_id, max_side| {
        library
            .store_canvas_image_cache(asset_id, max_side, &[255, 0, 0, 255], 1, 1)
            .unwrap()
    };
    let removed = [store(7, 2000), store(7, 4000)];
    // Asset 70 shares the "asset-7" text prefix.
    let kept = store(70, 2000);
    assert!(removed.iter().chain([&kept]).all(|path| path.is_file()));

    library.remove_asset_canvas_caches(7).unwrap();

    assert!(removed.iter().all(|path| !path.exists()));
    assert!(kept.is_file());
}

#[test]
fn embedding_backfill_includes_unplaced_library_assets() {
    let root = TempRoot::new("snappyref-library-embedding-test");

    let library = Library::initialize(root.to_path_buf()).unwrap();
    let ready_path = library
        .store_rgba_image("ready", &[255, 0, 0, 255], 1, 1)
        .unwrap();
    let pending_path = library
        .store_rgba_image("pending", &[0, 255, 0, 255], 1, 1)
        .unwrap();
    let unplaced_path = library
        .store_rgba_image("unplaced", &[0, 0, 255, 255], 1, 1)
        .unwrap();

    let ready = library
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
                    .register_original(&ready_path, "image/png", 1, 1, 4, None)
                    .unwrap(),
            ),
        )
        .unwrap();
    let pending = library
        .fixture_placement(
            library.load_user_session().unwrap().active_board_id,
            crate::data::Placement::loaded(
                crate::data::PlacementState::new(
                    library.allocate_document_id(),
                    egui::pos2(10.0, 0.0),
                    1.0,
                    0.0,
                    0,
                    false,
                ),
                library
                    .register_original(&pending_path, "image/png", 1, 1, 4, None)
                    .unwrap(),
            ),
        )
        .unwrap();
    let unplaced = library
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
                    .register_original(&unplaced_path, "image/png", 1, 1, 4, None)
                    .unwrap(),
            ),
        )
        .unwrap();

    library.fixture_board_document(library.load_user_session().unwrap().active_board_id, crate::canvas::CanvasDocumentChanges {archived_placement_ids: [unplaced.state.id].to_vec(), stack_order: Some(library.load_board_document(library.load_user_session().unwrap().active_board_id).unwrap().stack_order.into_iter().filter(|item| !matches!(item, crate::data::CanvasStackItem::Placement(id) if [unplaced.state.id].contains(id))).collect()), ..Default::default()}).unwrap();
    library
        .upsert_asset_embedding_blob(
            ready.require_loaded_asset().id,
            "mobileclip_s0",
            &[1.0, 0.0],
        )
        .unwrap();

    let job = library
        .next_embedding_backfill_job_for_model("mobileclip_s0", 0)
        .unwrap()
        .unwrap();
    assert_eq!(job.asset_id, pending.require_loaded_asset().id);
    library
        .upsert_asset_embedding_blob(job.asset_id, "mobileclip_s0", &[1.0, 0.0])
        .unwrap();
    let job = library
        .next_embedding_backfill_job_for_model("mobileclip_s0", 0)
        .unwrap()
        .unwrap();
    assert_eq!(job.asset_id, unplaced.require_loaded_asset().id);
}

#[test]
fn embedding_backfill_skips_svg_assets() {
    let root = TempRoot::new("snappyref-svg-embedding-backfill-test");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let raster_path = library
        .store_rgba_image("raster", &[255, 0, 0, 255], 1, 1)
        .unwrap();
    let svg_path = root.join("icon.svg");
    fs::write(
        &svg_path,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="1" height="1"></svg>"#,
    )
    .unwrap();

    let raster = library
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
                    .register_original(&raster_path, "image/png", 1, 1, 4, None)
                    .unwrap(),
            ),
        )
        .unwrap();
    let svg = library
        .fixture_placement(
            library.load_user_session().unwrap().active_board_id,
            crate::data::Placement::loaded(
                crate::data::PlacementState::new(
                    library.allocate_document_id(),
                    egui::pos2(1.0, 0.0),
                    1.0,
                    0.0,
                    0,
                    false,
                ),
                library
                    .register_original(
                        &svg_path,
                        "image/svg+xml",
                        1,
                        1,
                        fs::metadata(&svg_path).unwrap().len(),
                        None,
                    )
                    .unwrap(),
            ),
        )
        .unwrap();

    let job = library
        .next_embedding_backfill_job_for_model("mobileclip_s0", 0)
        .unwrap()
        .unwrap();
    assert_eq!(job.asset_id, raster.require_loaded_asset().id);
    library
        .upsert_asset_embedding_blob(job.asset_id, "mobileclip_s0", &[1.0, 0.0])
        .unwrap();
    assert!(
        library
            .next_embedding_backfill_job_for_model("mobileclip_s0", 0)
            .unwrap()
            .is_none()
    );
    library
        .record_retryable_asset_embedding_failure(
            svg.require_loaded_asset().id,
            "mobileclip_s0",
            2,
            "unsupported",
            0,
        )
        .unwrap();
    assert!(
        library
            .next_embedding_backfill_job_for_model("mobileclip_s0", 0)
            .unwrap()
            .is_none()
    );
}

#[test]
fn embedding_backfill_retries_failed_assets_after_never_tried_assets() {
    let root = TempRoot::new("snappyref-embedding-retry-order-test");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let retry_path = library
        .store_rgba_image("retry", &[255, 0, 0, 255], 1, 1)
        .unwrap();
    let fresh_path = library
        .store_rgba_image("fresh", &[0, 255, 0, 255], 1, 1)
        .unwrap();

    let retry = library
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
                    .register_original(&retry_path, "image/png", 1, 1, 4, None)
                    .unwrap(),
            ),
        )
        .unwrap();
    let fresh = library
        .fixture_placement(
            library.load_user_session().unwrap().active_board_id,
            crate::data::Placement::loaded(
                crate::data::PlacementState::new(
                    library.allocate_document_id(),
                    egui::pos2(1.0, 0.0),
                    1.0,
                    0.0,
                    0,
                    false,
                ),
                library
                    .register_original(&fresh_path, "image/png", 1, 1, 4, None)
                    .unwrap(),
            ),
        )
        .unwrap();

    library
        .record_retryable_asset_embedding_failure(
            retry.require_loaded_asset().id,
            "mobileclip_s0",
            768,
            "temporary failure",
            1_000,
        )
        .unwrap();

    let job = library
        .next_embedding_backfill_job_for_model("mobileclip_s0", 1_000)
        .unwrap()
        .unwrap();
    assert_eq!(job.asset_id, fresh.require_loaded_asset().id);
    library
        .upsert_asset_embedding_blob(job.asset_id, "mobileclip_s0", &[1.0, 0.0])
        .unwrap();
    let job = library
        .next_embedding_backfill_job_for_model("mobileclip_s0", 1_000)
        .unwrap()
        .unwrap();
    assert_eq!(job.asset_id, retry.require_loaded_asset().id);
}

#[test]
fn embedding_helpers_scope_to_requested_model_key() {
    let root = TempRoot::new("snappyref-embedding-model-key-test");

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
                    egui::pos2(10.0, 0.0),
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

    library
        .upsert_asset_embedding_blob(first.require_loaded_asset().id, "clip_key", &[1.0, 0.0])
        .unwrap();
    library
        .upsert_asset_embedding_blob(second.require_loaded_asset().id, "clip_key", &[0.0, 1.0])
        .unwrap();
    library
        .upsert_asset_embedding_blob(first.require_loaded_asset().id, "siglip_key", &[0.5, 0.5])
        .unwrap();

    let job = library
        .next_embedding_backfill_job_for_model("siglip_key", 0)
        .unwrap()
        .unwrap();
    assert_eq!(job.asset_id, second.require_loaded_asset().id);

    let embeddings = library.load_library_embeddings("siglip_key").unwrap();
    assert_eq!(embeddings.len(), 1);
    assert_eq!(embeddings[0].asset_id, first.require_loaded_asset().id);
}

#[test]
fn closed_library_move_rebases_managed_originals_and_history() {
    let parent = TempRoot::new("relocated-library");
    let old_root = parent.join("before");
    let new_root = parent.join("after");
    let library = Library::initialize(old_root.clone()).unwrap();
    let path = library
        .store_rgba_image("portable", &[33, 66, 99, 255], 1, 1)
        .unwrap();
    let asset = library
        .register_original(&path, "image/png", 1, 1, 4, None)
        .unwrap();
    let placement = library
        .fixture_placement(
            1,
            crate::data::Placement::loaded(
                crate::data::PlacementState::new(
                    library.allocate_document_id(),
                    egui::pos2(9.0, 8.0),
                    1.0,
                    0.4,
                    0,
                    false,
                ),
                asset.clone(),
            ),
        )
        .unwrap();
    let mut history = crate::active_board::history::DocumentHistory::default();
    history.record_applied(
        1,
        crate::active_board::history::HistoryAction::document(
            "Import".into(),
            vec![crate::active_board::history::HistoryChange {
                before: None,
                after: Some(placement),
            }],
            vec![],
            None,
        ),
    );
    library
        .persist_board_unit(
            &crate::library::storage::BoardDurabilityOperation::Document {
                board_id: 1,
                changes: Default::default(),
            },
            &history,
        )
        .unwrap();
    let mut session = library.load_user_session().unwrap();
    session.settings.placement_gap = 51.0;
    session.recent_searches = vec!["portable references".into()];
    library.save_user_session(&session).unwrap();
    drop(library);
    fs::rename(&old_root, &new_root).unwrap();
    let moved = Library::initialize(new_root.clone()).unwrap();
    let restored = moved.load_placements().unwrap();
    let restored_path = &restored[0].require_loaded_asset().original_path;
    assert!(
        restored_path.starts_with(&new_root),
        "managed path must follow library move: {restored_path:?}"
    );
    assert_eq!(
        image::open(restored_path).unwrap().to_rgba8().as_raw(),
        &[33, 66, 99, 255]
    );
    assert_eq!(moved.load_user_session().unwrap(), session);
    let undo = moved.load_document_history().unwrap().undo(1).unwrap();
    let crate::active_board::history::HistoryAction::Document(action) = undo.action else {
        panic!("document action")
    };
    assert_eq!(
        action.placements[0]
            .after
            .as_ref()
            .unwrap()
            .require_loaded_asset()
            .original_path,
        *restored_path
    );
    drop(moved);
    let copy_root = parent.join("copied");
    fs::create_dir_all(copy_root.join("originals")).unwrap();
    fs::copy(new_root.join("library.db"), copy_root.join("library.db")).unwrap();
    for entry in fs::read_dir(new_root.join("originals")).unwrap() {
        let entry = entry.unwrap();
        fs::copy(
            entry.path(),
            copy_root.join("originals").join(entry.file_name()),
        )
        .unwrap();
    }
    let copied = Library::initialize(copy_root.clone()).unwrap();
    let copied_asset = copied.load_asset(asset.id).unwrap();
    assert!(copied_asset.original_path.starts_with(&copy_root));
    fs::remove_dir_all(new_root).unwrap();
    assert_eq!(
        image::open(copied_asset.original_path)
            .unwrap()
            .to_rgba8()
            .as_raw(),
        &[33, 66, 99, 255]
    );
    assert_eq!(copied.load_user_session().unwrap(), session);
    assert!(copied.load_document_history().unwrap().undo(1).is_some());
}
