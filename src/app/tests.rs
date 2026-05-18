use super::{
    AppConfig, ArrangementAction, BoardSummariesCache, EmbeddingPhaseMarkers,
    ModelLoadTimingSummary, SUPPORTED_SHORTCUTS, SnappyRefApp, apply_arrangement_action,
};
use crate::data::{
    CanvasAnnotationKind, CanvasBackground, CanvasStackItem, CanvasStrokeWidth, CanvasTextSize,
    ImageAsset, Placement, PlacementState,
};
use crate::jobs::PreparedPlacementImage;
use crate::library::storage::Library;
#[cfg(feature = "semantic-search")]
use crate::search::palette::{
    PALETTE_WIDTH, search_result_grid_content_height, search_result_grid_layout,
    search_result_tile_height,
};
#[cfg(feature = "semantic-search")]
use crate::search::semantic::{LocalModelStoreStatus, SearchResultAssetEntry, SearchScope};
#[cfg(feature = "semantic-search")]
use crate::sources::crop::CropUndoEntry;
use crate::ui::sidebar::{
    SidebarTab, block_sidebar_input, should_close_sidebar, show_sidebar, sidebar_rect,
};
use eframe::egui;
use egui_kittest::{Harness, kittest::Queryable};
use std::fs;
use std::path::PathBuf;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[test]
fn embedding_phase_markers_report_summary_durations() {
    let markers = EmbeddingPhaseMarkers {
        first_embedding_queued_ms: Some(100.0),
        semantic_model_load_started_ms: Some(125.0),
        semantic_model_ready_ms: Some(425.0),
        first_embedding_started_ms: Some(450.0),
        first_embedding_done_ms: Some(475.0),
        all_embeddings_done_ms: Some(825.0),
        semantic_model_unload_requested_ms: Some(900.0),
        semantic_model_unloaded_ms: Some(940.0),
        model_load_timing: Some(ModelLoadTimingSummary {
            assets_resolved_ms: 5.0,
            tokenizer_loaded_ms: 0.0,
            text_session_loaded_ms: 0.0,
            vision_session_loaded_ms: 290.0,
            total_ms: 300.0,
        }),
    };

    let summary = markers.summary().unwrap();

    assert_eq!(summary.queue_to_model_load_started_ms, Some(25.0));
    assert_eq!(summary.model_cold_mount_ms, Some(300.0));
    assert_eq!(
        summary.first_embedding_start_after_model_ready_ms,
        Some(25.0)
    );
    assert_eq!(summary.first_embedding_after_model_ready_ms, Some(50.0));
    assert_eq!(summary.first_embedding_run_ms, Some(25.0));
    assert_eq!(summary.total_import_embedding_drain_ms, Some(725.0));
    assert_eq!(summary.model_unload_ms, Some(40.0));
    assert_eq!(
        summary.model_load_timing.unwrap().vision_session_loaded_ms,
        290.0
    );
}

#[test]
fn persist_settings_if_changed_true_calls_save_once() {
    let mut calls = 0;
    SnappyRefApp::persist_settings_if_changed_with(true, || {
        calls += 1;
    });
    assert_eq!(calls, 1);
}

#[test]
fn persist_settings_if_changed_false_skips_save() {
    let mut calls = 0;
    SnappyRefApp::persist_settings_if_changed_with(false, || {
        calls += 1;
    });
    assert_eq!(calls, 0);
}

#[test]
fn save_user_session_round_trips_import_gap() {
    let root = temp_library_root("snappyref-app-gap");
    let library = Library::initialize(root.clone()).unwrap();
    let mut app = test_app(Some(library.clone()));
    app.settings.auto_image_scaling = false;
    app.settings.import_gap = 37.0;
    app.settings.import_target_canvas_area = 45_000.0;
    app.settings.canvas_image_max_side = 4096;
    app.canvas
        .set_background(crate::data::CanvasBackground::Grid);

    app.save_user_session();

    let session = library.load_user_session().unwrap();
    assert!(!session.settings.auto_image_scaling);
    assert_eq!(session.settings.import_gap, 37.0);
    assert_eq!(session.settings.import_target_canvas_area, 45_000.0);
    assert_eq!(session.settings.canvas_image_max_side, 4096);
    assert_eq!(
        session.viewport.canvas_background,
        crate::data::CanvasBackground::Grid
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn app_config_restores_always_on_top_workspace_setting() {
    let root = temp_library_root("snappyref-always-on-top-startup");
    let library = Library::initialize(root.clone()).unwrap();
    let mut session = library.load_user_session().unwrap();
    session.settings.always_on_top = true;
    library.save_user_session(&session).unwrap();

    let config = AppConfig {
        screenshot_path: None,
        library_root: Ok(root.clone()),
        no_startup_focus: false,
        perf_log: false,
        perf_report: false,
        perf_decode_path: None,
        perf_import_path: None,
        perf_embedding_lod: false,
        perf_embedding_lod_limit: 30,
        perf_scenario: None,
        perf_corpus: None,
        perf_run_dir: None,
        startup_trace: false,
        #[cfg(debug_assertions)]
        verification_mode: None,
        #[cfg(debug_assertions)]
        verification_search_query: None,
        #[cfg(all(debug_assertions, feature = "semantic-search"))]
        verification_search_scope: crate::search::semantic::SearchScope::ThisBoard,
        update_smoke_install: false,
        update_smoke_status_path: None,
    };

    assert!(
        config
            .restored_workspace_settings()
            .is_some_and(|settings| settings.always_on_top)
    );

    let _ = fs::remove_dir_all(root);
}

#[cfg(feature = "semantic-search")]
#[test]
fn first_launch_semantic_search_prompt_waits_for_storage_and_missing_assets() {
    let root = temp_library_root("snappyref-semantic-prompt");
    let library = Library::initialize(root.clone()).unwrap();
    let mut app = test_app(Some(library));

    assert!(!app.should_show_semantic_search_first_launch_prompt());

    app.search
        .set_local_model_store_status_for_verification(LocalModelStoreStatus::Missing);
    assert!(app.should_show_semantic_search_first_launch_prompt());

    app.settings.semantic_search_first_launch_prompt_seen = true;
    assert!(!app.should_show_semantic_search_first_launch_prompt());

    let _ = fs::remove_dir_all(root);
}

#[cfg(feature = "semantic-search")]
#[test]
fn first_launch_semantic_search_prompt_skips_installed_and_migrated_assets() {
    let root = temp_library_root("snappyref-semantic-prompt-installed");
    let library = Library::initialize(root.clone()).unwrap();
    let mut app = test_app(Some(library));

    app.search
        .set_local_model_store_status_for_verification(LocalModelStoreStatus::Installed);
    assert!(!app.should_show_semantic_search_first_launch_prompt());

    app.search.set_local_model_store_status_for_verification(
        LocalModelStoreStatus::InstalledViaMigration,
    );
    assert!(!app.should_show_semantic_search_first_launch_prompt());

    let _ = fs::remove_dir_all(root);
}

#[cfg(feature = "semantic-search")]
#[test]
fn first_launch_semantic_search_prompt_skips_without_storage() {
    let mut app = test_app(None);
    app.search
        .set_local_model_store_status_for_verification(LocalModelStoreStatus::Missing);

    assert!(!app.should_show_semantic_search_first_launch_prompt());
}

#[cfg(feature = "semantic-search")]
#[test]
fn first_launch_semantic_search_prompt_not_now_persists_suppression() {
    let root = temp_library_root("snappyref-semantic-prompt-not-now");
    let library = Library::initialize(root.clone()).unwrap();
    let mut app = test_app(Some(library.clone()));
    app.search
        .set_local_model_store_status_for_verification(LocalModelStoreStatus::Missing);

    app.decline_semantic_search_first_launch_prompt();

    assert!(!app.should_show_semantic_search_first_launch_prompt());
    assert!(
        library
            .load_user_session()
            .unwrap()
            .settings
            .semantic_search_first_launch_prompt_seen
    );

    let _ = fs::remove_dir_all(root);
}

#[cfg(feature = "semantic-search")]
#[test]
fn first_launch_semantic_search_prompt_enable_persists_seen_and_starts_install() {
    let ctx = egui::Context::default();
    let root = temp_library_root("snappyref-semantic-prompt-enable");
    let library = Library::initialize(root.clone()).unwrap();
    let starts = std::rc::Rc::new(std::cell::Cell::new(0));
    let observed = starts.clone();
    let mut app = test_app(Some(library.clone()));
    app.search
        .set_local_model_store_status_for_verification(LocalModelStoreStatus::Missing);
    app.search.set_semantic_install_for_test(move |_| {
        observed.set(observed.get() + 1);
    });

    app.enable_semantic_search_from_first_launch_prompt(&ctx);

    assert_eq!(starts.get(), 1);
    assert_eq!(
        app.search.local_model_store_status(),
        LocalModelStoreStatus::Downloading { percent: 0 }
    );
    assert!(
        library
            .load_user_session()
            .unwrap()
            .settings
            .semantic_search_first_launch_prompt_seen
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
#[cfg(feature = "semantic-search")]
fn removing_semantic_search_keeps_first_launch_prompt_seen() {
    let mut app = test_app(None);
    app.settings.semantic_search_first_launch_prompt_seen = true;
    app.search
        .set_local_model_store_status_for_verification(LocalModelStoreStatus::Installed);

    app.search
        .remove_semantic_search_assets_from_settings(&mut app.status);

    assert!(app.settings.semantic_search_first_launch_prompt_seen);
    assert_eq!(
        app.search.local_model_store_status(),
        LocalModelStoreStatus::Missing
    );
    assert!(!app.should_show_semantic_search_first_launch_prompt());
}

#[test]
fn save_user_session_round_trips_sidebar_state() {
    let root = temp_library_root("snappyref-app-sidebar-session");
    let library = Library::initialize(root.clone()).unwrap();
    let mut app = test_app(Some(library.clone()));
    app.sidebar_open = true;
    app.sidebar_tab = SidebarTab::Boards;

    app.save_user_session();

    let mut reopened = test_app(Some(library));
    reopened.load_user_session();
    assert!(reopened.sidebar_open);
    assert_eq!(reopened.sidebar_tab, SidebarTab::Boards);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn board_summaries_are_served_from_cache_until_refreshed() {
    let root = temp_library_root("snappyref-board-summary-cache");
    let library = Library::initialize(root.clone()).unwrap();
    let mut app = test_app(Some(library.clone()));

    app.refresh_board_summaries();
    assert_eq!(app.board_summaries().len(), 1);

    library.create_board("Second").unwrap();
    assert_eq!(
        app.board_summaries().len(),
        1,
        "rendering should use cached Board Summaries, not query storage every frame"
    );

    app.refresh_board_summaries();
    assert_eq!(app.board_summaries().len(), 2);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn creating_board_refreshes_board_summaries_cache() {
    let root = temp_library_root("snappyref-board-summary-create-refresh");
    let library = Library::initialize(root.clone()).unwrap();
    let mut app = test_app(Some(library));
    app.refresh_board_summaries();

    app.create_and_switch_board(true);

    let summaries = app.board_summaries();
    assert_eq!(summaries.len(), 2);
    assert!(
        summaries
            .iter()
            .any(|summary| summary.board.name == "Board 2")
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn persist_canvas_changes_saves_viewport_changes() {
    let root = temp_library_root("snappyref-app-viewport-session");
    let library = Library::initialize(root.clone()).unwrap();
    let mut app = test_app(Some(library.clone()));
    app.canvas
        .apply_viewport_state(&crate::data::ViewportState {
            camera_pan_x: 10.0,
            camera_pan_y: 20.0,
            camera_zoom: 1.75,
            canvas_background: crate::data::CanvasBackground::Dots,
        });

    app.persist_canvas_changes();

    let session = library.load_user_session().unwrap();
    assert_eq!(session.viewport.camera_pan_x, 10.0);
    assert_eq!(session.viewport.camera_pan_y, 20.0);
    assert_eq!(session.viewport.camera_zoom, 1.75);
    assert_eq!(
        session.viewport.canvas_background,
        crate::data::CanvasBackground::Dots
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn debounced_canvas_persistence_defers_session_only_viewport_changes() {
    let ctx = egui::Context::default();
    let root = temp_library_root("snappyref-app-viewport-session-debounce");
    let library = Library::initialize(root.clone()).unwrap();
    let mut app = test_app(Some(library.clone()));
    app.canvas
        .apply_viewport_state(&crate::data::ViewportState {
            camera_pan_x: 10.0,
            camera_pan_y: 20.0,
            camera_zoom: 1.75,
            canvas_background: crate::data::CanvasBackground::Dots,
        });

    app.persist_canvas_changes_debounced(&ctx);

    let session = library.load_user_session().unwrap();
    assert_ne!(session.viewport.camera_pan_x, 10.0);
    assert!(app.pending_user_session.is_some());

    app.pending_user_session_save_at = Some(Instant::now() - Duration::from_millis(1));
    app.persist_canvas_changes_debounced(&ctx);

    let session = library.load_user_session().unwrap();
    assert_eq!(session.viewport.camera_pan_x, 10.0);
    assert_eq!(session.viewport.camera_pan_y, 20.0);
    assert_eq!(session.viewport.camera_zoom, 1.75);
    assert!(app.pending_user_session.is_none());

    let _ = fs::remove_dir_all(root);
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
        let asset = ImageAsset {
            id: *id,
            original_path: PathBuf::from(format!("fixture-{id}.png")),
            filename: format!("fixture-{id}.png"),
            mime_type: "image/png".to_owned(),
            width: *width,
            height: *height,
            file_size: 0,
            imported_at: 0,
        };
        let placement = Placement::loaded(
            PlacementState {
                id: *id,
                asset_id: Some(*id),
                center: eframe::egui::pos2(index as f32 * 500.0, index as f32 * 120.0),
                scale: 1.0,
                rotation: 0.0,
                z_index: index as i32,
                archived: false,
            },
            asset,
        );
        let rgba = vec![255; (*width as usize) * (*height as usize) * 4];
        canvas.add_prepared_placement_image(
            &ctx,
            PreparedPlacementImage {
                target_board_id: 1,
                queue_id: None,
                placement,
                preview_rgba: rgba,
                preview_width: *width,
                preview_height: *height,
                animated_preview: None,
                preview_texture: crate::jobs::ImportedPreviewTexture::Cpu,
                loaded_lod: crate::data::CanvasImageLod::Working,
                existing_load_diagnostics: None,
                origin: crate::jobs::ImportOrigin::Existing,
                trace: None,
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
fn sidebar_close_helper_only_closes_for_outside_clicks() {
    let sidebar_rect = egui::Rect::from_min_max(egui::pos2(20.0, 30.0), egui::pos2(220.0, 180.0));

    assert!(!should_close_sidebar(
        true,
        Some(egui::pos2(60.0, 90.0)),
        sidebar_rect
    ));
    assert!(should_close_sidebar(
        true,
        Some(egui::pos2(300.0, 90.0)),
        sidebar_rect
    ));
    assert!(!should_close_sidebar(
        false,
        Some(egui::pos2(300.0, 90.0)),
        sidebar_rect
    ));
    assert!(!should_close_sidebar(true, None, sidebar_rect));
}

#[test]
fn cross_board_paste_creates_new_images_and_annotations_on_active_board() {
    let ctx = egui::Context::default();
    let root = temp_library_root("snappyref-cross-board-paste");
    let library = Library::initialize(root.clone()).unwrap();
    let first = library.list_boards().unwrap()[0].clone();
    let second = library.create_board("Second").unwrap();
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
    let source_annotation = library
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
    app.canvas.add_prepared_placement_image(
        &ctx,
        PreparedPlacementImage {
            target_board_id: first.id,
            queue_id: None,
            placement: source_item.clone(),
            preview_rgba: vec![255, 0, 0, 255],
            preview_width: 1,
            preview_height: 1,
            animated_preview: None,
            preview_texture: crate::jobs::ImportedPreviewTexture::Cpu,
            loaded_lod: crate::data::CanvasImageLod::Working,
            existing_load_diagnostics: None,
            origin: crate::jobs::ImportOrigin::Existing,
            trace: None,
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
        .save_canvas_stack_order(&source_stack_order)
        .unwrap();
    app.canvas
        .select_content(&[source_item.state.id], &[source_annotation.id]);

    let payload = app.copy_selected_board_payload().unwrap();
    library.set_active_board(second.id).unwrap();
    app.active_board_id = second.id;
    app.paste_board_payload_at(&payload, egui::pos2(300.0, 300.0))
        .unwrap();

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
        pasted_annotations[0].start_x - pasted_items[0].state.center.x,
        pasted_annotations[0].start_y - pasted_items[0].state.center.y,
    );
    let source_delta = egui::vec2(
        source_annotation.start_x - source_item.state.center.x,
        source_annotation.start_y - source_item.state.center.y,
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
    assert_eq!(
        library.load_canvas_stack_order().unwrap(),
        pasted_stack_order
    );
    library.set_active_board(first.id).unwrap();
    assert_eq!(
        library.load_canvas_stack_order().unwrap(),
        source_stack_order
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn delete_selected_annotation_records_undo_history() {
    let root = temp_library_root("snappyref-delete-annotation-history");
    let library = Library::initialize(root.clone()).unwrap();
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
    app.canvas.add_annotation(annotation.clone());
    app.canvas.select_content(&[], &[annotation.id]);

    app.delete_selected_items_from_board(test_screen_rect());
    assert!(library.load_canvas_annotations().unwrap().is_empty());

    assert_eq!(
        app.document_history
            .undo(&library)
            .unwrap()
            .map(|applied| applied.label),
        Some("Deleted 1 annotation".to_owned())
    );
    assert_eq!(library.load_canvas_annotations().unwrap(), vec![annotation]);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn undo_delete_annotation_patches_active_canvas_without_board_reload() {
    let ctx = egui::Context::default();
    let root = temp_library_root("snappyref-undo-no-board-reload");
    let library = Library::initialize(root.clone()).unwrap();
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
    app.import_worker = None;
    app.canvas.add_annotation(annotation.clone());
    app.canvas.select_content(&[], &[annotation.id]);

    app.delete_selected_items_from_board(test_screen_rect());
    assert!(app.canvas.annotation_for_id(annotation.id).is_none());

    app.undo_document_history(&ctx);

    assert_eq!(
        app.canvas.annotation_for_id(annotation.id),
        Some(annotation)
    );
    assert_eq!(app.status, "Undid Deleted 1 annotation");

    let _ = fs::remove_dir_all(root);
}

#[test]
fn paste_preserves_mixed_selection_stack_order_and_places_group_at_front() {
    let ctx = egui::Context::default();
    let root = temp_library_root("snappyref-mixed-paste-stack-order");
    let library = Library::initialize(root.clone()).unwrap();
    let image_path = library
        .store_rgba_image("source", &[255, 0, 0, 255], 1, 1)
        .unwrap();
    let back_item = library
        .persist_imported_image(&image_path, "image/png", 1, 1, 4, egui::pos2(0.0, 0.0), 1.0)
        .unwrap();
    let source_item = library
        .create_placement_from_asset(
            back_item.asset.as_ref().expect("loaded placement asset").id,
            egui::pos2(10.0, 20.0),
            1.0,
            0.0,
            0,
        )
        .unwrap();
    let source_annotation = library
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
    let front_item = library
        .create_placement_from_asset(
            back_item.asset.as_ref().expect("loaded placement asset").id,
            egui::pos2(100.0, 100.0),
            1.0,
            0.0,
            0,
        )
        .unwrap();
    let mut app = test_app(Some(library.clone()));
    for placement in [&back_item, &source_item, &front_item] {
        app.canvas.add_prepared_placement_image(
            &ctx,
            PreparedPlacementImage {
                target_board_id: 1,
                queue_id: None,
                placement: placement.clone(),
                preview_rgba: vec![255, 0, 0, 255],
                preview_width: 1,
                preview_height: 1,
                animated_preview: None,
                preview_texture: crate::jobs::ImportedPreviewTexture::Cpu,
                loaded_lod: crate::data::CanvasImageLod::Working,
                existing_load_diagnostics: None,
                origin: crate::jobs::ImportOrigin::Existing,
                trace: None,
            },
        );
    }
    app.canvas.add_annotation(source_annotation.clone());
    app.canvas.set_canvas_stack_order(vec![
        CanvasStackItem::Placement(back_item.state.id),
        CanvasStackItem::Annotation(source_annotation.id),
        CanvasStackItem::Placement(source_item.state.id),
        CanvasStackItem::Placement(front_item.state.id),
    ]);
    app.canvas
        .select_content(&[source_item.state.id], &[source_annotation.id]);

    let payload = app.copy_selected_board_payload().unwrap();
    app.paste_board_payload_at(&payload, egui::pos2(300.0, 300.0))
        .unwrap();

    let pasted_item_id = app.canvas.selected_placement_ids()[0];
    let pasted_annotation_id = app.canvas.selected_annotation_ids()[0];
    assert_eq!(
        app.canvas.canvas_stack_order(),
        vec![
            CanvasStackItem::Placement(back_item.state.id),
            CanvasStackItem::Annotation(source_annotation.id),
            CanvasStackItem::Placement(source_item.state.id),
            CanvasStackItem::Placement(front_item.state.id),
            CanvasStackItem::Annotation(pasted_annotation_id),
            CanvasStackItem::Placement(pasted_item_id),
        ]
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn paste_preserves_image_only_stack_order_and_places_group_at_front() {
    let ctx = egui::Context::default();
    let root = temp_library_root("snappyref-image-paste-stack-order");
    let library = Library::initialize(root.clone()).unwrap();
    let image_path = library
        .store_rgba_image("source", &[255, 0, 0, 255], 1, 1)
        .unwrap();
    let back_item = library
        .persist_imported_image(&image_path, "image/png", 1, 1, 4, egui::pos2(0.0, 0.0), 1.0)
        .unwrap();
    let lower_selected = library
        .create_placement_from_asset(
            back_item.asset.as_ref().expect("loaded placement asset").id,
            egui::pos2(10.0, 20.0),
            1.0,
            0.0,
            0,
        )
        .unwrap();
    let upper_selected = library
        .create_placement_from_asset(
            back_item.asset.as_ref().expect("loaded placement asset").id,
            egui::pos2(30.0, 40.0),
            1.0,
            0.0,
            0,
        )
        .unwrap();
    let front_item = library
        .create_placement_from_asset(
            back_item.asset.as_ref().expect("loaded placement asset").id,
            egui::pos2(100.0, 100.0),
            1.0,
            0.0,
            0,
        )
        .unwrap();
    let mut app = test_app(Some(library.clone()));
    for placement in [&back_item, &lower_selected, &upper_selected, &front_item] {
        add_loaded_record(&mut app, &ctx, placement.clone());
    }
    app.canvas.set_canvas_stack_order(vec![
        CanvasStackItem::Placement(back_item.state.id),
        CanvasStackItem::Placement(upper_selected.state.id),
        CanvasStackItem::Placement(lower_selected.state.id),
        CanvasStackItem::Placement(front_item.state.id),
    ]);
    app.canvas
        .select_placements(&[lower_selected.state.id, upper_selected.state.id]);

    let payload = app.copy_selected_board_payload().unwrap();
    app.paste_board_payload_at(&payload, egui::pos2(300.0, 300.0))
        .unwrap();

    let selected = app.canvas.selected_placement_ids();
    let pasted_lower_id = selected[0];
    let pasted_upper_id = selected[1];
    let expected = vec![
        CanvasStackItem::Placement(back_item.state.id),
        CanvasStackItem::Placement(upper_selected.state.id),
        CanvasStackItem::Placement(lower_selected.state.id),
        CanvasStackItem::Placement(front_item.state.id),
        CanvasStackItem::Placement(pasted_upper_id),
        CanvasStackItem::Placement(pasted_lower_id),
    ];
    assert_eq!(app.canvas.canvas_stack_order(), expected);
    assert_eq!(library.load_canvas_stack_order().unwrap(), expected);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn paste_preserves_annotation_only_stack_order_and_places_group_at_front() {
    let root = temp_library_root("snappyref-annotation-paste-stack-order");
    let library = Library::initialize(root.clone()).unwrap();
    let back_annotation = test_canvas_annotation(1, egui::pos2(0.0, 0.0));
    let lower_selected = test_canvas_annotation(2, egui::pos2(10.0, 20.0));
    let upper_selected = test_canvas_annotation(3, egui::pos2(30.0, 40.0));
    let front_annotation = test_canvas_annotation(4, egui::pos2(100.0, 100.0));
    for annotation in [
        &back_annotation,
        &lower_selected,
        &upper_selected,
        &front_annotation,
    ] {
        library
            .create_canvas_annotation(
                annotation.kind,
                egui::pos2(annotation.start_x, annotation.start_y),
                egui::pos2(annotation.end_x, annotation.end_y),
                &annotation.text,
                annotation.text_size,
                annotation.stroke_width,
                annotation.color,
                annotation.background_enabled,
            )
            .unwrap();
    }
    let annotations = library.load_canvas_annotations().unwrap();
    let back_id = annotations[0].id;
    let lower_id = annotations[1].id;
    let upper_id = annotations[2].id;
    let front_id = annotations[3].id;
    let mut app = test_app(Some(library.clone()));
    app.canvas.add_annotations(annotations);
    app.canvas.set_canvas_stack_order(vec![
        CanvasStackItem::Annotation(back_id),
        CanvasStackItem::Annotation(upper_id),
        CanvasStackItem::Annotation(lower_id),
        CanvasStackItem::Annotation(front_id),
    ]);
    app.canvas.select_content(&[], &[lower_id, upper_id]);

    let payload = app.copy_selected_board_payload().unwrap();
    app.paste_board_payload_at(&payload, egui::pos2(300.0, 300.0))
        .unwrap();

    let selected = app.canvas.selected_annotation_ids();
    let pasted_lower_id = selected[0];
    let pasted_upper_id = selected[1];
    let expected = vec![
        CanvasStackItem::Annotation(back_id),
        CanvasStackItem::Annotation(upper_id),
        CanvasStackItem::Annotation(lower_id),
        CanvasStackItem::Annotation(front_id),
        CanvasStackItem::Annotation(pasted_upper_id),
        CanvasStackItem::Annotation(pasted_lower_id),
    ];
    assert_eq!(app.canvas.canvas_stack_order(), expected);
    assert_eq!(library.load_canvas_stack_order().unwrap(), expected);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn duplicate_offsets_mixed_selection_on_same_board_and_selects_new_content() {
    let ctx = egui::Context::default();
    let root = temp_library_root("snappyref-mixed-duplicate");
    let library = Library::initialize(root.clone()).unwrap();
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
    let source_annotation = library
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
    app.settings.import_gap = 24.0;
    app.canvas.add_prepared_placement_image(
        &ctx,
        PreparedPlacementImage {
            target_board_id: 1,
            queue_id: None,
            placement: source_item.clone(),
            preview_rgba: vec![255, 0, 0, 255],
            preview_width: 1,
            preview_height: 1,
            animated_preview: None,
            preview_texture: crate::jobs::ImportedPreviewTexture::Cpu,
            loaded_lod: crate::data::CanvasImageLod::Working,
            existing_load_diagnostics: None,
            origin: crate::jobs::ImportOrigin::Existing,
            trace: None,
        },
    );
    app.canvas.add_annotation(source_annotation.clone());
    app.canvas.set_canvas_stack_order(vec![
        CanvasStackItem::Annotation(source_annotation.id),
        CanvasStackItem::Placement(source_item.state.id),
    ]);
    app.canvas
        .select_content(&[source_item.state.id], &[source_annotation.id]);

    app.duplicate_selection_with_gap();

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
        duplicated_item.state.center.x - source_item.state.center.x,
        duplicated_item.state.center.y - source_item.state.center.y,
    );
    let annotation_offset = egui::vec2(
        duplicated_annotation.start_x - source_annotation.start_x,
        duplicated_annotation.start_y - source_annotation.start_y,
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
    assert_eq!(library.load_canvas_stack_order().unwrap(), expected);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn duplicate_preserves_image_only_stack_order_and_places_group_at_front() {
    let ctx = egui::Context::default();
    let root = temp_library_root("snappyref-image-duplicate-stack-order");
    let library = Library::initialize(root.clone()).unwrap();
    let image_path = library
        .store_rgba_image("source", &[255, 0, 0, 255], 1, 1)
        .unwrap();
    let back_item = library
        .persist_imported_image(&image_path, "image/png", 1, 1, 4, egui::pos2(0.0, 0.0), 1.0)
        .unwrap();
    let lower_selected = library
        .create_placement_from_asset(
            back_item.asset.as_ref().expect("loaded placement asset").id,
            egui::pos2(10.0, 20.0),
            1.0,
            0.0,
            0,
        )
        .unwrap();
    let upper_selected = library
        .create_placement_from_asset(
            back_item.asset.as_ref().expect("loaded placement asset").id,
            egui::pos2(30.0, 40.0),
            1.0,
            0.0,
            0,
        )
        .unwrap();
    let mut app = test_app(Some(library.clone()));
    app.settings.import_gap = 24.0;
    for placement in [&back_item, &lower_selected, &upper_selected] {
        add_loaded_record(&mut app, &ctx, placement.clone());
    }
    app.canvas.set_canvas_stack_order(vec![
        CanvasStackItem::Placement(back_item.state.id),
        CanvasStackItem::Placement(upper_selected.state.id),
        CanvasStackItem::Placement(lower_selected.state.id),
    ]);
    app.canvas
        .select_placements(&[lower_selected.state.id, upper_selected.state.id]);

    app.duplicate_selection_with_gap();

    let selected = app.canvas.selected_placement_ids();
    let duplicated_lower_id = selected[0];
    let duplicated_upper_id = selected[1];
    let expected = vec![
        CanvasStackItem::Placement(back_item.state.id),
        CanvasStackItem::Placement(upper_selected.state.id),
        CanvasStackItem::Placement(lower_selected.state.id),
        CanvasStackItem::Placement(duplicated_upper_id),
        CanvasStackItem::Placement(duplicated_lower_id),
    ];
    assert_eq!(app.canvas.canvas_stack_order(), expected);
    assert_eq!(library.load_canvas_stack_order().unwrap(), expected);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn duplicate_preserves_annotation_only_stack_order_and_places_group_at_front() {
    let root = temp_library_root("snappyref-annotation-duplicate-stack-order");
    let library = Library::initialize(root.clone()).unwrap();
    let back_annotation = test_canvas_annotation(1, egui::pos2(0.0, 0.0));
    let lower_selected = test_canvas_annotation(2, egui::pos2(10.0, 20.0));
    let upper_selected = test_canvas_annotation(3, egui::pos2(30.0, 40.0));
    for annotation in [&back_annotation, &lower_selected, &upper_selected] {
        library
            .create_canvas_annotation(
                annotation.kind,
                egui::pos2(annotation.start_x, annotation.start_y),
                egui::pos2(annotation.end_x, annotation.end_y),
                &annotation.text,
                annotation.text_size,
                annotation.stroke_width,
                annotation.color,
                annotation.background_enabled,
            )
            .unwrap();
    }
    let annotations = library.load_canvas_annotations().unwrap();
    let back_id = annotations[0].id;
    let lower_id = annotations[1].id;
    let upper_id = annotations[2].id;
    let mut app = test_app(Some(library.clone()));
    app.settings.import_gap = 24.0;
    app.canvas.add_annotations(annotations);
    app.canvas.set_canvas_stack_order(vec![
        CanvasStackItem::Annotation(back_id),
        CanvasStackItem::Annotation(upper_id),
        CanvasStackItem::Annotation(lower_id),
    ]);
    app.canvas.select_content(&[], &[lower_id, upper_id]);

    app.duplicate_selection_with_gap();

    let selected = app.canvas.selected_annotation_ids();
    let duplicated_lower_id = selected[0];
    let duplicated_upper_id = selected[1];
    let expected = vec![
        CanvasStackItem::Annotation(back_id),
        CanvasStackItem::Annotation(upper_id),
        CanvasStackItem::Annotation(lower_id),
        CanvasStackItem::Annotation(duplicated_upper_id),
        CanvasStackItem::Annotation(duplicated_lower_id),
    ];
    assert_eq!(app.canvas.canvas_stack_order(), expected);
    assert_eq!(library.load_canvas_stack_order().unwrap(), expected);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn command_a_selects_all_active_board_images_and_annotations() {
    let ctx = egui::Context::default();
    let mut app = test_app(None);
    for id in [1_i64, 2] {
        app.canvas
            .add_prepared_placement_image(&ctx, test_loaded_item(id, 1));
    }
    app.canvas.add_annotation(crate::data::CanvasAnnotation {
        id: 10,
        kind: CanvasAnnotationKind::Textbox,
        start_x: 0.0,
        start_y: 0.0,
        end_x: 100.0,
        end_y: 40.0,
        text: "one".to_owned(),
        text_size: CanvasTextSize::M,
        stroke_width: CanvasStrokeWidth::M,
        color: crate::data::CanvasAnnotationColor::Default,
        background_enabled: false,
        archived: false,
    });
    app.canvas.add_annotation(crate::data::CanvasAnnotation {
        id: 11,
        kind: CanvasAnnotationKind::Line,
        start_x: 20.0,
        start_y: 20.0,
        end_x: 80.0,
        end_y: 80.0,
        text: String::new(),
        text_size: CanvasTextSize::M,
        stroke_width: CanvasStrokeWidth::M,
        color: crate::data::CanvasAnnotationColor::Default,
        background_enabled: false,
        archived: false,
    });
    app.canvas.select_content(&[], &[]);

    let _ = ctx.run_ui(
        egui::RawInput {
            modifiers: egui::Modifiers {
                command: true,
                ..Default::default()
            },
            events: vec![egui::Event::Key {
                key: egui::Key::A,
                physical_key: Some(egui::Key::A),
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers {
                    command: true,
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

    assert_eq!(app.canvas.selected_placement_ids(), vec![1, 2]);
    assert_eq!(app.canvas.selected_annotation_ids(), vec![10, 11]);
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
    app.settings.import_gap = 20.0;

    for (id, x, y) in [(1_i64, 10.0, 50.0), (2, 170.0, 50.0), (3, 50.0, 160.0)] {
        let mut loaded = test_loaded_item(id, 1);
        loaded
            .placement
            .asset
            .as_mut()
            .expect("loaded placement asset")
            .width = 100;
        loaded
            .placement
            .asset
            .as_mut()
            .expect("loaded placement asset")
            .height = 40;
        loaded.placement.visual_size = egui::vec2(100.0, 40.0);
        loaded.placement.state.center.x = x;
        loaded.placement.state.center.y = y;
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
    assert_eq!(second.state.center.x + 50.0, 220.0);
    assert_eq!(
        first.state.center.x + 50.0 + app.settings.import_gap,
        second.state.center.x - 50.0
    );
    assert_eq!(third.state.center.x + 50.0, 220.0);
    assert_eq!(app.status, "Settled 2 images right");
}

#[test]
fn shortcuts_list_includes_directional_settle_arrange() {
    assert!(SUPPORTED_SHORTCUTS.iter().any(|shortcut| {
        shortcut.action == "Directional settle arrange"
            && shortcut.windows == "Arrow keys"
            && shortcut.mac == "Arrow keys"
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
    app.canvas.add_annotation(crate::data::CanvasAnnotation {
        id: 20,
        kind: CanvasAnnotationKind::Textbox,
        start_x: 10.0,
        start_y: 10.0,
        end_x: 70.0,
        end_y: 40.0,
        text: "caption".to_owned(),
        text_size: CanvasTextSize::M,
        stroke_width: CanvasStrokeWidth::M,
        color: crate::data::CanvasAnnotationColor::Default,
        background_enabled: false,
        archived: false,
    });
    app.canvas.select_content(&[1], &[20]);

    let copy_payload =
        crate::sources::clipboard::build_selection_copy_payload(&app.canvas, app.library.as_ref())
            .unwrap()
            .unwrap();
    assert!(copy_payload.external_image.is_none());
    let payload = serde_json::from_str::<crate::active_board::commands::ClipboardBoardSelection>(
        &copy_payload.internal_json,
    )
    .unwrap();
    assert_eq!(
        payload.kind,
        crate::active_board::commands::COPY_PAYLOAD_KIND
    );
    assert_eq!(payload.items.len(), 1);
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
    app.canvas.add_annotation(crate::data::CanvasAnnotation {
        id: 20,
        kind: CanvasAnnotationKind::Textbox,
        start_x: 10.0,
        start_y: 10.0,
        end_x: 70.0,
        end_y: 40.0,
        text: "caption".to_owned(),
        text_size: CanvasTextSize::M,
        stroke_width: CanvasStrokeWidth::M,
        color: crate::data::CanvasAnnotationColor::Default,
        background_enabled: false,
        archived: false,
    });
    app.canvas.select_content(&[1], &[20]);

    let items = app.context_menu_items();

    assert_eq!(
        items.iter().map(|item| item.label).collect::<Vec<_>>(),
        vec![
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
                super::ContextMenuAction::Copy,
                Some(super::platform_shortcut_label("⌘ C", "Ctrl C")),
            ),
            (
                super::ContextMenuAction::Paste,
                Some(super::platform_shortcut_label("⌘ V", "Ctrl V")),
            ),
            (
                super::ContextMenuAction::Duplicate,
                Some(super::platform_shortcut_label("⌘ D", "Ctrl D")),
            ),
            (super::ContextMenuAction::DeleteFromBoard, Some("del")),
            (
                super::ContextMenuAction::MoveToFront,
                Some(super::platform_shortcut_label("⌘ .", "Ctrl .")),
            ),
            (
                super::ContextMenuAction::MoveToBack,
                Some(super::platform_shortcut_label("⌘ ,", "Ctrl ,")),
            ),
            (
                super::ContextMenuAction::SearchUsingImage,
                Some(super::platform_shortcut_label("⌘ F", "Ctrl F")),
            ),
            (super::ContextMenuAction::RemoveFromLibrary, None),
        ]
    );
    assert!(item_enabled(&items, super::ContextMenuAction::Copy));
    assert!(item_enabled(&items, super::ContextMenuAction::Paste));
    assert!(item_enabled(&items, super::ContextMenuAction::Duplicate));
    assert!(item_enabled(
        &items,
        super::ContextMenuAction::DeleteFromBoard
    ));
    assert!(item_enabled(&items, super::ContextMenuAction::MoveToFront));
    assert!(item_enabled(&items, super::ContextMenuAction::MoveToBack));
    assert!(!item_enabled(
        &items,
        super::ContextMenuAction::SearchUsingImage
    ));
    assert!(item_enabled(
        &items,
        super::ContextMenuAction::RemoveFromLibrary
    ));
}

#[test]
fn context_menu_primary_press_inside_menu_keeps_menu_open_for_click_release() {
    let menu_rect = egui::Rect::from_min_size(egui::pos2(100.0, 120.0), egui::vec2(190.0, 240.0));

    assert!(!super::context_menu_should_close(
        false,
        false,
        menu_rect,
        Some(menu_rect.center()),
        true,
        false,
        false,
    ));
}

#[test]
fn context_menu_primary_press_outside_menu_closes_menu() {
    let menu_rect = egui::Rect::from_min_size(egui::pos2(100.0, 120.0), egui::vec2(190.0, 240.0));

    assert!(super::context_menu_should_close(
        false,
        false,
        menu_rect,
        Some(egui::pos2(menu_rect.right() + 8.0, menu_rect.center().y)),
        true,
        false,
        false,
    ));
}

#[test]
fn open_context_menu_blocks_canvas_input_behind_menu() {
    let ctx = egui::Context::default();
    let mut app = test_app(None);

    assert!(!app.blocks_canvas_input(&ctx));

    app.context_menu_pos = Some(egui::pos2(520.0, 260.0));

    assert!(app.blocks_canvas_input(&ctx));
}

#[test]
fn stack_order_context_actions_are_disabled_without_canvas_item_selection() {
    let app = test_app(None);

    let items = app.context_menu_items();

    assert!(!item_enabled(&items, super::ContextMenuAction::MoveToFront));
    assert!(!item_enabled(&items, super::ContextMenuAction::MoveToBack));
}

#[test]
fn stack_order_context_actions_are_enabled_for_image_and_annotation_selections() {
    let ctx = egui::Context::default();
    let mut app = test_app(None);
    app.canvas
        .add_prepared_placement_image(&ctx, test_loaded_item(1, 1));
    app.canvas.add_annotation(crate::data::CanvasAnnotation {
        id: 20,
        kind: CanvasAnnotationKind::Textbox,
        start_x: 10.0,
        start_y: 10.0,
        end_x: 70.0,
        end_y: 40.0,
        text: "caption".to_owned(),
        text_size: CanvasTextSize::M,
        stroke_width: CanvasStrokeWidth::M,
        color: crate::data::CanvasAnnotationColor::Default,
        background_enabled: false,
        archived: false,
    });

    app.canvas.select_content(&[1], &[]);
    let image_only = app.context_menu_items();
    assert!(item_enabled(
        &image_only,
        super::ContextMenuAction::MoveToFront
    ));
    assert!(item_enabled(
        &image_only,
        super::ContextMenuAction::MoveToBack
    ));

    app.canvas.select_content(&[], &[20]);
    let annotation_only = app.context_menu_items();
    assert!(item_enabled(
        &annotation_only,
        super::ContextMenuAction::MoveToFront
    ));
    assert!(item_enabled(
        &annotation_only,
        super::ContextMenuAction::MoveToBack
    ));
}

#[test]
fn image_search_context_action_requires_exactly_one_image() {
    let ctx = egui::Context::default();
    let mut app = test_app(None);
    app.canvas
        .add_prepared_placement_image(&ctx, test_loaded_item(1, 1));
    app.canvas
        .add_prepared_placement_image(&ctx, test_loaded_item(2, 1));

    app.canvas.select_placements(&[1]);
    let single = app.context_menu_items();
    assert!(item_enabled(
        &single,
        super::ContextMenuAction::SearchUsingImage
    ));

    app.canvas.select_placements(&[1, 2]);
    let multiple = app.context_menu_items();
    assert!(!item_enabled(
        &multiple,
        super::ContextMenuAction::SearchUsingImage
    ));
}

#[test]
fn remove_from_library_context_action_waits_for_confirmation() {
    let ctx = egui::Context::default();
    let root = temp_library_root("snappyref-context-remove-confirm");
    let library = Library::initialize(root.clone()).unwrap();
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
    app.canvas.add_prepared_placement_image(
        &ctx,
        PreparedPlacementImage {
            target_board_id: 1,
            queue_id: None,
            placement: source_item.clone(),
            preview_rgba: vec![255, 0, 0, 255],
            preview_width: 1,
            preview_height: 1,
            animated_preview: None,
            preview_texture: crate::jobs::ImportedPreviewTexture::Cpu,
            loaded_lod: crate::data::CanvasImageLod::Working,
            existing_load_diagnostics: None,
            origin: crate::jobs::ImportOrigin::Existing,
            trace: None,
        },
    );
    app.canvas.select_placements(&[source_item.state.id]);

    app.handle_context_menu_action(
        super::ContextMenuAction::RemoveFromLibrary,
        &ctx,
        egui::Rect::EVERYTHING,
    );

    assert!(app.pending_library_remove.is_some());
    assert_eq!(library.load_placements().unwrap().len(), 1);

    app.confirm_pending_library_remove();

    assert!(app.pending_library_remove.is_none());
    assert!(library.load_placements().unwrap().is_empty());
    assert!(
        source_item
            .asset
            .as_ref()
            .expect("loaded placement asset")
            .original_path
            .exists()
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn move_to_front_context_action_persists_canvas_stack_order() {
    let ctx = egui::Context::default();
    let root = temp_library_root("snappyref-context-move-front");
    let library = Library::initialize(root.clone()).unwrap();
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
        super::ContextMenuAction::MoveToFront,
        &ctx,
        egui::Rect::EVERYTHING,
    );

    assert_eq!(
        library.load_canvas_stack_order().unwrap(),
        vec![
            crate::data::CanvasStackItem::Placement(second.state.id),
            crate::data::CanvasStackItem::Placement(first.state.id),
            crate::data::CanvasStackItem::Annotation(annotation.id),
        ]
    );

    app.handle_context_menu_action(
        super::ContextMenuAction::MoveToBack,
        &ctx,
        egui::Rect::EVERYTHING,
    );

    assert_eq!(
        library.load_canvas_stack_order().unwrap(),
        vec![
            crate::data::CanvasStackItem::Placement(first.state.id),
            crate::data::CanvasStackItem::Annotation(annotation.id),
            crate::data::CanvasStackItem::Placement(second.state.id),
        ]
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn delete_from_board_archives_image_placements_and_annotations() {
    let ctx = egui::Context::default();
    let root = temp_library_root("snappyref-delete-from-board");
    let library = Library::initialize(root.clone()).unwrap();
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
    let annotation = library
        .create_canvas_annotation(
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
    app.canvas.add_prepared_placement_image(
        &ctx,
        PreparedPlacementImage {
            target_board_id: 1,
            queue_id: None,
            placement: source_item.clone(),
            preview_rgba: vec![255, 0, 0, 255],
            preview_width: 1,
            preview_height: 1,
            animated_preview: None,
            preview_texture: crate::jobs::ImportedPreviewTexture::Cpu,
            loaded_lod: crate::data::CanvasImageLod::Working,
            existing_load_diagnostics: None,
            origin: crate::jobs::ImportOrigin::Existing,
            trace: None,
        },
    );
    app.canvas.add_annotation(annotation.clone());
    app.canvas
        .select_content(&[source_item.state.id], &[annotation.id]);

    app.handle_context_menu_action(
        super::ContextMenuAction::DeleteFromBoard,
        &ctx,
        egui::Rect::EVERYTHING,
    );

    assert!(library.load_placements().unwrap().is_empty());
    assert!(
        library
            .asset_original_paths_for_assets(&[source_item
                .asset
                .as_ref()
                .expect("loaded placement asset")
                .id])
            .unwrap()
            .contains_key(
                &source_item
                    .asset
                    .as_ref()
                    .expect("loaded placement asset")
                    .id
            )
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

    let _ = fs::remove_dir_all(root);
}

#[cfg(feature = "semantic-search")]
#[test]
fn search_using_image_context_action_opens_image_pivot_search() {
    let ctx = egui::Context::default();
    let mut app = test_app(None);
    app.settings.semantic_search_enabled = true;
    app.canvas
        .add_prepared_placement_image(&ctx, test_loaded_item(42, 1));
    app.canvas.select_placements(&[42]);

    app.handle_context_menu_action(
        super::ContextMenuAction::SearchUsingImage,
        &ctx,
        egui::Rect::EVERYTHING,
    );

    assert!(app.search.overlay.open);
    assert!(app.search.overlay.suppress_next_outside_click);
    let pivot = app.search.overlay.image_pivot.unwrap();
    assert_eq!(pivot.asset_id, 42);
    assert_eq!(pivot.item_id, 42);
}

#[cfg(feature = "semantic-search")]
#[test]
fn search_using_image_context_action_survives_activation_click_outside_overlay() {
    let ctx = egui::Context::default();
    let mut app = test_app(None);
    app.settings.semantic_search_enabled = true;
    app.canvas
        .add_prepared_placement_image(&ctx, test_loaded_item(42, 1));
    app.canvas.select_placements(&[42]);

    app.handle_context_menu_action(
        super::ContextMenuAction::SearchUsingImage,
        &ctx,
        egui::Rect::EVERYTHING,
    );

    let _ = ctx.run_ui(
        egui::RawInput {
            events: vec![
                egui::Event::PointerMoved(egui::pos2(520.0, 320.0)),
                egui::Event::PointerButton {
                    pos: egui::pos2(520.0, 320.0),
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::default(),
                },
            ],
            ..Default::default()
        },
        |ui| {
            app.show_search_overlay(
                ui.ctx(),
                egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(960.0, 640.0)),
            )
        },
    );

    assert!(app.search.overlay.open);
    assert!(!app.search.overlay.suppress_next_outside_click);
}

#[test]
fn sidebar_create_board_generates_name_switches_and_stays_open() {
    let root = temp_library_root("snappyref-create-board");
    let library = Library::initialize(root.clone()).unwrap();
    let mut app = test_app(Some(library.clone()));
    app.sidebar_open = true;

    app.create_and_switch_board(true);

    let boards = library.list_boards().unwrap();
    assert_eq!(boards.len(), 2);
    assert_eq!(boards[0].name, "My Board");
    assert_eq!(boards[1].name, "Board 2");
    assert_eq!(app.active_board_id, boards[1].id);
    assert_eq!(
        library.load_user_session().unwrap().active_board_id,
        boards[1].id
    );
    assert!(app.sidebar_open);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn generated_board_names_fill_smallest_visible_gap() {
    let root = temp_library_root("snappyref-board-name-gap");
    let library = Library::initialize(root.clone()).unwrap();
    let board_2 = library.create_board("Board 2").unwrap();
    library.create_board("Board 3").unwrap();
    {
        let connection = rusqlite::Connection::open(root.join("library.db")).unwrap();
        connection
            .execute("UPDATE boards SET archived = 1 WHERE id = ?1", [board_2.id])
            .unwrap();
    }
    let mut app = test_app(Some(library.clone()));

    app.create_and_switch_board(false);

    let boards = library.list_boards().unwrap();
    assert!(boards.iter().any(|board| board.name == "Board 2"));
    assert_eq!(app.active_board_id, boards.last().unwrap().id);
    assert!(!app.sidebar_open);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn command_n_creates_board_without_opening_sidebar() {
    let ctx = egui::Context::default();
    let root = temp_library_root("snappyref-cmd-n-board");
    let library = Library::initialize(root.clone()).unwrap();
    let mut app = test_app(Some(library.clone()));
    app.sidebar_open = false;

    let _ = ctx.run_ui(
        egui::RawInput {
            modifiers: egui::Modifiers {
                command: true,
                ..Default::default()
            },
            events: vec![egui::Event::Key {
                key: egui::Key::N,
                physical_key: Some(egui::Key::N),
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers {
                    command: true,
                    ..Default::default()
                },
            }],
            ..Default::default()
        },
        |ui| app.handle_board_shortcuts(ui.ctx()),
    );

    let boards = library.list_boards().unwrap();
    assert_eq!(boards.len(), 2);
    assert_eq!(boards[1].name, "Board 2");
    assert_eq!(app.active_board_id, boards[1].id);
    assert!(!app.sidebar_open);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn command_number_switches_visible_board() {
    let ctx = egui::Context::default();
    let root = temp_library_root("snappyref-cmd-number-board");
    let library = Library::initialize(root.clone()).unwrap();
    let second = library.create_board("Second").unwrap();
    let third = library.create_board("Third").unwrap();
    let mut app = test_app(Some(library.clone()));
    app.active_board_id = second.id;
    library.set_active_board(second.id).unwrap();

    let _ = ctx.run_ui(
        egui::RawInput {
            modifiers: egui::Modifiers {
                command: true,
                ..Default::default()
            },
            events: vec![egui::Event::Key {
                key: egui::Key::Num3,
                physical_key: Some(egui::Key::Num3),
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers {
                    command: true,
                    ..Default::default()
                },
            }],
            ..Default::default()
        },
        |ui| app.handle_board_shortcuts(ui.ctx()),
    );

    assert_eq!(app.active_board_id, third.id);
    assert_eq!(
        library.load_user_session().unwrap().active_board_id,
        third.id
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn command_zero_switches_tenth_visible_board() {
    let ctx = egui::Context::default();
    let root = temp_library_root("snappyref-cmd-zero-board");
    let library = Library::initialize(root.clone()).unwrap();
    let mut tenth = None;
    for index in 2..=10 {
        let board = library.create_board(&format!("Board {index}")).unwrap();
        if index == 10 {
            tenth = Some(board);
        }
    }
    let tenth = tenth.unwrap();
    let mut app = test_app(Some(library.clone()));

    let _ = ctx.run_ui(
        egui::RawInput {
            modifiers: egui::Modifiers {
                command: true,
                ..Default::default()
            },
            events: vec![egui::Event::Key {
                key: egui::Key::Num0,
                physical_key: Some(egui::Key::Num0),
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers {
                    command: true,
                    ..Default::default()
                },
            }],
            ..Default::default()
        },
        |ui| app.handle_board_shortcuts(ui.ctx()),
    );

    assert_eq!(app.active_board_id, tenth.id);

    let _ = fs::remove_dir_all(root);
}

#[cfg(feature = "semantic-search")]
#[test]
fn board_shortcuts_are_suppressed_while_search_is_open() {
    let ctx = egui::Context::default();
    let root = temp_library_root("snappyref-board-shortcut-search");
    let library = Library::initialize(root.clone()).unwrap();
    library.create_board("Second").unwrap();
    let mut app = test_app(Some(library.clone()));
    app.search.overlay.open = true;

    let _ = ctx.run_ui(
        egui::RawInput {
            modifiers: egui::Modifiers {
                command: true,
                ..Default::default()
            },
            events: vec![egui::Event::Key {
                key: egui::Key::Num2,
                physical_key: Some(egui::Key::Num2),
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers {
                    command: true,
                    ..Default::default()
                },
            }],
            ..Default::default()
        },
        |ui| app.handle_board_shortcuts(ui.ctx()),
    );

    assert_eq!(app.active_board_id, 1);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn switching_boards_saves_previous_viewport_and_loads_target_viewport() {
    let root = temp_library_root("snappyref-board-switch-viewport");
    let library = Library::initialize(root.clone()).unwrap();
    let first = library.list_boards().unwrap()[0].clone();
    let second = library.create_board("Second").unwrap();
    library.set_active_board(second.id).unwrap();
    let mut second_session = library.load_user_session().unwrap();
    second_session.viewport.camera_pan_x = 75.0;
    second_session.viewport.canvas_background = crate::data::CanvasBackground::Dots;
    library.save_user_session(&second_session).unwrap();
    library.set_active_board(first.id).unwrap();
    let mut app = test_app(Some(library.clone()));
    app.active_board_id = first.id;
    app.canvas
        .apply_viewport_state(&crate::data::ViewportState {
            camera_pan_x: 22.0,
            camera_pan_y: 33.0,
            camera_zoom: 1.5,
            canvas_background: crate::data::CanvasBackground::Grid,
        });

    app.switch_to_board(second.id);

    let saved_first = {
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

    let _ = fs::remove_dir_all(root);
}

#[test]
fn active_board_load_applies_viewport_without_reloading_global_settings() {
    let root = temp_library_root("snappyref-active-load-settings-scope");
    let library = Library::initialize(root.clone()).unwrap();
    let second = library.create_board("Second").unwrap();
    library.set_active_board(second.id).unwrap();
    let mut stored_session = library.load_user_session().unwrap();
    stored_session.settings.import_gap = 99.0;
    stored_session.viewport.camera_pan_x = 75.0;
    stored_session.viewport.canvas_background = crate::data::CanvasBackground::Dots;
    library.save_user_session(&stored_session).unwrap();

    let mut app = test_app(Some(library.clone()));
    app.settings.import_gap = 24.0;

    app.load_active_board();

    assert_eq!(app.active_board_id, second.id);
    assert_eq!(app.canvas.background(), crate::data::CanvasBackground::Dots);
    assert_eq!(app.canvas.viewport_state().camera_pan_x, 75.0);
    assert_eq!(app.settings.import_gap, 24.0);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn board_delete_requires_confirmation_before_archiving_active_board() {
    let root = temp_library_root("snappyref-confirm-delete-board");
    let library = Library::initialize(root.clone()).unwrap();
    let first = library.list_boards().unwrap()[0].clone();
    let second = library.create_board("Second").unwrap();
    library.set_active_board(second.id).unwrap();
    let mut app = test_app(Some(library.clone()));
    app.active_board_id = second.id;
    app.sidebar_open = true;

    app.request_board_delete(second.id, second.name.clone());

    assert!(app.pending_board_delete.is_some());
    assert_eq!(library.list_boards().unwrap().len(), 2);

    app.confirm_pending_board_delete();

    assert!(app.pending_board_delete.is_none());
    assert!(app.sidebar_open);
    assert_eq!(app.active_board_id, first.id);
    assert_eq!(
        library
            .list_boards()
            .unwrap()
            .iter()
            .map(|board| board.id)
            .collect::<Vec<_>>(),
        vec![first.id]
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn board_delete_confirmation_keeps_sidebar_open_and_removes_inactive_board() {
    let root = temp_library_root("snappyref-delete-inactive-board");
    let library = Library::initialize(root.clone()).unwrap();
    let first = library.list_boards().unwrap()[0].clone();
    let second = library.create_board("Second").unwrap();
    let mut app = test_app(Some(library.clone()));
    app.active_board_id = first.id;
    app.sidebar_open = true;

    app.request_board_delete(second.id, second.name.clone());
    app.confirm_pending_board_delete();

    assert!(app.pending_board_delete.is_none());
    assert!(app.sidebar_open);
    assert_eq!(app.active_board_id, first.id);
    assert_eq!(
        library
            .list_boards()
            .unwrap()
            .iter()
            .map(|board| board.id)
            .collect::<Vec<_>>(),
        vec![first.id]
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn board_delete_modal_click_routes_through_egui_layers() {
    let root = temp_library_root("snappyref-delete-modal-click-routing");
    let library = Library::initialize(root.clone()).unwrap();
    let first = library.list_boards().unwrap()[0].clone();
    let second = library.create_board("Second").unwrap();
    let mut app = test_app(Some(library.clone()));
    app.active_board_id = second.id;
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
    assert_eq!(
        library
            .list_boards()
            .unwrap()
            .iter()
            .map(|board| board.id)
            .collect::<Vec<_>>(),
        vec![first.id]
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn board_sidebar_stays_clickable_after_delete_modal_confirms() {
    let root = temp_library_root("snappyref-sidebar-click-after-delete");
    let library = Library::initialize(root.clone()).unwrap();
    let second = library.create_board("Second").unwrap();
    let mut app = test_app(Some(library.clone()));
    app.active_board_id = second.id;
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

    let _ = fs::remove_dir_all(root);
}

#[test]
fn board_sidebar_name_click_switches_board_immediately() {
    let root = temp_library_root("snappyref-sidebar-name-click-switch");
    let library = Library::initialize(root.clone()).unwrap();
    let first = library.list_boards().unwrap()[0].clone();
    let second = library.create_board("Second").unwrap();
    let mut app = test_app(Some(library.clone()));
    app.active_board_id = first.id;
    app.sidebar_open = true;
    app.sidebar_tab = SidebarTab::Boards;
    let mut harness = app_harness(app, |ctx, app| {
        render_sidebar_for_test(ctx, app);
    });

    harness.get_by_label("Board Second").click();
    harness.run();

    assert_eq!(harness.state().active_board_id, second.id);

    let _ = fs::remove_dir_all(root);
}

#[test]
#[cfg(feature = "semantic-search")]
fn settings_semantic_search_install_and_cancel_share_download_state() {
    let mut app = test_app(None);
    app.sidebar_open = true;
    app.sidebar_tab = SidebarTab::Options;
    app.search
        .set_local_model_store_status_for_verification(LocalModelStoreStatus::Missing);
    let mut harness = app_harness(app, |ctx, app| {
        render_sidebar_for_test(ctx, app);
    });

    harness.get_by_label("Semantic Search").scroll_to_me();
    harness.run();
    harness.get_by_label("Install").click();
    harness.run();

    assert_eq!(
        harness.state().search.local_model_store_status(),
        LocalModelStoreStatus::Downloading { percent: 0 }
    );
    assert_eq!(harness.state().status, "Installing Semantic Search");

    harness.get_by_label("Semantic Search").scroll_to_me();
    harness.run();
    harness.get_by_label("Cancel").click();
    harness.run();

    assert_eq!(
        harness.state().search.local_model_store_status(),
        LocalModelStoreStatus::Missing
    );
    assert_eq!(harness.state().status, "Semantic Search install canceled");
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
#[cfg(feature = "semantic-search")]
fn settings_semantic_search_failed_state_retries_shared_install() {
    let mut app = test_app(None);
    app.sidebar_open = true;
    app.sidebar_tab = SidebarTab::Options;
    app.search
        .set_local_model_store_status_for_verification(LocalModelStoreStatus::Failed(
            "network: timeout".to_owned(),
        ));
    let mut harness = app_harness(app, |ctx, app| {
        render_sidebar_for_test(ctx, app);
    });

    harness.get_by_label("Semantic Search").scroll_to_me();
    harness.run();
    harness.get_by_label("Retry").click();
    harness.run();

    assert_eq!(
        harness.state().search.local_model_store_status(),
        LocalModelStoreStatus::Downloading { percent: 0 }
    );
    assert_eq!(harness.state().status, "Installing Semantic Search");
}

#[test]
#[cfg(feature = "semantic-search")]
fn settings_semantic_search_remove_requires_confirmation() {
    let root = temp_library_root("snappyref-semantic-search-remove-confirm");
    let library = Library::initialize(root.clone()).unwrap();
    let mut app = test_app(Some(library));
    app.sidebar_open = true;
    app.sidebar_tab = SidebarTab::Options;
    app.settings.semantic_search_first_launch_prompt_seen = true;
    app.search
        .set_local_model_store_status_for_verification(LocalModelStoreStatus::Installed);
    let mut harness = app_harness(app, |ctx, app| {
        render_sidebar_for_test(ctx, app);
        app.show_semantic_search_remove_confirmation(ctx);
    });

    harness.get_by_label("Semantic Search").scroll_to_me();
    harness.run();
    harness.get_by_label("Remove").click();
    harness.run();

    assert!(harness.state().pending_semantic_search_remove);
    assert_eq!(
        harness.state().search.local_model_store_status(),
        LocalModelStoreStatus::Installed
    );

    harness.get_by_label("Remove Semantic Search").click();
    harness.run();

    assert!(!harness.state().pending_semantic_search_remove);
    assert_eq!(
        harness.state().search.local_model_store_status(),
        LocalModelStoreStatus::Missing
    );
    assert!(
        harness
            .state()
            .settings
            .semantic_search_first_launch_prompt_seen
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn rapid_sidebar_board_name_clicks_keep_last_requested_board_active() {
    let root = temp_library_root("snappyref-rapid-sidebar-board-clicks");
    let library = Library::initialize(root.clone()).unwrap();
    let first = library.list_boards().unwrap()[0].clone();
    let second = library.create_board("Second").unwrap();
    let _third = library.create_board("Third").unwrap();
    let mut app = test_app(Some(library.clone()));
    app.active_board_id = first.id;
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
    assert_eq!(
        library.load_user_session().unwrap().active_board_id,
        second.id
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn board_delete_modal_blocks_sidebar_board_switches_until_closed() {
    let root = temp_library_root("snappyref-delete-modal-blocks-sidebar");
    let library = Library::initialize(root.clone()).unwrap();
    let first = library.list_boards().unwrap()[0].clone();
    let second = library.create_board("Second").unwrap();
    let mut app = test_app(Some(library.clone()));
    app.active_board_id = first.id;
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
    assert_eq!(
        library.load_user_session().unwrap().active_board_id,
        second.id
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn board_delete_modal_blocks_sidebar_tabs_and_settings() {
    let root = temp_library_root("snappyref-delete-modal-blocks-sidebar-state");
    let library = Library::initialize(root.clone()).unwrap();
    let second = library.create_board("Second").unwrap();
    let mut app = test_app(Some(library.clone()));
    app.sidebar_open = true;
    app.sidebar_tab = SidebarTab::Boards;
    app.settings.auto_image_scaling = true;
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
    assert!(harness.state().settings.auto_image_scaling);
    assert!(harness.state().pending_board_delete.is_some());

    let _ = fs::remove_dir_all(root);
}

#[test]
fn sidebar_board_lifecycle_clicks_create_switch_and_persist_active_board() {
    let root = temp_library_root("snappyref-sidebar-board-lifecycle");
    let library = Library::initialize(root.clone()).unwrap();
    let first = library.list_boards().unwrap()[0].clone();
    let mut app = test_app(Some(library.clone()));
    app.active_board_id = first.id;
    app.sidebar_open = true;
    app.sidebar_tab = SidebarTab::Boards;
    let mut harness = app_harness(app, |ctx, app| {
        render_sidebar_for_test(ctx, app);
    });

    harness.get_by_label("New board").click();
    harness.run();

    let boards = library.list_boards().unwrap();
    assert_eq!(boards.len(), 2);
    assert_eq!(boards[1].name, "Board 2");
    assert_eq!(harness.state().active_board_id, boards[1].id);
    assert!(harness.state().sidebar_open);

    harness.get_by_label("Board My Board").click();
    harness.run();

    assert_eq!(harness.state().active_board_id, first.id);
    assert_eq!(
        library.load_user_session().unwrap().active_board_id,
        first.id
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn rapid_board_switches_ignore_stale_import_completions_and_last_switch_wins() {
    let ctx = egui::Context::default();
    let root = temp_library_root("snappyref-rapid-switch-stale-imports");
    let library = Library::initialize(root.clone()).unwrap();
    let first = library.list_boards().unwrap()[0].clone();
    let second = library.create_board("Second").unwrap();
    let third = library.create_board("Third").unwrap();
    let mut app = test_app(Some(library));
    app.active_board_id = first.id;

    app.switch_to_board(second.id);
    app.integrate_loaded_import(&ctx, test_loaded_item(41, first.id));
    app.switch_to_board(third.id);
    app.integrate_loaded_import(&ctx, test_loaded_item(42, second.id));
    app.switch_to_board(first.id);
    app.integrate_loaded_import(&ctx, test_loaded_item(43, third.id));

    assert_eq!(app.active_board_id, first.id);
    assert!(app.canvas.placement_for_id(41).is_none());
    assert!(app.canvas.placement_for_id(42).is_none());
    assert!(app.canvas.placement_for_id(43).is_none());

    let _ = fs::remove_dir_all(root);
}

#[cfg(feature = "semantic-search")]
#[test]
fn switching_boards_clears_selection_search_and_transient_undo() {
    let ctx = egui::Context::default();
    let root = temp_library_root("snappyref-board-switch-cleanup");
    let library = Library::initialize(root.clone()).unwrap();
    let second = library.create_board("Second").unwrap();
    let mut app = test_app(Some(library.clone()));
    let asset = ImageAsset {
        id: 1,
        original_path: PathBuf::from("fixture.png"),
        filename: "fixture.png".to_owned(),
        mime_type: "image/png".to_owned(),
        width: 1,
        height: 1,
        file_size: 4,
        imported_at: 0,
    };
    let placement = Placement::loaded(
        PlacementState {
            id: 1,
            asset_id: Some(1),
            center: eframe::egui::pos2(0.0, 0.0),
            scale: 1.0,
            rotation: 0.0,
            z_index: 0,
            archived: false,
        },
        asset,
    );
    app.canvas.add_prepared_placement_image(
        &ctx,
        PreparedPlacementImage {
            target_board_id: 1,
            queue_id: None,
            placement: placement.clone(),
            preview_rgba: vec![255, 255, 255, 255],
            preview_width: 1,
            preview_height: 1,
            animated_preview: None,
            preview_texture: crate::jobs::ImportedPreviewTexture::Cpu,
            loaded_lod: crate::data::CanvasImageLod::Working,
            existing_load_diagnostics: None,
            origin: crate::jobs::ImportOrigin::Existing,
            trace: None,
        },
    );
    app.canvas.select_placements(&[1]);
    app.search.overlay.open = true;
    app.canvas.set_search_matches(vec![1], Some(1));
    app.crop_flow.push_test_undo_entry(CropUndoEntry {
        previous_record: placement,
        previous_rgba: vec![255, 255, 255, 255].into(),
        previous_preview_width: 1,
        previous_preview_height: 1,
        removed_asset_id: 2,
    });

    app.switch_to_board(second.id);

    assert!(!app.canvas.has_selection());
    assert!(!app.search.is_open());
    assert!(app.crop_flow.undo_stack_is_empty());

    let _ = fs::remove_dir_all(root);
}

#[cfg(feature = "semantic-search")]
#[test]
fn search_shortcut_opens_overlay_when_enabled() {
    let ctx = egui::Context::default();
    let root = temp_library_root("snappyref-search-shortcut-enabled");
    let library = Library::initialize(root.clone()).unwrap();
    let mut app = test_app(Some(library));
    app.settings.semantic_search_enabled = true;
    app.search
        .set_local_model_store_status_for_verification(LocalModelStoreStatus::Installed);

    let _ = ctx.run_ui(
        egui::RawInput {
            modifiers: egui::Modifiers {
                command: true,
                ..Default::default()
            },
            events: vec![egui::Event::Key {
                key: egui::Key::F,
                physical_key: Some(egui::Key::F),
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers {
                    command: true,
                    ..Default::default()
                },
            }],
            ..Default::default()
        },
        |ui| app.handle_search_shortcut(ui.ctx()),
    );

    assert!(app.search.overlay.open);
    assert!(app.search.overlay.focus_requested);

    let _ = fs::remove_dir_all(root);
}

#[cfg(feature = "semantic-search")]
#[test]
fn search_shortcut_opens_missing_palette_with_focused_install_action() {
    let ctx = egui::Context::default();
    let mut app = test_app(None);
    app.settings.semantic_search_enabled = true;
    app.search
        .set_local_model_store_status_for_verification(LocalModelStoreStatus::Missing);

    let _ = ctx.run_ui(
        egui::RawInput {
            modifiers: egui::Modifiers {
                command: true,
                ..Default::default()
            },
            events: vec![egui::Event::Key {
                key: egui::Key::F,
                physical_key: Some(egui::Key::F),
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers {
                    command: true,
                    ..Default::default()
                },
            }],
            ..Default::default()
        },
        |ui| app.handle_search_shortcut(ui.ctx()),
    );

    assert!(app.search.overlay.open);
    assert!(app.search.overlay.install_action_focus_requested);
    assert!(app.search.overlay.results.is_empty());
    assert_eq!(
        app.search.local_model_store_status(),
        LocalModelStoreStatus::Missing
    );
}

#[cfg(feature = "semantic-search")]
#[test]
fn escape_closes_search_overlay_and_clears_matches() {
    let ctx = egui::Context::default();
    let mut app = test_app(None);
    app.search.overlay.open = true;
    app.search.overlay.query = "cat".to_owned();
    app.canvas.set_search_matches(vec![1, 2], Some(1));

    let _ = ctx.run_ui(
        egui::RawInput {
            events: vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: Some(egui::Key::Escape),
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::default(),
            }],
            ..Default::default()
        },
        |ui| {
            app.show_search_overlay(
                ui.ctx(),
                egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(960.0, 640.0)),
            )
        },
    );

    assert!(!app.search.overlay.open);
    assert!(app.search.overlay.query.is_empty());
}

#[cfg(feature = "semantic-search")]
#[test]
fn search_overlay_focuses_text_input_when_requested() {
    let ctx = egui::Context::default();
    let mut app = test_app(None);
    app.search.overlay.open = true;
    app.search.overlay.focus_requested = true;

    let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
        app.show_search_overlay(
            ui.ctx(),
            egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(960.0, 640.0)),
        )
    });

    assert!(ctx.memory(|memory| memory.has_focus(egui::Id::new("semantic_search_input"))));
    assert!(!app.search.overlay.focus_requested);
}

#[cfg(feature = "semantic-search")]
#[test]
fn tab_switches_search_scope_from_any_search_overlay_focus_state() {
    let ctx = egui::Context::default();
    let mut app = test_app(None);
    app.search.overlay.open = true;
    app.search.overlay.focus_requested = true;
    app.search.overlay.scope = SearchScope::ThisBoard;

    let _ = ctx.run_ui(
        egui::RawInput {
            events: vec![egui::Event::Key {
                key: egui::Key::Tab,
                physical_key: Some(egui::Key::Tab),
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::default(),
            }],
            ..Default::default()
        },
        |ui| {
            app.show_search_overlay(
                ui.ctx(),
                egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(960.0, 640.0)),
            )
        },
    );

    assert_eq!(app.search.overlay.scope, SearchScope::Library);
}

#[cfg(feature = "semantic-search")]
#[test]
fn search_keyboard_flow_owns_tab_and_escape_through_app_render_path() {
    let root = temp_library_root("snappyref-search-keyboard-routing");
    let library = Library::initialize(root.clone()).unwrap();
    let second = library.create_board("Second").unwrap();
    let mut app = test_app(Some(library.clone()));
    app.settings.semantic_search_enabled = true;
    app.search
        .set_local_model_store_status_for_verification(LocalModelStoreStatus::Installed);
    app.search.overlay.scope = SearchScope::ThisBoard;
    let mut harness = app_harness(app, |ctx, app| {
        let screen_rect = test_screen_rect();
        app.handle_search_shortcut(ctx);
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

    harness.key_press(egui::Key::Tab);
    harness.step();

    assert_eq!(harness.state().search.overlay.scope, SearchScope::Library);
    assert_eq!(harness.state().active_board_id, 1);

    harness.key_press_modifiers(
        egui::Modifiers {
            command: true,
            ..Default::default()
        },
        egui::Key::Num2,
    );
    harness.step();

    assert_eq!(harness.state().active_board_id, 1);

    harness.key_press(egui::Key::Escape);
    harness.step();

    assert!(!harness.state().search.overlay.open);
    assert!(harness.state().search.overlay.query.is_empty());
    assert_eq!(library.load_user_session().unwrap().active_board_id, 1);
    assert_ne!(second.id, 1);

    let _ = fs::remove_dir_all(root);
}

#[cfg(feature = "semantic-search")]
#[test]
fn enter_places_unplaced_library_search_result_on_empty_active_board() {
    let root = temp_library_root("snappyref-library-search-enter-unplaced");
    let library = Library::initialize(root.clone()).unwrap();
    let first_path = library
        .store_rgba_image("library-only", &[255, 0, 0, 255], 1, 1)
        .unwrap();
    let library_item = library
        .persist_imported_image(&first_path, "image/png", 1, 1, 4, egui::pos2(0.0, 0.0), 1.0)
        .unwrap();
    let empty_board = library.create_board("Empty").unwrap();
    library.set_active_board(empty_board.id).unwrap();
    let mut app = test_app(Some(library.clone()));
    app.active_board_id = empty_board.id;
    app.search.overlay.open = true;
    app.search.overlay.scope = SearchScope::Library;
    app.search.overlay.results = vec![SearchResultAssetEntry {
        asset_id: library_item
            .asset
            .as_ref()
            .expect("loaded placement asset")
            .id,
        asset_preview_path: None,
        asset_original_path: Some(
            library_item
                .asset
                .as_ref()
                .expect("loaded placement asset")
                .original_path
                .clone(),
        ),
        asset_width: 1,
        asset_height: 1,
        similarity: 0.9,
        placement_ids: Vec::new(),
        instance_count: 0,
    }];
    app.search.overlay.result_selection.replace(0);
    let mut harness = app_harness(app, |ctx, app| {
        app.show_search_overlay(ctx, test_screen_rect());
    });

    harness.key_press(egui::Key::Enter);
    harness.step();
    harness.hover_at(egui::pos2(900.0, 600.0));
    harness.drag_at(egui::pos2(900.0, 600.0));
    harness.drop_at(egui::pos2(900.0, 600.0));
    harness.step();

    let active_items = library.load_placements().unwrap();
    assert_eq!(active_items.len(), 1);
    assert_eq!(
        active_items[0]
            .asset
            .as_ref()
            .expect("loaded placement asset")
            .id,
        library_item
            .asset
            .as_ref()
            .expect("loaded placement asset")
            .id
    );
    assert_eq!(
        harness.state().canvas.selected_placement_ids(),
        vec![active_items[0].state.id]
    );
    assert!(!harness.state().search.overlay.open);

    let _ = fs::remove_dir_all(root);
}

#[cfg(feature = "semantic-search")]
#[test]
fn enter_places_unplaced_library_search_result_at_import_scale() {
    let root = temp_library_root("snappyref-library-search-enter-import-scale");
    let library = Library::initialize(root.clone()).unwrap();
    let width = 1000_u32;
    let height = 1000_u32;
    let first_path = library
        .store_rgba_image(
            "library-only-large",
            &vec![255; width as usize * height as usize * 4],
            width,
            height,
        )
        .unwrap();
    let library_item = library
        .persist_imported_image(
            &first_path,
            "image/png",
            width,
            height,
            4,
            egui::pos2(0.0, 0.0),
            1.0,
        )
        .unwrap();
    library
        .archive_placements(&[library_item.state.id])
        .unwrap();
    let empty_board = library.create_board("Empty").unwrap();
    library.set_active_board(empty_board.id).unwrap();
    let mut app = test_app(Some(library.clone()));
    app.active_board_id = empty_board.id;
    app.search.overlay.open = true;
    app.search.overlay.scope = SearchScope::Library;
    app.search.overlay.results = vec![SearchResultAssetEntry {
        asset_id: library_item
            .asset
            .as_ref()
            .expect("loaded placement asset")
            .id,
        asset_preview_path: None,
        asset_original_path: Some(
            library_item
                .asset
                .as_ref()
                .expect("loaded placement asset")
                .original_path
                .clone(),
        ),
        asset_width: 1,
        asset_height: 1,
        similarity: 0.9,
        placement_ids: Vec::new(),
        instance_count: 0,
    }];
    app.search.overlay.result_selection.replace(0);
    let mut harness = app_harness(app, |ctx, app| {
        app.show_search_overlay(ctx, test_screen_rect());
    });

    harness.key_press(egui::Key::Enter);
    harness.step();
    harness.hover_at(egui::pos2(900.0, 600.0));
    harness.drag_at(egui::pos2(900.0, 600.0));
    harness.drop_at(egui::pos2(900.0, 600.0));
    harness.step();

    let active_items = library.load_placements().unwrap();
    let expected_scale = crate::jobs::initial_import_scale(
        width,
        height,
        true,
        crate::data::DEFAULT_IMPORT_TARGET_CANVAS_AREA,
    );
    assert_eq!(active_items.len(), 1);
    assert_eq!(active_items[0].state.scale, expected_scale);

    let _ = fs::remove_dir_all(root);
}

#[cfg(feature = "semantic-search")]
#[test]
fn library_scope_survives_search_result_placement() {
    let root = temp_library_root("snappyref-library-search-scope-after-placement");
    let library = Library::initialize(root.clone()).unwrap();
    let first_path = library
        .store_rgba_image("library-only", &[255, 0, 0, 255], 1, 1)
        .unwrap();
    let library_item = library
        .persist_imported_image(&first_path, "image/png", 1, 1, 4, egui::pos2(0.0, 0.0), 1.0)
        .unwrap();
    library
        .archive_placements(&[library_item.state.id])
        .unwrap();
    let empty_board = library.create_board("Empty").unwrap();
    library.set_active_board(empty_board.id).unwrap();
    let mut app = test_app(Some(library.clone()));
    app.active_board_id = empty_board.id;
    app.search.overlay.open = true;
    app.search.overlay.scope = SearchScope::Library;
    app.search.overlay.results = vec![SearchResultAssetEntry {
        asset_id: library_item
            .asset
            .as_ref()
            .expect("loaded placement asset")
            .id,
        asset_preview_path: None,
        asset_original_path: Some(
            library_item
                .asset
                .as_ref()
                .expect("loaded placement asset")
                .original_path
                .clone(),
        ),
        asset_width: 1,
        asset_height: 1,
        similarity: 0.9,
        placement_ids: Vec::new(),
        instance_count: 0,
    }];
    app.search.overlay.result_selection.replace(0);
    let mut harness = app_harness(app, |ctx, app| {
        app.show_search_overlay(ctx, test_screen_rect());
    });

    harness.key_press(egui::Key::Enter);
    harness.step();
    harness.hover_at(egui::pos2(900.0, 600.0));
    harness.drag_at(egui::pos2(900.0, 600.0));
    harness.drop_at(egui::pos2(900.0, 600.0));
    harness.step();

    assert_eq!(harness.state().search.overlay.scope, SearchScope::Library);

    let _ = fs::remove_dir_all(root);
}

#[cfg(feature = "semantic-search")]
#[test]
fn dropped_library_search_result_is_visible_before_worker_decode_finishes() {
    let ctx = egui::Context::default();
    let root = temp_library_root("snappyref-library-search-visible-before-worker");
    let library = Library::initialize(root.clone()).unwrap();
    let first_path = library
        .store_rgba_image("library-only", &[255, 0, 0, 255], 1, 1)
        .unwrap();
    let library_item = library
        .persist_imported_image(&first_path, "image/png", 1, 1, 4, egui::pos2(0.0, 0.0), 1.0)
        .unwrap();
    library
        .archive_placements(&[library_item.state.id])
        .unwrap();
    let empty_board = library.create_board("Empty").unwrap();
    library.set_active_board(empty_board.id).unwrap();
    let mut app = test_app(Some(library.clone()));
    app.import_worker = Some(crate::jobs::ImportWorker::new(
        library.clone(),
        ctx.clone(),
        None,
    ));
    app.active_board_id = empty_board.id;
    app.search.overlay.open = true;
    app.search.overlay.scope = SearchScope::Library;
    app.search.overlay.results = vec![SearchResultAssetEntry {
        asset_id: library_item
            .asset
            .as_ref()
            .expect("loaded placement asset")
            .id,
        asset_preview_path: None,
        asset_original_path: Some(
            library_item
                .asset
                .as_ref()
                .expect("loaded placement asset")
                .original_path
                .clone(),
        ),
        asset_width: 1,
        asset_height: 1,
        similarity: 0.9,
        placement_ids: Vec::new(),
        instance_count: 0,
    }];
    app.search.overlay.result_selection.replace(0);
    let mut harness = app_harness(app, |ctx, app| {
        app.show_search_overlay(ctx, test_screen_rect());
    });

    harness.key_press(egui::Key::Enter);
    harness.step();
    harness.hover_at(egui::pos2(900.0, 600.0));
    harness.drag_at(egui::pos2(900.0, 600.0));
    harness.drop_at(egui::pos2(900.0, 600.0));
    harness.step();

    let active_items = library.load_placements().unwrap();
    assert_eq!(active_items.len(), 1);
    assert!(
        harness
            .state()
            .canvas
            .placement_for_id(active_items[0].state.id)
            .is_some()
    );
    assert_eq!(harness.state().canvas.placement_preview_slot_count(), 0);
    assert_eq!(harness.state().pending_import_batches.len(), 0);

    let _ = fs::remove_dir_all(root);
}

#[cfg(feature = "semantic-search")]
#[test]
fn long_search_error_does_not_expand_palette_width() {
    let mut app = test_app(None);
    app.search.overlay.open = true;
    app.search.overlay.query = "boat".to_owned();
    app.search.overlay.status = crate::search::palette::SearchOverlayStatus::Error(format!(
        "failed to open image /Users/noelcserepy/dev/snappyref/target/debug/library/originals/{}-image-2025-08-05-160929.jpg: Format error decoding Jpeg: Error parsing image. Illegal start bytes: 8950",
        "1777813588290-19"
    ));
    let mut harness = app_harness(app, |ctx, app| {
        app.show_search_overlay(ctx, test_screen_rect());
    });

    harness.step();

    let overlay_width = harness
        .state()
        .search
        .overlay
        .last_expanded_overlay_rect
        .map(|rect| rect.width())
        .unwrap();
    assert!(overlay_width <= PALETTE_WIDTH + 2.0);
}

#[cfg(feature = "semantic-search")]
#[test]
fn command_enter_library_search_results_show_pending_cursor_placeholders() {
    let root = temp_library_root("snappyref-library-search-cmd-enter-placeholder-preview");
    let library = Library::initialize(root.clone()).unwrap();
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
    app.search.overlay.scope = SearchScope::Library;
    app.search.overlay.results = vec![
        SearchResultAssetEntry {
            asset_id: first.asset.as_ref().expect("loaded placement asset").id,
            asset_preview_path: None,
            asset_original_path: Some(
                first
                    .asset
                    .as_ref()
                    .expect("loaded placement asset")
                    .original_path
                    .clone(),
            ),
            asset_width: 1,
            asset_height: 1,
            similarity: 0.9,
            placement_ids: Vec::new(),
            instance_count: 0,
        },
        SearchResultAssetEntry {
            asset_id: second.asset.as_ref().expect("loaded placement asset").id,
            asset_preview_path: None,
            asset_original_path: Some(
                second
                    .asset
                    .as_ref()
                    .expect("loaded placement asset")
                    .original_path
                    .clone(),
            ),
            asset_width: 1,
            asset_height: 1,
            similarity: 0.8,
            placement_ids: Vec::new(),
            instance_count: 0,
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

    assert_eq!(
        harness
            .state()
            .canvas
            .provisional_records_in_selection_order()
            .len(),
        2
    );

    let _ = fs::remove_dir_all(root);
}

#[cfg(feature = "semantic-search")]
#[test]
fn search_asset_preview_uses_known_asset_dimensions_without_decoding_original_path() {
    let root = temp_library_root("snappyref-library-search-known-dimensions-preview");
    let library = Library::initialize(root.clone()).unwrap();
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
    app.search.overlay.scope = SearchScope::Library;
    app.search.overlay.results = vec![
        SearchResultAssetEntry {
            asset_id: first.asset.as_ref().expect("loaded placement asset").id,
            asset_preview_path: None,
            asset_original_path: None,
            asset_width: first.asset.as_ref().expect("loaded placement asset").width,
            asset_height: first.asset.as_ref().expect("loaded placement asset").height,
            similarity: 0.9,
            placement_ids: Vec::new(),
            instance_count: 0,
        },
        SearchResultAssetEntry {
            asset_id: second.asset.as_ref().expect("loaded placement asset").id,
            asset_preview_path: None,
            asset_original_path: None,
            asset_width: second.asset.as_ref().expect("loaded placement asset").width,
            asset_height: second
                .asset
                .as_ref()
                .expect("loaded placement asset")
                .height,
            similarity: 0.8,
            placement_ids: Vec::new(),
            instance_count: 0,
        },
    ];
    app.search.overlay.result_selection.replace(0);
    app.search.overlay.result_selection.toggle(1);
    let mut harness = app_harness(app, |ctx, app| {
        app.show_search_overlay(ctx, test_screen_rect());
    });

    harness.key_press(egui::Key::Enter);
    harness.step();

    let layout = harness
        .state()
        .canvas
        .provisional_records_in_selection_order()
        .into_iter()
        .map(|placement| crate::data::PlacementFootprint {
            id: placement.state.id,
            center: placement.state.center,
            width: placement.scaled_visual_size().x,
            height: placement.scaled_visual_size().y,
        })
        .collect::<Vec<_>>();
    assert_eq!(layout.len(), 2);
    let first_size = harness.state().import_size_world(
        first.asset.as_ref().expect("loaded placement asset").width,
        first.asset.as_ref().expect("loaded placement asset").height,
    );
    let second_size = harness.state().import_size_world(
        second.asset.as_ref().expect("loaded placement asset").width,
        second
            .asset
            .as_ref()
            .expect("loaded placement asset")
            .height,
    );
    assert!(layout.iter().any(|item| {
        (item.width - first_size.x).abs() < 0.1 && (item.height - first_size.y).abs() < 0.1
    }));
    assert!(layout.iter().any(|item| {
        (item.width - second_size.x).abs() < 0.1 && (item.height - second_size.y).abs() < 0.1
    }));

    let _ = fs::remove_dir_all(root);
}

#[cfg(feature = "semantic-search")]
#[test]
fn multi_select_enter_library_search_results_show_pending_cursor_placeholders() {
    let root = temp_library_root("snappyref-library-search-multi-enter-placeholder-preview");
    let library = Library::initialize(root.clone()).unwrap();
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
    app.search.overlay.scope = SearchScope::Library;
    app.search.overlay.results = vec![
        SearchResultAssetEntry {
            asset_id: first.asset.as_ref().expect("loaded placement asset").id,
            asset_preview_path: None,
            asset_original_path: Some(
                first
                    .asset
                    .as_ref()
                    .expect("loaded placement asset")
                    .original_path
                    .clone(),
            ),
            asset_width: 1,
            asset_height: 1,
            similarity: 0.9,
            placement_ids: Vec::new(),
            instance_count: 0,
        },
        SearchResultAssetEntry {
            asset_id: second.asset.as_ref().expect("loaded placement asset").id,
            asset_preview_path: None,
            asset_original_path: Some(
                second
                    .asset
                    .as_ref()
                    .expect("loaded placement asset")
                    .original_path
                    .clone(),
            ),
            asset_width: 1,
            asset_height: 1,
            similarity: 0.8,
            placement_ids: Vec::new(),
            instance_count: 0,
        },
    ];
    app.search.overlay.result_selection.replace(0);
    app.search.overlay.result_selection.toggle(1);
    let mut harness = app_harness(app, |ctx, app| {
        app.show_search_overlay(ctx, test_screen_rect());
    });

    harness.key_press(egui::Key::Enter);
    harness.step();

    assert_eq!(
        harness
            .state()
            .canvas
            .provisional_records_in_selection_order()
            .len(),
        2
    );

    let _ = fs::remove_dir_all(root);
}

#[cfg(feature = "semantic-search")]
#[test]
fn clicked_multi_select_enter_places_all_selected_library_results() {
    let root = temp_library_root("snappyref-library-search-click-multi-enter-drop");
    let library = Library::initialize(root.clone()).unwrap();
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
    app.search.overlay.scope = SearchScope::Library;
    app.search.overlay.results = vec![
        SearchResultAssetEntry {
            asset_id: first.asset.as_ref().expect("loaded placement asset").id,
            asset_preview_path: None,
            asset_original_path: Some(
                first
                    .asset
                    .as_ref()
                    .expect("loaded placement asset")
                    .original_path
                    .clone(),
            ),
            asset_width: 1,
            asset_height: 1,
            similarity: 0.9,
            placement_ids: Vec::new(),
            instance_count: 0,
        },
        SearchResultAssetEntry {
            asset_id: second.asset.as_ref().expect("loaded placement asset").id,
            asset_preview_path: None,
            asset_original_path: Some(
                second
                    .asset
                    .as_ref()
                    .expect("loaded placement asset")
                    .original_path
                    .clone(),
            ),
            asset_width: 1,
            asset_height: 1,
            similarity: 0.8,
            placement_ids: Vec::new(),
            instance_count: 0,
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
            .provisional_records_in_selection_order()
            .len(),
        2
    );

    harness.hover_at(egui::pos2(900.0, 600.0));
    harness.drag_at(egui::pos2(900.0, 600.0));
    harness.drop_at(egui::pos2(900.0, 600.0));
    harness.step();

    let mut asset_ids = library
        .load_placements()
        .unwrap()
        .into_iter()
        .map(|placement| placement.asset.as_ref().expect("loaded placement asset").id)
        .collect::<Vec<_>>();
    asset_ids.sort_unstable();
    let mut expected_asset_ids = vec![
        first.asset.as_ref().expect("loaded placement asset").id,
        second.asset.as_ref().expect("loaded placement asset").id,
    ];
    expected_asset_ids.sort_unstable();
    assert_eq!(asset_ids, expected_asset_ids);

    let _ = fs::remove_dir_all(root);
}

#[cfg(feature = "semantic-search")]
#[test]
fn command_enter_places_all_unplaced_library_search_results_on_empty_active_board() {
    let root = temp_library_root("snappyref-library-search-command-enter-unplaced");
    let library = Library::initialize(root.clone()).unwrap();
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
            egui::pos2(20.0, 0.0),
            1.0,
        )
        .unwrap();
    let empty_board = library.create_board("Empty").unwrap();
    library.set_active_board(empty_board.id).unwrap();
    let mut app = test_app(Some(library.clone()));
    app.active_board_id = empty_board.id;
    app.search.overlay.open = true;
    app.search.overlay.scope = SearchScope::Library;
    app.search.overlay.results = vec![
        SearchResultAssetEntry {
            asset_id: first.asset.as_ref().expect("loaded placement asset").id,
            asset_preview_path: None,
            asset_original_path: Some(
                first
                    .asset
                    .as_ref()
                    .expect("loaded placement asset")
                    .original_path
                    .clone(),
            ),
            asset_width: 1,
            asset_height: 1,
            similarity: 0.9,
            placement_ids: Vec::new(),
            instance_count: 0,
        },
        SearchResultAssetEntry {
            asset_id: second.asset.as_ref().expect("loaded placement asset").id,
            asset_preview_path: None,
            asset_original_path: Some(
                second
                    .asset
                    .as_ref()
                    .expect("loaded placement asset")
                    .original_path
                    .clone(),
            ),
            asset_width: 1,
            asset_height: 1,
            similarity: 0.8,
            placement_ids: Vec::new(),
            instance_count: 0,
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

    let mut asset_ids = library
        .load_placements()
        .unwrap()
        .into_iter()
        .map(|placement| placement.asset.as_ref().expect("loaded placement asset").id)
        .collect::<Vec<_>>();
    asset_ids.sort_unstable();
    let mut expected_asset_ids = vec![
        first.asset.as_ref().expect("loaded placement asset").id,
        second.asset.as_ref().expect("loaded placement asset").id,
    ];
    expected_asset_ids.sort_unstable();
    assert_eq!(asset_ids, expected_asset_ids);
    assert_eq!(harness.state().canvas.selected_placement_ids().len(), 2);
    assert!(!harness.state().search.overlay.open);

    let _ = fs::remove_dir_all(root);
}

#[cfg(feature = "semantic-search")]
#[test]
fn submitted_search_is_saved_to_recent_searches() {
    let root = temp_library_root("snappyref-search-recent-session");
    let library = Library::initialize(root.clone()).unwrap();
    let mut app = test_app(Some(library.clone()));
    app.search.overlay.open = true;
    app.search.overlay.query = "brutalist chair".to_owned();
    app.search.overlay.last_query_edit_at = Some(Instant::now() - Duration::from_millis(200));
    let mut harness = app_harness(app, |ctx, app| {
        app.show_search_overlay(ctx, test_screen_rect());
    });

    harness.step();

    assert_eq!(
        library.load_user_session().unwrap().recent_searches,
        vec!["brutalist chair".to_owned()]
    );

    let _ = fs::remove_dir_all(root);
}

#[cfg(feature = "semantic-search")]
#[test]
fn empty_recent_searches_do_not_submit_boilerplate_query() {
    let root = temp_library_root("snappyref-search-no-boilerplate-recent");
    let library = Library::initialize(root.clone()).unwrap();
    let mut app = test_app(Some(library.clone()));
    app.search.overlay.open = true;
    let mut harness = app_harness(app, |ctx, app| {
        app.show_search_overlay(ctx, test_screen_rect());
    });

    harness.key_press(egui::Key::Enter);
    harness.step();

    assert!(harness.state().search.overlay.query.is_empty());
    assert!(
        library
            .load_user_session()
            .unwrap()
            .recent_searches
            .is_empty()
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn context_menu_move_to_front_routes_through_rendered_menu_row() {
    let ctx = egui::Context::default();
    let root = temp_library_root("snappyref-context-menu-move-front");
    let library = Library::initialize(root.clone()).unwrap();
    let image_path = library
        .store_rgba_image("source", &[255, 0, 0, 255], 1, 1)
        .unwrap();
    let back_item = library
        .persist_imported_image(&image_path, "image/png", 1, 1, 4, egui::pos2(0.0, 0.0), 1.0)
        .unwrap();
    let selected_item = library
        .create_placement_from_asset(
            back_item.asset.as_ref().expect("loaded placement asset").id,
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
    assert_eq!(
        library.load_canvas_stack_order().unwrap(),
        harness.state().canvas.canvas_stack_order()
    );

    let _ = fs::remove_dir_all(root);
}

#[cfg(feature = "semantic-search")]
#[test]
fn search_result_grid_layout_balances_five_results_into_five_columns() {
    let shell_content_width = PALETTE_WIDTH - 24.0;
    let grid_layout = search_result_grid_layout(shell_content_width, 5);

    assert_eq!(grid_layout.columns, 5);
}

#[cfg(feature = "semantic-search")]
#[test]
fn search_result_grid_layout_keeps_partial_rows_at_five_column_width() {
    let shell_content_width = PALETTE_WIDTH - 24.0;
    let five_result_layout = search_result_grid_layout(shell_content_width, 5);
    let two_result_layout = search_result_grid_layout(shell_content_width, 2);

    assert_eq!(two_result_layout.columns, 5);
    assert_eq!(two_result_layout.tile_width, five_result_layout.tile_width);
}

#[cfg(feature = "semantic-search")]
#[test]
fn search_result_grid_content_height_grows_by_rows() {
    let height = search_result_grid_content_height(6, 5);

    assert_eq!(
        height,
        search_result_tile_height() * 2.0 + crate::ui::theme::SPACE_2
    );
}

#[cfg(feature = "semantic-search")]
#[test]
fn search_drop_source_center_uses_image_extents() {
    let ctx = egui::Context::default();
    let mut app = test_app(None);

    for (id, x, width) in [(1_i64, 0.0_f32, 100_u32), (2, 200.0, 300)] {
        let asset = ImageAsset {
            id,
            original_path: PathBuf::from(format!("fixture-{id}.png")),
            filename: format!("fixture-{id}.png"),
            mime_type: "image/png".to_owned(),
            width,
            height: 100,
            file_size: 0,
            imported_at: 0,
        };
        let placement = Placement::loaded(
            PlacementState {
                id,
                asset_id: Some(id),
                center: egui::pos2(x, 0.0),
                scale: 1.0,
                rotation: 0.0,
                z_index: id as i32,
                archived: false,
            },
            asset,
        );
        app.canvas.add_prepared_placement_image(
            &ctx,
            PreparedPlacementImage {
                target_board_id: 1,
                queue_id: None,
                placement,
                preview_rgba: vec![255; width as usize * 100 * 4],
                preview_width: width,
                preview_height: 100,
                animated_preview: None,
                preview_texture: crate::jobs::ImportedPreviewTexture::Cpu,
                loaded_lod: crate::data::CanvasImageLod::Working,
                existing_load_diagnostics: None,
                origin: crate::jobs::ImportOrigin::Existing,
                trace: None,
            },
        );
    }

    let center = app.source_center_for_placement_ids(&[1, 2]).unwrap();

    assert_eq!(center, egui::pos2(150.0, 0.0));
}

#[test]
fn queued_placement_preview_slots_replace_and_cleanup_by_queue_id() {
    let mut app = test_app(None);

    app.queue_placement_preview_slot_for_board(
        app.active_board_id,
        7,
        "first".to_owned(),
        egui::pos2(10.0, 20.0),
        300,
        200,
    );
    app.queue_placement_preview_slot_for_board(
        app.active_board_id,
        7,
        "second".to_owned(),
        egui::pos2(30.0, 40.0),
        120,
        90,
    );

    assert_eq!(app.canvas.placement_preview_slot_count(), 1);
    app.canvas.remove_placement_preview_slot(7);
    assert_eq!(app.canvas.placement_preview_slot_count(), 0);
}

#[test]
fn completed_multi_file_import_batch_selects_without_mutating_geometry() {
    let ctx = egui::Context::default();
    let mut app = test_app(None);
    app.register_import_batch(app.active_board_id, egui::pos2(120.0, 80.0), vec![1, 2, 3]);

    for (id, width, height) in [(1_i64, 180_u32, 80_u32), (2, 70, 210), (3, 140, 120)] {
        let asset = ImageAsset {
            id,
            original_path: PathBuf::from(format!("fixture-{id}.png")),
            filename: format!("fixture-{id}.png"),
            mime_type: "image/png".to_owned(),
            width,
            height,
            file_size: 4,
            imported_at: 0,
        };
        let placement = Placement::loaded(
            PlacementState {
                id,
                asset_id: Some(id),
                center: eframe::egui::pos2(0.0, 0.0),
                scale: 1.0,
                rotation: 0.0,
                z_index: id as i32,
                archived: false,
            },
            asset,
        );
        app.integrate_loaded_import(
            &ctx,
            PreparedPlacementImage {
                target_board_id: 1,
                queue_id: Some(id as u64),
                placement,
                preview_rgba: vec![255; width as usize * height as usize * 4],
                preview_width: width,
                preview_height: height,
                animated_preview: None,
                preview_texture: crate::jobs::ImportedPreviewTexture::Cpu,
                loaded_lod: crate::data::CanvasImageLod::Working,
                existing_load_diagnostics: None,
                origin: crate::jobs::ImportOrigin::Path,
                trace: None,
            },
        );
    }

    assert_eq!(app.canvas.selected_placement_ids(), vec![1, 2, 3]);
    for placement_id in [1_i64, 2, 3] {
        let placement = app.canvas.placement_for_id(placement_id).unwrap();
        assert_eq!(placement.state.center.x, 0.0);
        assert_eq!(placement.state.center.y, 0.0);
    }
    assert!(app.pending_import_batches.is_empty());
}

#[test]
fn completed_import_batch_uses_original_anchor_after_out_of_order_failures() {
    let ctx = egui::Context::default();
    let mut app = test_app(None);
    let anchor = egui::pos2(-340.0, 210.0);
    app.register_import_batch(app.active_board_id, anchor, vec![1, 2, 3]);

    let mut third = test_loaded_item(3, app.active_board_id);
    third
        .placement
        .asset
        .as_mut()
        .expect("loaded placement asset")
        .width = 140;
    third
        .placement
        .asset
        .as_mut()
        .expect("loaded placement asset")
        .height = 80;
    third.placement.visual_size = egui::vec2(140.0, 80.0);
    third.placement.state.center.x = 4000.0;
    third.placement.state.center.y = -1000.0;
    app.integrate_loaded_import(&ctx, third);

    app.finish_import_queue_id_in_batches(Some(app.active_board_id), Some(2), None);

    let mut first = test_loaded_item(1, app.active_board_id);
    first
        .placement
        .asset
        .as_mut()
        .expect("loaded placement asset")
        .width = 90;
    first
        .placement
        .asset
        .as_mut()
        .expect("loaded placement asset")
        .height = 170;
    first.placement.visual_size = egui::vec2(90.0, 170.0);
    first.placement.state.center.x = -2500.0;
    first.placement.state.center.y = 3600.0;
    app.integrate_loaded_import(&ctx, first);

    assert_eq!(app.canvas.selected_placement_ids(), vec![3, 1]);
    assert_eq!(
        app.canvas.placement_for_id(3).unwrap().state.center.x,
        4000.0
    );
    assert_eq!(
        app.canvas.placement_for_id(3).unwrap().state.center.y,
        -1000.0
    );
    assert_eq!(
        app.canvas.placement_for_id(1).unwrap().state.center.x,
        -2500.0
    );
    assert_eq!(
        app.canvas.placement_for_id(1).unwrap().state.center.y,
        3600.0
    );
    assert!(app.pending_import_batches.is_empty());
}

#[test]
fn completed_inactive_board_import_batch_does_not_mutate_active_canvas() {
    let ctx = egui::Context::default();
    let mut app = test_app(None);
    app.register_import_batch(1, egui::pos2(0.0, 0.0), vec![1, 2]);
    app.active_board_id = 2;

    app.integrate_loaded_import(&ctx, test_loaded_item(1, 1));
    app.integrate_loaded_import(&ctx, test_loaded_item(2, 1));

    assert!(app.canvas.selected_placement_ids().is_empty());
    assert!(app.canvas.placement_for_id(1).is_none());
    assert!(app.pending_import_batches.is_empty());
}

#[test]
fn import_reservation_creates_loading_placements_for_known_and_fallback_dimensions() {
    let root = temp_library_root("snappyref-import-reservation");
    let library = Library::initialize(root.clone()).unwrap();
    let second = library.create_board("Second").unwrap();
    let mut app = test_app(Some(library));
    app.active_board_id = second.id;
    let mut layout = Vec::new();

    let known = app.reserve_import(
        &mut layout,
        "known".to_owned(),
        Some((100, 80)),
        egui::Pos2::ZERO,
        test_viewport_world(),
        crate::data::ContentResolutionSource::Unknown {
            description: "known test import".to_owned(),
        },
    );
    let unknown = app.reserve_import(
        &mut layout,
        "unknown".to_owned(),
        None,
        egui::pos2(200.0, 0.0),
        test_viewport_world(),
        crate::data::ContentResolutionSource::Unknown {
            description: "unknown test import".to_owned(),
        },
    );

    assert_eq!(known.target_board_id, second.id);
    assert_eq!(known.queue_id, 1);
    assert!(known.placement_id.is_some());
    assert_eq!(known.reserved_world, Some(egui::Pos2::ZERO));
    assert_eq!(unknown.target_board_id, second.id);
    assert_eq!(unknown.queue_id, 2);
    assert!(unknown.placement_id.is_some());
    assert_eq!(unknown.reserved_world, Some(egui::pos2(200.0, 0.0)));
    assert_eq!(app.canvas.placement_preview_slot_count(), 0);
    assert_eq!(layout.len(), 2);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn completed_import_for_inactive_board_does_not_enter_active_canvas() {
    let ctx = egui::Context::default();
    let root = temp_library_root("snappyref-inactive-import-completion");
    let library = Library::initialize(root.clone()).unwrap();
    let second = library.create_board("Second").unwrap();
    let mut app = test_app(Some(library));
    app.active_board_id = 1;
    let loaded = test_loaded_item(41, second.id);

    app.integrate_loaded_import(&ctx, loaded);

    assert!(app.canvas.placement_for_id(41).is_none());
    assert_eq!(app.status, "Imported image");

    let _ = fs::remove_dir_all(root);
}

#[test]
fn completed_import_for_active_board_enters_active_canvas() {
    let ctx = egui::Context::default();
    let mut app = test_app(None);
    app.active_board_id = 1;
    let loaded = test_loaded_item(42, 1);

    app.integrate_loaded_import(&ctx, loaded);

    assert!(app.canvas.placement_for_id(42).is_some());
    assert_eq!(app.status, "Imported image");
}

#[test]
fn queued_placement_preview_slots_are_scoped_to_their_board_across_switches() {
    let root = temp_library_root("snappyref-placeholder-board-scope");
    let library = Library::initialize(root.clone()).unwrap();
    let second = library.create_board("Second").unwrap();
    let mut app = test_app(Some(library));
    app.active_board_id = 1;
    app.queue_placement_preview_slot_for_board(
        app.active_board_id,
        9,
        "first".to_owned(),
        egui::pos2(10.0, 20.0),
        100,
        100,
    );

    app.switch_to_board(second.id);
    assert_eq!(app.canvas.placement_preview_slot_count(), 0);

    app.switch_to_board(1);
    assert_eq!(app.canvas.placement_preview_slot_count(), 1);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn normalize_import_url_accepts_http_and_https_only() {
    assert_eq!(
        crate::sources::import::normalize_import_url(" https://example.com/cat.png?size=large "),
        Some("https://example.com/cat.png?size=large".to_owned())
    );
    assert_eq!(
        crate::sources::import::normalize_import_url("http://example.com/dog.gif"),
        Some("http://example.com/dog.gif".to_owned())
    );
    assert_eq!(
        crate::sources::import::normalize_import_url("file:///tmp/cat.png"),
        None
    );
    assert_eq!(
        crate::sources::import::normalize_import_url("not a url"),
        None
    );
}

#[test]
fn import_source_label_prefers_url_path_basename_then_host() {
    assert_eq!(
        crate::sources::import::import_source_label(&super::ExternalImportSource::Url(
            "https://images.example.com/path/cat.gif?size=large".to_owned()
        )),
        "cat.gif"
    );
    assert_eq!(
        crate::sources::import::import_source_label(&super::ExternalImportSource::Url(
            "https://images.example.com".to_owned()
        )),
        "images.example.com"
    );
}

#[test]
fn url_batch_previews_use_balanced_layout_for_approximate_dimensions() {
    let mut app = test_app(None);
    let sources = (0..13)
        .map(|index| {
            super::ExternalImportSource::Url(format!("https://example.com/image-{index}.png"))
        })
        .collect::<Vec<_>>();

    let previews =
        app.dropped_import_previews(&sources, egui::pos2(100.0, 200.0), test_screen_rect());
    let distinct_x = previews
        .iter()
        .fold(Vec::<i32>::new(), |mut values, (_, _, center)| {
            let rounded = center.x.round() as i32;
            if !values.contains(&rounded) {
                values.push(rounded);
            }
            values
        });
    let distinct_y = previews
        .iter()
        .fold(Vec::<i32>::new(), |mut values, (_, _, center)| {
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

#[test]
fn import_requests_preserve_import_area_cap_when_auto_scaling_setting_is_off() {
    let mut app = test_app(None);
    app.settings.auto_image_scaling = false;

    let config = app.import_request_config();

    assert!(config.auto_image_scaling);
}

#[test]
fn raw_dropped_file_enqueues_external_import() {
    let ctx = egui::Context::default();
    let root = temp_library_root("snappyref-raw-file-drop");
    let library = Library::initialize(root.clone()).unwrap();
    let source_path = library
        .store_rgba_image("drop-source", &[255, 0, 0, 255], 1, 1)
        .unwrap();
    let mut app = test_app(Some(library.clone()));
    app.import_worker = Some(crate::jobs::ImportWorker::new(
        library.clone(),
        ctx.clone(),
        None,
    ));
    let mut harness = app_harness(app, |ctx, app| {
        egui::Area::new(egui::Id::new("drop-test-surface"))
            .fixed_pos(egui::Pos2::ZERO)
            .show(ctx, |ui| {
                ui.set_min_size(test_screen_rect().size());
                app.handle_hovered_files(ui, test_screen_rect());
                app.handle_dropped_files(ui, test_screen_rect());
            });
    });
    let drop_pos = egui::pos2(900.0, 600.0);

    harness.hover_at(drop_pos);
    harness.input_mut().hovered_files = vec![egui::HoveredFile {
        path: Some(source_path.clone()),
        ..Default::default()
    }];
    harness.step();
    assert_eq!(harness.state().canvas.hover_preview_slot_count(), 1);

    harness.input_mut().hovered_files.clear();
    harness.input_mut().dropped_files = vec![egui::DroppedFile {
        path: Some(source_path.clone()),
        name: source_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("drop-source.png")
            .to_owned(),
        ..Default::default()
    }];
    harness.step();

    assert_eq!(harness.state().canvas.hover_preview_slot_count(), 0);
    assert_eq!(harness.state().content_resolution.pending_len(), 1);
    let items = library.load_placements().unwrap();
    assert_eq!(items.len(), 1);
    let screen_rect = test_screen_rect();
    let placement = &items[0];
    let expected_world = harness
        .state()
        .canvas
        .screen_to_world(drop_pos, screen_rect);
    assert!(
        egui::pos2(placement.state.center.x, placement.state.center.y).distance(expected_world)
            < 1.0
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn missing_drop_event_fallback_is_single_use_after_hover_ends() {
    let mut app = test_app(None);
    let hover_pos = egui::pos2(320.0, 240.0);
    let source = crate::sources::import::ExternalImportSource::Path(std::path::PathBuf::from(
        "C:/Users/Admin/Pictures/drop.png",
    ));
    app.pending_missing_drop_fallback = Some(crate::sources::external_drag::ExternalDragIntake {
        sources: vec![source.clone()],
        screen_pos: Some(hover_pos),
    });

    let fallback = app
        .take_missing_drop_event_fallback()
        .expect("hover-end fallback should be available once");

    assert_eq!(fallback.sources, vec![source]);
    assert_eq!(fallback.screen_pos, Some(hover_pos));
    assert!(app.take_missing_drop_event_fallback().is_none());
}

fn test_app(library: Option<Library>) -> SnappyRefApp {
    let mut app = SnappyRefApp {
        canvas: crate::canvas::CanvasState::default(),
        screenshot_capture: crate::sources::screenshot::ScreenshotCapture::new(None),
        window_capture: None,
        wgpu_render_state: None,
        library,
        import_worker: None,
        active_board_id: 1,
        window_state: crate::data::WindowState::default(),
        persist_window_state: true,
        crop_flow: super::CropFlow::default(),
        document_history: super::DocumentHistory::default(),
        max_texture_side: 8192,
        settings: crate::data::WorkspaceSettings::default(),
        #[cfg(feature = "semantic-search")]
        search: super::SearchController::default(),
        frame_perf: super::FramePerfState::default(),
        import_intake: super::ImportIntake::new(),
        content_resolution: super::ContentResolutionRuntime::new(),
        residency: super::ImageResidencyCoordinator::default(),
        pending_import_batches: crate::placement::creation::PlacementCreationBatchTracker::default(
        ),
        sidebar_open: false,
        sidebar_tab: super::SidebarTab::default(),
        pending_board_delete: None,
        pending_library_remove: None,
        #[cfg(feature = "semantic-search")]
        pending_semantic_search_remove: false,
        context_menu_pos: None,
        status: String::new(),
        perf_log: None,
        perf_scenario: None,
        hover_preview_dimensions: std::collections::HashMap::new(),
        last_hovered_external_drag: None,
        pending_missing_drop_fallback: None,
        document_persistence: super::DocumentPersistence::default(),
        pending_user_session: None,
        pending_user_session_save_at: None,
        board_summaries_cache: BoardSummariesCache::default(),
        #[cfg(debug_assertions)]
        verification_mode: None,
        #[cfg(debug_assertions)]
        verification_search_query: None,
        #[cfg(all(debug_assertions, feature = "semantic-search"))]
        verification_search_scope: crate::search::semantic::SearchScope::ThisBoard,
        #[cfg(debug_assertions)]
        sidebar_hot_preview: false,
        update_service: super::UpdateService::new(super::UpdateConfig::default()),
        user_update_flow_active: false,
        update_smoke: None,
        last_manual_update_check: None,
    };
    app.refresh_board_summaries();
    app
}

fn test_screen_rect() -> egui::Rect {
    egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 800.0))
}

fn render_sidebar_for_test(ctx: &egui::Context, app: &mut SnappyRefApp) {
    let screen_rect = test_screen_rect();
    if app.should_block_sidebar_input() {
        block_sidebar_input(ctx, sidebar_rect(screen_rect));
    }
    let board_summaries = app.board_summaries();
    let about_updates = app.about_updates_view();
    let sidebar_result = show_sidebar(
        ctx,
        screen_rect,
        app.sidebar_open,
        !app.modal_open(),
        &mut app.sidebar_tab,
        &mut app.settings,
        &mut app.canvas,
        &board_summaries,
        app.active_board_id,
        None,
        app.update_service.state(),
        about_updates,
    );
    let board_action = if app.modal_open() {
        None
    } else {
        sidebar_result.board_action
    };
    app.handle_board_action(board_action);
    app.handle_semantic_search_settings_action(sidebar_result.semantic_search_action, ctx);
    if sidebar_result.update_action_clicked {
        app.handle_update_action_click(ctx);
    }
}

fn app_harness<'a>(
    app: SnappyRefApp,
    mut render: impl FnMut(&egui::Context, &mut SnappyRefApp) + 'a,
) -> Harness<'a, SnappyRefApp> {
    #[expect(deprecated)]
    Harness::builder()
        .with_size(egui::vec2(1200.0, 800.0))
        .with_pixels_per_point(1.0)
        .with_os(egui::os::OperatingSystem::Nix)
        .with_theme(egui::Theme::Dark)
        .with_max_steps(4)
        .build_state(
            move |ctx, app| {
                crate::ui::theme::configure_theme(ctx);
                render(ctx, app);
            },
            app,
        )
}

fn drag_test_pos<State>(harness: &Harness<'_, State>, from: egui::Pos2, to: egui::Pos2) {
    harness.hover_at(from);
    harness.drag_at(from);
    harness.hover_at(to);
    harness.drop_at(to);
}

fn temp_library_root(prefix: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "{prefix}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&root).unwrap();
    root
}

fn test_viewport_world() -> egui::Rect {
    egui::Rect::from_min_max(egui::pos2(-240.0, -180.0), egui::pos2(240.0, 180.0))
}

fn item_enabled(items: &[super::ContextMenuItem], action: super::ContextMenuAction) -> bool {
    items
        .iter()
        .find(|item| item.action == action)
        .is_some_and(|item| item.enabled)
}

fn test_loaded_item(placement_id: i64, target_board_id: i64) -> PreparedPlacementImage {
    let asset = ImageAsset {
        id: placement_id,
        original_path: PathBuf::from(format!("fixture-{placement_id}.png")),
        filename: format!("fixture-{placement_id}.png"),
        mime_type: "image/png".to_owned(),
        width: 1,
        height: 1,
        file_size: 4,
        imported_at: 0,
    };
    let placement = Placement::loaded(
        PlacementState {
            id: placement_id,
            asset_id: Some(placement_id),
            center: eframe::egui::pos2(0.0, 0.0),
            scale: 1.0,
            rotation: 0.0,
            z_index: 0,
            archived: false,
        },
        asset,
    );
    PreparedPlacementImage {
        target_board_id,
        queue_id: Some(placement_id as u64),
        placement,
        preview_rgba: vec![255, 255, 255, 255],
        preview_width: 1,
        preview_height: 1,
        animated_preview: None,
        preview_texture: crate::jobs::ImportedPreviewTexture::Cpu,
        loaded_lod: crate::data::CanvasImageLod::Working,
        existing_load_diagnostics: None,
        origin: crate::jobs::ImportOrigin::Clipboard,
        trace: None,
    }
}

fn add_loaded_record(app: &mut SnappyRefApp, ctx: &egui::Context, placement: Placement) {
    app.canvas.add_prepared_placement_image(
        ctx,
        PreparedPlacementImage {
            target_board_id: 1,
            queue_id: None,
            placement,
            preview_rgba: vec![255, 0, 0, 255],
            preview_width: 1,
            preview_height: 1,
            animated_preview: None,
            preview_texture: crate::jobs::ImportedPreviewTexture::Cpu,
            loaded_lod: crate::data::CanvasImageLod::Working,
            existing_load_diagnostics: None,
            origin: crate::jobs::ImportOrigin::Existing,
            trace: None,
        },
    );
}

fn test_canvas_annotation(id: i64, start: egui::Pos2) -> crate::data::CanvasAnnotation {
    crate::data::CanvasAnnotation {
        id,
        kind: CanvasAnnotationKind::Line,
        start_x: start.x,
        start_y: start.y,
        end_x: start.x + 40.0,
        end_y: start.y + 30.0,
        text: String::new(),
        text_size: CanvasTextSize::M,
        stroke_width: CanvasStrokeWidth::M,
        color: crate::data::CanvasAnnotationColor::Gold,
        background_enabled: false,
        archived: false,
    }
}
