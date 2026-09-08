use super::*;

#[test]
fn archived_loading_placement_rejects_late_import_completion() {
    let root = TempRoot::new("snappyref-failed-loading-placement-test");
    let source = root.join("source.png");
    fs::write(&source, [255, 255, 255, 255]).unwrap();
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let loading = library
        .persist_committed_loading_placement(
            egui::pos2(10.0, 20.0),
            1.0,
            100.0,
            80.0,
            ContentResolutionSource::Unknown {
                description: "clipboard".to_owned(),
            },
            Some("Queued".to_owned()),
        )
        .unwrap();

    library.archive_placements(&[loading.state.id]).unwrap();
    assert!(
        library
            .persist_imported_image_resolving_placement(
                1,
                loading.state.id,
                &source,
                "image/png",
                100,
                80,
                4,
                egui::Pos2::ZERO,
                1.0,
            )
            .is_err()
    );
    assert!(library.load_placements().unwrap().is_empty());
}

#[test]
fn exact_dedupe_relinks_placements_to_first_asset_and_deletes_duplicates() {
    let root = TempRoot::new("snappyref-exact-dedupe-test");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let second_board = library.create_board("Second").unwrap();
    let first_path = library
        .store_rgba_image("first", &[255, 0, 0, 255], 1, 1)
        .unwrap();
    let duplicate_path = library
        .store_rgba_image("duplicate", &[255, 0, 0, 255], 1, 1)
        .unwrap();

    let first = library
        .persist_imported_image(
            &first_path,
            "image/png",
            1,
            1,
            fs::metadata(&first_path).unwrap().len(),
            egui::pos2(10.0, 20.0),
            1.0,
        )
        .unwrap();
    library.set_active_board(second_board.id).unwrap();
    let duplicate = library
        .persist_imported_image(
            &duplicate_path,
            "image/png",
            1,
            1,
            fs::metadata(&duplicate_path).unwrap().len(),
            egui::pos2(30.0, 40.0),
            1.0,
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

    let report = library.dedupe_exact_assets(usize::MAX, usize::MAX).unwrap();

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
fn hash_aware_import_reuses_existing_exact_asset() {
    let root = TempRoot::new("snappyref-hash-aware-import-reuse-test");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let first_path = library
        .store_rgba_image("first", &[255, 0, 0, 255], 1, 1)
        .unwrap();
    let duplicate_path = library
        .store_rgba_image("duplicate", &[255, 0, 0, 255], 1, 1)
        .unwrap();
    let original_sha256 = crate::library::storage::sha256_file(&first_path).unwrap();

    let first = library
        .register_imported_asset_for_placement(
            1,
            library.allocate_document_id(),
            &first_path,
            "image/png",
            1,
            1,
            fs::metadata(&first_path).unwrap().len(),
            Some(&original_sha256),
            egui::pos2(0.0, 0.0),
            1.0,
        )
        .unwrap();
    let duplicate = library
        .register_imported_asset_for_placement(
            1,
            library.allocate_document_id(),
            &duplicate_path,
            "image/png",
            1,
            1,
            fs::metadata(&duplicate_path).unwrap().len(),
            Some(&original_sha256),
            egui::pos2(10.0, 0.0),
            1.0,
        )
        .unwrap();

    assert_eq!(
        duplicate.require_loaded_asset().id,
        first.require_loaded_asset().id
    );
    assert_eq!(
        duplicate.require_loaded_asset().original_path,
        first.require_loaded_asset().original_path
    );
    assert_eq!(duplicate.asset_id(), first.asset_id());
    assert!(!duplicate_path.exists());
    assert!(first_path.exists());

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
                .register_imported_asset_for_placement(
                    1,
                    library.allocate_document_id(),
                    &first_path,
                    "image/png",
                    1,
                    1,
                    fs::metadata(&first_path).unwrap().len(),
                    Some(&original_sha256),
                    egui::pos2(0.0, 0.0),
                    1.0,
                )
                .unwrap()
                .require_loaded_asset()
                .id
        })
    };
    let duplicate_thread = {
        let library = library.clone();
        let barrier = Arc::clone(&barrier);
        std::thread::spawn(move || {
            barrier.wait();
            library
                .register_imported_asset_for_placement(
                    1,
                    library.allocate_document_id(),
                    &duplicate_path,
                    "image/png",
                    1,
                    1,
                    fs::metadata(&duplicate_path).unwrap().len(),
                    Some(&original_sha256),
                    egui::pos2(10.0, 0.0),
                    1.0,
                )
                .unwrap()
                .require_loaded_asset()
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
fn canvas_image_cache_path_is_deterministic() {
    let root = std::env::temp_dir().join(format!(
        "snappyref-thumb-path-test-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&root).unwrap();

    let library = Library::initialize(root.to_path_buf()).unwrap();
    let path = library
        .store_canvas_image_cache(42, 2000, &[255, 0, 0, 255], 1, 1)
        .unwrap();
    let path = path.unwrap();

    assert_eq!(
        path,
        root.join("thumbs")
            .join("asset-42-canvas-v3-downscale-working-2000.jpg")
    );
}

#[test]
fn store_and_remove_canvas_image_cache_round_trips_thumb_file() {
    let root = std::env::temp_dir().join(format!(
        "snappyref-thumb-store-test-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&root).unwrap();

    let library = Library::initialize(root.to_path_buf()).unwrap();
    let preview_path = library
        .store_canvas_image_cache(7, 2000, &[255, 0, 0, 255], 1, 1)
        .unwrap()
        .unwrap();

    assert!(preview_path.is_file());

    library.remove_asset_canvas_caches(7).unwrap();
    assert!(!preview_path.exists());
}

#[test]
fn remove_asset_canvas_caches_removes_all_variants_for_asset() {
    let root = std::env::temp_dir().join(format!(
        "snappyref-thumb-remove-variants-test-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&root).unwrap();

    let library = Library::initialize(root.to_path_buf()).unwrap();
    let first = library
        .store_canvas_image_cache(7, 2000, &[255, 0, 0, 255], 1, 1)
        .unwrap()
        .unwrap();
    let second = library
        .store_canvas_image_cache(7, 4000, &[255, 0, 0, 255], 1, 1)
        .unwrap()
        .unwrap();

    library.remove_asset_canvas_caches(7).unwrap();

    assert!(!first.exists());
    assert!(!second.exists());
}

#[test]
fn embedding_backfill_includes_unplaced_library_assets() {
    let root = std::env::temp_dir().join(format!(
        "snappyref-library-embedding-test-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&root).unwrap();

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
        .persist_imported_image(&ready_path, "image/png", 1, 1, 4, egui::pos2(0.0, 0.0), 1.0)
        .unwrap();
    let pending = library
        .persist_imported_image(
            &pending_path,
            "image/png",
            1,
            1,
            4,
            egui::pos2(10.0, 0.0),
            1.0,
        )
        .unwrap();
    let unplaced = library
        .persist_imported_image(
            &unplaced_path,
            "image/png",
            1,
            1,
            4,
            egui::pos2(20.0, 0.0),
            1.0,
        )
        .unwrap();

    library.archive_placements(&[unplaced.state.id]).unwrap();
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
        .persist_imported_image(
            &raster_path,
            "image/png",
            1,
            1,
            4,
            egui::pos2(0.0, 0.0),
            1.0,
        )
        .unwrap();
    let svg = library
        .persist_imported_image(
            &svg_path,
            "image/svg+xml",
            1,
            1,
            fs::metadata(&svg_path).unwrap().len(),
            egui::pos2(1.0, 0.0),
            1.0,
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
        .persist_imported_image(&retry_path, "image/png", 1, 1, 4, egui::pos2(0.0, 0.0), 1.0)
        .unwrap();
    let fresh = library
        .persist_imported_image(&fresh_path, "image/png", 1, 1, 4, egui::pos2(1.0, 0.0), 1.0)
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
fn next_embedding_backfill_job_prefers_never_tried_assets_before_retries() {
    let root = TempRoot::new("snappyref-next-embedding-backfill-job-test");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let retry_path = library
        .store_rgba_image("retry", &[255, 0, 0, 255], 1, 1)
        .unwrap();
    let fresh_path = library
        .store_rgba_image("fresh", &[0, 255, 0, 255], 1, 1)
        .unwrap();

    let retry = library
        .persist_imported_image(&retry_path, "image/png", 1, 1, 4, egui::pos2(0.0, 0.0), 1.0)
        .unwrap();
    let fresh = library
        .persist_imported_image(&fresh_path, "image/png", 1, 1, 4, egui::pos2(1.0, 0.0), 1.0)
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
        .upsert_asset_embedding_blob(
            fresh.require_loaded_asset().id,
            "mobileclip_s0",
            &[1.0, 0.0],
        )
        .unwrap();
    let retry_job = library
        .next_embedding_backfill_job_for_model("mobileclip_s0", 1_000)
        .unwrap()
        .unwrap();
    assert_eq!(retry_job.asset_id, retry.require_loaded_asset().id);
}

#[test]
fn embedding_helpers_scope_to_requested_model_key() {
    let root = std::env::temp_dir().join(format!(
        "snappyref-embedding-model-key-test-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&root).unwrap();

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
