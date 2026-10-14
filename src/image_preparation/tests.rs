use super::{
    CanvasImageSettings, ImagePrepPreviewTexture, ImagePrepResult, ImagePrepTimings,
    ImagePrepTrace, ImagePrepWorker, ImagePrepWorkerConfig, ImportRequest, ImportSource,
    build_preview_texture, crop_item, import, import_path_sync, import_sync, load_existing,
};
use crate::data::CanvasImageLod;
use crate::library::storage::Library;
use crate::placement::DEFAULT_PLACEMENT_TARGET_AREA;
use crate::test_support::TempRoot;
use image::{ColorType, ImageFormat};
use pollster::block_on;
use rusqlite::Connection;
use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

fn downscaled_canvas_settings(max_side: u32) -> CanvasImageSettings {
    CanvasImageSettings { max_side }
}

#[test]
fn file_imports_land_in_same_area_band() {
    let root = TempRoot::new("file-import-scaling");
    let library = Library::initialize(root.to_path_buf()).unwrap();

    let portrait_path = write_png(&root, "portrait.png", 1000, 1500);
    let landscape_path = write_png(&root, "landscape.png", 1920, 1080);
    let square_path = write_png(&root, "square.png", 1000, 1000);
    let small_path = write_png(&root, "small.png", 180, 240);

    let portrait = import_path_sync(&library, portrait_path, eframe::egui::Pos2::ZERO).unwrap();
    let landscape = import_path_sync(&library, landscape_path, eframe::egui::Pos2::ZERO).unwrap();
    let square = import_path_sync(&library, square_path, eframe::egui::Pos2::ZERO).unwrap();
    let small = import_path_sync(&library, small_path, eframe::egui::Pos2::ZERO).unwrap();

    assert!((displayed_area(&portrait) - DEFAULT_PLACEMENT_TARGET_AREA).abs() < 0.1);
    assert!((displayed_area(&landscape) - DEFAULT_PLACEMENT_TARGET_AREA).abs() < 0.1);
    assert!((displayed_area(&square) - DEFAULT_PLACEMENT_TARGET_AREA).abs() < 0.1);
    assert_eq!(small.placement.state.geometry.scale, 1.0);
}

#[test]
fn clipboard_import_uses_configured_target_area() {
    let root = TempRoot::new("clipboard-import-custom-area");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let target_area = 40_000.0;

    let loaded = match import(
        &library,
        reserve_import_fixture(
            &library,
            ImportRequest {
                target_board_id: 1,
                queue_id: 1,
                placement_id: 0,
                source: ImportSource::Clipboard {
                    rgba: solid_rgba(1000, 1500),
                    width: 1000,
                    height: 1500,
                },
                reserved_world: eframe::egui::Pos2::ZERO,

                max_texture_side: usize::MAX,
                canvas_image_settings: CanvasImageSettings::default(),
                placement_target_area: target_area,

                trace: None,
            },
        ),
        None,
        &super::decode::ImagePrepHelper::default(),
    ) {
        ImagePrepResult::Loaded(loaded) => loaded,
        other => panic!("expected loaded clipboard image, got {other:?}"),
    };

    assert!((displayed_area(&loaded) - target_area).abs() < 0.1);
}

#[test]
fn import_returns_reserved_world_and_target_board() {
    let root = TempRoot::new("import-reserved-world");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let second = library.fixture_board("Second").unwrap();
    let reserved_world = eframe::egui::pos2(240.0, 36.0);

    let loaded = match import(
        &library,
        reserve_import_fixture(
            &library,
            ImportRequest {
                target_board_id: second.id,
                queue_id: 1,
                placement_id: 0,
                source: ImportSource::Clipboard {
                    rgba: solid_rgba(100, 100),
                    width: 100,
                    height: 100,
                },
                reserved_world,

                max_texture_side: usize::MAX,
                canvas_image_settings: CanvasImageSettings::default(),
                placement_target_area: DEFAULT_PLACEMENT_TARGET_AREA,

                trace: None,
            },
        ),
        None,
        &super::decode::ImagePrepHelper::default(),
    ) {
        ImagePrepResult::Loaded(loaded) => loaded,
        other => panic!("expected reserved clipboard import to load, got {other:?}"),
    };

    assert_eq!(loaded.target_board_id, second.id);
    assert_eq!(loaded.placement.state.geometry.center, reserved_world);
}

#[test]
fn url_import_downloads_and_persists_original_bytes() {
    let root = TempRoot::new("url-import");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let png_bytes = solid_png_bytes(64, 48, [188, 146, 255, 255]);
    let (url, server) = serve_bytes_once("/test-image.png", "image/png", png_bytes.clone());

    let loaded = import_sync(
        &library,
        reserve_import_fixture(
            &library,
            ImportRequest {
                target_board_id: 1,
                queue_id: 7,
                placement_id: 0,
                source: ImportSource::Url(url.clone()),
                reserved_world: eframe::egui::Pos2::ZERO,

                max_texture_side: usize::MAX,
                canvas_image_settings: CanvasImageSettings::default(),
                placement_target_area: DEFAULT_PLACEMENT_TARGET_AREA,

                trace: None,
            },
        ),
        None,
        &super::decode::ImagePrepHelper::default(),
    )
    .unwrap();

    server.join().unwrap();

    assert_eq!(
        loaded.placement.require_loaded_asset().mime_type,
        "image/png"
    );
    assert_eq!(loaded.placement.require_loaded_asset().width, 64);
    assert_eq!(loaded.placement.require_loaded_asset().height, 48);
    assert_eq!(
        loaded
            .placement
            .require_loaded_asset()
            .original_path
            .extension()
            .and_then(|value| value.to_str()),
        Some("png")
    );
    assert!(
        loaded
            .placement
            .require_loaded_asset()
            .original_path
            .file_name()
            .and_then(|value| value.to_str())
            .is_some_and(|name| name.ends_with("test-image.png"))
    );
    assert_eq!(
        fs::read(&loaded.placement.require_loaded_asset().original_path).unwrap(),
        png_bytes
    );
}

#[test]
fn path_import_reuses_existing_exact_asset_and_removes_duplicate_original() {
    let root = TempRoot::new("path-import-exact-reuse");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let source = write_png(&root, "source.png", 64, 48);

    let first = import_path_sync(&library, source.clone(), eframe::egui::Pos2::ZERO).unwrap();
    let second = import_path_sync(&library, source, eframe::egui::pos2(100.0, 0.0)).unwrap();

    assert_eq!(
        second.placement.require_loaded_asset().id,
        first.placement.require_loaded_asset().id
    );
    assert_eq!(
        second.placement.require_loaded_asset().original_path,
        first.placement.require_loaded_asset().original_path
    );
    assert!(
        first
            .placement
            .require_loaded_asset()
            .original_path
            .exists()
    );
    assert_eq!(fs::read_dir(root.join("originals")).unwrap().count(), 1);

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
fn url_import_preserves_svg_as_vector_source() {
    let root = TempRoot::new("url-svg-import");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let svg_bytes = svg_bytes(128, 96);
    let (url, server) = serve_bytes_once("/remote-icon.svg", "image/svg+xml", svg_bytes.clone());

    let loaded = import_sync(
        &library,
        reserve_import_fixture(
            &library,
            ImportRequest {
                target_board_id: 1,
                queue_id: 9,
                placement_id: 0,
                source: ImportSource::Url(url.clone()),
                reserved_world: eframe::egui::Pos2::ZERO,

                max_texture_side: usize::MAX,
                canvas_image_settings: CanvasImageSettings::default(),
                placement_target_area: DEFAULT_PLACEMENT_TARGET_AREA,

                trace: None,
            },
        ),
        None,
        &super::decode::ImagePrepHelper::default(),
    )
    .unwrap();

    server.join().unwrap();

    assert_eq!(loaded.origin, super::ImportOrigin::Url);
    assert_eq!(
        loaded.placement.require_loaded_asset().mime_type,
        "image/svg+xml"
    );
    assert_eq!(loaded.placement.require_loaded_asset().width, 128);
    assert_eq!(loaded.placement.require_loaded_asset().height, 96);
    assert!(loaded.preview_rgba.is_empty());
    assert!(matches!(
        loaded.preview_texture,
        ImagePrepPreviewTexture::Svg { .. }
    ));
    assert_eq!(
        fs::read(&loaded.placement.require_loaded_asset().original_path).unwrap(),
        svg_bytes
    );
    assert!(
        library
            .canvas_image_cache_path(loaded.placement.require_loaded_asset().id, 2000)
            .is_none()
    );
}

#[test]
fn svg_import_preserves_vector_source_and_skips_canvas_cache() {
    let root = TempRoot::new("svg-import");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let svg_path = write_svg(&root, "icon.svg", 320, 180);

    let loaded = import_path_sync(&library, svg_path.clone(), eframe::egui::Pos2::ZERO).unwrap();

    assert_eq!(
        loaded.placement.require_loaded_asset().mime_type,
        "image/svg+xml"
    );
    assert_eq!(loaded.placement.require_loaded_asset().width, 320);
    assert_eq!(loaded.placement.require_loaded_asset().height, 180);
    assert!(loaded.preview_rgba.is_empty());
    assert!(matches!(
        loaded.preview_texture,
        ImagePrepPreviewTexture::Svg { .. }
    ));
    assert_eq!(
        fs::read_to_string(&loaded.placement.require_loaded_asset().original_path).unwrap(),
        fs::read_to_string(&svg_path).unwrap()
    );
    assert!(
        library
            .canvas_image_cache_path(loaded.placement.require_loaded_asset().id, 2000)
            .is_none()
    );
}

#[test]
fn xpm_import_uses_raster_pipeline_with_original_mime_type() {
    let root = TempRoot::new("xpm-import");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let source = root.join("sample.xpm");
    fs::write(
        &source,
        r##"/* XPM */
static char * sample_xpm[] = {
"2 2 2 1",
"a c #ff0000",
". c None",
"a.",
".a"};"##,
    )
    .unwrap();

    let loaded = import_path_sync(&library, source.clone(), eframe::egui::Pos2::ZERO).unwrap();

    assert_eq!(
        loaded.placement.require_loaded_asset().mime_type,
        "image/x-xpixmap"
    );
    assert_eq!(loaded.placement.require_loaded_asset().width, 2);
    assert_eq!(loaded.placement.require_loaded_asset().height, 2);
    assert_eq!(loaded.preview_rgba.len(), 2 * 2 * 4);
    assert_eq!(
        fs::read(&loaded.placement.require_loaded_asset().original_path).unwrap(),
        fs::read(source).unwrap()
    );
    assert!(
        library
            .canvas_image_cache_path(loaded.placement.require_loaded_asset().id, 2000)
            .is_some()
    );
}

#[test]
fn crop_item_rejects_svg_with_clear_error() {
    let root = TempRoot::new("svg-crop");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let svg_path = write_svg(&root, "icon.svg", 100, 80);
    let loaded = import_path_sync(&library, svg_path, eframe::egui::Pos2::ZERO).unwrap();

    let result = crop_item(
        &library,
        crate::canvas::CanvasCropRequest {
            placement_id: loaded.placement.state.id,
            source_asset: loaded.placement.require_loaded_asset().clone(),
            crop_rect: crate::canvas::PixelCropRect {
                x: 0,
                y: 0,
                width: 50,
                height: 40,
            },
            crop_offset_local: eframe::egui::Vec2::ZERO,
        },
        usize::MAX,
        CanvasImageSettings::default(),
        None,
    );

    match result {
        ImagePrepResult::Failed {
            placement_id,
            error,
            ..
        } => {
            assert_eq!(placement_id, Some(loaded.placement.state.id));
            assert_eq!(error, "SVG cropping is not supported yet");
        }
        other => panic!("expected SVG crop failure, got {other:?}"),
    }
}

#[test]
fn load_existing_svg_uses_original_vector_source() {
    let root = TempRoot::new("svg-existing-load");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let svg_path = write_svg(&root, "icon.svg", 240, 120);
    let imported = import_path_sync(&library, svg_path, eframe::egui::Pos2::ZERO).unwrap();

    let loaded = match load_existing(
        &library,
        1,
        None,
        imported.placement.clone(),
        usize::MAX,
        CanvasImageSettings::default(),
        CanvasImageLod::Working,
        None,
        None,
    ) {
        ImagePrepResult::Loaded(loaded) => loaded,
        other => panic!("expected SVG existing load, got {other:?}"),
    };

    assert_eq!(loaded.origin, super::ImportOrigin::Existing);
    assert_eq!(loaded.preview_width, 240);
    assert_eq!(loaded.preview_height, 120);
    assert!(loaded.preview_rgba.is_empty());
    assert!(matches!(
        loaded.preview_texture,
        ImagePrepPreviewTexture::Svg { .. }
    ));
    assert!(
        library
            .canvas_image_cache_path(loaded.placement.require_loaded_asset().id, 2000)
            .is_none()
    );
}

#[test]
fn interactive_import_queue_pops_smallest_bytes_first() {
    let root = TempRoot::new("interactive-queue-by-bytes");
    // Write three real PNGs of different byte sizes so metadata-based
    // priority can be derived from the filesystem at enqueue time.
    let small_path = write_png(&root, "small.png", 32, 32);
    let medium_path = write_png(&root, "medium.png", 128, 128);
    let large_path = write_png(&root, "large.png", 512, 512);
    let small_bytes = fs::metadata(&small_path).unwrap().len();
    let medium_bytes = fs::metadata(&medium_path).unwrap().len();
    let large_bytes = fs::metadata(&large_path).unwrap().len();
    assert!(
        small_bytes < medium_bytes && medium_bytes < large_bytes,
        "expected strictly ascending file sizes, got {small_bytes}/{medium_bytes}/{large_bytes}"
    );

    let queue = super::RequestQueue::default();
    // Enqueue in worst-case "large first" order so a FIFO would return the
    // large file first; the priority queue must invert this.
    for path in [&large_path, &medium_path, &small_path] {
        queue
            .push(make_import_path_request(path.clone()))
            .expect("queue push");
    }

    let first = queue
        .try_pop_one(super::WorkerKind::Interactive)
        .expect("first pop");
    let second = queue
        .try_pop_one(super::WorkerKind::Interactive)
        .expect("second pop");
    let third = queue
        .try_pop_one(super::WorkerKind::Interactive)
        .expect("third pop");

    assert_eq!(import_path_source(&first), small_path);
    assert_eq!(import_path_source(&second), medium_path);
    assert_eq!(import_path_source(&third), large_path);
}

#[test]
fn interactive_import_queue_breaks_ties_by_arrival_order() {
    let root = TempRoot::new("interactive-queue-tie-break");
    // Two identical 64x64 PNGs produce identical byte counts; FIFO order
    // must be preserved when the sort key ties.
    let first_path = write_png(&root, "first.png", 64, 64);
    let second_path = write_png(&root, "second.png", 64, 64);
    let third_path = write_png(&root, "third.png", 64, 64);
    let a = fs::metadata(&first_path).unwrap().len();
    let b = fs::metadata(&second_path).unwrap().len();
    let c = fs::metadata(&third_path).unwrap().len();
    assert_eq!(a, b);
    assert_eq!(b, c);

    let queue = super::RequestQueue::default();
    for path in [&first_path, &second_path, &third_path] {
        queue
            .push(make_import_path_request(path.clone()))
            .expect("queue push");
    }

    let popped: Vec<PathBuf> = (0..3)
        .map(|_| {
            import_path_source(
                &queue
                    .try_pop_one(super::WorkerKind::Interactive)
                    .expect("pop"),
            )
        })
        .collect();
    assert_eq!(popped, vec![first_path, second_path, third_path]);
}

fn make_import_path_request(source_path: PathBuf) -> super::ImagePrepRequest {
    super::ImagePrepRequest::Import(ImportRequest {
        target_board_id: 1,
        queue_id: 0,
        placement_id: crate::placement::content_resolution::PlacementCreation::new()
            .reserve_placement(1, 1, eframe::egui::Pos2::ZERO)
            .placement_id,
        source: ImportSource::Path(source_path),
        reserved_world: eframe::egui::Pos2::ZERO,

        max_texture_side: usize::MAX,
        canvas_image_settings: CanvasImageSettings::default(),
        placement_target_area: DEFAULT_PLACEMENT_TARGET_AREA,

        trace: None,
    })
}

fn import_path_source(request: &super::ImagePrepRequest) -> PathBuf {
    match request {
        super::ImagePrepRequest::Import(ImportRequest {
            source: ImportSource::Path(source_path),
            ..
        }) => source_path.clone(),
        other => panic!("expected path import, got {other:?}"),
    }
}

#[test]
fn import_worker_drains_ready_results_in_capped_batches() {
    let (worker, result_tx) = ImagePrepWorker::controlled();

    for index in 0..5 {
        result_tx
            .send(ImagePrepResult::Failed {
                target_board_id: None,
                queue_id: None,
                placement_id: Some(index),
                context: format!("item-{index}"),
                error: "synthetic".to_owned(),
            })
            .unwrap();
    }

    let first_batch = worker.try_recv_up_to(4);
    let second_batch = worker.try_recv_up_to(4);

    let identities = |batch: Vec<ImagePrepResult>| {
        batch
            .into_iter()
            .map(|result| match result {
                ImagePrepResult::Failed {
                    placement_id,
                    context,
                    error,
                    ..
                } => (placement_id, context, error),
                other => panic!("expected failed completion, got {other:?}"),
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(
        identities(first_batch),
        vec![
            (Some(0), "item-0".to_owned(), "synthetic".to_owned()),
            (Some(1), "item-1".to_owned(), "synthetic".to_owned()),
            (Some(2), "item-2".to_owned(), "synthetic".to_owned()),
            (Some(3), "item-3".to_owned(), "synthetic".to_owned()),
        ]
    );
    assert_eq!(
        identities(second_batch),
        vec![(Some(4), "item-4".to_owned(), "synthetic".to_owned()),]
    );
}

#[test]
fn worker_config_derives_general_fallback_for_single_core() {
    assert_eq!(
        ImagePrepWorkerConfig::for_parallelism(1),
        ImagePrepWorkerConfig {
            general_workers: 1,
            interactive_workers: 0,
            background_workers: 0,
        }
    );
}

#[test]
fn worker_config_reserves_background_worker_on_multi_core() {
    assert_eq!(
        ImagePrepWorkerConfig::for_parallelism(4),
        ImagePrepWorkerConfig {
            general_workers: 0,
            interactive_workers: 3,
            background_workers: 1,
        }
    );
}

#[test]
fn traced_import_records_queue_wait_and_stage_timings() {
    let root = TempRoot::new("clipboard-trace");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let trace = ImagePrepTrace {
        queued_at: Instant::now() - Duration::from_millis(1),
        timings: ImagePrepTimings::default(),
    };

    let loaded = match import(
        &library,
        reserve_import_fixture(
            &library,
            ImportRequest {
                target_board_id: 1,
                queue_id: 1,
                placement_id: 0,
                source: ImportSource::Clipboard {
                    rgba: solid_rgba(64, 64),
                    width: 64,
                    height: 64,
                },
                reserved_world: eframe::egui::Pos2::ZERO,

                max_texture_side: usize::MAX,
                canvas_image_settings: CanvasImageSettings::default(),
                placement_target_area: DEFAULT_PLACEMENT_TARGET_AREA,

                trace: Some(trace),
            },
        ),
        None,
        &super::decode::ImagePrepHelper::default(),
    ) {
        ImagePrepResult::Loaded(loaded) => loaded,
        other => panic!("expected traced clipboard import to load, got {other:?}"),
    };

    let trace = loaded
        .trace
        .expect("missing trace on loaded clipboard import");
    let timings = trace.timings;
    assert!(timings.queue_wait >= Duration::from_millis(1));
    assert!(timings.store_and_persist > Duration::ZERO);
    assert!(timings.preview_prepare > Duration::ZERO);
    assert!(
        timings.worker_total
            >= timings.decode_or_load + timings.store_and_persist + timings.preview_prepare
    );
}

#[test]
fn native_preview_texture_stays_registered_until_last_reference_drops() {
    let Some(render_state) = test_render_state() else {
        eprintln!("skipping native preview texture lifecycle test: no wgpu adapter");
        return;
    };
    let preview = crate::image_preparation::image_formats::preview_image_from_rgba(
        8,
        8,
        &solid_rgba(8, 8),
        usize::MAX,
    )
    .unwrap();

    let handle = match build_preview_texture(Some(&render_state), &preview).unwrap() {
        ImagePrepPreviewTexture::Native(handle) => handle,
        ImagePrepPreviewTexture::Cpu => panic!("expected native preview texture"),
        ImagePrepPreviewTexture::Svg { .. } => panic!("expected native preview texture"),
    };
    let texture_id = handle.texture_id();
    assert!(render_state.renderer.read().texture(&texture_id).is_some());

    let duplicate = handle.clone();
    drop(handle);
    assert!(render_state.renderer.read().texture(&texture_id).is_some());

    std::thread::spawn(move || drop(duplicate)).join().unwrap();
    assert!(render_state.renderer.read().texture(&texture_id).is_none());
}

#[test]
fn load_existing_backfills_versioned_canvas_cache_and_prefers_it_on_second_load() {
    let root = TempRoot::new("load-existing-canvas-cache");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let original_path = write_png(&root, "source.png", 4000, 3000);
    let file_size = fs::metadata(&original_path).unwrap().len();
    let placement = library
        .fixture_placement(
            library.load_user_session().unwrap().active_board_id,
            crate::data::Placement::loaded(
                crate::data::PlacementState::new(
                    library.allocate_document_id(),
                    eframe::egui::Pos2::ZERO,
                    1.0,
                    0.0,
                    0,
                    false,
                ),
                library
                    .register_original(&original_path, "image/png", 4000, 3000, file_size, None)
                    .unwrap(),
            ),
        )
        .unwrap();

    let settings = downscaled_canvas_settings(2000);
    assert!(
        library
            .canvas_image_cache_path(placement.require_loaded_asset().id, settings.max_side)
            .is_none()
    );

    let first = match load_existing(
        &library,
        1,
        None,
        placement.clone(),
        usize::MAX,
        settings,
        CanvasImageLod::Working,
        None,
        None,
    ) {
        ImagePrepResult::Loaded(loaded) => loaded,
        other => panic!("expected first load_existing to succeed, got {other:?}"),
    };
    let cache_path = library
        .canvas_image_cache_path(placement.require_loaded_asset().id, settings.max_side)
        .unwrap();
    assert!(cache_path.is_file());
    assert_eq!(first.preview_width, 2000);
    assert_eq!(first.preview_height, 1500);

    fs::remove_file(&placement.require_loaded_asset().original_path).unwrap();

    let second = match load_existing(
        &library,
        1,
        None,
        placement.clone(),
        usize::MAX,
        settings,
        CanvasImageLod::Working,
        None,
        None,
    ) {
        ImagePrepResult::Loaded(loaded) => loaded,
        other => {
            panic!("expected second load_existing to use versioned canvas cache, got {other:?}")
        }
    };
    assert_eq!(second.preview_width, 2000);
    assert_eq!(second.preview_height, 1500);

    let cached = crate::image_preparation::image_formats::decode_image_file(&cache_path).unwrap();
    assert_eq!(cached.width, 2000);
    assert_eq!(cached.height, 1500);
}

#[test]
fn load_existing_preserves_animated_webp_in_downscale_mode() {
    let root = TempRoot::new("load-existing-animated-webp");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let original_path = root.join("source.webp");
    fs::write(&original_path, ANIMATED_WEBP_2X2_2_FRAMES).unwrap();
    let file_size = fs::metadata(&original_path).unwrap().len();
    let placement = library
        .fixture_placement(
            library.load_user_session().unwrap().active_board_id,
            crate::data::Placement::loaded(
                crate::data::PlacementState::new(
                    library.allocate_document_id(),
                    eframe::egui::Pos2::ZERO,
                    1.0,
                    0.0,
                    0,
                    false,
                ),
                library
                    .register_original(
                        &original_path,
                        image::ImageFormat::WebP.to_mime_type(),
                        2,
                        2,
                        file_size,
                        None,
                    )
                    .unwrap(),
            ),
        )
        .unwrap();

    let loaded = match load_existing(
        &library,
        1,
        None,
        placement,
        usize::MAX,
        downscaled_canvas_settings(2000),
        CanvasImageLod::Working,
        None,
        None,
    ) {
        ImagePrepResult::Loaded(loaded) => loaded,
        other => panic!("expected animated WebP load_existing to succeed, got {other:?}"),
    };

    let animation = loaded
        .animated_preview
        .expect("expected animated WebP preview");
    assert_eq!(animation.frames.len(), 2);
    assert_eq!(animation.width, 2);
    assert_eq!(animation.height, 2);
    assert_eq!(animation.frames[0].rgba, [252, 0, 0, 255].repeat(4));
    assert_eq!(animation.frames[1].rgba, [0, 0, 252, 255].repeat(4));
    assert_eq!(animation.frames[0].delay_seconds, 0.12);
    assert_eq!(animation.frames[1].delay_seconds, 0.12);
}

#[test]
fn load_existing_backfills_new_cache_when_canvas_max_side_changes() {
    let root = TempRoot::new("load-existing-canvas-cache-variants");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let original_path = write_png(&root, "source.png", 4000, 3000);
    let file_size = fs::metadata(&original_path).unwrap().len();
    let placement = library
        .fixture_placement(
            library.load_user_session().unwrap().active_board_id,
            crate::data::Placement::loaded(
                crate::data::PlacementState::new(
                    library.allocate_document_id(),
                    eframe::egui::Pos2::ZERO,
                    1.0,
                    0.0,
                    0,
                    false,
                ),
                library
                    .register_original(&original_path, "image/png", 4000, 3000, file_size, None)
                    .unwrap(),
            ),
        )
        .unwrap();

    let first_settings = downscaled_canvas_settings(2000);
    let second_settings = downscaled_canvas_settings(1000);
    assert!(
        library
            .canvas_image_cache_path(placement.require_loaded_asset().id, first_settings.max_side)
            .is_none()
    );
    assert!(
        library
            .canvas_image_cache_path(
                placement.require_loaded_asset().id,
                second_settings.max_side
            )
            .is_none()
    );

    match load_existing(
        &library,
        1,
        None,
        placement.clone(),
        usize::MAX,
        first_settings,
        CanvasImageLod::Working,
        None,
        None,
    ) {
        ImagePrepResult::Loaded(_) => {}
        other => panic!("expected first cache backfill to succeed, got {other:?}"),
    }
    let first_cache = library
        .canvas_image_cache_path(placement.require_loaded_asset().id, first_settings.max_side)
        .unwrap();
    assert!(first_cache.is_file());
    assert!(
        library
            .canvas_image_cache_path(
                placement.require_loaded_asset().id,
                second_settings.max_side
            )
            .is_none()
    );

    let second = match load_existing(
        &library,
        1,
        None,
        placement.clone(),
        usize::MAX,
        second_settings,
        CanvasImageLod::Working,
        None,
        None,
    ) {
        ImagePrepResult::Loaded(loaded) => loaded,
        other => panic!("expected second cache backfill to succeed, got {other:?}"),
    };
    assert_eq!(second.preview_width, 1000);
    assert_eq!(second.preview_height, 750);
    let second_cache = library
        .canvas_image_cache_path(
            placement.require_loaded_asset().id,
            second_settings.max_side,
        )
        .unwrap();
    assert_ne!(first_cache, second_cache);
    assert!(second_cache.is_file());
}

#[test]
fn crop_redecodes_original_path_and_preserves_exact_pixels() {
    let root = TempRoot::new("crop-redecode");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let source_rgba = vec![
        1, 2, 3, 255, 4, 5, 6, 255, 7, 8, 9, 255, 10, 11, 12, 255, 13, 14, 15, 255, 16, 17, 18,
        255, 19, 20, 21, 255, 22, 23, 24, 255, 25, 26, 27, 255, 28, 29, 30, 255, 31, 32, 33, 255,
        34, 35, 36, 255, 37, 38, 39, 255, 40, 41, 42, 255, 43, 44, 45, 255, 46, 47, 48, 255,
    ];
    let stored_path = library
        .store_rgba_image("crop-source", &source_rgba, 4, 4)
        .unwrap();
    let request = crate::canvas::CanvasCropRequest {
        placement_id: 1,
        source_asset: crate::data::Asset {
            id: 1,
            original_path: stored_path.clone(),
            filename: "crop-source.png".to_owned(),
            mime_type: "image/png".to_owned(),
            width: 4,
            height: 4,
        },
        crop_rect: crate::canvas::PixelCropRect {
            x: 1,
            y: 1,
            width: 2,
            height: 2,
        },
        crop_offset_local: eframe::egui::Vec2::ZERO,
    };

    let cropped = match crop_item(
        &library,
        request,
        usize::MAX,
        CanvasImageSettings::default(),
        None,
    ) {
        ImagePrepResult::Cropped(cropped) => cropped,
        other => panic!("expected cropped result, got {other:?}"),
    };
    let decoded =
        crate::image_preparation::image_formats::decode_image_file(&cropped.stored_path).unwrap();

    assert_eq!(decoded.width, 2);
    assert_eq!(decoded.height, 2);
    assert_eq!(
        decoded.rgba,
        vec![
            16, 17, 18, 255, 19, 20, 21, 255, 28, 29, 30, 255, 31, 32, 33, 255,
        ]
    );
}

fn write_png(root: &Path, filename: &str, width: u32, height: u32) -> PathBuf {
    let path = root.join(filename);
    let rgba = solid_rgba(width, height);
    image::save_buffer_with_format(
        &path,
        &rgba,
        width,
        height,
        ColorType::Rgba8,
        ImageFormat::Png,
    )
    .unwrap();
    path
}
fn write_svg(root: &Path, filename: &str, width: u32, height: u32) -> PathBuf {
    let path = root.join(filename);
    fs::write(&path, svg_bytes(width, height)).unwrap();
    path
}

fn svg_bytes(width: u32, height: u32) -> Vec<u8> {
    format!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}"><rect width="{width}" height="{height}" fill="#f8fafc"/><circle cx="80" cy="48" r="32" fill="#2563eb"/></svg>"##
        )
        .into_bytes()
}

fn solid_rgba(width: u32, height: u32) -> Vec<u8> {
    vec![255; width as usize * height as usize * 4]
}

const ANIMATED_WEBP_2X2_2_FRAMES: &[u8] = &[
    0x52, 0x49, 0x46, 0x46, 0x84, 0x00, 0x00, 0x00, 0x57, 0x45, 0x42, 0x50, 0x56, 0x50, 0x38, 0x58,
    0x0a, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x01, 0x00, 0x00, 0x41, 0x4e,
    0x49, 0x4d, 0x06, 0x00, 0x00, 0x00, 0x00, 0xff, 0x00, 0xff, 0x00, 0x00, 0x41, 0x4e, 0x4d, 0x46,
    0x28, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x01, 0x00, 0x00,
    0x78, 0x00, 0x00, 0x02, 0x56, 0x50, 0x38, 0x4c, 0x0f, 0x00, 0x00, 0x00, 0x2f, 0x01, 0x40, 0x00,
    0x00, 0x07, 0x10, 0xe5, 0x8f, 0xfe, 0x07, 0x22, 0xa2, 0xff, 0x01, 0x00, 0x41, 0x4e, 0x4d, 0x46,
    0x28, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x01, 0x00, 0x00,
    0x78, 0x00, 0x00, 0x00, 0x56, 0x50, 0x38, 0x4c, 0x0f, 0x00, 0x00, 0x00, 0x2f, 0x01, 0x40, 0x00,
    0x00, 0x07, 0x10, 0xd1, 0xfe, 0xfe, 0x07, 0x22, 0xa2, 0xff, 0x01, 0x00,
];

fn solid_png_bytes(width: u32, height: u32, color: [u8; 4]) -> Vec<u8> {
    let rgba = vec![color; (width * height) as usize]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    let image = image::RgbaImage::from_raw(width, height, rgba).unwrap();
    let mut bytes = Vec::new();
    image
        .write_to(&mut std::io::Cursor::new(&mut bytes), ImageFormat::Png)
        .unwrap();
    bytes
}

fn displayed_area(loaded: &super::PreparedPlacementImage) -> f32 {
    loaded.placement.require_loaded_asset().width as f32
        * loaded.placement.require_loaded_asset().height as f32
        * loaded.placement.state.geometry.scale
        * loaded.placement.state.geometry.scale
}

fn test_render_state() -> Option<eframe::egui_wgpu::RenderState> {
    let instance =
        eframe::wgpu::Instance::new(eframe::wgpu::InstanceDescriptor::new_without_display_handle());
    block_on(eframe::egui_wgpu::RenderState::create(
        &eframe::egui_wgpu::WgpuConfiguration::default(),
        &instance,
        None,
        eframe::egui_wgpu::RendererOptions::default(),
    ))
    .ok()
}

fn serve_bytes_once(
    path: &str,
    content_type: &str,
    body: Vec<u8>,
) -> (String, std::thread::JoinHandle<()>) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let address = listener.local_addr().unwrap();
    let path = path.to_owned();
    let request_path = path.clone();
    let content_type = content_type.to_owned();
    let handle = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0_u8; 1024];
        let bytes_read = stream.read(&mut request).unwrap();
        let request_text = String::from_utf8_lossy(&request[..bytes_read]);
        assert!(request_text.starts_with(&format!("GET {request_path} ")));

        let headers = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: {}\r\nConnection: close\r\n\r\n",
            body.len(),
            content_type
        );
        stream.write_all(headers.as_bytes()).unwrap();
        stream.write_all(&body).unwrap();
        stream.flush().unwrap();
    });
    (format!("http://{address}{path}"), handle)
}

fn next_preparation(worker: &ImagePrepWorker) -> ImagePrepResult {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if let Some(result) = worker.try_recv_up_to(1).into_iter().next() {
            return result;
        }
        assert!(
            Instant::now() < deadline,
            "preparation did not finish: {:?}",
            worker.stats()
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Reserve a real Loading Placement before worker tests prepare its content.
pub(super) fn reserve_import_fixture(
    library: &Library,
    mut request: ImportRequest,
) -> ImportRequest {
    let source = match &request.source {
        ImportSource::Path(path) => crate::data::ContentResolutionSource::ExternalPath {
            path: path.to_string_lossy().into_owned(),
        },
        ImportSource::Url(url) => {
            crate::data::ContentResolutionSource::ExternalUrl { url: url.clone() }
        }
        ImportSource::Clipboard { .. } => crate::data::ContentResolutionSource::Clipboard,
    };
    let placement = library.new_committed_loading_placement(
        request.reserved_world,
        1.0,
        eframe::egui::vec2(100.0, 100.0),
        source,
        None,
    );
    let mut creation = crate::placement::content_resolution::PlacementCreation::new();
    let reservation = creation.reserve_placement(
        request.target_board_id,
        placement.id(),
        placement.state.geometry.center,
    );
    request.queue_id = reservation.queue_id;
    request.placement_id = reservation.placement_id;
    request.reserved_world = reservation.reserved_world;
    request
}

#[test]
fn local_intake_failures_unicode_dedup_and_managed_source_restart() {
    let root = TempRoot::new("intake-managed-restart");
    let source_root = TempRoot::new("intake-external-source");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let source = write_png(&source_root, "Référence 明 A.png", 64, 48);
    let original_pixels = image::open(&source).unwrap().into_rgba8();
    for (name, bytes) in [
        ("corrupt.png", b"not png".as_slice()),
        ("empty.png", b"".as_slice()),
        ("unsupported.txt", b"plain text".as_slice()),
    ] {
        let path = source_root.join(name);
        fs::write(&path, bytes).unwrap();
        assert!(
            import_path_sync(&library, path, eframe::egui::Pos2::ZERO).is_err(),
            "{name}"
        );
    }
    let first = import_path_sync(&library, source.clone(), eframe::egui::Pos2::ZERO).unwrap();
    assert_eq!(
        image::open(&first.placement.require_loaded_asset().original_path)
            .unwrap()
            .into_rgba8(),
        original_pixels
    );
    let first_board = library.load_user_session().unwrap().active_board_id;
    library
        .fixture_placement(first_board, first.placement.clone())
        .unwrap();
    let other = library.fixture_board("Other").unwrap();
    library.set_active_board(other.id).unwrap();
    let duplicate = source_root.join("another name.png");
    fs::copy(&source, &duplicate).unwrap();
    let second = import_path_sync(
        &library,
        duplicate.clone(),
        eframe::egui::pos2(200.0, 100.0),
    )
    .unwrap();
    library
        .fixture_placement(other.id, second.placement.clone())
        .unwrap();
    assert_ne!(first.placement.id(), second.placement.id());
    assert_eq!(first.placement.asset_id(), second.placement.asset_id());
    fs::rename(&source, source_root.join("moved-original.png")).unwrap();
    fs::remove_file(duplicate).unwrap();
    drop(library);
    let reopened = Library::initialize(root.to_path_buf()).unwrap();
    for board in [first_board, other.id] {
        let placements = reopened.load_placements_for_board(board).unwrap();
        assert_eq!(placements.len(), 1);
        assert_eq!(
            image::open(&placements[0].require_loaded_asset().original_path)
                .unwrap()
                .into_rgba8(),
            original_pixels
        );
        assert!(matches!(
            load_existing(
                &reopened,
                board,
                None,
                placements[0].clone(),
                usize::MAX,
                CanvasImageSettings::default(),
                CanvasImageLod::Working,
                None,
                None
            ),
            ImagePrepResult::Loaded(_)
        ));
    }
}

#[test]
fn malformed_xpm_fails_its_import_and_the_worker_accepts_the_next_image() {
    let root = TempRoot::new("malformed-xpm-worker");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let worker = ImagePrepWorker::new_with_config(
        library.clone(),
        eframe::egui::Context::default(),
        None,
        ImagePrepWorkerConfig::single_worker(),
    );
    for (index, contents) in [
        "\"1 1 1 1\" \"é c #ff0000\" \"a\"",
        "\"1 1 1 1\" \"a c #ff0000\" \"é\"",
        "\"1 1 1 1\" \"a c #aé123\" \"a\"",
    ]
    .into_iter()
    .enumerate()
    {
        let source = root.join(format!("bad-{index}.xpm"));
        fs::write(&source, contents).unwrap();
        let mut request = reserve_import_fixture(
            &library,
            ImportRequest {
                target_board_id: 1,
                queue_id: index as u64 + 10,
                placement_id: 0,
                source: ImportSource::Path(source.clone()),
                reserved_world: eframe::egui::Pos2::ZERO,
                max_texture_side: 16,
                canvas_image_settings: CanvasImageSettings::default(),
                placement_target_area: DEFAULT_PLACEMENT_TARGET_AREA,
                trace: None,
            },
        );
        request.queue_id = index as u64 + 10;
        let placement_id = request.placement_id;
        worker
            .enqueue(super::ImagePrepRequest::Import(request))
            .unwrap();
        match next_preparation(&worker) {
            ImagePrepResult::Failed {
                target_board_id,
                queue_id,
                placement_id: failed_id,
                context,
                error,
            } => {
                assert_eq!(
                    (target_board_id, queue_id, failed_id),
                    (Some(1), Some(index as u64 + 10), Some(placement_id))
                );
                assert_eq!(context, source.display().to_string());
                assert_eq!(
                    error.split("; native fallback failed:").next().unwrap(),
                    format!(
                        "{}: XPM strings must contain only ASCII characters",
                        source.display()
                    )
                );
            }
            other => panic!("expected failed XPM completion, got {other:?}"),
        }
    }
    let source = root.join("valid.xpm");
    fs::write(&source, "\"1 1 1 1\" \"a c #1e64dc\" \"a\"").unwrap();
    let mut request = reserve_import_fixture(
        &library,
        ImportRequest {
            target_board_id: 1,
            queue_id: 20,
            placement_id: 0,
            source: ImportSource::Path(source),
            reserved_world: eframe::egui::pos2(12.0, 34.0),
            max_texture_side: 16,
            canvas_image_settings: CanvasImageSettings::default(),
            placement_target_area: DEFAULT_PLACEMENT_TARGET_AREA,
            trace: None,
        },
    );
    request.queue_id = 20;
    worker
        .enqueue(super::ImagePrepRequest::Import(request))
        .unwrap();
    match next_preparation(&worker) {
        ImagePrepResult::Loaded(loaded) => {
            assert_eq!(loaded.queue_id, Some(20));
            assert_eq!((loaded.preview_width, loaded.preview_height), (1, 1));
            assert_eq!(loaded.preview_rgba, [30, 100, 220, 255]);
            assert_eq!(
                loaded.placement.state.geometry.center,
                eframe::egui::pos2(12.0, 34.0)
            );
        }
        other => panic!("expected next import to load, got {other:?}"),
    }
    assert_eq!(worker.stats().in_flight, 0);
}

pub(super) fn helper_fixture() -> &'static Path {
    static FIXTURE: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    FIXTURE.get_or_init(|| {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target/imageprep-process-fixture")
            .join(std::process::id().to_string());
        fs::create_dir_all(&root).unwrap();
        let binary = root.join(format!("helper{}", std::env::consts::EXE_SUFFIX));
        let result = std::process::Command::new("rustc")
            .arg("--edition=2024")
            .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/helper/process.rs"))
            .arg("-o")
            .arg(&binary)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        // Warm the newly compiled executable before testing short deadlines.
        let warmup = root.join("warmup.rgba");
        let result = std::process::Command::new(&binary)
            .arg("--imageprep")
            .arg("--source=warmup.png")
            .arg(format!("--rgba-output={}", warmup.display()))
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert_eq!(fs::read(&warmup).unwrap(), [30, 100, 220, 255]);
        fs::remove_file(warmup).unwrap();
        binary
    })
}

fn assert_helper_failure_then_success(mode: &str, expected_error: &str) {
    let root = TempRoot::new("imageprep-process-recovery");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let helper = super::decode::ImagePrepHelper {
        executable: Some(helper_fixture().to_path_buf()),
        timeout: if mode == "hang" {
            Duration::from_secs(1)
        } else {
            Duration::from_secs(5)
        },
    };
    let worker_library = library.clone();
    let worker = ImagePrepWorker::new_with_processor(
        eframe::egui::Context::default(),
        ImagePrepWorkerConfig::single_worker(),
        move |request| match request {
            super::ImagePrepRequest::Import(request) => {
                if request.source.label().ends_with("panic.png") {
                    panic!("fixture panic");
                }
                import(&worker_library, request, None, &helper)
            }
            other => panic!("unexpected fixture request {other:?}"),
        },
    );
    let source = write_png(&root, &format!("{mode}.png"), 1, 1);
    let mut request = reserve_import_fixture(
        &library,
        ImportRequest {
            target_board_id: 1,
            queue_id: 10,
            placement_id: 0,
            source: ImportSource::Path(source.clone()),
            reserved_world: eframe::egui::Pos2::ZERO,
            max_texture_side: 16,
            canvas_image_settings: CanvasImageSettings::default(),
            placement_target_area: DEFAULT_PLACEMENT_TARGET_AREA,
            trace: None,
        },
    );
    request.queue_id = 10;
    let placement_id = request.placement_id;
    let started = Instant::now();
    worker
        .enqueue(super::ImagePrepRequest::Import(request))
        .unwrap();
    let result = next_preparation(&worker);
    match result {
        ImagePrepResult::Failed {
            target_board_id,
            queue_id,
            placement_id: failed_id,
            context,
            error,
        } => {
            assert_eq!(
                (target_board_id, queue_id, failed_id),
                (Some(1), Some(10), Some(placement_id))
            );
            assert_eq!(context, source.display().to_string());
            assert!(error.starts_with(expected_error), "{error}");
        }
        other => panic!("expected {mode} failure, got {other:?}"),
    }
    if mode == "hang" {
        assert!(started.elapsed() < Duration::from_secs(2));
    }
    let source = write_png(&root, "next.png", 1, 1);
    let mut request = reserve_import_fixture(
        &library,
        ImportRequest {
            target_board_id: 1,
            queue_id: 11,
            placement_id: 0,
            source: ImportSource::Path(source),
            reserved_world: eframe::egui::pos2(12.0, 34.0),
            max_texture_side: 16,
            canvas_image_settings: CanvasImageSettings::default(),
            placement_target_area: DEFAULT_PLACEMENT_TARGET_AREA,
            trace: None,
        },
    );
    request.queue_id = 11;
    worker
        .enqueue(super::ImagePrepRequest::Import(request))
        .unwrap();
    match next_preparation(&worker) {
        ImagePrepResult::Loaded(loaded) => {
            assert_eq!(loaded.queue_id, Some(11));
            assert_eq!((loaded.preview_width, loaded.preview_height), (1, 1));
            assert_eq!(loaded.preview_rgba, [30, 100, 220, 255]);
            assert_eq!(
                loaded.placement.state.geometry.center,
                eframe::egui::pos2(12.0, 34.0)
            );
        }
        other => panic!("expected next image after {mode}, got {other:?}"),
    }
    assert_eq!(worker.stats().in_flight, 0);
}

#[test]
fn helper_crash_fails_import_and_next_import_loads() {
    assert_helper_failure_then_success("crash", "imageprep helper failed");
}

#[test]
fn helper_invalid_output_fails_import_and_next_import_loads() {
    assert_helper_failure_then_success("invalid", "failed to decode imageprep helper output");
}

#[test]
fn helper_hang_fails_import_and_next_import_loads() {
    assert_helper_failure_then_success("hang", "imageprep helper timed out");
}

#[test]
fn worker_panic_fails_import_and_next_import_loads() {
    assert_helper_failure_then_success("panic", "image preparation panicked");
}

fn assert_failed_existing_preparation_preserves_saved_images(mode: &str) {
    use crate::active_board::editing::BoardEditing;
    use crate::active_board::persistence::DocumentPersistence;
    use crate::placement::completion::{CompletionContext, PreparationOutcome};
    use crate::placement::content_resolution::PlacementCreation;

    let root = TempRoot::new(&format!("existing-load-preserves-{mode}"));
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let mut placements = Vec::new();
    for (id, name, pixel) in [
        (1, mode, [30, 100, 220, 255]),
        (2, "valid", [10, 20, 30, 255]),
    ] {
        let source = root.join(format!("{name}.png"));
        image::RgbaImage::from_pixel(1, 1, image::Rgba(pixel))
            .save(&source)
            .unwrap();
        let mut placement = import_path_sync(&library, source, eframe::egui::Pos2::ZERO)
            .unwrap()
            .placement;
        placement.state.id = id;
        placements.push(library.fixture_placement(1, placement).unwrap());
    }
    assert_eq!(
        library
            .load_placements()
            .unwrap()
            .iter()
            .map(|p| (p.id(), p.asset_id()))
            .collect::<Vec<_>>(),
        vec![(1, Some(1)), (2, Some(2))],
    );
    for placement in &placements {
        if let Some(cache) = library.canvas_image_cache_path_for_lod(
            placement.require_loaded_asset().id,
            CanvasImageSettings::default().max_side,
            CanvasImageLod::Working,
        ) {
            fs::remove_file(cache).unwrap();
        }
    }
    let ctx = eframe::egui::Context::default();
    let worker_library = library.clone();
    let helper = super::decode::ImagePrepHelper {
        executable: Some(helper_fixture().to_path_buf()),
        timeout: Duration::from_secs(5),
    };
    let worker = ImagePrepWorker::new_with_processor(
        ctx.clone(),
        ImagePrepWorkerConfig::single_worker(),
        move |request| match request {
            super::ImagePrepRequest::LoadExisting {
                target_board_id,
                queue_id,
                placement,
                max_texture_side,
                canvas_image_settings,
                target_lod,
                trace,
                ..
            } => {
                if placement
                    .require_loaded_asset()
                    .original_path
                    .to_string_lossy()
                    .ends_with("panic.png")
                {
                    panic!("fixture existing-load panic");
                }
                super::existing_load::load_existing_with_helper(
                    &worker_library,
                    target_board_id,
                    queue_id,
                    placement,
                    max_texture_side,
                    canvas_image_settings,
                    target_lod,
                    trace,
                    None,
                    &helper,
                )
            }
            other => panic!("unexpected existing-load request {other:?}"),
        },
    );
    let mut canvas = crate::canvas::CanvasState::default();
    for placement in &placements {
        canvas.add_unresolved_placement(placement.clone());
    }
    let mut editing = BoardEditing::default();
    editing.accepted_board_document(&library, 1).unwrap();
    editing.persistence = DocumentPersistence::queued(library.clone());
    let mut creation = PlacementCreation::new();
    let settings = crate::data::WorkspaceSettings::default();
    let mut residency = crate::canvas::residency::ImageResidencyCoordinator::default();
    let mut context = CompletionContext {
        canvas: &mut canvas,
        library: Some(&library),
        board_id: 1,
        worker: None,
        settings: &settings,
        residency: &mut residency,
        editing: &mut editing,
    };
    for (index, placement) in placements.into_iter().enumerate() {
        worker
            .enqueue(super::ImagePrepRequest::LoadExisting {
                target_board_id: 1,
                queue_id: None,
                placement,
                max_texture_side: 16,
                canvas_image_settings: CanvasImageSettings::default(),
                target_lod: CanvasImageLod::Overview,
                priority: super::ExistingLoadPriority::Background,
                trace: None,
            })
            .unwrap();
        let result = next_preparation(&worker);
        if index == 0 {
            match &result {
                ImagePrepResult::ExistingLoadFailed { error, .. } => {
                    let expected = if mode == "crash" {
                        "imageprep helper failed"
                    } else {
                        "image preparation panicked"
                    };
                    assert!(error.starts_with(expected), "{error}");
                }
                other => panic!("expected existing-load failure, got {other:?}"),
            }
            assert!(matches!(
                creation.accept_result(&ctx, &mut context, result),
                PreparationOutcome::Status(Some(_))
            ));
        } else {
            match &result {
                ImagePrepResult::Loaded(loaded) => {
                    assert_eq!((loaded.preview_width, loaded.preview_height), (1, 1));
                    assert_eq!(loaded.preview_rgba, [30, 100, 220, 255]);
                }
                other => panic!("expected next existing image to load, got {other:?}"),
            }
            assert!(matches!(
                creation.accept_result(&ctx, &mut context, result),
                PreparationOutcome::Loaded(_)
            ));
        }
        context.editing.persistence.flush().unwrap();
        let reopened = Library::initialize(root.to_path_buf()).unwrap();
        let saved = reopened.load_placements().unwrap();
        assert_eq!(
            saved
                .iter()
                .map(|p| (p.id(), p.asset_id()))
                .collect::<Vec<_>>(),
            vec![(1, Some(1)), (2, Some(2))],
            "saved asset links after {mode} completion {index}",
        );
        assert_eq!(
            context.canvas.placement_for_id(1).unwrap().asset_id(),
            Some(1),
        );
    }
    assert_eq!(worker.stats().in_flight, 0);
}

#[test]
fn helper_failure_preserves_saved_images_after_reopen() {
    assert_failed_existing_preparation_preserves_saved_images("crash");
}

#[test]
fn existing_load_panic_preserves_saved_images_after_reopen() {
    assert_failed_existing_preparation_preserves_saved_images("panic");
}
