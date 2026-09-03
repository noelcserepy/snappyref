use super::{
    CanvasImageSettings, DEFAULT_PLACEMENT_TARGET_AREA, ImagePrepPreviewTexture, ImagePrepResult,
    ImagePrepTimings, ImagePrepTrace, ImagePrepWorker, ImagePrepWorkerConfig, arrange_rect_world,
    build_preview_texture, crop_item, display_scale_for_placement_target_area,
    file_import_perf_report, import_clipboard, import_path_sync, import_url_sync_traced,
    load_existing, rects_overlap_with_gap,
};
use crate::data::CanvasImageLod;
use crate::library::storage::Library;
use image::{ColorType, ImageFormat};
use pollster::block_on;
use rusqlite::Connection;
use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

fn downscaled_canvas_settings(max_side: u32) -> CanvasImageSettings {
    CanvasImageSettings { max_side }
}

#[test]
fn scales_down_large_portrait_images_to_target_area() {
    let scale =
        display_scale_for_placement_target_area(1000, 1500, true, DEFAULT_PLACEMENT_TARGET_AREA);
    assert!((scale - (DEFAULT_PLACEMENT_TARGET_AREA / 1_500_000.0).sqrt()).abs() < f32::EPSILON);
}

#[test]
fn scales_down_large_landscape_images_by_area_cap() {
    let scale =
        display_scale_for_placement_target_area(1920, 1080, true, DEFAULT_PLACEMENT_TARGET_AREA);
    assert!((scale - (DEFAULT_PLACEMENT_TARGET_AREA / 2_073_600.0).sqrt()).abs() < f32::EPSILON);
}

#[test]
fn scales_down_large_images_to_configured_target_area() {
    let target_area = 40_000.0;
    let scale = display_scale_for_placement_target_area(1000, 1500, true, target_area);

    assert!((scale - (target_area / 1_500_000.0).sqrt()).abs() < f32::EPSILON);
}

#[test]
fn keeps_smaller_images_at_full_scale() {
    assert_eq!(
        display_scale_for_placement_target_area(180, 240, true, DEFAULT_PLACEMENT_TARGET_AREA),
        1.0
    );
}

#[test]
fn disabling_auto_scale_keeps_full_scale() {
    assert_eq!(
        display_scale_for_placement_target_area(1200, 2000, false, DEFAULT_PLACEMENT_TARGET_AREA),
        1.0
    );
}

#[test]
fn file_imports_land_in_same_area_band() {
    let root = temp_library_root("file-import-scaling");
    let library = Library::initialize(root.clone()).unwrap();

    let portrait_path = write_png(&root, "portrait.png", 1000, 1500);
    let landscape_path = write_png(&root, "landscape.png", 1920, 1080);
    let square_path = write_png(&root, "square.png", 1000, 1000);
    let small_path = write_png(&root, "small.png", 180, 240);

    let portrait = import_path_sync(
        &library,
        portrait_path,
        eframe::egui::Pos2::ZERO,
        test_viewport_world(),
        true,
        crate::data::DEFAULT_PLACEMENT_GAP,
    )
    .unwrap();
    let landscape = import_path_sync(
        &library,
        landscape_path,
        eframe::egui::Pos2::ZERO,
        test_viewport_world(),
        true,
        crate::data::DEFAULT_PLACEMENT_GAP,
    )
    .unwrap();
    let square = import_path_sync(
        &library,
        square_path,
        eframe::egui::Pos2::ZERO,
        test_viewport_world(),
        true,
        crate::data::DEFAULT_PLACEMENT_GAP,
    )
    .unwrap();
    let small = import_path_sync(
        &library,
        small_path,
        eframe::egui::Pos2::ZERO,
        test_viewport_world(),
        true,
        crate::data::DEFAULT_PLACEMENT_GAP,
    )
    .unwrap();

    assert!((displayed_area(&portrait) - DEFAULT_PLACEMENT_TARGET_AREA).abs() < 0.1);
    assert!((displayed_area(&landscape) - DEFAULT_PLACEMENT_TARGET_AREA).abs() < 0.1);
    assert!((displayed_area(&square) - DEFAULT_PLACEMENT_TARGET_AREA).abs() < 0.1);
    assert_eq!(small.placement.state.geometry.scale, 1.0);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn clipboard_import_respects_toggle_and_small_assets_stay_unchanged() {
    let root = temp_library_root("clipboard-import-scaling");
    let library = Library::initialize(root.clone()).unwrap();

    let tall_rgba = solid_rgba(390, 844);
    let small_rgba = solid_rgba(160, 200);

    let tall = match import_clipboard(
        &library,
        1,
        1,
        Some(library.allocate_document_id()),
        tall_rgba,
        390,
        844,
        eframe::egui::Pos2::ZERO,
        None,
        test_viewport_world(),
        usize::MAX,
        CanvasImageSettings::default(),
        true,
        DEFAULT_PLACEMENT_TARGET_AREA,
        crate::data::DEFAULT_PLACEMENT_GAP,
        None,
        None,
    ) {
        ImagePrepResult::Loaded(loaded) => loaded,
        other => panic!("expected loaded tall clipboard image, got {other:?}"),
    };
    let small = match import_clipboard(
        &library,
        1,
        2,
        Some(library.allocate_document_id()),
        small_rgba,
        160,
        200,
        eframe::egui::Pos2::ZERO,
        None,
        test_viewport_world(),
        usize::MAX,
        CanvasImageSettings::default(),
        false,
        DEFAULT_PLACEMENT_TARGET_AREA,
        crate::data::DEFAULT_PLACEMENT_GAP,
        None,
        None,
    ) {
        ImagePrepResult::Loaded(loaded) => loaded,
        other => panic!("expected loaded small clipboard image, got {other:?}"),
    };

    assert!((displayed_area(&tall) - DEFAULT_PLACEMENT_TARGET_AREA).abs() < 0.1);
    assert_eq!(small.placement.state.geometry.scale, 1.0);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn clipboard_import_uses_configured_target_area() {
    let root = temp_library_root("clipboard-import-custom-area");
    let library = Library::initialize(root.clone()).unwrap();
    let target_area = 40_000.0;

    let loaded = match import_clipboard(
        &library,
        1,
        1,
        Some(library.allocate_document_id()),
        solid_rgba(1000, 1500),
        1000,
        1500,
        eframe::egui::Pos2::ZERO,
        None,
        test_viewport_world(),
        usize::MAX,
        CanvasImageSettings::default(),
        true,
        target_area,
        crate::data::DEFAULT_PLACEMENT_GAP,
        None,
        None,
    ) {
        ImagePrepResult::Loaded(loaded) => loaded,
        other => panic!("expected loaded clipboard image, got {other:?}"),
    };

    assert!((displayed_area(&loaded) - target_area).abs() < 0.1);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn clipboard_import_persists_to_queued_board_even_when_another_board_is_active() {
    let root = temp_library_root("clipboard-import-target-board");
    let library = Library::initialize(root.clone()).unwrap();
    seed_placement(&library, 42.0, 24.0, 160, 120, 1.0);
    let second = library.create_board("Second").unwrap();

    let loaded = match import_clipboard(
        &library,
        second.id,
        1,
        Some(library.allocate_document_id()),
        solid_rgba(160, 120),
        160,
        120,
        eframe::egui::pos2(42.0, 24.0),
        None,
        test_viewport_world(),
        usize::MAX,
        CanvasImageSettings::default(),
        false,
        DEFAULT_PLACEMENT_TARGET_AREA,
        crate::data::DEFAULT_PLACEMENT_GAP,
        None,
        None,
    ) {
        ImagePrepResult::Loaded(loaded) => loaded,
        other => panic!("expected target-board clipboard import to load, got {other:?}"),
    };

    assert_eq!(loaded.target_board_id, second.id);
    assert_eq!(loaded.placement.state.geometry.center.x, 42.0);
    assert_eq!(loaded.placement.state.geometry.center.y, 24.0);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn empty_board_placement_uses_requested_anchor() {
    let root = temp_library_root("empty-placement");
    let library = Library::initialize(root.clone()).unwrap();
    let anchor = eframe::egui::pos2(120.0, -48.0);

    let arranged =
        arrange_rect_world(&library, anchor, test_viewport_world(), 300.0, 200.0, 12.0).unwrap();

    assert_eq!(arranged, anchor);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn deterministic_candidate_order_prefers_right_before_below_on_tie() {
    let root = temp_library_root("placement-order");
    let library = Library::initialize(root.clone()).unwrap();
    seed_placement(&library, 0.0, 0.0, 100, 100, 1.0);

    let arranged = arrange_rect_world(
        &library,
        eframe::egui::pos2(106.0, 106.0),
        test_viewport_world(),
        100.0,
        100.0,
        12.0,
    )
    .unwrap();

    assert_eq!(arranged, eframe::egui::pos2(112.0, 0.0));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn blocked_right_side_candidate_falls_through_to_valid_lower_slot() {
    let root = temp_library_root("placement-blocked-right");
    let library = Library::initialize(root.clone()).unwrap();
    seed_placement(&library, 0.0, 0.0, 100, 100, 1.0);
    seed_placement(&library, 112.0, 0.0, 100, 100, 1.0);

    let arranged = arrange_rect_world(
        &library,
        eframe::egui::pos2(112.0, 50.0),
        test_viewport_world(),
        100.0,
        100.0,
        12.0,
    )
    .unwrap();

    assert_eq!(arranged, eframe::egui::pos2(112.0, 112.0));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn square_scoring_beats_anchor_distance_for_adjacent_imports() {
    let root = temp_library_root("placement-square-scoring");
    let library = Library::initialize(root.clone()).unwrap();
    seed_placement(&library, 0.0, 0.0, 240, 80, 1.0);

    let arranged = arrange_rect_world(
        &library,
        eframe::egui::pos2(172.0, 0.0),
        test_viewport_world(),
        80.0,
        80.0,
        12.0,
    )
    .unwrap();

    assert_eq!(arranged, eframe::egui::pos2(172.0, 0.0));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn viewport_bounding_prefers_on_screen_candidate() {
    let root = temp_library_root("placement-viewport-bounded");
    let library = Library::initialize(root.clone()).unwrap();
    seed_placement(&library, 0.0, 0.0, 240, 80, 1.0);
    let viewport = eframe::egui::Rect::from_min_max(
        eframe::egui::pos2(100.0, -60.0),
        eframe::egui::pos2(260.0, 60.0),
    );

    let arranged = arrange_rect_world(
        &library,
        eframe::egui::pos2(172.0, 0.0),
        viewport,
        80.0,
        80.0,
        12.0,
    )
    .unwrap();

    assert_eq!(arranged, eframe::egui::pos2(172.0, 0.0));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn sequential_imports_preserve_custom_gap_in_displayed_bounds() {
    let root = temp_library_root("placement-custom-gap");
    let library = Library::initialize(root.clone()).unwrap();
    let gap = 8.0;
    seed_placement(&library, 0.0, 0.0, 200, 200, 1.0);

    let second = match import_clipboard(
        &library,
        1,
        2,
        Some(library.allocate_document_id()),
        solid_rgba(200, 200),
        200,
        200,
        eframe::egui::Pos2::ZERO,
        None,
        test_viewport_world(),
        usize::MAX,
        CanvasImageSettings::default(),
        false,
        DEFAULT_PLACEMENT_TARGET_AREA,
        gap,
        None,
        None,
    ) {
        ImagePrepResult::Loaded(loaded) => loaded,
        other => panic!("expected second loaded image, got {other:?}"),
    };

    let first_rect = eframe::egui::Rect::from_center_size(
        eframe::egui::Pos2::ZERO,
        eframe::egui::vec2(200.0, 200.0),
    );
    let second_rect = eframe::egui::Rect::from_center_size(
        eframe::egui::pos2(
            second.placement.state.geometry.center.x,
            second.placement.state.geometry.center.y,
        ),
        eframe::egui::vec2(200.0, 200.0),
    );

    assert_eq!(second_rect.left() - first_rect.right(), gap);
    assert!(!rects_overlap_with_gap(first_rect, second_rect, gap));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn reserved_world_is_persisted_without_worker_relayout() {
    let root = temp_library_root("placement-reserved-world");
    let library = Library::initialize(root.clone()).unwrap();
    seed_placement(&library, 0.0, 0.0, 100, 100, 1.0);
    let reserved_world = eframe::egui::pos2(240.0, 36.0);

    let loaded = match import_clipboard(
        &library,
        1,
        2,
        Some(library.allocate_document_id()),
        solid_rgba(100, 100),
        100,
        100,
        eframe::egui::Pos2::ZERO,
        Some(reserved_world),
        test_viewport_world(),
        usize::MAX,
        CanvasImageSettings::default(),
        false,
        DEFAULT_PLACEMENT_TARGET_AREA,
        crate::data::DEFAULT_PLACEMENT_GAP,
        None,
        None,
    ) {
        ImagePrepResult::Loaded(loaded) => loaded,
        other => panic!("expected reserved clipboard import to load, got {other:?}"),
    };

    assert_eq!(loaded.placement.state.geometry.center.x, reserved_world.x);
    assert_eq!(loaded.placement.state.geometry.center.y, reserved_world.y);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn overlapping_reserved_world_is_preserved_when_persisting() {
    let root = temp_library_root("placement-preserved-reserved-world");
    let library = Library::initialize(root.clone()).unwrap();
    seed_placement(&library, 0.0, 0.0, 100, 100, 1.0);
    let reserved_world = eframe::egui::Pos2::ZERO;

    let loaded = match import_clipboard(
        &library,
        1,
        2,
        Some(library.allocate_document_id()),
        solid_rgba(100, 100),
        100,
        100,
        reserved_world,
        Some(reserved_world),
        test_viewport_world(),
        usize::MAX,
        CanvasImageSettings::default(),
        false,
        DEFAULT_PLACEMENT_TARGET_AREA,
        crate::data::DEFAULT_PLACEMENT_GAP,
        None,
        None,
    ) {
        ImagePrepResult::Loaded(loaded) => loaded,
        other => {
            panic!("expected overlapping reserved clipboard import to load, got {other:?}")
        }
    };

    assert_eq!(loaded.placement.state.geometry.center.x, reserved_world.x);
    assert_eq!(loaded.placement.state.geometry.center.y, reserved_world.y);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn url_import_downloads_and_persists_original_bytes() {
    let root = temp_library_root("url-import");
    fs::create_dir_all(&root).unwrap();
    let library = Library::initialize(root.clone()).unwrap();
    let png_bytes = solid_png_bytes(64, 48, [188, 146, 255, 255]);
    let (url, server) = serve_bytes_once("/test-image.png", "image/png", png_bytes.clone());

    let loaded = import_url_sync_traced(
        &library,
        1,
        Some(7),
        Some(library.allocate_document_id()),
        &url,
        eframe::egui::Pos2::ZERO,
        None,
        test_viewport_world(),
        usize::MAX,
        CanvasImageSettings::default(),
        false,
        DEFAULT_PLACEMENT_TARGET_AREA,
        crate::data::DEFAULT_PLACEMENT_GAP,
        None,
        None,
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

    let _ = fs::remove_dir_all(root);
}

#[test]
fn path_import_reuses_existing_exact_asset_and_removes_duplicate_original() {
    let root = temp_library_root("path-import-exact-reuse");
    fs::create_dir_all(&root).unwrap();
    let library = Library::initialize(root.clone()).unwrap();
    let source = write_png(&root, "source.png", 64, 48);

    let first = import_path_sync(
        &library,
        source.clone(),
        eframe::egui::Pos2::ZERO,
        test_viewport_world(),
        false,
        crate::data::DEFAULT_PLACEMENT_GAP,
    )
    .unwrap();
    let second = import_path_sync(
        &library,
        source,
        eframe::egui::pos2(100.0, 0.0),
        test_viewport_world(),
        false,
        crate::data::DEFAULT_PLACEMENT_GAP,
    )
    .unwrap();

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

    let _ = fs::remove_dir_all(root);
}

#[test]
fn url_import_preserves_svg_as_vector_source() {
    let root = temp_library_root("url-svg-import");
    fs::create_dir_all(&root).unwrap();
    let library = Library::initialize(root.clone()).unwrap();
    let svg_bytes = svg_bytes(128, 96);
    let (url, server) = serve_bytes_once("/remote-icon.svg", "image/svg+xml", svg_bytes.clone());

    let loaded = import_url_sync_traced(
        &library,
        1,
        Some(9),
        Some(library.allocate_document_id()),
        &url,
        eframe::egui::Pos2::ZERO,
        None,
        test_viewport_world(),
        usize::MAX,
        CanvasImageSettings::default(),
        false,
        DEFAULT_PLACEMENT_TARGET_AREA,
        crate::data::DEFAULT_PLACEMENT_GAP,
        None,
        None,
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

    let _ = fs::remove_dir_all(root);
}

#[test]
fn svg_import_preserves_vector_source_and_skips_canvas_cache() {
    let root = temp_library_root("svg-import");
    fs::create_dir_all(&root).unwrap();
    let library = Library::initialize(root.clone()).unwrap();
    let svg_path = write_svg(&root, "icon.svg", 320, 180);

    let loaded = import_path_sync(
        &library,
        svg_path.clone(),
        eframe::egui::Pos2::ZERO,
        test_viewport_world(),
        false,
        crate::data::DEFAULT_PLACEMENT_GAP,
    )
    .unwrap();

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

    let _ = fs::remove_dir_all(root);
}

#[test]
fn xpm_import_uses_raster_pipeline_with_original_mime_type() {
    let root = temp_library_root("xpm-import");
    fs::create_dir_all(&root).unwrap();
    let library = Library::initialize(root.clone()).unwrap();
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

    let loaded = import_path_sync(
        &library,
        source.clone(),
        eframe::egui::Pos2::ZERO,
        test_viewport_world(),
        false,
        crate::data::DEFAULT_PLACEMENT_GAP,
    )
    .unwrap();

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

    let _ = fs::remove_dir_all(root);
}

#[test]
fn crop_item_rejects_svg_with_clear_error() {
    let root = temp_library_root("svg-crop");
    fs::create_dir_all(&root).unwrap();
    let library = Library::initialize(root.clone()).unwrap();
    let svg_path = write_svg(&root, "icon.svg", 100, 80);
    let loaded = import_path_sync(
        &library,
        svg_path,
        eframe::egui::Pos2::ZERO,
        test_viewport_world(),
        false,
        crate::data::DEFAULT_PLACEMENT_GAP,
    )
    .unwrap();

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

    let _ = fs::remove_dir_all(root);
}

#[test]
fn load_existing_svg_uses_original_vector_source() {
    let root = temp_library_root("svg-existing-load");
    fs::create_dir_all(&root).unwrap();
    let library = Library::initialize(root.clone()).unwrap();
    let svg_path = write_svg(&root, "icon.svg", 240, 120);
    let imported = import_path_sync(
        &library,
        svg_path,
        eframe::egui::Pos2::ZERO,
        test_viewport_world(),
        false,
        crate::data::DEFAULT_PLACEMENT_GAP,
    )
    .unwrap();

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

    let _ = fs::remove_dir_all(root);
}

#[test]
fn interactive_import_queue_pops_smallest_bytes_first() {
    let root = temp_library_root("interactive-queue-by-bytes");
    fs::create_dir_all(&root).unwrap();
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

    let _ = fs::remove_dir_all(root);
}

#[test]
fn interactive_import_queue_breaks_ties_by_arrival_order() {
    let root = temp_library_root("interactive-queue-tie-break");
    fs::create_dir_all(&root).unwrap();
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

    let _ = fs::remove_dir_all(root);
}

fn make_import_path_request(source_path: PathBuf) -> super::ImagePrepRequest {
    super::ImagePrepRequest::ImportPath {
        target_board_id: 1,
        queue_id: 0,
        placement_id: None,
        source_path,
        desired_world: eframe::egui::Pos2::ZERO,
        reserved_world: None,
        viewport_world: test_viewport_world(),
        max_texture_side: usize::MAX,
        canvas_image_settings: CanvasImageSettings::default(),
        auto_scale: true,
        placement_target_area: DEFAULT_PLACEMENT_TARGET_AREA,
        placement_gap: crate::data::DEFAULT_PLACEMENT_GAP,
        trace: None,
    }
}

fn import_path_source(request: &super::ImagePrepRequest) -> PathBuf {
    match request {
        super::ImagePrepRequest::ImportPath { source_path, .. } => source_path.clone(),
        other => panic!("expected ImportPath, got {other:?}"),
    }
}

#[test]
fn import_worker_drains_ready_results_in_capped_batches() {
    let (result_tx, result_rx) = mpsc::channel();
    let worker = ImagePrepWorker {
        queue: Arc::new(super::RequestQueue::default()),
        receiver: result_rx,
        workers: Vec::new(),
    };

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

    assert_eq!(first_batch.len(), 4);
    assert_eq!(second_batch.len(), 1);
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
fn traced_clipboard_import_populates_non_negative_timings() {
    let root = temp_library_root("clipboard-trace");
    let library = Library::initialize(root.clone()).unwrap();
    let trace = ImagePrepTrace {
        queued_at: Instant::now()
            .checked_sub(Duration::from_millis(1))
            .unwrap_or_else(Instant::now),
        timings: ImagePrepTimings::default(),
    };

    let loaded = match import_clipboard(
        &library,
        1,
        1,
        Some(library.allocate_document_id()),
        solid_rgba(64, 64),
        64,
        64,
        eframe::egui::Pos2::ZERO,
        None,
        test_viewport_world(),
        usize::MAX,
        CanvasImageSettings::default(),
        false,
        DEFAULT_PLACEMENT_TARGET_AREA,
        crate::data::DEFAULT_PLACEMENT_GAP,
        Some(trace),
        None,
    ) {
        ImagePrepResult::Loaded(loaded) => loaded,
        other => panic!("expected traced clipboard import to load, got {other:?}"),
    };

    let trace = loaded
        .trace
        .expect("missing trace on loaded clipboard import");
    assert!(trace.timings.queue_wait >= Duration::ZERO);
    assert!(trace.timings.decode_or_load >= Duration::ZERO);
    assert!(trace.timings.store_and_persist >= Duration::ZERO);
    assert!(trace.timings.preview_prepare >= Duration::ZERO);
    assert!(trace.timings.worker_total >= trace.timings.store_and_persist);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn file_import_perf_report_imports_jpeg_and_reports_timings() {
    let root = temp_library_root("file-import-perf-report");
    fs::create_dir_all(&root).unwrap();
    let source_path = write_jpeg(&root, "source.jpg", 64, 48);

    let report = file_import_perf_report(source_path.clone()).unwrap();

    assert_eq!(report.source_path, source_path);
    assert_eq!(report.source_width, 64);
    assert_eq!(report.source_height, 48);
    assert_eq!(report.preview_width, 64);
    assert_eq!(report.preview_height, 48);
    assert!(report.source_file_size > 0);
    assert!(report.canvas_cache_file_size.unwrap_or_default() > 0);
    assert!(report.timings.decode_or_load >= Duration::ZERO);
    assert!(report.timings.store_and_persist >= Duration::ZERO);
    assert!(report.timings.preview_prepare >= Duration::ZERO);
    assert!(report.timings.worker_total >= report.timings.decode_or_load);
    assert!(report.total_ms >= 0.0);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn native_preview_texture_stays_registered_until_last_reference_drops() {
    let Some(render_state) = test_render_state() else {
        eprintln!("skipping native preview texture lifecycle test: no wgpu adapter");
        return;
    };
    let preview =
        crate::jobs::image_formats::preview_image_from_rgba(8, 8, &solid_rgba(8, 8), usize::MAX)
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
    let root = temp_library_root("load-existing-canvas-cache");
    let library = Library::initialize(root.clone()).unwrap();
    let original_path = write_png(&root, "source.png", 4000, 3000);
    let file_size = fs::metadata(&original_path).unwrap().len();
    let placement = library
        .persist_imported_image(
            &original_path,
            "image/png",
            4000,
            3000,
            file_size,
            eframe::egui::Pos2::ZERO,
            1.0,
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

    let cached = crate::jobs::image_formats::decode_image_file(&cache_path).unwrap();
    assert_eq!(cached.width, 2000);
    assert_eq!(cached.height, 1500);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn load_existing_preserves_animated_webp_in_downscale_mode() {
    let root = temp_library_root("load-existing-animated-webp");
    fs::create_dir_all(&root).unwrap();
    let library = Library::initialize(root.clone()).unwrap();
    let original_path = root.join("source.webp");
    fs::write(&original_path, ANIMATED_WEBP_2X2_2_FRAMES).unwrap();
    let file_size = fs::metadata(&original_path).unwrap().len();
    let placement = library
        .persist_imported_image(
            &original_path,
            image::ImageFormat::WebP.to_mime_type(),
            2,
            2,
            file_size,
            eframe::egui::Pos2::ZERO,
            1.0,
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

    let _ = fs::remove_dir_all(root);
}

#[test]
fn load_existing_backfills_new_cache_when_canvas_max_side_changes() {
    let root = temp_library_root("load-existing-canvas-cache-variants");
    let library = Library::initialize(root.clone()).unwrap();
    let original_path = write_png(&root, "source.png", 4000, 3000);
    let file_size = fs::metadata(&original_path).unwrap().len();
    let placement = library
        .persist_imported_image(
            &original_path,
            "image/png",
            4000,
            3000,
            file_size,
            eframe::egui::Pos2::ZERO,
            1.0,
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

    let _ = fs::remove_dir_all(root);
}

#[test]
fn downscale_mode_ignores_legacy_thumb_cache_paths() {
    let root = temp_library_root("load-existing-legacy-thumb");
    let library = Library::initialize(root.clone()).unwrap();
    let original_path = write_png(&root, "source.png", 4000, 3000);
    let file_size = fs::metadata(&original_path).unwrap().len();
    let placement = library
        .persist_imported_image(
            &original_path,
            "image/png",
            4000,
            3000,
            file_size,
            eframe::egui::Pos2::ZERO,
            1.0,
        )
        .unwrap();

    let legacy_thumb_path = root
        .join("thumbs")
        .join(format!("asset-{}.png", placement.require_loaded_asset().id));
    image::save_buffer_with_format(
        &legacy_thumb_path,
        &solid_rgba(64, 64),
        64,
        64,
        ColorType::Rgba8,
        ImageFormat::Png,
    )
    .unwrap();
    fs::remove_file(&placement.require_loaded_asset().original_path).unwrap();

    match load_existing(
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
        ImagePrepResult::Failed { .. } => {}
        other => panic!("expected legacy thumb path to be ignored, got {other:?}"),
    }

    let _ = fs::remove_dir_all(root);
}

#[test]
fn crop_redecodes_original_path_and_preserves_exact_pixels() {
    let root = temp_library_root("crop-redecode");
    let library = Library::initialize(root.clone()).unwrap();
    let source_rgba = vec![
        1, 2, 3, 255, 4, 5, 6, 255, 7, 8, 9, 255, 10, 11, 12, 255, 13, 14, 15, 255, 16, 17, 18,
        255, 19, 20, 21, 255, 22, 23, 24, 255, 25, 26, 27, 255, 28, 29, 30, 255, 31, 32, 33, 255,
        34, 35, 36, 255, 37, 38, 39, 255, 40, 41, 42, 255, 43, 44, 45, 255, 46, 47, 48, 255,
    ];
    let stored_path = library
        .store_rgba_image("crop-source", &source_rgba, 4, 4)
        .unwrap();
    let file_size = fs::metadata(&stored_path).unwrap().len();
    let request = crate::canvas::CanvasCropRequest {
        placement_id: 1,
        source_asset: crate::data::Asset {
            id: 1,
            original_path: stored_path.clone(),
            filename: "crop-source.png".to_owned(),
            mime_type: "image/png".to_owned(),
            width: 4,
            height: 4,
            file_size,
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
    let decoded = crate::jobs::image_formats::decode_image_file(&cropped.stored_path).unwrap();

    assert_eq!(decoded.width, 2);
    assert_eq!(decoded.height, 2);
    assert_eq!(
        decoded.rgba,
        vec![
            16, 17, 18, 255, 19, 20, 21, 255, 28, 29, 30, 255, 31, 32, 33, 255,
        ]
    );

    let _ = fs::remove_dir_all(root);
}

fn temp_library_root(label: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("snappyref-{label}-{unique}"))
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

fn write_jpeg(root: &Path, filename: &str, width: u32, height: u32) -> PathBuf {
    let path = root.join(filename);
    let rgb = vec![255; width as usize * height as usize * 3];
    image::save_buffer_with_format(
        &path,
        &rgb,
        width,
        height,
        ColorType::Rgb8,
        ImageFormat::Jpeg,
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
        .flat_map(|pixel| pixel)
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

fn test_viewport_world() -> eframe::egui::Rect {
    eframe::egui::Rect::from_min_max(
        eframe::egui::pos2(-10_000.0, -10_000.0),
        eframe::egui::pos2(10_000.0, 10_000.0),
    )
}

fn seed_placement(library: &Library, x: f32, y: f32, width: u32, height: u32, scale: f32) {
    let stored_path = library
        .store_rgba_image(
            &format!("seed-{x}-{y}"),
            &solid_rgba(width, height),
            width,
            height,
        )
        .unwrap();
    let file_size = fs::metadata(&stored_path).unwrap().len();
    library
        .persist_imported_image(
            &stored_path,
            image::ImageFormat::Png.to_mime_type(),
            width,
            height,
            file_size,
            eframe::egui::pos2(x, y),
            scale,
        )
        .unwrap();
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
