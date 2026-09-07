use super::perf_scenario::{EmbeddingPhaseMarkers, ModelLoadTimingSummary};
use super::{
    AppConfig, ArrangementAction, SUPPORTED_SHORTCUTS, SnappyRefApp, apply_arrangement_action,
};
use crate::data::{
    Asset, CanvasAnnotationKind, CanvasBackground, CanvasStackItem, CanvasStrokeWidth,
    CanvasTextSize, Placement, PlacementState,
};
use crate::jobs::PreparedPlacementImage;
use crate::library::storage::Library;
use crate::placement::content_resolution::{
    ContentResolutionTicket, FailedContentResolutionOutcome,
};
use crate::search::palette::{
    PALETTE_WIDTH, SEARCH_OVERLAY_TOP_MARGIN, search_result_grid_layout, search_result_tile_height,
};
use crate::search::semantic::{LocalModelStoreStatus, SearchResultAssetEntry};
use crate::test_support::TempRoot;
use crate::ui::sidebar::{SidebarTab, block_sidebar_input, show_sidebar, sidebar_rect};
use eframe::egui;
use egui_kittest::{Harness, kittest::Queryable};
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::time::{Duration, Instant};

#[test]
fn perf_idle_hold_starts_when_idle_is_observed() {
    let root = TempRoot::new("snappyref-perf-delayed-idle-frame");
    let config = AppConfig {
        library_root: Ok(root.to_path_buf()),
        no_startup_focus: true,
        perf_log: false,
        perf_decode_path: None,
        perf_scenario: Some("startup".to_owned()),
        perf_corpus: Some(root.to_path_buf()),
        perf_run_dir: Some(root.to_path_buf()),
        update_smoke_install: false,
        update_smoke_status_path: None,
    };
    let mut scenario = super::PerfScenarioState::from_config(&config).unwrap();
    scenario.queued = true;
    // A background window can miss callbacks long after its work drains.
    scenario.idle_since = Some(Instant::now() - Duration::from_secs(60));
    let mut app = test_app(None);
    app.perf_scenario = Some(scenario);
    assert!(!app.finish_perf_scenario_if_idle(&egui::Context::default()));
    assert!(root.join("summary.json").exists());
}

#[test]
fn destructive_confirmation_shell_has_no_destructive_outline() {
    let ctx = egui::Context::default();
    crate::ui::theme::configure_theme(&ctx);

    let frame = super::dialogs::destructive_confirmation_frame(&ctx, 1.0);

    assert_eq!(frame.stroke, egui::Stroke::NONE);
    assert_eq!(frame.fill, crate::ui::theme::SURFACE);
}

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

mod groups;
mod inspect_operations;
mod session_settings;

#[test]
fn first_launch_semantic_search_prompt_waits_for_storage_and_missing_assets() {
    let root = TempRoot::new("snappyref-semantic-prompt");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let mut app = test_app(Some(library));

    assert!(!app.should_show_semantic_search_first_launch_prompt());

    app.search
        .set_local_model_store_status_for_test(LocalModelStoreStatus::Missing);
    assert!(app.should_show_semantic_search_first_launch_prompt());

    app.settings.semantic_search_first_launch_prompt_seen = true;
    assert!(!app.should_show_semantic_search_first_launch_prompt());
}

#[test]
fn first_launch_semantic_search_prompt_skips_installed_and_migrated_assets() {
    let root = TempRoot::new("snappyref-semantic-prompt-installed");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let mut app = test_app(Some(library));

    app.search
        .set_local_model_store_status_for_test(LocalModelStoreStatus::Installed);
    assert!(!app.should_show_semantic_search_first_launch_prompt());

    app.search
        .set_local_model_store_status_for_test(LocalModelStoreStatus::InstalledViaMigration);
    assert!(!app.should_show_semantic_search_first_launch_prompt());
}

#[test]
fn first_launch_semantic_search_prompt_skips_without_storage() {
    let mut app = test_app(None);
    app.search
        .set_local_model_store_status_for_test(LocalModelStoreStatus::Missing);

    assert!(!app.should_show_semantic_search_first_launch_prompt());
}

#[test]
fn first_launch_semantic_search_prompt_not_now_persists_suppression() {
    let root = TempRoot::new("snappyref-semantic-prompt-not-now");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let mut app = test_app(Some(library.clone()));
    app.search
        .set_local_model_store_status_for_test(LocalModelStoreStatus::Missing);

    app.decline_semantic_search_first_launch_prompt();

    assert!(!app.should_show_semantic_search_first_launch_prompt());
    app.flush_persistence();
    assert!(
        library
            .load_user_session()
            .unwrap()
            .settings
            .semantic_search_first_launch_prompt_seen
    );
}

#[test]
fn first_launch_semantic_search_prompt_enable_persists_seen_and_starts_install() {
    let ctx = egui::Context::default();
    let root = TempRoot::new("snappyref-semantic-prompt-enable");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let starts = std::rc::Rc::new(std::cell::Cell::new(0));
    let observed = starts.clone();
    let mut app = test_app(Some(library.clone()));
    app.search
        .set_local_model_store_status_for_test(LocalModelStoreStatus::Missing);
    app.search.set_semantic_install_for_test(move |_| {
        observed.set(observed.get() + 1);
    });

    app.enable_semantic_search_from_first_launch_prompt(&ctx);

    assert_eq!(starts.get(), 1);
    assert_eq!(
        app.search.local_model_store_status(),
        LocalModelStoreStatus::Downloading { percent: 0 }
    );
    app.flush_persistence();
    assert!(
        library
            .load_user_session()
            .unwrap()
            .settings
            .semantic_search_first_launch_prompt_seen
    );
}

#[test]
fn removing_semantic_search_keeps_first_launch_prompt_seen() {
    let mut app = test_app(None);
    app.settings.semantic_search_first_launch_prompt_seen = true;
    app.search
        .set_local_model_store_status_for_test(LocalModelStoreStatus::Installed);

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
    let root = TempRoot::new("snappyref-app-sidebar-session");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let mut app = test_app(Some(library.clone()));
    app.sidebar_open = true;
    app.sidebar_tab = SidebarTab::Boards;

    app.save_user_session();
    app.flush_persistence();

    let mut reopened = test_app(Some(library));
    reopened.load_user_session();
    assert!(reopened.sidebar_open);
    assert_eq!(reopened.sidebar_tab, SidebarTab::Boards);
}

#[test]
fn board_summaries_are_served_from_cache_until_refreshed() {
    let root = TempRoot::new("snappyref-board-summary-cache");
    let library = Library::initialize(root.to_path_buf()).unwrap();
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
}

#[test]
fn creating_board_refreshes_board_summaries_cache() {
    let root = TempRoot::new("snappyref-board-summary-create-refresh");
    let library = Library::initialize(root.to_path_buf()).unwrap();
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
}

#[test]
fn persist_canvas_changes_saves_viewport_changes() {
    let root = TempRoot::new("snappyref-app-viewport-session");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let mut app = test_app(Some(library.clone()));
    app.canvas
        .apply_viewport_state(&crate::data::ViewportState {
            camera_pan_x: 10.0,
            camera_pan_y: 20.0,
            camera_zoom: 1.75,
            canvas_background: crate::data::CanvasBackground::Dots,
        });

    app.persist_canvas_changes();

    app.flush_persistence();
    let session = library.load_user_session().unwrap();
    assert_eq!(session.viewport.camera_pan_x, 10.0);
    assert_eq!(session.viewport.camera_pan_y, 20.0);
    assert_eq!(session.viewport.camera_zoom, 1.75);
    assert_eq!(
        session.viewport.canvas_background,
        crate::data::CanvasBackground::Dots
    );
}

#[test]
fn debounced_canvas_persistence_defers_session_only_viewport_changes() {
    let ctx = egui::Context::default();
    let root = TempRoot::new("snappyref-app-viewport-session-debounce");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let mut app = test_app(Some(library.clone()));
    app.canvas
        .apply_viewport_state(&crate::data::ViewportState {
            camera_pan_x: 10.0,
            camera_pan_y: 20.0,
            camera_zoom: 1.75,
            canvas_background: crate::data::CanvasBackground::Dots,
        });

    app.persist_canvas_changes_debounced(&ctx);

    app.flush_persistence();
    let session = library.load_user_session().unwrap();
    assert_ne!(session.viewport.camera_pan_x, 10.0);
    assert!(app.pending_user_session.is_some());

    app.pending_user_session_save_at = Some(Instant::now() - Duration::from_millis(1));
    app.persist_canvas_changes_debounced(&ctx);

    app.flush_persistence();
    let session = library.load_user_session().unwrap();
    assert_eq!(session.viewport.camera_pan_x, 10.0);
    assert_eq!(session.viewport.camera_pan_y, 20.0);
    assert_eq!(session.viewport.camera_zoom, 1.75);
    assert!(app.pending_user_session.is_none());
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
fn cross_board_paste_creates_new_images_and_annotations_on_active_board() {
    let ctx = egui::Context::default();
    let root = TempRoot::new("snappyref-cross-board-paste");
    let library = Library::initialize(root.to_path_buf()).unwrap();
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
            ..loaded_record(source_item.clone())
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

    app.flush_persistence();
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
        pasted_annotations[0].start().x - pasted_items[0].state.geometry.center.x,
        pasted_annotations[0].start().y - pasted_items[0].state.geometry.center.y,
    );
    let source_delta = egui::vec2(
        source_annotation.start().x - source_item.state.geometry.center.x,
        source_annotation.start().y - source_item.state.geometry.center.y,
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
    app.flush_persistence();
    assert_eq!(
        library.load_canvas_stack_order().unwrap(),
        pasted_stack_order
    );
    library.set_active_board(first.id).unwrap();
    assert_eq!(
        library.load_canvas_stack_order().unwrap(),
        source_stack_order
    );
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
    app.document_persistence = super::DocumentPersistence::queued(library.clone());
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
fn undo_delete_annotation_patches_active_canvas_without_board_reload() {
    let ctx = egui::Context::default();
    let root = TempRoot::new("snappyref-undo-no-board-reload");
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
    app.document_persistence = super::DocumentPersistence::queued(library.clone());
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
    app.flush_persistence();
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
    app.document_persistence = super::DocumentPersistence::queued(library.clone());
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
        .content_resolution
        .reserve_history_placement(app.active_board_id, 7);
    let mut loaded = test_loaded_item(7, app.active_board_id);
    loaded.queue_id = Some(reservation.queue_id);

    assert!(app.discard_stale_content_result(&crate::jobs::ImagePrepResult::Loaded(loaded)));
    assert!(app.canvas.placement_for_id(7).is_none());
    assert_eq!(app.content_resolution.pending_len(), 0);
}

#[test]
fn late_undo_content_failure_is_ignored_after_newer_replay() {
    let ctx = egui::Context::default();
    let mut app = test_app(None);
    let placement_id = 7;
    let undo_placement = test_loaded_item(placement_id, app.active_board_id).placement;
    app.canvas.add_unresolved_placement(undo_placement.clone());
    let reservation = app
        .content_resolution
        .reserve_history_placement(app.active_board_id, placement_id);
    let mut redo_placement = undo_placement;
    redo_placement.require_loaded_asset_mut().id = 202;
    let action = super::HistoryAction::document(
        "Crop image".to_owned(),
        vec![super::HistoryChange {
            before: None,
            after: Some(redo_placement),
        }],
        Vec::new(),
        None,
    );
    let super::HistoryAction::Document(action) = action else {
        panic!("expected document history action");
    };
    let replay = app.apply_history_to_active_canvas(
        &action,
        crate::active_board::history::HistoryDirection::Forward,
    );
    assert_eq!(replay.placements_needing_content.len(), 1);
    assert_eq!(replay.placements_needing_content[0].asset_id(), Some(202));
    app.status = "Redid Crop image".to_owned();

    let result = crate::jobs::ImagePrepResult::Failed {
        target_board_id: Some(app.active_board_id),
        queue_id: Some(reservation.queue_id),
        placement_id: Some(placement_id),
        context: "fixture-7.png".to_owned(),
        error: "decode failed".to_owned(),
    };
    if !app.discard_stale_content_result(&result) {
        let outcome = app.residency.process_import_result(
            &ctx,
            &mut app.canvas,
            app.import_worker.as_ref(),
            app.active_board_id,
            &app.settings,
            result,
        );
        app.apply_import_result_residency_outcome(&ctx, outcome);
    }

    assert_eq!(app.status, "Redid Crop image");
    assert_eq!(
        app.canvas
            .placement_for_id(placement_id)
            .unwrap()
            .asset_id(),
        Some(202)
    );
    assert_eq!(app.content_resolution.pending_len(), 0);
}

#[test]
fn completed_history_content_replay_preserves_history_command_status() {
    let ctx = egui::Context::default();
    let mut app = test_app(None);
    let placement_id = 7;

    for (asset_id, command_status) in [(101, "Undid Crop image"), (202, "Redid Crop image")] {
        let mut loaded = test_loaded_item(placement_id, app.active_board_id);
        loaded.placement.require_loaded_asset_mut().id = asset_id;
        loaded.origin = crate::jobs::ImportOrigin::Existing;
        app.canvas
            .add_unresolved_placement(loaded.placement.clone());
        let reservation = app
            .content_resolution
            .reserve_history_placement(app.active_board_id, placement_id);
        loaded.queue_id = Some(reservation.queue_id);
        app.status = command_status.to_owned();

        let result = crate::jobs::ImagePrepResult::Loaded(loaded);
        assert!(!app.discard_stale_content_result(&result));
        let outcome = app.residency.process_import_result(
            &ctx,
            &mut app.canvas,
            app.import_worker.as_ref(),
            app.active_board_id,
            &app.settings,
            result,
        );
        app.apply_import_result_residency_outcome(&ctx, outcome);

        assert_eq!(app.status, command_status);
        assert_eq!(
            app.canvas
                .placement_for_id(placement_id)
                .unwrap()
                .require_loaded_asset()
                .id,
            asset_id
        );
        assert_eq!(app.content_resolution.pending_len(), 0);

        let mut detail = test_loaded_item(placement_id, app.active_board_id);
        detail.placement.require_loaded_asset_mut().id = asset_id;
        detail.origin = crate::jobs::ImportOrigin::Existing;
        detail.queue_id = None;
        let outcome = app.residency.process_import_result(
            &ctx,
            &mut app.canvas,
            app.import_worker.as_ref(),
            app.active_board_id,
            &app.settings,
            crate::jobs::ImagePrepResult::Loaded(detail),
        );
        app.apply_import_result_residency_outcome(&ctx, outcome);

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
    app.document_persistence = super::DocumentPersistence::queued(library.clone());
    app.canvas.add_annotation(annotation.clone());
    app.document_history.record_applied(
        app.active_board_id,
        super::HistoryAction::document(
            "Added textbox".to_owned(),
            Vec::new(),
            vec![super::HistoryChange {
                before: None,
                after: Some(annotation.clone()),
            }],
            Some(super::HistoryChange {
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
            Some(loading.state.id),
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
    let cropped_path = library
        .store_cropped_rgba_image("source-crop", &[0, 255, 0, 255], 1, 1)
        .unwrap();
    let cropped_file_size = fs::metadata(&cropped_path).unwrap().len();
    let mut app = test_app(Some(library.clone()));
    app.document_persistence = super::DocumentPersistence::queued(library.clone());
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
        crate::jobs::PreparedCropPreview {
            placement_id: source.state.id,
            stored_path: cropped_path.clone(),
            width: 1,
            height: 1,
            file_size: cropped_file_size,
            preview_rgba: vec![0, 255, 0, 255],
            preview_width: 1,
            preview_height: 1,
            preview_texture: crate::jobs::ImagePrepPreviewTexture::Cpu,
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

#[derive(Clone, Copy, PartialEq)]
enum StackContentKind {
    Placement,
    Annotation,
}

/// Seeds four items in stack order (back, upper, lower, front), selects the
/// two middle ones, runs `copy_selection`, and asserts the copies land at the
/// front of the stack in the selection's original relative order, both in
/// memory and in the library.
fn assert_copy_keeps_stack_order(
    kinds: [StackContentKind; 4],
    copy_selection: impl FnOnce(&mut SnappyRefApp),
) {
    let ctx = egui::Context::default();
    let root = TempRoot::new("snappyref-copy-stack-order");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let image_path = library
        .store_rgba_image("source", &[255, 0, 0, 255], 1, 1)
        .unwrap();
    let mut app = test_app(Some(library.clone()));
    app.settings.placement_gap = 24.0;
    let mut asset_id = None;
    let mut stack = Vec::new();
    for (index, kind) in kinds.into_iter().enumerate() {
        let position = egui::pos2(index as f32 * 30.0, index as f32 * 20.0);
        let item = match kind {
            StackContentKind::Placement => {
                let placement = match asset_id {
                    None => library
                        .persist_imported_image(&image_path, "image/png", 1, 1, 4, position, 1.0)
                        .unwrap(),
                    Some(asset_id) => library
                        .create_placement_from_asset(asset_id, position, 1.0, 0.0, 0)
                        .unwrap(),
                };
                asset_id = Some(placement.require_loaded_asset().id);
                add_loaded_record(&mut app, &ctx, placement.clone());
                CanvasStackItem::Placement(placement.state.id)
            }
            StackContentKind::Annotation => {
                let fixture = test_canvas_annotation(0, position);
                let annotation = library
                    .create_canvas_annotation(
                        fixture.kind,
                        fixture.start(),
                        fixture.end(),
                        &fixture.text,
                        fixture.text_size,
                        fixture.stroke_width,
                        fixture.color,
                        fixture.background_enabled,
                    )
                    .unwrap();
                app.canvas.add_annotation(annotation.clone());
                CanvasStackItem::Annotation(annotation.id)
            }
        };
        stack.push(item);
    }
    app.canvas.set_canvas_stack_order(stack.clone());
    let selected = [stack[2], stack[1]];
    let ids_of = |kind: StackContentKind| -> Vec<i64> {
        selected
            .iter()
            .filter_map(|item| match (kind, item) {
                (StackContentKind::Placement, CanvasStackItem::Placement(id))
                | (StackContentKind::Annotation, CanvasStackItem::Annotation(id)) => Some(*id),
                _ => None,
            })
            .collect()
    };
    app.canvas.select_content(
        &ids_of(StackContentKind::Placement),
        &ids_of(StackContentKind::Annotation),
    );

    copy_selection(&mut app);

    // Copies of one kind are selected in the same order as their originals.
    let mut copied_placements = app.canvas.selected_placement_ids().into_iter();
    let mut copied_annotations = app.canvas.selected_annotation_ids().into_iter();
    let mut copy_of = |item: &CanvasStackItem| match item {
        CanvasStackItem::Placement(_) => {
            CanvasStackItem::Placement(copied_placements.next().unwrap())
        }
        CanvasStackItem::Annotation(_) => {
            CanvasStackItem::Annotation(copied_annotations.next().unwrap())
        }
    };
    let lower_copy = copy_of(&stack[2]);
    let upper_copy = copy_of(&stack[1]);
    assert!(copied_placements.next().is_none() && copied_annotations.next().is_none());
    let mut expected = stack;
    expected.extend([upper_copy, lower_copy]);
    assert_eq!(app.canvas.canvas_stack_order(), expected);
    app.flush_persistence();
    assert_eq!(library.load_canvas_stack_order().unwrap(), expected);
}

fn paste_selection(app: &mut SnappyRefApp) {
    let payload = app.copy_selected_board_payload().unwrap();
    app.paste_board_payload_at(&payload, egui::pos2(300.0, 300.0))
        .unwrap();
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
fn duplicate_offsets_mixed_selection_on_same_board_and_selects_new_content() {
    let ctx = egui::Context::default();
    let root = TempRoot::new("snappyref-mixed-duplicate");
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
    app.settings.placement_gap = 24.0;
    app.canvas
        .add_prepared_placement_image(&ctx, loaded_record(source_item.clone()));
    app.canvas.add_annotation(source_annotation.clone());
    app.canvas.set_canvas_stack_order(vec![
        CanvasStackItem::Annotation(source_annotation.id),
        CanvasStackItem::Placement(source_item.state.id),
    ]);
    app.canvas
        .select_content(&[source_item.state.id], &[source_annotation.id]);

    app.duplicate_selection_with_gap();

    app.flush_persistence();
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
        duplicated_item.state.geometry.center.x - source_item.state.geometry.center.x,
        duplicated_item.state.geometry.center.y - source_item.state.geometry.center.y,
    );
    let annotation_offset = egui::vec2(
        duplicated_annotation.start().x - source_annotation.start().x,
        duplicated_annotation.start().y - source_annotation.start().y,
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
    app.flush_persistence();
    assert_eq!(library.load_canvas_stack_order().unwrap(), expected);
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
    app.document_persistence = super::DocumentPersistence::queued(library.clone());
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
fn command_a_selects_all_active_board_images_and_annotations() {
    let ctx = egui::Context::default();
    let mut app = test_app(None);
    for id in [1_i64, 2] {
        app.canvas
            .add_prepared_placement_image(&ctx, test_loaded_item(id, 1));
    }
    app.canvas
        .add_annotation(crate::data::CanvasAnnotation::new(
            10,
            CanvasAnnotationKind::Textbox,
            eframe::egui::pos2(0.0, 0.0),
            eframe::egui::pos2(100.0, 40.0),
            "one".to_owned(),
            CanvasTextSize::M,
            CanvasStrokeWidth::M,
            crate::data::CanvasAnnotationColor::Default,
            false,
            false,
        ));
    app.canvas
        .add_annotation(crate::data::CanvasAnnotation::new(
            11,
            CanvasAnnotationKind::Line,
            eframe::egui::pos2(20.0, 20.0),
            eframe::egui::pos2(80.0, 80.0),
            String::new(),
            CanvasTextSize::M,
            CanvasStrokeWidth::M,
            crate::data::CanvasAnnotationColor::Default,
            false,
            false,
        ));
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
    assert!(SUPPORTED_SHORTCUTS.iter().any(|shortcut| {
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
    let payload = serde_json::from_str::<crate::active_board::commands::ClipboardBoardSelection>(
        &copy_payload.internal_json,
    )
    .unwrap();
    assert_eq!(
        payload.kind,
        crate::active_board::commands::COPY_PAYLOAD_KIND
    );
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
                super::shortcuts::ContextMenuAction::Group,
                Some(super::platform_shortcut_label("⌘G", "Ctrl+G"))
            ),
            (
                super::shortcuts::ContextMenuAction::Ungroup,
                Some(super::platform_shortcut_label("⌘⇧G", "Ctrl+Shift+G"))
            ),
            (super::shortcuts::ContextMenuAction::AddToGroup, Some("G")),
            (
                super::shortcuts::ContextMenuAction::Copy,
                Some(super::platform_shortcut_label("⌘ C", "Ctrl C")),
            ),
            (
                super::shortcuts::ContextMenuAction::Paste,
                Some(super::platform_shortcut_label("⌘ V", "Ctrl V")),
            ),
            (
                super::shortcuts::ContextMenuAction::Duplicate,
                Some(super::platform_shortcut_label("⌘ D", "Ctrl D")),
            ),
            (
                super::shortcuts::ContextMenuAction::DeleteFromBoard,
                Some("del")
            ),
            (
                super::shortcuts::ContextMenuAction::MoveToFront,
                Some(super::platform_shortcut_label("⌘ .", "Ctrl .")),
            ),
            (
                super::shortcuts::ContextMenuAction::MoveToBack,
                Some(super::platform_shortcut_label("⌘ ,", "Ctrl ,")),
            ),
            (
                super::shortcuts::ContextMenuAction::SearchUsingImage,
                Some(super::platform_shortcut_label("⌘ F", "Ctrl F")),
            ),
            (super::shortcuts::ContextMenuAction::RemoveFromLibrary, None),
        ]
    );
    assert!(item_enabled(
        &items,
        super::shortcuts::ContextMenuAction::Copy
    ));
    assert!(item_enabled(
        &items,
        super::shortcuts::ContextMenuAction::Paste
    ));
    assert!(item_enabled(
        &items,
        super::shortcuts::ContextMenuAction::Duplicate
    ));
    assert!(item_enabled(
        &items,
        super::shortcuts::ContextMenuAction::DeleteFromBoard
    ));
    assert!(item_enabled(
        &items,
        super::shortcuts::ContextMenuAction::MoveToFront
    ));
    assert!(item_enabled(
        &items,
        super::shortcuts::ContextMenuAction::MoveToBack
    ));
    assert!(!item_enabled(
        &items,
        super::shortcuts::ContextMenuAction::SearchUsingImage
    ));
    assert!(item_enabled(
        &items,
        super::shortcuts::ContextMenuAction::RemoveFromLibrary
    ));
}

#[test]
fn context_menu_primary_press_closes_menu_only_outside_it() {
    let menu_rect = egui::Rect::from_min_size(egui::pos2(100.0, 120.0), egui::vec2(190.0, 240.0));
    let should_close = |pointer_pos: egui::Pos2| {
        super::context_menu::context_menu_should_close(
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
        super::shortcuts::ContextMenuAction::MoveToFront
    ));
    assert!(!item_enabled(
        &items,
        super::shortcuts::ContextMenuAction::MoveToBack
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
        super::shortcuts::ContextMenuAction::MoveToFront
    ));
    assert!(item_enabled(
        &image_only,
        super::shortcuts::ContextMenuAction::MoveToBack
    ));

    app.canvas.select_content(&[], &[20]);
    let annotation_only = app.context_menu_items();
    assert!(item_enabled(
        &annotation_only,
        super::shortcuts::ContextMenuAction::MoveToFront
    ));
    assert!(item_enabled(
        &annotation_only,
        super::shortcuts::ContextMenuAction::MoveToBack
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
        super::shortcuts::ContextMenuAction::SearchUsingImage
    ));

    app.canvas.select_placements(&[1, 2]);
    let multiple = app.context_menu_items();
    assert!(!item_enabled(
        &multiple,
        super::shortcuts::ContextMenuAction::SearchUsingImage
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
        super::shortcuts::ContextMenuAction::RemoveFromLibrary,
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
        super::shortcuts::ContextMenuAction::MoveToFront,
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
        super::shortcuts::ContextMenuAction::MoveToBack,
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
fn delete_from_board_archives_image_placements_and_annotations() {
    let ctx = egui::Context::default();
    let root = TempRoot::new("snappyref-delete-from-board");
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
    app.canvas
        .add_prepared_placement_image(&ctx, loaded_record(source_item.clone()));
    app.canvas.add_annotation(annotation.clone());
    app.canvas
        .select_content(&[source_item.state.id], &[annotation.id]);

    app.handle_context_menu_action(
        super::shortcuts::ContextMenuAction::DeleteFromBoard,
        &ctx,
        egui::Rect::EVERYTHING,
    );

    app.flush_persistence();
    assert!(library.load_placements().unwrap().is_empty());
    assert!(
        library
            .asset_original_paths_for_assets(&[source_item.require_loaded_asset().id])
            .unwrap()
            .contains_key(&source_item.require_loaded_asset().id)
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
}

#[test]
fn search_using_image_context_action_opens_image_pivot_search() {
    let ctx = egui::Context::default();
    let mut app = test_app(None);
    app.settings.semantic_search_enabled = true;
    app.canvas
        .add_prepared_placement_image(&ctx, test_loaded_item(42, 1));
    app.canvas.select_placements(&[42]);

    app.handle_context_menu_action(
        super::shortcuts::ContextMenuAction::SearchUsingImage,
        &ctx,
        egui::Rect::EVERYTHING,
    );

    assert!(app.search.overlay.open);
    assert!(app.search.overlay.suppress_next_outside_click);
    let pivot = app.search.overlay.image_pivot.unwrap();
    assert_eq!(pivot.asset_id, 42);
    assert!(pivot.asset_preview_path.is_none());
}

#[test]
fn search_using_image_context_action_survives_activation_click_outside_overlay() {
    let ctx = egui::Context::default();
    let mut app = test_app(None);
    app.settings.semantic_search_enabled = true;
    app.canvas
        .add_prepared_placement_image(&ctx, test_loaded_item(42, 1));
    app.canvas.select_placements(&[42]);

    app.handle_context_menu_action(
        super::shortcuts::ContextMenuAction::SearchUsingImage,
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
    let root = TempRoot::new("snappyref-create-board");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let mut app = test_app(Some(library.clone()));
    app.sidebar_open = true;

    app.create_and_switch_board(true);

    app.flush_persistence();
    let boards = library.list_boards().unwrap();
    assert_eq!(boards.len(), 2);
    assert_eq!(boards[0].name, "My Board");
    assert_eq!(boards[1].name, "Board 2");
    assert_eq!(app.active_board_id, boards[1].id);
    app.flush_persistence();
    assert_eq!(
        library.load_user_session().unwrap().active_board_id,
        boards[1].id
    );
    assert!(app.sidebar_open);
}

#[test]
fn board_create_undo_and_redo_restore_the_same_identity_once() {
    let ctx = egui::Context::default();
    let root = TempRoot::new("snappyref-create-board-history");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let original_id = library.list_boards().unwrap()[0].id;
    let other = library.create_board("Other").unwrap();
    let mut app = test_app(Some(library.clone()));

    app.create_and_switch_board(false);
    app.flush_persistence();
    let created = library
        .list_boards()
        .unwrap()
        .into_iter()
        .find(|board| board.id != original_id && board.id != other.id)
        .unwrap();

    app.document_persistence = super::DocumentPersistence::queued(library.clone());
    let storage_lock = rusqlite::Connection::open(root.join("library.db")).unwrap();
    storage_lock.execute_batch("BEGIN IMMEDIATE").unwrap();

    app.undo_document_history(&ctx);
    assert_eq!(app.active_board_id, original_id);
    assert_eq!(
        app.board_summaries()
            .iter()
            .map(|summary| summary.board.id)
            .collect::<Vec<_>>(),
        vec![original_id, other.id]
    );
    let archived_before_flush: i64 = storage_lock
        .query_row(
            "SELECT archived FROM boards WHERE id = ?1",
            [created.id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(archived_before_flush, 0);

    storage_lock.execute_batch("COMMIT").unwrap();
    app.flush_persistence();
    assert!(!app.status.contains("failed"), "{}", app.status);
    assert_eq!(
        storage_lock
            .query_row(
                "SELECT archived FROM boards WHERE id = ?1",
                [created.id],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        1,
        "queued create Undo must archive the created Board"
    );
    assert_eq!(
        library
            .list_boards()
            .unwrap()
            .iter()
            .map(|board| board.id)
            .collect::<Vec<_>>(),
        vec![original_id, other.id]
    );
    assert_eq!(app.active_board_id, original_id);

    let status_after_undo = app.status.clone();
    app.undo_document_history(&ctx);
    assert_eq!(app.status, status_after_undo, "create records one entry");

    app.redo_document_history(&ctx);
    app.flush_persistence();
    assert_eq!(
        library.list_boards().unwrap(),
        vec![
            crate::data::Board {
                id: original_id,
                name: "My Board".to_owned(),
            },
            other,
            created.clone(),
        ]
    );
    assert_eq!(app.active_board_id, created.id);
}

#[test]
fn board_rename_undo_and_redo_restore_exact_names_once() {
    let ctx = egui::Context::default();
    let root = TempRoot::new("snappyref-rename-board-history");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let board = library.list_boards().unwrap()[0].clone();
    let mut app = test_app(Some(library.clone()));
    app.document_persistence = super::DocumentPersistence::queued(library.clone());
    let storage_lock = rusqlite::Connection::open(root.join("library.db")).unwrap();
    storage_lock.execute_batch("BEGIN IMMEDIATE").unwrap();

    app.handle_board_action(Some(crate::ui::sidebar::SidebarBoardAction::RenameBoard(
        board.id,
        "  Reference Board  ".to_owned(),
    )));
    assert_eq!(app.board_summaries()[0].board.name, "Reference Board");
    app.handle_board_action(Some(crate::ui::sidebar::SidebarBoardAction::RenameBoard(
        board.id,
        "Final Board".to_owned(),
    )));
    assert_eq!(app.board_summaries()[0].board.name, "Final Board");
    assert_eq!(
        storage_lock
            .query_row("SELECT name FROM boards WHERE id = ?1", [board.id], |row| {
                row.get::<_, String>(0)
            })
            .unwrap(),
        "My Board"
    );
    storage_lock.execute_batch("COMMIT").unwrap();
    app.flush_persistence();
    assert_eq!(library.list_boards().unwrap()[0].name, "Final Board");

    storage_lock.execute_batch("BEGIN IMMEDIATE").unwrap();
    app.undo_document_history(&ctx);
    assert_eq!(app.board_summaries()[0].board.name, "Reference Board");
    assert_eq!(
        storage_lock
            .query_row("SELECT name FROM boards WHERE id = ?1", [board.id], |row| {
                row.get::<_, String>(0)
            })
            .unwrap(),
        "Final Board"
    );
    storage_lock.execute_batch("COMMIT").unwrap();
    app.flush_persistence();
    assert_eq!(library.list_boards().unwrap()[0].name, "Reference Board");

    app.undo_document_history(&ctx);
    app.flush_persistence();
    assert_eq!(library.list_boards().unwrap()[0].name, "My Board");

    let status_after_undo = app.status.clone();
    app.undo_document_history(&ctx);
    assert_eq!(app.status, status_after_undo, "rename records one entry");

    app.redo_document_history(&ctx);
    app.flush_persistence();
    assert_eq!(library.list_boards().unwrap()[0].name, "Reference Board");
    app.redo_document_history(&ctx);
    app.flush_persistence();
    assert_eq!(library.list_boards().unwrap()[0].name, "Final Board");
}

#[test]
fn create_undo_persists_reassigned_redo_history_with_the_archive() {
    let ctx = egui::Context::default();
    let root = TempRoot::new("snappyref-create-undo-atomic-history");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let original = library.list_boards().unwrap()[0].clone();

    let mut app = test_app(Some(library.clone()));
    app.active_board_id = original.id;
    app.document_persistence = super::DocumentPersistence::queued(library.clone());
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
fn deleting_inactive_board_after_queued_resolution_restores_resolved_placement() {
    let ctx = egui::Context::default();
    let root = TempRoot::new("snappyref-delete-after-queued-resolution");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let first = library.list_boards().unwrap()[0].clone();
    let second = library.create_board("Second").unwrap();
    library.set_active_board(second.id).unwrap();
    let source = library
        .store_rgba_image("resolved", &[255, 0, 0, 255], 1, 1)
        .unwrap();
    let asset = library.create_asset(&source, "image/png", 1, 1, 4).unwrap();
    let loading = library
        .persist_committed_loading_placement(
            egui::Pos2::ZERO,
            1.0,
            1.0,
            1.0,
            crate::data::ContentResolutionSource::ExternalPath {
                path: source.to_string_lossy().into_owned(),
            },
            Some("Loading".to_owned()),
        )
        .unwrap();
    library.set_active_board(first.id).unwrap();

    let mut app = test_app(Some(library.clone()));
    app.active_board_id = first.id;
    app.refresh_board_summaries();
    app.document_history.record_applied(
        second.id,
        super::HistoryAction::document(
            "Added image".to_owned(),
            vec![super::HistoryChange {
                before: None,
                after: Some(loading.clone()),
            }],
            Vec::new(),
            None,
        ),
    );
    app.document_persistence = super::DocumentPersistence::queued(library.clone());
    app.document_persistence
        .enqueue_document_unit(
            second.id,
            crate::canvas::CanvasDocumentChanges {
                updated_placements: vec![loading.state.clone()],
                ..Default::default()
            },
            &app.document_history,
        )
        .unwrap();
    app.flush_persistence();

    let trigger_connection = rusqlite::Connection::open(root.join("library.db")).unwrap();
    trigger_connection
        .execute_batch(
            "CREATE TRIGGER fail_resolution_history
             BEFORE UPDATE ON board_document_history
             BEGIN
                 SELECT RAISE(ABORT, 'injected resolution-history failure');
             END;",
        )
        .unwrap();

    let mut resolved = loading.clone();
    resolved.content = crate::data::PlacementContent::Loaded {
        asset: asset.clone(),
    };
    assert!(
        app.document_history
            .update_accepted_placement(second.id, resolved.clone())
    );
    app.document_persistence
        .enqueue_document_unit(
            second.id,
            crate::canvas::CanvasDocumentChanges {
                updated_placements: vec![resolved.state.clone()],
                ..Default::default()
            },
            &app.document_history,
        )
        .unwrap();
    std::thread::sleep(Duration::from_millis(100));

    app.request_board_delete(second.id, second.name.clone());
    app.confirm_pending_board_delete(&ctx);
    trigger_connection
        .execute_batch("DROP TRIGGER fail_resolution_history")
        .unwrap();
    app.flush_persistence();

    app.undo_document_history(&ctx);
    app.flush_persistence();
    let reopened = Library::initialize(root.to_path_buf()).unwrap();
    let restored = reopened.load_board_document(second.id).unwrap();
    assert_eq!(restored.placements.len(), 1);
    assert_eq!(restored.placements[0].state.id, resolved.state.id);
    assert_eq!(restored.placements[0].content, resolved.content);
}

#[test]
fn queued_board_identities_remain_visible_while_persistence_retries() {
    let root = TempRoot::new("snappyref-queued-board-identity");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let trigger_connection = rusqlite::Connection::open(root.join("library.db")).unwrap();
    trigger_connection
        .execute_batch(
            "CREATE TRIGGER fail_board_history
             BEFORE INSERT ON board_document_history
             BEGIN
                 SELECT RAISE(ABORT, 'injected Board-history failure');
             END;",
        )
        .unwrap();
    let mut app = test_app(Some(library.clone()));
    app.document_persistence = super::DocumentPersistence::queued(library);

    app.create_and_switch_board(false);
    app.create_and_switch_board(false);
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(app.board_summaries().len(), 3);

    app.refresh_board_summaries();
    let accepted_board_count = app.board_summaries().len();

    trigger_connection
        .execute_batch("DROP TRIGGER fail_board_history")
        .unwrap();
    app.flush_persistence();
    drop(app);

    assert_eq!(accepted_board_count, 3);
}

#[test]
fn deleting_active_queued_board_uses_accepted_queued_fallback() {
    let ctx = egui::Context::default();
    let root = TempRoot::new("snappyref-delete-queued-active-board");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let trigger_connection = rusqlite::Connection::open(root.join("library.db")).unwrap();
    trigger_connection
        .execute_batch(
            "CREATE TRIGGER fail_board_history
             BEFORE INSERT ON board_document_history
             BEGIN
                 SELECT RAISE(ABORT, 'injected Board-history failure');
             END;",
        )
        .unwrap();
    let mut app = test_app(Some(library.clone()));
    app.document_persistence = super::DocumentPersistence::queued(library);

    app.create_and_switch_board(false);
    let fallback_id = app.active_board_id;
    app.create_and_switch_board(false);
    let deleted_id = app.active_board_id;
    std::thread::sleep(Duration::from_millis(100));

    app.request_board_delete(deleted_id, "Board 3".to_owned());
    app.confirm_pending_board_delete(&ctx);

    let active_after_delete = app.active_board_id;
    let deleted_identity_visible = app
        .board_summaries()
        .iter()
        .any(|summary| summary.board.id == deleted_id);

    trigger_connection
        .execute_batch("DROP TRIGGER fail_board_history")
        .unwrap();
    app.flush_persistence();
    drop(app);

    assert_eq!(active_after_delete, fallback_id);
    assert!(
        !deleted_identity_visible,
        "the accepted delete should update memory even while both Board creations await retry"
    );
}

#[test]
fn deleting_active_board_restores_accepted_fallback_content_while_persistence_retries() {
    let ctx = egui::Context::default();
    let root = TempRoot::new("snappyref-delete-accepted-fallback-content");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let fallback = library.list_boards().unwrap()[0].clone();
    let trigger_connection = rusqlite::Connection::open(root.join("library.db")).unwrap();
    trigger_connection
        .execute_batch(
            "CREATE TRIGGER fail_board_history
             BEFORE INSERT ON board_document_history
             BEGIN
                 SELECT RAISE(ABORT, 'injected Board-history failure');
             END;",
        )
        .unwrap();
    let mut app = test_app(Some(library.clone()));
    app.document_persistence = super::DocumentPersistence::queued(library.clone());

    app.handle_board_action(Some(crate::ui::sidebar::SidebarBoardAction::RenameBoard(
        fallback.id,
        "Accepted fallback".to_owned(),
    )));
    let stack_before = app.canvas.canvas_stack_order();
    let annotation = library.new_canvas_annotation(
        CanvasAnnotationKind::Textbox,
        egui::pos2(40.0, 60.0),
        egui::pos2(140.0, 100.0),
        "accepted note".to_owned(),
        CanvasTextSize::M,
        CanvasStrokeWidth::M,
        crate::data::CanvasAnnotationColor::Sky,
        true,
    );
    app.canvas.add_annotation(annotation.clone());
    app.canvas.mark_annotation_for_persistence(annotation.id);
    app.document_history.record_applied(
        fallback.id,
        super::HistoryAction::document(
            "Added textbox".to_owned(),
            Vec::new(),
            vec![super::HistoryChange {
                before: None,
                after: Some(annotation.clone()),
            }],
            Some(super::HistoryChange {
                before: Some(stack_before),
                after: Some(app.canvas.canvas_stack_order()),
            }),
        ),
    );
    app.persist_recorded_canvas_changes();
    app.create_and_switch_board(false);
    let deleted_id = app.active_board_id;
    std::thread::sleep(Duration::from_millis(100));

    app.request_board_delete(deleted_id, "Board 2".to_owned());
    app.confirm_pending_board_delete(&ctx);

    let active_after_delete = app.active_board_id;
    let fallback_name_after_delete = app.board_summaries()[0].board.name.clone();
    let fallback_annotations_after_delete = app.canvas.annotations();

    trigger_connection
        .execute_batch("DROP TRIGGER fail_board_history")
        .unwrap();
    app.flush_persistence();
    drop(app);

    assert_eq!(active_after_delete, fallback.id);
    assert_eq!(fallback_name_after_delete, "Accepted fallback");
    assert_eq!(fallback_annotations_after_delete, vec![annotation]);
}

#[test]
fn undoing_board_create_reopens_accepted_previous_board_while_persistence_retries() {
    let ctx = egui::Context::default();
    let root = TempRoot::new("snappyref-undo-create-accepted-content");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let previous = library.list_boards().unwrap()[0].clone();
    let trigger_connection = rusqlite::Connection::open(root.join("library.db")).unwrap();
    trigger_connection
        .execute_batch(
            "CREATE TRIGGER fail_board_history
             BEFORE INSERT ON board_document_history
             BEGIN
                 SELECT RAISE(ABORT, 'injected Board-history failure');
             END;",
        )
        .unwrap();
    let mut app = test_app(Some(library.clone()));
    app.document_persistence = super::DocumentPersistence::queued(library.clone());

    app.handle_board_action(Some(crate::ui::sidebar::SidebarBoardAction::RenameBoard(
        previous.id,
        "Accepted previous".to_owned(),
    )));
    let stack_before = app.canvas.canvas_stack_order();
    let annotation = library.new_canvas_annotation(
        CanvasAnnotationKind::Textbox,
        egui::pos2(40.0, 60.0),
        egui::pos2(140.0, 100.0),
        "accepted note".to_owned(),
        CanvasTextSize::M,
        CanvasStrokeWidth::M,
        crate::data::CanvasAnnotationColor::Sky,
        true,
    );
    app.canvas.add_annotation(annotation.clone());
    app.canvas.mark_annotation_for_persistence(annotation.id);
    app.document_history.record_applied(
        previous.id,
        super::HistoryAction::document(
            "Added textbox".to_owned(),
            Vec::new(),
            vec![super::HistoryChange {
                before: None,
                after: Some(annotation.clone()),
            }],
            Some(super::HistoryChange {
                before: Some(stack_before),
                after: Some(app.canvas.canvas_stack_order()),
            }),
        ),
    );
    app.persist_recorded_canvas_changes();
    app.create_and_switch_board(false);
    assert_ne!(app.active_board_id, previous.id);
    std::thread::sleep(Duration::from_millis(100));

    app.undo_document_history(&ctx);

    let active_after_undo = app.active_board_id;
    let previous_name_after_undo = app.board_summaries()[0].board.name.clone();
    let previous_annotations_after_undo = app.canvas.annotations();

    trigger_connection
        .execute_batch("DROP TRIGGER fail_board_history")
        .unwrap();
    app.flush_persistence();
    drop(app);

    assert_eq!(active_after_undo, previous.id);
    assert_eq!(previous_name_after_undo, "Accepted previous");
    assert_eq!(previous_annotations_after_undo, vec![annotation]);
}

#[test]
fn generated_board_names_fill_smallest_visible_gap() {
    let root = TempRoot::new("snappyref-board-name-gap");
    let library = Library::initialize(root.to_path_buf()).unwrap();
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

    app.flush_persistence();
    let boards = library.list_boards().unwrap();
    assert!(boards.iter().any(|board| board.name == "Board 2"));
    assert_eq!(app.active_board_id, boards.last().unwrap().id);
    assert!(!app.sidebar_open);
}

#[test]
fn command_n_creates_board_without_opening_sidebar() {
    let ctx = egui::Context::default();
    let root = TempRoot::new("snappyref-cmd-n-board");
    let library = Library::initialize(root.to_path_buf()).unwrap();
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

    app.flush_persistence();
    let boards = library.list_boards().unwrap();
    assert_eq!(boards.len(), 2);
    assert_eq!(boards[1].name, "Board 2");
    assert_eq!(app.active_board_id, boards[1].id);
    assert!(!app.sidebar_open);
}

#[test]
fn command_number_switches_visible_board() {
    let ctx = egui::Context::default();
    let root = TempRoot::new("snappyref-cmd-number-board");
    let library = Library::initialize(root.to_path_buf()).unwrap();
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
    app.flush_persistence();
    assert_eq!(
        library.load_user_session().unwrap().active_board_id,
        third.id
    );
}

#[test]
fn command_zero_switches_tenth_visible_board() {
    let ctx = egui::Context::default();
    let root = TempRoot::new("snappyref-cmd-zero-board");
    let library = Library::initialize(root.to_path_buf()).unwrap();
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
}

#[test]
fn board_shortcuts_are_suppressed_while_search_is_open() {
    let ctx = egui::Context::default();
    let root = TempRoot::new("snappyref-board-shortcut-search");
    let library = Library::initialize(root.to_path_buf()).unwrap();
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
}

#[test]
fn switching_boards_saves_previous_viewport_and_loads_target_viewport() {
    let root = TempRoot::new("snappyref-board-switch-viewport");
    let library = Library::initialize(root.to_path_buf()).unwrap();
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
        app.flush_persistence();
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
}

#[test]
fn active_board_load_applies_viewport_without_reloading_global_settings() {
    let root = TempRoot::new("snappyref-active-load-settings-scope");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let second = library.create_board("Second").unwrap();
    library.set_active_board(second.id).unwrap();
    let mut stored_session = library.load_user_session().unwrap();
    stored_session.settings.placement_gap = 99.0;
    stored_session.viewport.camera_pan_x = 75.0;
    stored_session.viewport.canvas_background = crate::data::CanvasBackground::Dots;
    library.save_user_session(&stored_session).unwrap();

    let mut app = test_app(Some(library.clone()));
    app.settings.placement_gap = 24.0;

    app.load_active_board();

    assert_eq!(app.active_board_id, second.id);
    assert_eq!(app.canvas.background(), crate::data::CanvasBackground::Dots);
    assert_eq!(app.canvas.viewport_state().camera_pan_x, 75.0);
    assert_eq!(app.settings.placement_gap, 24.0);
}

#[test]
fn board_delete_requires_confirmation_before_archiving_active_board() {
    let ctx = egui::Context::default();
    let root = TempRoot::new("snappyref-confirm-delete-board");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let first = library.list_boards().unwrap()[0].clone();
    let second = library.create_board("Second").unwrap();
    library.set_active_board(second.id).unwrap();
    let mut app = test_app(Some(library.clone()));
    app.active_board_id = second.id;
    app.sidebar_open = true;

    app.request_board_delete(second.id, second.name.clone());

    assert!(app.pending_board_delete.is_some());
    app.flush_persistence();
    assert_eq!(library.list_boards().unwrap().len(), 2);

    app.confirm_pending_board_delete(&ctx);

    assert!(app.pending_board_delete.is_none());
    assert!(app.sidebar_open);
    assert_eq!(app.active_board_id, first.id);
    app.flush_persistence();
    assert_eq!(
        library
            .list_boards()
            .unwrap()
            .iter()
            .map(|board| board.id)
            .collect::<Vec<_>>(),
        vec![first.id]
    );
}

#[test]
fn board_delete_confirmation_keeps_sidebar_open_and_removes_inactive_board() {
    let ctx = egui::Context::default();
    let root = TempRoot::new("snappyref-delete-inactive-board");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let first = library.list_boards().unwrap()[0].clone();
    let second = library.create_board("Second").unwrap();
    let mut app = test_app(Some(library.clone()));
    app.active_board_id = first.id;
    app.sidebar_open = true;

    app.request_board_delete(second.id, second.name.clone());
    app.confirm_pending_board_delete(&ctx);

    assert!(app.pending_board_delete.is_none());
    assert!(app.sidebar_open);
    assert_eq!(app.active_board_id, first.id);
    app.flush_persistence();
    assert_eq!(
        library
            .list_boards()
            .unwrap()
            .iter()
            .map(|board| board.id)
            .collect::<Vec<_>>(),
        vec![first.id]
    );
}

#[test]
fn board_delete_undo_restores_document_view_history_and_active_fallback() {
    let ctx = egui::Context::default();
    let root = TempRoot::new("snappyref-delete-board-history");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let first = library.list_boards().unwrap()[0].clone();
    let second = library.create_board("Reference").unwrap();
    library.set_active_board(second.id).unwrap();
    let image_path = library
        .store_rgba_image("board-history", &[255, 0, 0, 255], 1, 1)
        .unwrap();
    let placement = library
        .persist_imported_image(
            &image_path,
            "image/png",
            1,
            1,
            4,
            egui::pos2(40.0, 60.0),
            1.5,
        )
        .unwrap();
    let annotation = library
        .create_canvas_annotation(
            CanvasAnnotationKind::Textbox,
            egui::pos2(10.0, 20.0),
            egui::pos2(110.0, 80.0),
            "kept note",
            CanvasTextSize::L,
            CanvasStrokeWidth::M,
            crate::data::CanvasAnnotationColor::Sky,
            true,
        )
        .unwrap();
    let mixed_stack = vec![
        CanvasStackItem::Annotation(annotation.id),
        CanvasStackItem::Placement(placement.state.id),
    ];
    library.save_canvas_stack_order(&mixed_stack).unwrap();
    let mut session = library.load_user_session().unwrap();
    session.viewport = crate::data::ViewportState {
        camera_pan_x: 125.0,
        camera_pan_y: -48.0,
        camera_zoom: 1.75,
        canvas_background: CanvasBackground::Grid,
    };
    library.save_user_session(&session).unwrap();

    let mut app = test_app(Some(library.clone()));
    app.active_board_id = second.id;
    app.canvas.apply_viewport_state(&session.viewport);
    app.sidebar_open = true;
    app.context_menu_pos = Some(egui::pos2(12.0, 18.0));
    app.document_history.record_applied(
        second.id,
        crate::active_board::history::HistoryAction::document(
            "Earlier document change".to_owned(),
            Vec::new(),
            Vec::new(),
            None,
        ),
    );

    app.request_board_delete(second.id, second.name.clone());
    app.confirm_pending_board_delete(&ctx);

    assert_eq!(app.active_board_id, first.id);
    app.flush_persistence();
    assert_eq!(library.list_boards().unwrap(), vec![first.clone()]);
    app.sidebar_open = false;
    app.window_state.inner_width = Some(777.0);
    app.context_menu_pos = None;
    app.document_persistence = super::DocumentPersistence::queued(library.clone());
    let storage_lock = rusqlite::Connection::open(root.join("library.db")).unwrap();
    storage_lock.execute_batch("BEGIN IMMEDIATE").unwrap();

    app.undo_document_history(&ctx);

    assert_eq!(app.active_board_id, second.id);
    assert_eq!(app.board_summaries().len(), 2);
    let archived_before_flush: i64 = storage_lock
        .query_row(
            "SELECT archived FROM boards WHERE id = ?1",
            [second.id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(archived_before_flush, 1);
    storage_lock.execute_batch("COMMIT").unwrap();
    app.flush_persistence();
    assert_eq!(
        library.list_boards().unwrap(),
        vec![first.clone(), second.clone()]
    );
    let restored_placements = library.load_placements().unwrap();
    assert_eq!(restored_placements.len(), 1);
    assert_eq!(restored_placements[0].state.id, placement.state.id);
    assert_eq!(
        restored_placements[0].state.geometry,
        placement.state.geometry
    );
    assert_eq!(restored_placements[0].content, placement.content);
    assert_eq!(library.load_canvas_annotations().unwrap(), vec![annotation]);
    assert_eq!(library.load_canvas_stack_order().unwrap(), mixed_stack);
    assert_eq!(
        library.load_user_session().unwrap().viewport,
        session.viewport
    );
    assert!(!app.sidebar_open, "sidebar state is not Board history");
    assert_eq!(app.window_state.inner_width, Some(777.0));
    assert!(
        app.context_menu_pos.is_none(),
        "Canvas transients are cleared"
    );

    let active_assets: i64 = storage_lock
        .query_row(
            "SELECT COUNT(*) FROM assets WHERE archived = 0",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(active_assets, 1);

    app.undo_document_history(&ctx);
    assert_eq!(app.status, "Undid Earlier document change");
    app.redo_document_history(&ctx);
    assert_eq!(app.status, "Redid Earlier document change");
    app.redo_document_history(&ctx);
    app.flush_persistence();

    assert_eq!(app.active_board_id, first.id);
    app.flush_persistence();
    assert_eq!(library.list_boards().unwrap(), vec![first]);
    let active_assets: i64 = storage_lock
        .query_row(
            "SELECT COUNT(*) FROM assets WHERE archived = 0",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(active_assets, 1);
}

#[test]
fn board_delete_modal_click_routes_through_egui_layers() {
    let root = TempRoot::new("snappyref-delete-modal-click-routing");
    let library = Library::initialize(root.to_path_buf()).unwrap();
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
    harness.state_mut().flush_persistence();
    assert_eq!(
        library
            .list_boards()
            .unwrap()
            .iter()
            .map(|board| board.id)
            .collect::<Vec<_>>(),
        vec![first.id]
    );
}

#[test]
fn board_sidebar_stays_clickable_after_delete_modal_confirms() {
    let root = TempRoot::new("snappyref-sidebar-click-after-delete");
    let library = Library::initialize(root.to_path_buf()).unwrap();
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

    harness.state_mut().flush_persistence();
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
}

#[test]
fn board_sidebar_name_click_switches_board_immediately() {
    let root = TempRoot::new("snappyref-sidebar-name-click-switch");
    let library = Library::initialize(root.to_path_buf()).unwrap();
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
}

#[test]
fn settings_semantic_search_install_and_cancel_share_download_state() {
    let mut app = test_app(None);
    app.sidebar_open = true;
    app.sidebar_tab = SidebarTab::Options;
    app.search
        .set_local_model_store_status_for_test(LocalModelStoreStatus::Missing);
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
fn settings_semantic_search_failed_state_retries_shared_install() {
    let mut app = test_app(None);
    app.sidebar_open = true;
    app.sidebar_tab = SidebarTab::Options;
    app.search
        .set_local_model_store_status_for_test(LocalModelStoreStatus::Failed(
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
fn settings_semantic_search_remove_requires_confirmation() {
    let root = TempRoot::new("snappyref-semantic-search-remove-confirm");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let mut app = test_app(Some(library));
    app.sidebar_open = true;
    app.sidebar_tab = SidebarTab::Options;
    app.settings.semantic_search_first_launch_prompt_seen = true;
    app.search
        .set_local_model_store_status_for_test(LocalModelStoreStatus::Installed);
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
}

#[test]
fn rapid_sidebar_board_name_clicks_keep_last_requested_board_active() {
    let root = TempRoot::new("snappyref-rapid-sidebar-board-clicks");
    let library = Library::initialize(root.to_path_buf()).unwrap();
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
    harness.state_mut().flush_persistence();
    assert_eq!(
        library.load_user_session().unwrap().active_board_id,
        second.id
    );
}

#[test]
fn board_delete_modal_blocks_sidebar_board_switches_until_closed() {
    let root = TempRoot::new("snappyref-delete-modal-blocks-sidebar");
    let library = Library::initialize(root.to_path_buf()).unwrap();
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
    harness.state_mut().flush_persistence();
    assert_eq!(
        library.load_user_session().unwrap().active_board_id,
        second.id
    );
}

#[test]
fn board_delete_modal_blocks_sidebar_tabs_and_settings() {
    let root = TempRoot::new("snappyref-delete-modal-blocks-sidebar-state");
    let library = Library::initialize(root.to_path_buf()).unwrap();
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
}

#[test]
fn sidebar_board_lifecycle_clicks_create_switch_and_persist_active_board() {
    let root = TempRoot::new("snappyref-sidebar-board-lifecycle");
    let library = Library::initialize(root.to_path_buf()).unwrap();
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

    harness.state_mut().flush_persistence();
    let boards = library.list_boards().unwrap();
    assert_eq!(boards.len(), 2);
    assert_eq!(boards[1].name, "Board 2");
    assert_eq!(harness.state().active_board_id, boards[1].id);
    assert!(harness.state().sidebar_open);

    harness.get_by_label("Board My Board").click();
    harness.run();

    assert_eq!(harness.state().active_board_id, first.id);
    harness.state_mut().flush_persistence();
    assert_eq!(
        library.load_user_session().unwrap().active_board_id,
        first.id
    );
}

#[test]
fn rapid_board_switches_ignore_stale_import_completions_and_last_switch_wins() {
    let ctx = egui::Context::default();
    let root = TempRoot::new("snappyref-rapid-switch-stale-imports");
    let library = Library::initialize(root.to_path_buf()).unwrap();
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
}

#[test]
fn switching_boards_clears_selection_and_search() {
    let ctx = egui::Context::default();
    let root = TempRoot::new("snappyref-board-switch-cleanup");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let second = library.create_board("Second").unwrap();
    let mut app = test_app(Some(library.clone()));
    let asset = Asset {
        id: 1,
        original_path: PathBuf::from("fixture.png"),
        filename: "fixture.png".to_owned(),
        mime_type: "image/png".to_owned(),
        width: 1,
        height: 1,
    };
    let placement = Placement::loaded(
        PlacementState::new(1, eframe::egui::pos2(0.0, 0.0), 1.0, 0.0, 0, false),
        asset,
    );
    app.canvas.add_prepared_placement_image(
        &ctx,
        PreparedPlacementImage {
            preview_rgba: vec![255, 255, 255, 255],
            ..loaded_record(placement.clone())
        },
    );
    app.canvas.select_placements(&[1]);
    app.search.overlay.open = true;
    app.switch_to_board(second.id);

    assert!(!app.canvas.has_selection());
    assert!(!app.search.is_open());
}

#[test]
fn search_shortcut_opens_overlay_when_enabled() {
    let ctx = egui::Context::default();
    let root = TempRoot::new("snappyref-search-shortcut-enabled");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let mut app = test_app(Some(library));
    app.settings.semantic_search_enabled = true;
    app.search
        .set_local_model_store_status_for_test(LocalModelStoreStatus::Installed);

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
}

#[test]
fn search_shortcut_opens_missing_palette_with_focused_install_action() {
    let ctx = egui::Context::default();
    let mut app = test_app(None);
    app.settings.semantic_search_enabled = true;
    app.search
        .set_local_model_store_status_for_test(LocalModelStoreStatus::Missing);

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

#[test]
fn escape_closes_search_overlay() {
    let ctx = egui::Context::default();
    let mut app = test_app(None);
    app.search.overlay.open = true;
    app.search.overlay.query = "cat".to_owned();

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

#[test]
fn search_keyboard_flow_owns_escape_through_app_render_path() {
    let root = TempRoot::new("snappyref-search-keyboard-routing");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let second = library.create_board("Second").unwrap();
    let mut app = test_app(Some(library.clone()));
    app.settings.semantic_search_enabled = true;
    app.search
        .set_local_model_store_status_for_test(LocalModelStoreStatus::Installed);
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
    harness.state_mut().flush_persistence();
    assert_eq!(library.load_user_session().unwrap().active_board_id, 1);
    assert_ne!(second.id, 1);
}

#[test]
fn enter_places_unplaced_library_search_result_on_empty_active_board() {
    let root = TempRoot::new("snappyref-library-search-enter-unplaced");
    let library = Library::initialize(root.to_path_buf()).unwrap();
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
    app.search.overlay.results = vec![SearchResultAssetEntry {
        asset_id: library_item.require_loaded_asset().id,
        asset_preview_path: None,
        asset_width: 1,
        asset_height: 1,
    }];
    app.search.overlay.result_selection.replace(0);
    let mut harness = app_full_harness(app);

    harness.key_press(egui::Key::Enter);
    harness.step();
    harness.hover_at(egui::pos2(900.0, 600.0));
    harness.drag_at(egui::pos2(900.0, 600.0));
    harness.drop_at(egui::pos2(900.0, 600.0));
    harness.step();

    harness.state_mut().flush_persistence();
    let active_items = library.load_placements().unwrap();
    assert_eq!(active_items.len(), 1);
    assert_eq!(
        active_items[0].require_loaded_asset().id,
        library_item.require_loaded_asset().id
    );
    assert_eq!(
        harness.state().canvas.selected_placement_ids(),
        vec![active_items[0].state.id]
    );
    assert!(!harness.state().search.overlay.open);
}

#[test]
fn enter_places_unplaced_library_search_result_at_import_scale() {
    let root = TempRoot::new("snappyref-library-search-enter-import-scale");
    let library = Library::initialize(root.to_path_buf()).unwrap();
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
    app.search.overlay.results = vec![SearchResultAssetEntry {
        asset_id: library_item.require_loaded_asset().id,
        asset_preview_path: None,
        asset_width: 1,
        asset_height: 1,
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

    harness.state_mut().flush_persistence();
    let active_items = library.load_placements().unwrap();
    let expected_scale = crate::jobs::display_scale_for_placement_target_area(
        width,
        height,
        true,
        crate::data::DEFAULT_PLACEMENT_TARGET_AREA,
    );
    assert_eq!(active_items.len(), 1);
    assert_eq!(active_items[0].state.geometry.scale, expected_scale);
}

#[test]
fn dropped_library_search_result_is_visible_before_worker_decode_finishes() {
    let ctx = egui::Context::default();
    let root = TempRoot::new("snappyref-library-search-visible-before-worker");
    let library = Library::initialize(root.to_path_buf()).unwrap();
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
    app.import_worker = Some(crate::jobs::ImagePrepWorker::new(
        library.clone(),
        ctx.clone(),
        None,
    ));
    app.active_board_id = empty_board.id;
    app.search.overlay.open = true;
    app.search.overlay.results = vec![SearchResultAssetEntry {
        asset_id: library_item.require_loaded_asset().id,
        asset_preview_path: None,
        asset_width: 1,
        asset_height: 1,
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

    harness.state_mut().flush_persistence();
    let active_items = library.load_placements().unwrap();
    assert_eq!(active_items.len(), 1);
    assert!(
        harness
            .state()
            .canvas
            .placement_for_id(active_items[0].state.id)
            .is_some()
    );
    assert_eq!(harness.state().pending_import_batches.len(), 0);
}

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

#[test]
fn command_enter_library_search_results_show_pending_cursor_arrangement_inputs() {
    let root = TempRoot::new("snappyref-library-search-cmd-enter-provisional-placement");
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
            .provisional_placements_in_selection_order()
            .len(),
        2
    );
}

#[test]
fn search_asset_preview_uses_known_asset_dimensions_without_decoding_original_path() {
    let root = TempRoot::new("snappyref-library-search-known-dimensions-preview");
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
            asset_width: first.require_loaded_asset().width,
            asset_height: first.require_loaded_asset().height,
        },
        SearchResultAssetEntry {
            asset_id: second.require_loaded_asset().id,
            asset_preview_path: None,
            asset_width: second.require_loaded_asset().width,
            asset_height: second.require_loaded_asset().height,
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
        .provisional_placements_in_selection_order()
        .into_iter()
        .map(|placement| {
            crate::placement::arrangement::PlacementArrangementInput::new(
                placement.state.id,
                placement.state.geometry.center,
                placement.state.geometry.scaled_size(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(layout.len(), 2);
    let import_size_world = |asset: &crate::data::Asset| {
        crate::placement::creation::display_size_for_dimensions(
            asset.width,
            asset.height,
            harness.state().settings.placement_target_area,
        )
    };
    let first_size = import_size_world(first.require_loaded_asset());
    let second_size = import_size_world(second.require_loaded_asset());
    assert!(layout.iter().any(|item| {
        (item.width() - first_size.x).abs() < 0.1 && (item.height() - first_size.y).abs() < 0.1
    }));
    assert!(layout.iter().any(|item| {
        (item.width() - second_size.x).abs() < 0.1 && (item.height() - second_size.y).abs() < 0.1
    }));
}

#[test]
fn multi_select_enter_library_search_results_show_pending_cursor_arrangement_inputs() {
    let root = TempRoot::new("snappyref-library-search-multi-enter-provisional-placement");
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
            .provisional_placements_in_selection_order()
            .len(),
        2
    );
}

#[test]
fn dragging_library_search_result_out_of_palette_creates_provisional_placement() {
    let root = TempRoot::new("snappyref-library-search-pointer-drag-provisional-placement");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let source_path = library
        .store_rgba_image("first", &vec![255; 10 * 10 * 4], 10, 10)
        .unwrap();
    let source = library
        .persist_imported_image(
            &source_path,
            "image/png",
            10,
            10,
            4,
            egui::pos2(0.0, 0.0),
            1.0,
        )
        .unwrap();
    library.archive_placements(&[source.state.id]).unwrap();
    let empty_board = library.create_board("Empty").unwrap();
    library.set_active_board(empty_board.id).unwrap();
    let mut app = test_app(Some(library.clone()));
    app.active_board_id = empty_board.id;
    app.search.overlay.open = true;
    app.search.overlay.results = vec![SearchResultAssetEntry {
        asset_id: source.require_loaded_asset().id,
        asset_preview_path: None,
        asset_width: source.require_loaded_asset().width,
        asset_height: source.require_loaded_asset().height,
    }];
    app.search.overlay.result_selection.replace(0);
    let mut harness = app_harness(app, |ctx, app| {
        app.show_search_overlay(ctx, test_screen_rect());
    });

    harness.hover_at(egui::pos2(344.0, 206.0));
    harness.drag_at(egui::pos2(344.0, 206.0));
    harness.hover_at(egui::pos2(900.0, 600.0));
    harness.step();

    assert_eq!(
        harness
            .state()
            .canvas
            .provisional_placements_in_selection_order()
            .len(),
        1
    );
}

#[test]
fn inspect_search_result_drag_to_canvas_writes_artifact() {
    let mut scenario = InspectScenario::new("search-drag-to-canvas");
    scenario.event(
        "seed",
        serde_json::json!({
            "seed": "search-result-on-empty-board",
        }),
    );

    let root = TempRoot::new("snappyref-inspect-search-drag");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let source_path = library
        .store_rgba_image("first", &vec![255; 120 * 80 * 4], 120, 80)
        .unwrap();
    let source = library
        .persist_imported_image(
            &source_path,
            "image/png",
            120,
            80,
            4,
            egui::pos2(0.0, 0.0),
            1.0,
        )
        .unwrap();
    library.archive_placements(&[source.state.id]).unwrap();
    let empty_board = library.create_board("Empty").unwrap();
    library.set_active_board(empty_board.id).unwrap();

    let mut app = test_app(Some(library.clone()));
    app.active_board_id = empty_board.id;
    app.search.overlay.open = true;
    app.search.overlay.results = vec![SearchResultAssetEntry {
        asset_id: source.require_loaded_asset().id,
        asset_preview_path: None,
        asset_width: source.require_loaded_asset().width,
        asset_height: source.require_loaded_asset().height,
    }];
    app.search.overlay.result_selection.replace(0);
    let mut harness = app_harness(app, |ctx, app| {
        app.show_search_overlay(ctx, test_screen_rect());
    });

    let source_screen = egui::pos2(344.0, 206.0);
    let drop_screen = egui::pos2(900.0, 600.0);
    scenario.target(
        "snap.search.result[0]",
        "search-result",
        egui::Rect::from_center_size(source_screen, egui::vec2(96.0, 96.0)),
    );
    scenario.target("snap.canvas", "canvas", test_screen_rect());
    scenario.assertion(
        "search result is available before drag starts",
        harness.state().search.overlay.results.len() == 1,
    );

    scenario.action("hover search result", source_screen);
    harness.hover_at(source_screen);
    scenario.action("press search result", source_screen);
    harness.drag_at(source_screen);
    scenario.action("move pointer to canvas", drop_screen);
    harness.hover_at(drop_screen);
    harness.step();

    let provisional = harness
        .state()
        .canvas
        .provisional_placements_in_selection_order();
    let provisional_count = provisional.len();
    let mid_bounds = harness.state().canvas.selected_bounds_world();
    let pointer_world = harness
        .state()
        .canvas
        .screen_to_world(drop_screen, test_screen_rect());
    scenario.fact(
        "pointer.drag.mid",
        serde_json::json!({
            "screen": inspect_pos(drop_screen),
            "world": inspect_pos(pointer_world),
        }),
    );
    scenario.fact(
        "placement.provisional.mid",
        serde_json::json!({
            "count": provisional_count,
            "bounds_world": mid_bounds.map(inspect_rect),
        }),
    );
    scenario.assertion(
        "mid-drag provisional placement exists",
        provisional_count == 1,
    );
    let center_tracks_pointer = mid_bounds.is_some_and(|bounds| {
        (bounds.center().x - pointer_world.x).abs() < 0.1
            && (bounds.center().y - pointer_world.y).abs() < 0.1
    });
    scenario.assertion(
        "mid-drag group center tracks pointer",
        center_tracks_pointer,
    );

    scenario.action("release on canvas", drop_screen);
    harness.drop_at(drop_screen);
    harness.step();
    harness.state_mut().flush_persistence();

    let active_items = library.load_placements().unwrap();
    let committed = active_items.first().cloned();
    scenario.fact(
        "placement.committed",
        serde_json::json!({
            "count": active_items.len(),
            "placement": committed.as_ref().map(inspect_placement),
        }),
    );
    scenario.assertion("release commits one placement", active_items.len() == 1);
    scenario.assertion(
        "committed placement is selected",
        committed.as_ref().is_some_and(|placement| {
            harness.state().canvas.selected_placement_ids() == vec![placement.state.id]
        }),
    );
    if let Some(placement) = committed.as_ref() {
        scenario.capture_board_state_frame("board-after-release", placement, drop_screen);
    }

    let report = scenario.finish();
    assert!(
        report.passed,
        "inspect scenario failed; artifact={}",
        report.artifact_dir.display()
    );
}

#[test]
fn inspect_semantic_search_install_accepts_prompt() {
    let mut scenario = InspectScenario::new("semantic-search-install");
    scenario.event(
        "seed",
        serde_json::json!({
            "seed": "semantic-search-missing-first-launch-prompt",
        }),
    );

    let root = TempRoot::new("snappyref-inspect-semantic-install");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let starts = std::rc::Rc::new(std::cell::Cell::new(0));
    let observed = starts.clone();
    let mut app = test_app(Some(library.clone()));
    app.settings.semantic_search_first_launch_prompt_seen = false;
    app.search
        .set_local_model_store_status_for_test(LocalModelStoreStatus::Missing);
    app.search.set_semantic_install_for_test(move |_| {
        observed.set(observed.get() + 1);
    });
    let mut harness = app_full_harness(app);

    scenario.assertion(
        "semantic search first-launch prompt is visible",
        harness
            .state()
            .should_show_semantic_search_first_launch_prompt(),
    );
    let enable_rect = harness.get_by_label("Enable").rect();
    scenario.target("snap.semantic-search.install.enable", "button", enable_rect);
    scenario.action("click semantic search enable", enable_rect.center());
    harness.get_by_label("Enable").click();
    harness.run();

    scenario.fact(
        "semantic-search.install",
        serde_json::json!({
            "starts": starts.get(),
            "status": format!("{:?}", harness.state().search.local_model_store_status()),
            "app_status": harness.state().status,
            "prompt_seen": harness.state().settings.semantic_search_first_launch_prompt_seen,
        }),
    );
    scenario.assertion("install action started", starts.get() == 1);
    scenario.assertion(
        "semantic search status is downloading",
        harness.state().search.local_model_store_status()
            == LocalModelStoreStatus::Downloading { percent: 0 },
    );
    scenario.assertion(
        "first-launch prompt is marked seen",
        library
            .load_user_session()
            .unwrap()
            .settings
            .semantic_search_first_launch_prompt_seen,
    );

    let report = scenario.finish();
    assert!(
        report.passed,
        "inspect scenario failed; artifact={}",
        report.artifact_dir.display()
    );
}

#[test]
fn inspect_semantic_search_sidebar_status_writes_artifact() {
    let mut scenario = InspectScenario::new("semantic-search-sidebar-status");
    scenario.event(
        "seed",
        serde_json::json!({
            "seed": "sidebar-open-options-tab",
        }),
    );

    let root = TempRoot::new("snappyref-inspect-semantic-sidebar-status");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let actual_status = crate::search::semantic::default_local_model_store_status();
    let mut app = test_app(Some(library));
    app.sidebar_open = true;
    app.sidebar_tab = SidebarTab::Options;
    app.settings.semantic_search_first_launch_prompt_seen = true;
    app.search
        .set_local_model_store_status_for_test(actual_status.clone());
    let mut harness = app_full_harness(app);

    let section_rect = harness.get_by_label("Semantic Search").rect();
    scenario.target(
        "snap.sidebar.semantic-search",
        "settings-section",
        section_rect,
    );
    scenario.action(
        "scroll semantic search settings into view",
        section_rect.center(),
    );
    harness.get_by_label("Semantic Search").scroll_to_me();
    harness.run();

    let frame_stats = scenario.capture_screen("screen", &mut harness);
    scenario.fact(
        "semantic-search.local-model-store-status",
        serde_json::json!({
            "status": format!("{actual_status:?}"),
        }),
    );
    scenario.assertion(
        "semantic search settings section exists",
        section_rect.is_positive(),
    );
    scenario.assertion(
        "screen frame has expected viewport size",
        frame_stats.width == 1200 && frame_stats.height == 800,
    );
    scenario.assertion(
        "screen frame is a full app render",
        frame_stats.transparent_pixels == 0,
    );
    scenario.assertion("screen frame is nonblank", frame_stats.unique_colors > 1);

    let report = scenario.finish();
    assert!(
        report.passed,
        "inspect scenario failed; artifact={}",
        report.artifact_dir.display()
    );
}

/// Downloads the real model bundle. Run via
/// `docs/skills/inspect/scripts/run-scenario.sh semantic-search-real-install`.
#[test]
#[ignore]
fn inspect_semantic_search_real_install_writes_artifact() {
    let mut scenario = InspectScenario::new("semantic-search-real-install");
    scenario.event(
        "seed",
        serde_json::json!({
            "seed": "sidebar-open-options-tab-real-semantic-install",
        }),
    );

    let root = TempRoot::new("snappyref-inspect-semantic-real-install");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let initial_status = crate::search::semantic::default_local_model_store_status();
    let mut app = test_app(Some(library));
    app.sidebar_open = true;
    app.sidebar_tab = SidebarTab::Options;
    app.settings.semantic_search_first_launch_prompt_seen = true;
    app.search
        .set_local_model_store_status_for_test(initial_status.clone());
    app.search.set_semantic_install_for_test(|tx| {
        std::thread::spawn(move || {
            let data_root = match crate::library::location::resolve_data_root() {
                Ok(root) => root,
                Err(error) => {
                    let _ = tx.send(
                        crate::search::semantic::SemanticSearchInstallProgress::Failed { error },
                    );
                    return;
                }
            };
            let descriptor = crate::search::semantic::default_semantic_search_asset_descriptor();
            let canceled = std::sync::atomic::AtomicBool::new(false);
            let _ = crate::search::semantic::install_semantic_search_asset(
                &data_root,
                &descriptor,
                &canceled,
                |progress| {
                    let _ = tx.send(progress);
                },
            );
        });
    });

    let mut harness = app_full_harness(app);
    harness.get_by_label("Semantic Search").scroll_to_me();
    harness.run();
    scenario.capture_screen("screen-000-before-install", &mut harness);
    scenario.fact(
        "semantic-search.initial-status",
        serde_json::json!({
            "status": format!("{initial_status:?}"),
        }),
    );

    if matches!(
        initial_status,
        LocalModelStoreStatus::Installed | LocalModelStoreStatus::InstalledViaMigration
    ) {
        scenario.assertion("semantic search already installed", true);
        scenario.capture_screen("screen-001-installed", &mut harness);
        let report = scenario.finish();
        assert!(
            report.passed,
            "inspect scenario failed; artifact={}",
            report.artifact_dir.display()
        );
        return;
    }

    let install_rect = harness.get_by_label("Install").rect();
    scenario.target(
        "snap.sidebar.semantic-search.install",
        "button",
        install_rect,
    );
    scenario.action("click semantic search install", install_rect.center());
    harness.get_by_label("Install").click();
    harness.step();
    scenario.capture_screen("screen-001-install-clicked", &mut harness);

    scenario.drive_progress(
        &mut harness,
        InspectProgressConfig {
            start_frame_index: 2,
            timeout: Duration::from_secs(15 * 60),
            poll_interval: Duration::from_millis(250),
            fact_name: "semantic-search.install-status",
            timeout_fact_name: "semantic-search.install-timeout-status",
            terminal_assertion: "semantic search install reached terminal state",
            timeout_assertion: "semantic search install completed before timeout",
        },
        |app| app.search.local_model_store_status(),
        |status| inspect_status_slug(status),
        |status| {
            matches!(
                status,
                LocalModelStoreStatus::Installed | LocalModelStoreStatus::Failed(_)
            )
        },
        |status| matches!(status, LocalModelStoreStatus::Installed),
    );

    let report = scenario.finish();
    assert!(
        report.passed,
        "inspect scenario failed; artifact={}",
        report.artifact_dir.display()
    );
}

#[test]
fn inspect_sample_image_import_writes_artifact() {
    let mut scenario = InspectScenario::new("sample-image-import");
    scenario.event(
        "seed",
        serde_json::json!({
            "seed": "three-generated-sample-images",
        }),
    );

    let root = TempRoot::new("snappyref-inspect-sample-image-import");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let mut app = test_app(Some(library));
    app.settings.semantic_search_enabled = false;
    let mut harness = app_full_harness(app);

    let samples = [
        InspectImportSample {
            name: "red-landscape.png",
            width: 240,
            height: 140,
            color: [224, 68, 76, 255],
            center: egui::pos2(-180.0, -40.0),
        },
        InspectImportSample {
            name: "green-square.png",
            width: 180,
            height: 180,
            color: [74, 178, 102, 255],
            center: egui::pos2(0.0, -40.0),
        },
        InspectImportSample {
            name: "blue-portrait.png",
            width: 140,
            height: 240,
            color: [78, 126, 226, 255],
            center: egui::pos2(180.0, -40.0),
        },
    ];
    scenario.drive_import_lifecycle(&mut harness, &samples);

    let report = scenario.finish();
    assert!(
        report.passed,
        "inspect scenario failed; artifact={}",
        report.artifact_dir.display()
    );
}

#[test]
fn inspect_delete_board_modal_flash_writes_artifact() {
    let mut scenario = InspectScenario::new("delete-board-modal-flash");
    scenario.event(
        "scenario.seed",
        serde_json::json!({
            "seed": "boards-sidebar-delete-click",
            "viewport": inspect_rect(test_screen_rect()),
        }),
    );

    let root = TempRoot::new("snappyref-inspect-delete-board-flash");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let second = library.create_board("Second").unwrap();
    let mut app = test_app(Some(library.clone()));
    app.sidebar_open = true;
    app.sidebar_tab = SidebarTab::Boards;
    app.settings.semantic_search_first_launch_prompt_seen = true;

    let mut harness = app_full_harness(app);
    harness.step();
    scenario.capture_screen("screen-000-sidebar-open", &mut harness);

    let delete_rect = harness.get_by_label("Delete board Second").rect();
    scenario.target("snap.sidebar.board-delete", "button", delete_rect);

    harness.hover_at(delete_rect.center());
    harness.step();
    scenario.capture_screen("screen-001-delete-hovered", &mut harness);

    scenario.action("click delete board", delete_rect.center());
    inspect_click_at(&harness, delete_rect.center(), egui::Modifiers::NONE);
    harness.step();
    scenario.capture_screen("screen-002-dialog-open-first-frame", &mut harness);
    let first_modal_frame = scenario
        .artifact_dir
        .join("frames/screen-002-dialog-open-first-frame.png");
    let debug_red_pixels =
        inspect_bright_red_pixels_in_rect(&first_modal_frame, sidebar_rect(test_screen_rect()));
    scenario.fact(
        "delete-board-modal.sidebar-debug-red-pixels",
        serde_json::json!({
            "count": debug_red_pixels,
        }),
    );

    harness.step();
    scenario.capture_screen("screen-003-dialog-open-next-frame", &mut harness);

    scenario.fact(
        "delete-board-modal.state",
        serde_json::json!({
            "pending": harness.state().pending_board_delete.is_some(),
            "sidebar_open": harness.state().sidebar_open,
            "target_board": second.name,
        }),
    );
    scenario.assertion(
        "delete board click opens confirmation",
        harness.state().pending_board_delete.is_some(),
    );
    scenario.assertion(
        "sidebar has no bright debug-red flash on first modal frame",
        debug_red_pixels == 0,
    );
    scenario.assertion(
        "sidebar remains open behind modal",
        harness.state().sidebar_open,
    );

    let report = scenario.finish();
    assert!(
        report.passed,
        "inspect scenario failed; artifact={}",
        report.artifact_dir.display()
    );
}

#[test]
fn inspect_board_lifecycle_undo_redo_writes_artifact() {
    let mut scenario = InspectScenario::new("board-lifecycle-undo-redo");
    scenario.event(
        "scenario.seed",
        serde_json::json!({
            "seed": "one-initial-board",
            "purpose": "pointer-create, pointer-rename, confirmed pointer-delete, then keyboard lifecycle replay",
        }),
    );

    let root = TempRoot::new("snappyref-inspect-board-lifecycle");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let first = library.list_boards().unwrap()[0].clone();
    let mut app = test_app(Some(library.clone()));
    app.document_persistence = super::DocumentPersistence::queued(library.clone());
    app.sidebar_open = true;
    app.sidebar_tab = SidebarTab::Boards;
    app.settings.semantic_search_first_launch_prompt_seen = true;
    let mut harness = app_full_harness_with_step_dt(app, 0.1);
    harness.step();
    scenario.capture_screen("screen-000-initial-board", &mut harness);

    let create_rect = harness.get_by_label("New board").rect();
    scenario.target("sidebar.new-board", "button", create_rect);
    scenario.action("pointer click New board", create_rect.center());
    inspect_click_at(&harness, create_rect.center(), egui::Modifiers::NONE);
    harness.step();
    harness.state_mut().flush_persistence();
    let created = library
        .list_boards()
        .unwrap()
        .into_iter()
        .find(|board| board.id != first.id)
        .unwrap();
    scenario.assertion(
        "pointer create activates one exact new Board",
        created.name == "Board 2" && harness.state().active_board_id == created.id,
    );
    scenario.capture_screen("screen-001-created-board", &mut harness);

    let source = library
        .store_rgba_image(
            "lifecycle-blue",
            &solid_rgba([78, 126, 226, 255], 140, 100),
            140,
            100,
        )
        .unwrap();
    let placement = library
        .persist_imported_image(
            &source,
            "image/png",
            140,
            100,
            140 * 100 * 4,
            egui::pos2(80.0, -30.0),
            1.0,
        )
        .unwrap();
    let expected_view = crate::data::ViewportState {
        camera_pan_x: 90.0,
        camera_pan_y: -45.0,
        camera_zoom: 1.25,
        canvas_background: CanvasBackground::Grid,
    };
    harness
        .state_mut()
        .canvas
        .apply_viewport_state(&expected_view);
    let ctx = harness.ctx.clone();
    harness.state_mut().canvas.add_prepared_placement_image(
        &ctx,
        inspect_prepared_loaded_placement(
            placement.clone(),
            [78, 126, 226, 255],
            140,
            100,
            None,
            crate::jobs::ImportOrigin::Existing,
        ),
    );
    for _ in 0..7 {
        harness.step();
    }

    let board_name_rect = harness.get_by_label("Board Board 2").rect();
    scenario.target("sidebar.created-board-name", "button", board_name_rect);
    scenario.action(
        "pointer double-click Board 2 name",
        board_name_rect.center(),
    );
    harness.get_by_label("Board Board 2").click();
    harness.step();
    harness.get_by_label("Board Board 2").click();
    harness.step();
    harness.key_press_modifiers(inspect_command_modifier(), egui::Key::A);
    harness.event(egui::Event::Text("Lifecycle Board".to_owned()));
    harness.key_press(egui::Key::Enter);
    harness.step();
    let pointer_rename_committed =
        harness.state().board_summaries().iter().any(|summary| {
            summary.board.id == created.id && summary.board.name == "Lifecycle Board"
        });
    scenario.fact(
        "board-lifecycle.rename-harness",
        serde_json::json!({
            "pointer_double_click_committed": pointer_rename_committed,
            "limitation": "egui_kittest did not expose the overlapping board-row title response as a double click",
        }),
    );
    if !pointer_rename_committed {
        harness.state_mut().handle_board_action(Some(
            crate::ui::sidebar::SidebarBoardAction::RenameBoard(
                created.id,
                "Lifecycle Board".to_owned(),
            ),
        ));
        harness.step();
    }
    scenario.assertion(
        "rename action commits the exact requested name",
        harness.state().board_summaries().iter().any(|summary| {
            summary.board.id == created.id && summary.board.name == "Lifecycle Board"
        }),
    );
    scenario.capture_screen("screen-002-renamed-board-with-content", &mut harness);

    let delete_rect = harness.get_by_label("Delete board Lifecycle Board").rect();
    scenario.action("pointer click Board delete", delete_rect.center());
    inspect_click_at(&harness, delete_rect.center(), egui::Modifiers::NONE);
    harness.step();
    scenario.capture_screen("screen-003-delete-confirmation", &mut harness);
    let confirm_rect = harness.get_by_label("Delete board").rect();
    scenario.action("pointer click delete confirmation", confirm_rect.center());
    inspect_click_at(&harness, confirm_rect.center(), egui::Modifiers::NONE);
    harness.step();
    scenario.assertion(
        "confirmed delete activates the valid fallback",
        harness.state().active_board_id == first.id
            && !harness
                .state()
                .board_summaries()
                .iter()
                .any(|summary| summary.board.id == created.id),
    );
    scenario.capture_screen("screen-004-deleted-fallback-active", &mut harness);

    scenario.event(
        "action",
        serde_json::json!({"name": "press Command-Z to restore deleted Board"}),
    );
    harness.key_press_modifiers(inspect_command_modifier(), egui::Key::Z);
    harness.step();
    let restored = harness.state().canvas.placement_for_id(placement.state.id);
    scenario.fact(
        "board-lifecycle.after-delete-undo",
        serde_json::json!({
            "expected_board_id": created.id,
            "active_board_id": harness.state().active_board_id,
            "name": harness.state().board_summaries().iter()
                .find(|summary| summary.board.id == created.id)
                .map(|summary| summary.board.name.clone()),
            "placement_id": restored.as_ref().map(|placement| placement.state.id),
            "viewport": {
                "pan_x": harness.state().canvas.viewport_state().camera_pan_x,
                "pan_y": harness.state().canvas.viewport_state().camera_pan_y,
                "zoom": harness.state().canvas.viewport_state().camera_zoom,
                "background": harness.state().canvas.viewport_state().canvas_background.as_str(),
            },
        }),
    );
    scenario.assertion(
        "Undo restores exact Board identity, name, content, and view",
        harness.state().active_board_id == created.id
            && harness.state().board_summaries().iter().any(|summary| {
                summary.board.id == created.id && summary.board.name == "Lifecycle Board"
            })
            && restored.is_some_and(|placement| placement.is_loaded())
            && harness.state().canvas.viewport_state() == expected_view,
    );
    scenario.capture_screen("screen-005-delete-undone-content-restored", &mut harness);

    scenario.event(
        "action",
        serde_json::json!({"name": "press Command-Shift-Z to delete Board again"}),
    );
    harness.key_press_modifiers(
        egui::Modifiers {
            command: true,
            shift: true,
            ..Default::default()
        },
        egui::Key::Z,
    );
    harness.step();
    scenario.assertion(
        "Redo deletes the same Board and restores fallback",
        harness.state().active_board_id == first.id
            && !harness
                .state()
                .board_summaries()
                .iter()
                .any(|summary| summary.board.id == created.id),
    );

    harness.key_press_modifiers(inspect_command_modifier(), egui::Key::Z);
    harness.step();
    harness.key_press_modifiers(inspect_command_modifier(), egui::Key::Z);
    harness.step();
    scenario.assertion(
        "second Undo restores the exact pre-rename name",
        harness.state().active_board_id == created.id
            && harness
                .state()
                .board_summaries()
                .iter()
                .any(|summary| summary.board.id == created.id && summary.board.name == "Board 2"),
    );
    harness.key_press_modifiers(inspect_command_modifier(), egui::Key::Z);
    harness.step();
    scenario.assertion(
        "third Undo reverses creation to the exact prior active Board",
        harness.state().active_board_id == first.id
            && !harness
                .state()
                .board_summaries()
                .iter()
                .any(|summary| summary.board.id == created.id),
    );

    let redo = egui::Modifiers {
        command: true,
        shift: true,
        ..Default::default()
    };
    harness.key_press_modifiers(redo, egui::Key::Z);
    harness.step();
    scenario.assertion(
        "creation Redo restores the same Board ID",
        harness.state().active_board_id == created.id
            && harness
                .state()
                .board_summaries()
                .iter()
                .any(|summary| summary.board.id == created.id && summary.board.name == "Board 2"),
    );
    harness.key_press_modifiers(redo, egui::Key::Z);
    harness.step();
    scenario.assertion(
        "rename Redo restores the exact final name",
        harness.state().board_summaries().iter().any(|summary| {
            summary.board.id == created.id && summary.board.name == "Lifecycle Board"
        }),
    );
    harness.key_press_modifiers(redo, egui::Key::Z);
    harness.step();
    scenario.assertion(
        "delete Redo ends on the same fallback Board",
        harness.state().active_board_id == first.id
            && !harness
                .state()
                .board_summaries()
                .iter()
                .any(|summary| summary.board.id == created.id),
    );
    scenario.capture_screen("screen-006-full-lifecycle-redone", &mut harness);

    harness.state_mut().flush_persistence();
    scenario.assertion(
        "final lifecycle replay is durable",
        library.load_user_session().unwrap().active_board_id == first.id
            && library
                .list_boards()
                .unwrap()
                .iter()
                .all(|board| board.id != created.id),
    );

    let report = scenario.finish();
    assert!(
        report.passed,
        "inspect scenario failed; artifact={}",
        report.artifact_dir.display()
    );
}

#[test]
fn inspect_compact_windows_column_images_writes_artifact() {
    let mut scenario = InspectScenario::new("compact-windows-column-images");
    scenario.event(
        "seed",
        serde_json::json!({
            "seed": "copied-windows-library-column-asset-sizes",
            "source_assets": [
                "1779098228313-7-dmytro-koplyk--xbiOACMg8A-unsplash.jpg",
                "1779098229588-9-steve-gribble-5SzX75APpo0-unsplash.jpg",
                "1779013778968-6-sung-jin-cho-6iNPrmVctzQ-unsplash.jpg"
            ],
        }),
    );

    let mut app = test_app(None);
    app.settings.placement_gap = 0.0;
    let mut harness = app_full_harness(app);
    let ctx = harness.ctx.clone();
    let mut id = 1_i64;

    for index in 0..21 {
        add_inspect_compact_image(
            &mut harness,
            &ctx,
            id,
            346,
            260,
            if index % 2 == 0 {
                [74, 84, 76, 255]
            } else {
                [112, 122, 110, 255]
            },
            egui::pos2(-480.0, -320.0 + index as f32 * 260.0),
        );
        id += 1;
    }
    add_inspect_compact_image(
        &mut harness,
        &ctx,
        id,
        369,
        246,
        [150, 156, 140, 255],
        egui::pos2(-480.0, -320.0 + 21.0 * 260.0),
    );
    id += 1;
    for index in 0..21 {
        add_inspect_compact_image(
            &mut harness,
            &ctx,
            id,
            369,
            244,
            if index % 2 == 0 {
                [82, 92, 74, 255]
            } else {
                [122, 132, 118, 255]
            },
            egui::pos2(-110.0, -320.0 + index as f32 * 244.0),
        );
        id += 1;
    }

    harness
        .state_mut()
        .canvas
        .select_placements(&(1_i64..id).collect::<Vec<_>>());
    harness.step();

    scenario.capture_screen("screen-000-before-compact", &mut harness);
    scenario.fact(
        "compact.windows-column.before",
        compact_selection_fact(harness.state()),
    );
    harness.key_press(egui::Key::Q);
    harness.step();
    harness
        .state_mut()
        .canvas
        .focus_selection_in_view(test_screen_rect(), test_screen_rect());
    harness.step();
    let after_stats = scenario.capture_screen("screen-001-after-q-compact", &mut harness);
    let after_fact = compact_selection_fact(harness.state());
    scenario.fact("compact.windows-column.after", after_fact.clone());
    scenario.assertion(
        "compact copied Windows column frame is opaque",
        after_stats.transparent_pixels == 0,
    );
    scenario.assertion(
        "q compact keeps copied Windows column within sixteen by nine",
        after_fact["bounds_aspect"]
            .as_f64()
            .is_some_and(|aspect| aspect <= 16.0 / 9.0),
    );

    let report = scenario.finish();
    assert!(
        report.passed,
        "inspect scenario failed; artifact={}",
        report.artifact_dir.display()
    );
}

#[test]
fn inspect_compact_mixed_column_images_writes_artifact() {
    let mut scenario = InspectScenario::new("compact-mixed-column-images");
    scenario.event(
        "seed",
        serde_json::json!({
            "seed": "many-tiny-landscape-images-plus-mixed-sizes",
        }),
    );

    let mut app = test_app(None);
    app.settings.placement_gap = 12.0;
    let mut harness = app_full_harness(app);
    let ctx = harness.ctx.clone();
    let mut id = 1_i64;

    for index in 0..24 {
        let width = 20_u32 + (index % 3) as u32;
        let height = 24_u32 + (index % 2) as u32;
        let color = if index % 2 == 0 {
            [82, 92, 74, 255]
        } else {
            [120, 128, 116, 255]
        };
        add_inspect_compact_image(
            &mut harness,
            &ctx,
            id,
            width,
            height,
            color,
            egui::pos2(-500.0 + index as f32 * 14.0, -260.0 + index as f32 * 10.0),
        );
        id += 1;
    }

    for (width, height, color, center) in [
        (160, 100, [206, 184, 150, 255], egui::pos2(120.0, 0.0)),
        (120, 150, [92, 132, 172, 255], egui::pos2(270.0, -70.0)),
        (90, 70, [178, 170, 158, 255], egui::pos2(400.0, 40.0)),
        (70, 110, [198, 94, 70, 255], egui::pos2(540.0, -100.0)),
        (54, 64, [220, 216, 198, 255], egui::pos2(650.0, 20.0)),
        (92, 42, [78, 126, 226, 255], egui::pos2(760.0, -60.0)),
    ] {
        add_inspect_compact_image(&mut harness, &ctx, id, width, height, color, center);
        id += 1;
    }

    harness
        .state_mut()
        .canvas
        .select_placements(&(1_i64..id).collect::<Vec<_>>());
    harness.step();

    scenario.capture_screen("screen-000-before-compact", &mut harness);
    scenario.fact(
        "compact.mixed-column.before",
        compact_selection_fact(harness.state()),
    );
    harness.key_press(egui::Key::Q);
    harness.step();
    let after_stats = scenario.capture_screen("screen-001-after-q-compact", &mut harness);
    let after_fact = compact_selection_fact(harness.state());
    scenario.fact("compact.mixed-column.after", after_fact.clone());
    scenario.assertion(
        "compact mixed-column frame is opaque",
        after_stats.transparent_pixels == 0,
    );
    scenario.assertion(
        "q compact avoids a dominant tall thumbnail column",
        after_fact["max_column_count"]
            .as_u64()
            .is_some_and(|count| count <= 12),
    );
    scenario.assertion(
        "q compact keeps mixed selection in a rectangle-like bounds",
        after_fact["bounds_aspect"]
            .as_f64()
            .is_some_and(|aspect| aspect <= 3.0),
    );

    let report = scenario.finish();
    assert!(
        report.passed,
        "inspect scenario failed; artifact={}",
        report.artifact_dir.display()
    );
}

#[test]
fn inspect_compact_equal_size_images_writes_artifact() {
    let mut scenario = InspectScenario::new("compact-equal-size-images");
    scenario.event(
        "seed",
        serde_json::json!({
            "seed": "six-equal-size-selected-images",
        }),
    );

    let mut app = test_app(None);
    app.settings.placement_gap = 12.0;
    let mut harness = app_full_harness(app);
    let ctx = harness.ctx.clone();
    let colors = [
        [228, 171, 72, 255],
        [155, 108, 218, 255],
        [76, 190, 184, 255],
        [206, 89, 94, 255],
        [97, 145, 228, 255],
        [133, 190, 92, 255],
    ];

    for id in 1_i64..=6 {
        let asset = Asset {
            id,
            original_path: PathBuf::from(format!("compact-equal-{id}.png")),
            filename: format!("compact-equal-{id}.png"),
            mime_type: "image/png".to_owned(),
            width: 150,
            height: 40,
        };
        let placement = Placement::loaded(
            PlacementState::with_display_size(
                id,
                egui::pos2(-360.0 + id as f32 * 150.0, -120.0 + id as f32 * 20.0),
                egui::vec2(150.0, 40.0),
                1.0,
                0.0,
                id as i32,
                false,
            ),
            asset,
        );
        harness.state_mut().canvas.add_prepared_placement_image(
            &ctx,
            PreparedPlacementImage {
                preview_rgba: solid_rgba(colors[(id - 1) as usize], 150, 40),
                preview_width: 150,
                preview_height: 40,
                ..loaded_record(placement)
            },
        );
    }
    harness
        .state_mut()
        .canvas
        .select_placements(&(1_i64..=6).collect::<Vec<_>>());
    harness.step();

    scenario.capture_screen("screen-000-before-compact", &mut harness);
    scenario.fact(
        "compact.equal.before",
        compact_selection_fact(harness.state()),
    );
    harness.key_press(egui::Key::Q);
    harness.step();
    let after_stats = scenario.capture_screen("screen-001-after-q-compact", &mut harness);
    let after_fact = compact_selection_fact(harness.state());
    scenario.fact("compact.equal.after", after_fact.clone());
    scenario.assertion(
        "compact equal-size frame is opaque",
        after_stats.transparent_pixels == 0,
    );
    scenario.assertion(
        "q compact arranges six equal-size images wider than one column",
        after_fact["column_count"]
            .as_u64()
            .is_some_and(|count| count > 1),
    );

    let report = scenario.finish();
    assert!(
        report.passed,
        "inspect scenario failed; artifact={}",
        report.artifact_dir.display()
    );
}

#[test]
fn inspect_search_palette_footer_writes_artifact() {
    let mut scenario = InspectScenario::new("search-palette-footer");
    scenario.event(
        "seed",
        serde_json::json!({
            "seed": "empty-search-palette-plus-results-footer",
        }),
    );

    let root = TempRoot::new("snappyref-inspect-search-palette-footer");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let result_specs = [
        ("first-result", [228, 171, 72, 255], 180_u32, 120_u32),
        ("second-result", [155, 108, 218, 255], 140_u32, 140_u32),
    ];
    let mut result_placements = Vec::new();
    for (index, (name, color, width, height)) in result_specs.into_iter().enumerate() {
        let path = library
            .store_rgba_image(name, &solid_rgba(color, width, height), width, height)
            .unwrap();
        let placement = library
            .persist_imported_image(
                &path,
                "image/png",
                width,
                height,
                width as u64 * height as u64 * 4,
                egui::pos2(index as f32 * 32.0, 0.0),
                1.0,
            )
            .unwrap();
        library.archive_placements(&[placement.state.id]).unwrap();
        result_placements.push(placement);
    }

    let mut app = test_app(Some(library.clone()));
    app.search.overlay.open = true;
    app.search.overlay.status = crate::search::palette::SearchOverlayStatus::Ready;
    let mut harness = app_full_harness(app);

    let empty_stats = scenario.capture_screen("screen-000-empty-search-palette", &mut harness);
    scenario.fact(
        "search.palette.footer.empty",
        serde_json::json!({
            "status": "Ready",
            "result_count": harness.state().search.overlay.results.len(),
        }),
    );
    scenario.assertion(
        "empty palette frame is opaque",
        empty_stats.transparent_pixels == 0,
    );

    let results = result_placements
        .iter()
        .map(inspect_search_result_entry)
        .collect::<Vec<_>>();
    harness.state_mut().search.overlay.results = results;
    harness.state_mut().search.overlay.has_more_results = true;
    harness
        .state_mut()
        .search
        .overlay
        .result_selection
        .replace(0);
    harness.step();

    let results_stats = scenario.capture_screen("screen-001-results-search-palette", &mut harness);
    scenario.fact(
        "search.palette.footer.results",
        serde_json::json!({
            "has_more_results": harness.state().search.overlay.has_more_results,
            "result_count": harness.state().search.overlay.results.len(),
            "selection_count": harness.state().search.overlay.result_selection.selected().len(),
        }),
    );
    scenario.assertion(
        "results palette frame is opaque",
        results_stats.transparent_pixels == 0,
    );

    let report = scenario.finish();
    assert!(
        report.passed,
        "inspect scenario failed; artifact={}",
        report.artifact_dir.display()
    );
}

#[test]
fn inspect_blue_image_search_drag_writes_artifact() {
    let mut scenario = InspectScenario::new("blue-image-search-drag");
    scenario.event(
        "seed",
        serde_json::json!({
            "seed": "blue-image-plus-three-search-results",
        }),
    );

    let root = TempRoot::new("snappyref-inspect-blue-image-search-drag");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let blue_path = library
        .store_rgba_image(
            "blue-selected",
            &solid_rgba([78, 126, 226, 255], 140, 240),
            140,
            240,
        )
        .unwrap();
    let blue = library
        .persist_imported_image(
            &blue_path,
            "image/png",
            140,
            240,
            140 * 240 * 4,
            egui::pos2(0.0, 0.0),
            1.0,
        )
        .unwrap();
    let result_specs = [
        ("amber-result", [228, 171, 72, 255], 220_u32, 150_u32),
        ("violet-result", [155, 108, 218, 255], 180_u32, 180_u32),
        ("teal-result", [76, 190, 184, 255], 150_u32, 220_u32),
    ];
    let mut result_placements = Vec::new();
    for (index, (name, color, width, height)) in result_specs.into_iter().enumerate() {
        let path = library
            .store_rgba_image(name, &solid_rgba(color, width, height), width, height)
            .unwrap();
        let placement = library
            .persist_imported_image(
                &path,
                "image/png",
                width,
                height,
                width as u64 * height as u64 * 4,
                egui::pos2(index as f32 * 32.0, 0.0),
                1.0,
            )
            .unwrap();
        library.archive_placements(&[placement.state.id]).unwrap();
        result_placements.push(placement);
    }

    let mut app = test_app(Some(library.clone()));
    app.settings.semantic_search_enabled = true;
    app.search
        .set_local_model_store_status_for_test(LocalModelStoreStatus::Installed);
    let mut harness = app_full_harness(app);
    let blue_prepared = inspect_prepared_loaded_placement(
        blue.clone(),
        [78, 126, 226, 255],
        140,
        240,
        None,
        crate::jobs::ImportOrigin::Existing,
    );
    let ctx = harness.ctx.clone();
    harness
        .state_mut()
        .canvas
        .add_prepared_placement_image(&ctx, blue_prepared);
    harness.step();
    scenario.capture_screen("screen-000-app-open-blue-image", &mut harness);

    let blue_start_screen = inspect_world_to_screen(egui::pos2(0.0, 0.0), test_screen_rect());
    scenario.action("click blue image", blue_start_screen);
    inspect_click_at(&harness, blue_start_screen, egui::Modifiers::NONE);
    harness.step();
    scenario.capture_screen("screen-001-blue-selected", &mut harness);

    let blue_drop_screen = blue_start_screen + egui::vec2(110.0, 0.0);
    scenario.action("drag blue image right", blue_drop_screen);
    harness.hover_at(blue_start_screen);
    harness.drag_at(blue_start_screen);
    harness.hover_at(blue_drop_screen);
    harness.step();
    scenario.capture_screen("screen-002-blue-moving-right", &mut harness);
    harness.drop_at(blue_drop_screen);
    harness.step();
    scenario.capture_screen("screen-003-blue-moved-right", &mut harness);

    scenario.action(
        "start image search from selected blue image",
        blue_drop_screen,
    );
    let blue_asset = blue.require_loaded_asset();
    harness.state_mut().search.overlay.open = true;
    harness.state_mut().search.overlay.focus_requested = true;
    harness.state_mut().search.overlay.image_pivot =
        Some(crate::search::palette::ImageSearchPivot {
            asset_id: blue_asset.id,
            asset_preview_path: Some(blue_asset.original_path.clone()),
        });
    harness.state_mut().search.overlay.status =
        crate::search::palette::SearchOverlayStatus::Loading("Searching…".to_owned());
    harness.step();
    scenario.capture_screen("screen-004-image-search-started", &mut harness);

    let results = result_placements
        .iter()
        .map(inspect_search_result_entry)
        .collect::<Vec<_>>();
    harness.state_mut().search.overlay.results = results;
    harness.state_mut().search.overlay.status = crate::search::palette::SearchOverlayStatus::Ready;
    harness.step();
    scenario.capture_screen("screen-005-search-results-ready", &mut harness);

    let first_result = inspect_search_result_tile_center(0, result_placements.len());
    let second_result = inspect_search_result_tile_center(1, result_placements.len());
    let third_result = inspect_search_result_tile_center(2, result_placements.len());
    scenario.action("select first search result", first_result);
    inspect_click_at(&harness, first_result, egui::Modifiers::NONE);
    harness.step();
    scenario.action("add second search result to selection", second_result);
    inspect_click_at(&harness, second_result, inspect_command_modifier());
    harness.step();
    scenario.action("add third search result to selection", third_result);
    inspect_click_at(&harness, third_result, inspect_command_modifier());
    harness.step();
    scenario.capture_screen("screen-006-three-search-results-selected", &mut harness);
    let selected_before_drag = harness
        .state()
        .search
        .overlay
        .result_selection
        .selected()
        .len();

    let provisional_screen = egui::pos2(890.0, 610.0);
    scenario.action(
        "drag selected search results over canvas",
        provisional_screen,
    );
    harness.hover_at(first_result);
    harness.drag_at(first_result);
    harness.hover_at(provisional_screen);
    harness.step();
    harness.step();
    let provisional_count = harness
        .state()
        .canvas
        .provisional_placements_in_selection_order()
        .len();
    scenario.fact(
        "blue-image-search-drag.provisional",
        serde_json::json!({
            "count": provisional_count,
        }),
    );
    scenario.capture_screen(
        "screen-007-provisional-search-placements-before-drop",
        &mut harness,
    );

    scenario.action("drop search placements on canvas", provisional_screen);
    harness.drop_at(provisional_screen);
    harness.step();
    scenario.capture_screen("screen-008-search-images-dropped-loaded", &mut harness);

    let final_placement_count = harness.state().canvas.all_placement_ids().len();
    scenario.fact(
        "blue-image-search-drag.final",
        serde_json::json!({
            "selected_results_before_drag": selected_before_drag,
            "final_placement_count": final_placement_count,
            "status": harness.state().status,
        }),
    );
    scenario.assertion(
        "three search results selected before drag",
        selected_before_drag == 3,
    );
    scenario.assertion(
        "three provisional search placements visible before drop",
        provisional_count == 3,
    );
    scenario.assertion(
        "search drag created three additional placements",
        final_placement_count == 4,
    );
    scenario.assertion(
        "search images loaded after drop",
        harness.state().content_resolution.pending_len() == 0,
    );

    let report = scenario.finish();
    assert!(
        report.passed,
        "inspect scenario failed; artifact={}",
        report.artifact_dir.display()
    );
}

#[test]
fn inspect_placement_click_hold_flash_writes_artifact() {
    let mut scenario = InspectScenario::new("placement-click-hold-flash");
    scenario.event(
        "seed",
        serde_json::json!({
            "seed": "single-blue-placement-click-hold",
            "purpose": "capture frames around primary-click selection held past movement debounce",
        }),
    );

    let root = TempRoot::new("snappyref-inspect-placement-click-hold-flash");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let blue_path = library
        .store_rgba_image(
            "blue-click-hold",
            &solid_rgba([78, 126, 226, 255], 140, 240),
            140,
            240,
        )
        .unwrap();
    let blue = library
        .persist_imported_image(
            &blue_path,
            "image/png",
            140,
            240,
            140 * 240 * 4,
            egui::pos2(0.0, 0.0),
            1.0,
        )
        .unwrap();

    let app = test_app(Some(library.clone()));
    let mut harness = app_full_harness_with_step_dt(app, 1.0 / 60.0);
    let blue_prepared = inspect_prepared_loaded_placement(
        blue.clone(),
        [78, 126, 226, 255],
        140,
        240,
        None,
        crate::jobs::ImportOrigin::Existing,
    );
    let ctx = harness.ctx.clone();
    harness
        .state_mut()
        .canvas
        .add_prepared_placement_image(&ctx, blue_prepared);
    harness.step();

    let placement_id = blue.state.id;
    let click_screen = inspect_world_to_screen(egui::pos2(0.0, 0.0), test_screen_rect());
    let placement_screen_rect =
        egui::Rect::from_center_size(click_screen, egui::vec2(140.0, 240.0));
    scenario.target("blue placement", "placement", placement_screen_rect);
    scenario.capture_click_hold_fact("screen-000-before-press", &harness, placement_id);
    scenario.capture_screen("screen-000-before-press", &mut harness);

    scenario.action("primary press on blue placement", click_screen);
    harness.event(egui::Event::PointerButton {
        pos: click_screen,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::NONE,
    });
    harness.step();
    scenario.capture_click_hold_fact("screen-001-pressed-selected", &harness, placement_id);
    scenario.capture_screen("screen-001-pressed-selected", &mut harness);

    for index in 2..=8 {
        harness.step();
        let name = format!("screen-{index:03}-held");
        scenario.capture_click_hold_fact(&name, &harness, placement_id);
        scenario.capture_screen(&name, &mut harness);
    }
    let held_time_seconds = harness.ctx.input(|input| input.time);
    scenario.assertion(
        "selection border stays visible during stationary held click",
        harness
            .state()
            .canvas
            .selection_border_visible_for_test(held_time_seconds),
    );

    scenario.action("primary release on blue placement", click_screen);
    harness.event(egui::Event::PointerButton {
        pos: click_screen,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    });
    harness.step();
    scenario.capture_click_hold_fact("screen-009-released", &harness, placement_id);
    scenario.capture_screen("screen-009-released", &mut harness);

    let selected = harness.state().canvas.selected_placement_ids();
    scenario.assertion(
        "blue placement remains selected after click hold",
        selected == vec![placement_id],
    );

    let report = scenario.finish();
    assert!(
        report.passed,
        "inspect scenario failed; artifact={}",
        report.artifact_dir.display()
    );
}

#[test]
fn inspect_delete_undo_redo_placement_writes_artifact() {
    let mut scenario = InspectScenario::new("delete-undo-redo-placement");
    scenario.event(
        "seed",
        serde_json::json!({
            "seed": "single-blue-placement",
            "purpose": "click placement, delete it, then exercise Undo and Redo shortcuts",
        }),
    );

    let root = TempRoot::new("snappyref-inspect-delete-undo-redo-placement");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let blue_path = library
        .store_rgba_image(
            "blue-undo-redo",
            &solid_rgba([78, 126, 226, 255], 140, 100),
            140,
            100,
        )
        .unwrap();
    let blue = library
        .persist_imported_image(
            &blue_path,
            "image/png",
            140,
            100,
            140 * 100 * 4,
            egui::pos2(0.0, 0.0),
            1.0,
        )
        .unwrap();

    let mut app = test_app(Some(library.clone()));
    app.document_persistence = super::DocumentPersistence::queued(library.clone());
    let mut harness = app_full_harness(app);
    let ctx = harness.ctx.clone();
    harness.state_mut().canvas.add_prepared_placement_image(
        &ctx,
        inspect_prepared_loaded_placement(
            blue.clone(),
            [78, 126, 226, 255],
            140,
            100,
            None,
            crate::jobs::ImportOrigin::Existing,
        ),
    );
    harness.step();

    let placement_id = blue.state.id;
    let click_screen = inspect_world_to_screen(blue.state.geometry.center, test_screen_rect());
    scenario.target(
        "blue placement",
        "placement",
        egui::Rect::from_center_size(click_screen, egui::vec2(140.0, 100.0)),
    );
    scenario.capture_screen("screen-000-before-click", &mut harness);

    scenario.action("click blue placement", click_screen);
    inspect_click_at(&harness, click_screen, egui::Modifiers::NONE);
    harness.step();
    scenario.assertion(
        "pointer click selects placement",
        harness.state().canvas.selected_placement_ids() == vec![placement_id],
    );
    scenario.capture_screen("screen-001-selected", &mut harness);

    scenario.event(
        "action",
        serde_json::json!({"name": "press Delete shortcut"}),
    );
    harness.key_press(egui::Key::Backspace);
    harness.step();
    scenario.fact(
        "placement.after-delete",
        serde_json::json!({
            "present": harness.state().canvas.placement_for_id(placement_id).is_some(),
            "status": harness.state().status,
        }),
    );
    scenario.assertion(
        "Delete removes placement from active canvas",
        harness
            .state()
            .canvas
            .placement_for_id(placement_id)
            .is_none(),
    );
    scenario.capture_screen("screen-002-deleted", &mut harness);

    let undo_modifiers = egui::Modifiers {
        command: true,
        ..Default::default()
    };
    scenario.event(
        "action",
        serde_json::json!({"name": "press Command-Z Undo shortcut"}),
    );
    for pressed in [true, false] {
        harness.event_modifiers(
            egui::Event::Key {
                key: egui::Key::Z,
                physical_key: Some(egui::Key::Z),
                pressed,
                repeat: false,
                modifiers: undo_modifiers,
            },
            undo_modifiers,
        );
    }
    harness.step();
    let restored = harness.state().canvas.placement_for_id(placement_id);
    scenario.fact(
        "placement.after-undo",
        serde_json::json!({
            "present": restored.is_some(),
            "loaded": restored.as_ref().is_some_and(|placement| placement.is_loaded()),
            "status": harness.state().status,
        }),
    );
    scenario.assertion(
        "Undo restores loaded placement to active canvas",
        restored.is_some_and(|placement| placement.is_loaded()),
    );
    scenario.capture_screen("screen-003-undone", &mut harness);

    let redo_modifiers = egui::Modifiers {
        command: true,
        shift: true,
        ..Default::default()
    };
    scenario.event(
        "action",
        serde_json::json!({"name": "press Command-Shift-Z Redo shortcut"}),
    );
    for pressed in [true, false] {
        harness.event_modifiers(
            egui::Event::Key {
                key: egui::Key::Z,
                physical_key: Some(egui::Key::Z),
                pressed,
                repeat: false,
                modifiers: redo_modifiers,
            },
            redo_modifiers,
        );
    }
    harness.step();
    scenario.fact(
        "placement.after-redo",
        serde_json::json!({
            "present": harness.state().canvas.placement_for_id(placement_id).is_some(),
            "status": harness.state().status,
        }),
    );
    scenario.assertion(
        "Redo removes placement from active canvas again",
        harness
            .state()
            .canvas
            .placement_for_id(placement_id)
            .is_none(),
    );
    scenario.capture_screen("screen-004-redone", &mut harness);

    harness.state_mut().flush_persistence();
    scenario.assertion(
        "explicit flush makes final Redo durable",
        library.load_placements().unwrap().is_empty(),
    );

    let report = scenario.finish();
    assert!(
        report.passed,
        "inspect scenario failed; artifact={}",
        report.artifact_dir.display()
    );
}

#[test]
fn inspect_crop_undo_redo_placement_writes_artifact() {
    let mut scenario = InspectScenario::new("crop-undo-redo-placement");
    scenario.event(
        "seed",
        serde_json::json!({
            "seed": "single-loaded-placement",
            "purpose": "select the image, crop with C plus a pointer drag, then exercise Undo and Redo",
        }),
    );

    let root = TempRoot::new("snappyref-inspect-crop-undo-redo-placement");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let width = 160;
    let height = 120;
    let source_rgba = (0..height)
        .flat_map(|y| {
            (0..width).flat_map(move |x| {
                let red = (x * 255 / width) as u8;
                let blue = (y * 255 / height) as u8;
                [red, 96, blue, 255]
            })
        })
        .collect::<Vec<_>>();
    let source_path = library
        .store_rgba_image("crop-source", &source_rgba, width, height)
        .unwrap();
    let loading = library
        .persist_committed_loading_placement(
            egui::Pos2::ZERO,
            1.0,
            width as f32,
            height as f32,
            crate::data::ContentResolutionSource::ExternalPath {
                path: source_path.to_string_lossy().into_owned(),
            },
            Some("Loading".to_owned()),
        )
        .unwrap();
    let source = library
        .persist_imported_image_resolving_placement(
            library.load_user_session().unwrap().active_board_id,
            Some(loading.state.id),
            &source_path,
            "image/png",
            width,
            height,
            source_rgba.len() as u64,
            egui::Pos2::ZERO,
            1.0,
        )
        .unwrap();
    let source_asset_id = source.require_loaded_asset().id;

    let mut app = test_app(Some(library.clone()));
    app.settings.semantic_search_enabled = false;
    app.document_persistence = super::DocumentPersistence::queued(library.clone());
    let mut harness = app_full_harness(app);
    let ctx = harness.ctx.clone();
    harness.state_mut().import_worker = Some(crate::jobs::ImagePrepWorker::new(
        library.clone(),
        ctx.clone(),
        None,
    ));
    let active_board_id = harness.state().active_board_id;
    harness.state_mut().canvas.add_prepared_placement_image(
        &ctx,
        PreparedPlacementImage {
            target_board_id: active_board_id,
            preview_rgba: source_rgba,
            preview_width: width,
            preview_height: height,
            ..loaded_record(source.clone())
        },
    );
    harness.step();

    let placement_id = source.state.id;
    let image_center = inspect_world_to_screen(source.state.geometry.center, test_screen_rect());
    let crop_start = image_center + egui::vec2(-55.0, -40.0);
    let crop_end = image_center + egui::vec2(45.0, 30.0);
    scenario.target(
        "source image",
        "placement",
        egui::Rect::from_center_size(image_center, egui::vec2(width as f32, height as f32)),
    );
    scenario.capture_screen("screen-000-source", &mut harness);

    scenario.action("click source image", image_center);
    inspect_click_at(&harness, image_center, egui::Modifiers::NONE);
    harness.step();
    scenario.assertion(
        "pointer click selects source image",
        harness.state().canvas.selected_placement_ids() == vec![placement_id],
    );
    scenario.capture_screen("screen-001-selected", &mut harness);

    scenario.event(
        "action",
        serde_json::json!({
            "name": "hold C and pointer-drag crop rectangle",
            "start": inspect_pos(crop_start),
            "end": inspect_pos(crop_end),
        }),
    );
    harness.key_down(egui::Key::C);
    harness.hover_at(crop_start);
    harness.drag_at(crop_start);
    harness.step();
    harness.hover_at(crop_end);
    harness.step();
    scenario.capture_screen("screen-002-crop-drag", &mut harness);
    harness.drop_at(crop_end);
    harness.key_up(egui::Key::C);
    harness.step();

    let crop_deadline = Instant::now() + Duration::from_secs(5);
    while harness
        .state()
        .canvas
        .placement_for_id(placement_id)
        .is_some_and(|placement| placement.require_loaded_asset().id == source_asset_id)
        && Instant::now() < crop_deadline
    {
        std::thread::sleep(Duration::from_millis(10));
        harness.step();
    }
    let cropped = harness
        .state()
        .canvas
        .placement_for_id(placement_id)
        .unwrap();
    let cropped_asset_id = cropped.require_loaded_asset().id;
    scenario.fact(
        "placement.after-crop",
        serde_json::json!({
            "source_asset_id": source_asset_id,
            "asset_id": cropped_asset_id,
            "asset_dimensions": [cropped.require_loaded_asset().width, cropped.require_loaded_asset().height],
            "status": harness.state().status,
        }),
    );
    scenario.assertion(
        "pointer crop creates one derived Asset",
        cropped_asset_id != source_asset_id,
    );
    scenario.capture_screen("screen-003-cropped", &mut harness);

    scenario.event(
        "action",
        serde_json::json!({"name": "press Command-Z Undo shortcut"}),
    );
    harness.key_press_modifiers(inspect_command_modifier(), egui::Key::Z);
    harness.step();
    let undo_deadline = Instant::now() + Duration::from_secs(5);
    while harness.state().content_resolution.pending_len() > 0 && Instant::now() < undo_deadline {
        std::thread::sleep(Duration::from_millis(10));
        harness.step();
    }
    let after_undo = harness
        .state()
        .canvas
        .placement_for_id(placement_id)
        .unwrap();
    scenario.fact(
        "placement.after-undo",
        serde_json::json!({
            "asset_id": after_undo.require_loaded_asset().id,
            "status": harness.state().status,
        }),
    );
    scenario.assertion(
        "one Undo restores the source Asset",
        after_undo.require_loaded_asset().id == source_asset_id
            && harness.state().status == "Undid Crop image",
    );
    scenario.capture_screen("screen-004-undone", &mut harness);

    scenario.event(
        "action",
        serde_json::json!({"name": "press Command-Z a second time"}),
    );
    harness.key_press_modifiers(inspect_command_modifier(), egui::Key::Z);
    harness.step();
    scenario.fact(
        "placement.after-second-undo",
        serde_json::json!({
            "asset_id": harness
                .state()
                .canvas
                .placement_for_id(placement_id)
                .unwrap()
                .require_loaded_asset()
                .id,
            "status": harness.state().status,
        }),
    );
    scenario.assertion(
        "second Undo does not reverse the crop again",
        harness
            .state()
            .canvas
            .placement_for_id(placement_id)
            .unwrap()
            .require_loaded_asset()
            .id
            == source_asset_id
            && harness.state().status == "Undid Crop image",
    );
    scenario.capture_screen("screen-005-second-undo", &mut harness);

    let redo_modifiers = egui::Modifiers {
        command: true,
        shift: true,
        ..Default::default()
    };
    scenario.event(
        "action",
        serde_json::json!({"name": "press Command-Shift-Z Redo shortcut"}),
    );
    harness.key_press_modifiers(redo_modifiers, egui::Key::Z);
    harness.step();
    let redo_deadline = Instant::now() + Duration::from_secs(5);
    while harness.state().content_resolution.pending_len() > 0 && Instant::now() < redo_deadline {
        std::thread::sleep(Duration::from_millis(10));
        harness.step();
    }
    let after_redo = harness
        .state()
        .canvas
        .placement_for_id(placement_id)
        .unwrap();
    scenario.fact(
        "placement.after-redo",
        serde_json::json!({
            "asset_id": after_redo.require_loaded_asset().id,
            "status": harness.state().status,
        }),
    );
    scenario.assertion(
        "Redo restores the same cropped Asset",
        after_redo.require_loaded_asset().id == cropped_asset_id
            && harness.state().status == "Redid Crop image",
    );
    scenario.capture_screen("screen-006-redone", &mut harness);

    harness.state_mut().flush_persistence();
    let durable = &library.load_placements().unwrap()[0];
    scenario.assertion(
        "explicit flush preserves the cropped Asset relation",
        durable.require_loaded_asset().id == cropped_asset_id,
    );

    let report = scenario.finish();
    assert!(
        report.passed,
        "inspect scenario failed; artifact={}",
        report.artifact_dir.display()
    );
}

#[test]
fn dragging_multiple_library_search_results_centers_provisional_group_on_pointer() {
    let root = TempRoot::new("snappyref-library-search-pointer-drag-multi-center");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let first_path = library
        .store_rgba_image("first", &vec![255; 120 * 80 * 4], 120, 80)
        .unwrap();
    let second_path = library
        .store_rgba_image("second", &vec![180; 80 * 120 * 4], 80, 120)
        .unwrap();
    let first = library
        .persist_imported_image(
            &first_path,
            "image/png",
            120,
            80,
            4,
            egui::pos2(0.0, 0.0),
            1.0,
        )
        .unwrap();
    let second = library
        .persist_imported_image(
            &second_path,
            "image/png",
            80,
            120,
            4,
            egui::pos2(0.0, 0.0),
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
            asset_width: first.require_loaded_asset().width,
            asset_height: first.require_loaded_asset().height,
        },
        SearchResultAssetEntry {
            asset_id: second.require_loaded_asset().id,
            asset_preview_path: None,
            asset_width: second.require_loaded_asset().width,
            asset_height: second.require_loaded_asset().height,
        },
    ];
    app.search.overlay.result_selection.replace(0);
    app.search.overlay.result_selection.toggle(1);
    let mut harness = app_harness(app, |ctx, app| {
        app.show_search_overlay(ctx, test_screen_rect());
    });

    let pointer_screen = egui::pos2(900.0, 600.0);
    harness.hover_at(egui::pos2(344.0, 206.0));
    harness.drag_at(egui::pos2(344.0, 206.0));
    harness.hover_at(pointer_screen);
    harness.step();

    let bounds = harness.state().canvas.selected_bounds_world().unwrap();
    let pointer_world = harness
        .state()
        .canvas
        .screen_to_world(pointer_screen, test_screen_rect());
    assert_eq!(
        harness
            .state()
            .canvas
            .provisional_placements_in_selection_order()
            .len(),
        2
    );
    assert!(
        (bounds.center().x - pointer_world.x).abs() < 0.1,
        "group center x {:?} should match pointer world x {:?}",
        bounds.center().x,
        pointer_world.x
    );
    assert!(
        (bounds.center().y - pointer_world.y).abs() < 0.1,
        "group center y {:?} should match pointer world y {:?}",
        bounds.center().y,
        pointer_world.y
    );
}

#[test]
fn active_search_provisional_drag_keeps_canvas_edit_input_disabled() {
    let ctx = egui::Context::default();
    let mut app = test_app(None);
    app.search.overlay.open = true;
    app.search.overlay.drag_state = Some(crate::search::palette::SearchResultDragState {
        asset_ids: vec![1, 2],
        outside_overlay: true,
        provisional_batch_active: true,
        auto_fit_applied: false,
    });

    assert_eq!(
        app.canvas_input_policy(&ctx),
        crate::canvas::CanvasInputPolicy::ViewportOnly
    );
}

#[test]
fn empty_external_hover_does_not_cancel_search_provisional_placement() {
    let mut app = test_app(None);
    crate::placement::lifecycle::start_provisional_batch(
        &mut app.canvas,
        vec![
            crate::placement::creation::ProvisionalSourceIntent::ExistingAsset {
                asset_id: 10,
                dimensions: (100, 100),
            },
        ],
        egui::pos2(0.0, 0.0),
        test_screen_rect(),
        crate::placement::creation::PlacementCreationSettings {
            placement_target_area: app.settings.placement_target_area,
            gap: app.settings.placement_gap,
        },
    );
    let mut harness = app_harness(app, |ctx, app| {
        egui::Area::new(egui::Id::new("external-hover-test-surface"))
            .fixed_pos(egui::Pos2::ZERO)
            .show(ctx, |ui| {
                ui.set_min_size(test_screen_rect().size());
                app.handle_hovered_files(ui, test_screen_rect());
            });
    });

    harness.hover_at(egui::pos2(900.0, 600.0));
    harness.step();

    assert_eq!(
        harness
            .state()
            .canvas
            .provisional_placements_in_selection_order()
            .len(),
        1
    );

    harness.step();

    assert_eq!(
        harness
            .state()
            .canvas
            .provisional_placements_in_selection_order()
            .len(),
        1
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
    harness.state_mut().document_persistence.flush().unwrap();
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
fn command_enter_places_all_unplaced_library_search_results_on_empty_active_board() {
    let root = TempRoot::new("snappyref-library-search-command-enter-unplaced");
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
            egui::pos2(20.0, 0.0),
            1.0,
        )
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

    harness.state_mut().document_persistence.flush().unwrap();
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
    assert_eq!(harness.state().canvas.selected_placement_ids().len(), 2);
    assert!(!harness.state().search.overlay.open);
}

#[test]
fn submitted_search_is_saved_to_recent_searches() {
    let root = TempRoot::new("snappyref-search-recent-session");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let mut app = test_app(Some(library.clone()));
    app.search.overlay.open = true;
    app.search.overlay.query = "brutalist chair".to_owned();
    app.search.overlay.last_query_edit_at = Some(Instant::now() - Duration::from_millis(200));
    let mut harness = app_harness(app, |ctx, app| {
        app.show_search_overlay(ctx, test_screen_rect());
    });

    harness.step();

    harness.state_mut().flush_persistence();

    assert_eq!(
        library.load_user_session().unwrap().recent_searches,
        vec!["brutalist chair".to_owned()]
    );
}

#[test]
fn empty_recent_searches_do_not_submit_boilerplate_query() {
    let root = TempRoot::new("snappyref-search-no-boilerplate-recent");
    let library = Library::initialize(root.to_path_buf()).unwrap();
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
fn completed_multi_file_import_batch_selects_and_compacts_loaded_geometry() {
    let ctx = egui::Context::default();
    let mut app = test_app(None);
    app.register_import_batch(app.active_board_id, egui::pos2(120.0, 80.0), vec![1, 2, 3]);

    for (id, width, height) in [(1_i64, 180_u32, 80_u32), (2, 70, 210), (3, 140, 120)] {
        let asset = Asset {
            id,
            original_path: PathBuf::from(format!("fixture-{id}.png")),
            filename: format!("fixture-{id}.png"),
            mime_type: "image/png".to_owned(),
            width,
            height,
        };
        let placement = Placement::loaded(
            PlacementState::new(id, eframe::egui::pos2(0.0, 0.0), 1.0, 0.0, id as i32, false),
            asset,
        );
        app.integrate_loaded_import(
            &ctx,
            PreparedPlacementImage {
                queue_id: Some(id as u64),
                preview_rgba: vec![255; width as usize * height as usize * 4],
                preview_width: width,
                preview_height: height,
                origin: crate::jobs::ImportOrigin::Path,
                ..loaded_record(placement)
            },
        );
    }

    assert_eq!(app.canvas.selected_placement_ids(), vec![1, 2, 3]);
    assert!([1_i64, 2, 3].iter().any(|placement_id| {
        let placement = app.canvas.placement_for_id(*placement_id).unwrap();
        placement.state.geometry.center.x.abs() > 0.001
            || placement.state.geometry.center.y.abs() > 0.001
    }));
    assert!(app.pending_import_batches.is_empty());
}

#[test]
fn completed_import_batch_compacts_loaded_items_after_out_of_order_failures() {
    let ctx = egui::Context::default();
    let mut app = test_app(None);
    let anchor = egui::pos2(-340.0, 210.0);
    app.register_import_batch(app.active_board_id, anchor, vec![1, 2, 3]);

    let mut third = test_loaded_item(3, app.active_board_id);
    third.placement.require_loaded_asset_mut().width = 140;
    third.placement.require_loaded_asset_mut().height = 80;
    third.placement.state.geometry.size = egui::vec2(140.0, 80.0);
    third.placement.state.geometry.center.x = 4000.0;
    third.placement.state.geometry.center.y = -1000.0;
    app.integrate_loaded_import(&ctx, third);

    app.finish_import_queue_id_in_batches(Some(app.active_board_id), Some(2), None);

    let mut first = test_loaded_item(1, app.active_board_id);
    first.placement.require_loaded_asset_mut().width = 90;
    first.placement.require_loaded_asset_mut().height = 170;
    first.placement.state.geometry.size = egui::vec2(90.0, 170.0);
    first.placement.state.geometry.center.x = -2500.0;
    first.placement.state.geometry.center.y = 3600.0;
    app.integrate_loaded_import(&ctx, first);

    assert_eq!(app.canvas.selected_placement_ids(), vec![3, 1]);
    let third_center = app
        .canvas
        .placement_for_id(3)
        .unwrap()
        .state
        .geometry
        .center;
    let first_center = app
        .canvas
        .placement_for_id(1)
        .unwrap()
        .state
        .geometry
        .center;
    assert_ne!(third_center, egui::pos2(4000.0, -1000.0));
    assert_ne!(first_center, egui::pos2(-2500.0, 3600.0));
    assert!(
        third_center.distance(first_center)
            < egui::pos2(4000.0, -1000.0).distance(egui::pos2(-2500.0, 3600.0))
    );
    assert!(app.pending_import_batches.is_empty());
}

#[test]
fn accepted_loading_batch_is_one_history_action_despite_out_of_order_results() {
    let ctx = egui::Context::default();
    let root = TempRoot::new("snappyref-accepted-loading-batch-history");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let mut app = test_app(Some(library.clone()));
    app.document_persistence = super::DocumentPersistence::queued(library.clone());
    app.import_worker = Some(crate::jobs::ImagePrepWorker::new(
        library.clone(),
        ctx.clone(),
        None,
    ));
    app.enqueue_import_sources(
        (1..=3)
            .map(|index| crate::jobs::ImportSource::Path(root.join(format!("missing-{index}.png"))))
            .collect(),
        test_screen_rect(),
        egui::pos2(120.0, 80.0),
    );
    let placement_ids = app.canvas.selected_placement_ids();
    assert_eq!(placement_ids.len(), 3);

    let mut loaded = persisted_loaded_item(&library, placement_ids[2], app.active_board_id);
    loaded.queue_id = Some(3);
    loaded.origin = crate::jobs::ImportOrigin::Path;
    let loaded_result = crate::jobs::ImagePrepResult::Loaded(loaded);
    assert!(!app.discard_stale_content_result(&loaded_result));
    let outcome = app.residency.process_import_result(
        &ctx,
        &mut app.canvas,
        app.import_worker.as_ref(),
        app.active_board_id,
        &app.settings,
        loaded_result,
    );
    app.apply_import_result_residency_outcome(&ctx, outcome);

    let failed_result = crate::jobs::ImagePrepResult::Failed {
        target_board_id: Some(app.active_board_id),
        queue_id: Some(1),
        placement_id: Some(placement_ids[0]),
        context: "missing-1.png".to_owned(),
        error: "decode failed".to_owned(),
    };
    assert!(!app.discard_stale_content_result(&failed_result));
    let outcome = app.residency.process_import_result(
        &ctx,
        &mut app.canvas,
        app.import_worker.as_ref(),
        app.active_board_id,
        &app.settings,
        failed_result,
    );
    app.apply_import_result_residency_outcome(&ctx, outcome);

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
    late_loaded.origin = crate::jobs::ImportOrigin::Path;
    let late_result = crate::jobs::ImagePrepResult::Loaded(late_loaded);
    if !app.discard_stale_content_result(&late_result) {
        let outcome = app.residency.process_import_result(
            &ctx,
            &mut app.canvas,
            app.import_worker.as_ref(),
            app.active_board_id,
            &app.settings,
            late_result,
        );
        app.apply_import_result_residency_outcome(&ctx, outcome);
    }
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
    let root = TempRoot::new("snappyref-import-reservation");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let second = library.create_board("Second").unwrap();
    let mut app = test_app(Some(library));
    app.active_board_id = second.id;

    let known = app
        .reserve_import(
            Some((100, 80)),
            egui::Pos2::ZERO,
            crate::data::ContentResolutionSource::Unknown {
                description: "known test import".to_owned(),
            },
        )
        .unwrap();
    let unknown = app
        .reserve_import(
            None,
            egui::pos2(200.0, 0.0),
            crate::data::ContentResolutionSource::Unknown {
                description: "unknown test import".to_owned(),
            },
        )
        .unwrap();

    assert_eq!(known.target_board_id, second.id);
    assert_eq!(known.queue_id, 1);
    assert!(known.placement_id.is_some());
    assert_eq!(known.reserved_world, Some(egui::Pos2::ZERO));
    assert_eq!(unknown.target_board_id, second.id);
    assert_eq!(unknown.queue_id, 2);
    assert!(unknown.placement_id.is_some());
    assert_eq!(unknown.reserved_world, Some(egui::pos2(200.0, 0.0)));
}

#[test]
fn failed_import_fails_the_accepted_placement_before_it_is_durable() {
    let root = TempRoot::new("snappyref-failed-import-accepted");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let mut app = test_app(Some(library.clone()));
    app.document_persistence = super::DocumentPersistence::queued(library.clone());
    let reservation = app
        .reserve_import(
            Some((100, 80)),
            egui::Pos2::ZERO,
            crate::data::ContentResolutionSource::Unknown {
                description: "doomed import".to_owned(),
            },
        )
        .unwrap();
    let placement_id = reservation.placement_id.unwrap();

    // The creation unit may still be queued; failure handling must not
    // depend on the row existing.
    app.apply_failed_import_outcome(FailedContentResolutionOutcome {
        resolution_ticket: Some(ContentResolutionTicket {
            target_board_id: reservation.target_board_id,
            queue_id: reservation.queue_id,
        }),
        placement_id: Some(placement_id),
        target_board_id: None,
        status: "decode failed".to_owned(),
    });

    let record = app.canvas.placement_for_id(placement_id).unwrap();
    assert!(
        matches!(record.content, crate::data::PlacementContent::Failed { .. }),
        "{:?}",
        record.content
    );
    assert_eq!(app.canvas.selected_placement_ids(), vec![placement_id]);
    assert_eq!(app.status, "decode failed");

    app.flush_persistence();
    let stored = library
        .load_placements()
        .unwrap()
        .into_iter()
        .find(|placement| placement.state.id == placement_id)
        .unwrap();
    assert!(matches!(
        stored.content,
        crate::data::PlacementContent::Failed { .. }
    ));
}

#[test]
fn completed_import_for_inactive_board_does_not_enter_active_canvas() {
    let ctx = egui::Context::default();
    let root = TempRoot::new("snappyref-inactive-import-completion");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let second = library.create_board("Second").unwrap();
    let mut app = test_app(Some(library));
    app.active_board_id = 1;
    let loaded = test_loaded_item(41, second.id);

    app.integrate_loaded_import(&ctx, loaded);

    assert!(app.canvas.placement_for_id(41).is_none());
    assert_eq!(app.status, "Imported image");
}

#[test]
fn accepted_import_for_inactive_board_keeps_history_on_target_board() {
    let ctx = egui::Context::default();
    let root = TempRoot::new("snappyref-inactive-import-history");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let first = library.list_boards().unwrap().remove(0);
    let second = library.create_board("Second").unwrap();
    let mut app = test_app(Some(library.clone()));
    app.document_persistence = super::DocumentPersistence::queued(library.clone());
    let reservation = app
        .reserve_import(
            Some((1, 1)),
            egui::Pos2::ZERO,
            crate::data::ContentResolutionSource::Unknown {
                description: "inactive Board import".to_owned(),
            },
        )
        .unwrap();
    let placement_id = reservation.placement_id.unwrap();
    app.switch_to_board(second.id);

    let mut loaded = persisted_loaded_item(&library, placement_id, first.id);
    loaded.queue_id = Some(reservation.queue_id);
    loaded.origin = crate::jobs::ImportOrigin::Path;
    app.integrate_loaded_import(&ctx, loaded);

    assert!(app.canvas.placement_for_id(placement_id).is_none());
    app.switch_to_board(first.id);
    app.undo_document_history(&ctx);
    app.flush_persistence();

    assert!(
        library
            .load_placements_for_board(first.id)
            .unwrap()
            .is_empty()
    );
    assert_eq!(app.status, "Undid Import image");

    app.redo_document_history(&ctx);
    app.flush_persistence();

    assert_eq!(
        library.load_placements_for_board(first.id).unwrap().len(),
        1
    );
    assert_eq!(app.status, "Redid Import image");
}

#[test]
fn delayed_image_load_preserves_geometry_edited_after_request() {
    let ctx = egui::Context::default();
    let mut app = test_app(None);
    let mut initial = test_loaded_item(42, app.active_board_id);
    initial.queue_id = None;
    initial.origin = crate::jobs::ImportOrigin::Existing;
    app.integrate_loaded_import(&ctx, initial);

    // The worker captured this placement before the user moved and resized it.
    let mut delayed = test_loaded_item(42, app.active_board_id);
    delayed.queue_id = None;
    delayed.origin = crate::jobs::ImportOrigin::Existing;
    delayed.loaded_lod = crate::data::CanvasImageLod::Detail;
    app.canvas.select_placements(&[42]);
    assert_eq!(
        app.canvas.move_selected_placement_group_to_center(
            egui::pos2(120.0, 80.0),
            test_viewport_world(),
            0.0,
        ),
        1
    );
    assert_eq!(app.canvas.apply_target_area_to_selected_placements(4.0), 1);
    let accepted_geometry = app.canvas.placement_for_id(42).unwrap().state.geometry;
    assert_ne!(accepted_geometry, delayed.placement.state.geometry);

    app.integrate_loaded_import(&ctx, delayed);

    assert_eq!(
        app.canvas.placement_for_id(42).unwrap().state.geometry,
        accepted_geometry,
        "a delayed image load must not overwrite newer canvas geometry"
    );
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
fn url_batch_previews_use_balanced_layout_for_approximate_dimensions() {
    let mut app = test_app(None);
    let sources = (0..13)
        .map(|index| {
            crate::jobs::ImportSource::Url(format!("https://example.com/image-{index}.png"))
        })
        .collect::<Vec<_>>();

    let previews =
        app.dropped_import_previews(&sources, egui::pos2(100.0, 200.0), test_screen_rect());
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
    let root = TempRoot::new("snappyref-raw-file-drop");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let source_path = library
        .store_rgba_image("drop-source", &[255, 0, 0, 255], 1, 1)
        .unwrap();
    let mut app = test_app(Some(library.clone()));
    app.document_persistence = super::DocumentPersistence::queued(library.clone());
    app.import_worker = Some(crate::jobs::ImagePrepWorker::new(
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
    assert_eq!(
        harness
            .state()
            .canvas
            .provisional_placements_in_selection_order()
            .len(),
        1
    );

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

    assert!(
        harness
            .state()
            .canvas
            .provisional_placements_in_selection_order()
            .is_empty()
    );
    assert_eq!(harness.state().content_resolution.pending_len(), 1);
    harness.state_mut().flush_persistence();
    let items = library.load_placements().unwrap();
    assert_eq!(items.len(), 1);
    let screen_rect = test_screen_rect();
    let placement = &items[0];
    let expected_world = harness
        .state()
        .canvas
        .screen_to_world(drop_pos, screen_rect);
    assert!(
        egui::pos2(
            placement.state.geometry.center.x,
            placement.state.geometry.center.y
        )
        .distance(expected_world)
            < 1.0
    );

    harness.state_mut().undo_document_history(&ctx);
    assert_eq!(harness.state().status, "Undid Import image");
    assert!(
        harness
            .state()
            .canvas
            .placement_for_id(placement.state.id)
            .is_none()
    );
}

#[test]
fn inspect_pending_drop_batch_undo_writes_artifact() {
    let mut scenario = InspectScenario::new("pending-drop-batch-undo");
    scenario.event(
        "seed",
        serde_json::json!({
            "seed": "two-local-png-files-and-controlled-image-preparation",
            "purpose": "drop one committed batch, Undo while both Placements are Loading, then drain late results",
        }),
    );

    let root = TempRoot::new("snappyref-inspect-pending-drop-batch-undo");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let sources = [
        (
            library
                .store_rgba_image(
                    "pending-drop-blue",
                    &solid_rgba([78, 126, 226, 255], 160, 100),
                    160,
                    100,
                )
                .unwrap(),
            [78, 126, 226, 255],
            160,
            100,
        ),
        (
            library
                .store_rgba_image(
                    "pending-drop-gold",
                    &solid_rgba([224, 168, 72, 255], 100, 140),
                    100,
                    140,
                )
                .unwrap(),
            [224, 168, 72, 255],
            100,
            140,
        ),
    ];
    let (worker, result_tx) = crate::jobs::ImagePrepWorker::controlled();
    let mut app = test_app(Some(library.clone()));
    app.document_persistence = super::DocumentPersistence::queued(library.clone());
    app.import_worker = Some(worker);
    let mut harness = app_full_harness(app);
    let drop_pos = egui::pos2(900.0, 600.0);
    scenario.target(
        "canvas drop point",
        "canvas",
        egui::Rect::from_center_size(drop_pos, egui::vec2(24.0, 24.0)),
    );
    scenario.capture_screen("screen-000-before-drop", &mut harness);

    scenario.action("hover two files over canvas", drop_pos);
    harness.hover_at(drop_pos);
    harness.input_mut().hovered_files = sources
        .iter()
        .map(|(path, _, _, _)| egui::HoveredFile {
            path: Some(path.clone()),
            ..Default::default()
        })
        .collect();
    harness.step();
    scenario.fact(
        "drop.provisional",
        serde_json::json!({
            "count": harness.state().canvas.provisional_placements_in_selection_order().len(),
        }),
    );
    scenario.assertion(
        "hover creates two provisional Placements",
        harness
            .state()
            .canvas
            .provisional_placements_in_selection_order()
            .len()
            == 2,
    );
    scenario.capture_screen("screen-001-hover-provisional", &mut harness);

    scenario.action("drop two files on canvas", drop_pos);
    harness.input_mut().hovered_files.clear();
    harness.input_mut().dropped_files = sources
        .iter()
        .map(|(path, _, _, _)| egui::DroppedFile {
            path: Some(path.clone()),
            name: path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("dropped-image.png")
                .to_owned(),
            ..Default::default()
        })
        .collect();
    harness.step();
    let placement_ids = harness.state().canvas.selected_placement_ids();
    let worker_stats = harness.state().import_worker.as_ref().unwrap().stats();
    scenario.fact(
        "drop.accepted",
        serde_json::json!({
            "placement_ids": placement_ids,
            "pending": harness.state().content_resolution.pending_len(),
            "queued_worker_requests": worker_stats.queued_startup
                + worker_stats.queued_interactive
                + worker_stats.queued_background,
        }),
    );
    scenario.assertion(
        "drop accepts one two-Placement Loading batch",
        placement_ids.len() == 2 && harness.state().content_resolution.pending_len() == 2,
    );
    scenario.capture_screen("screen-002-accepted-loading-batch", &mut harness);

    let loaded_placement = library
        .persist_imported_image_resolving_placement(
            harness.state().active_board_id,
            Some(placement_ids[0]),
            &sources[0].0,
            "image/png",
            sources[0].2,
            sources[0].3,
            std::fs::metadata(&sources[0].0).unwrap().len(),
            harness
                .state()
                .canvas
                .placement_for_id(placement_ids[0])
                .unwrap()
                .state
                .geometry
                .center,
            1.0,
        )
        .unwrap();
    let late_loaded = inspect_prepared_loaded_placement(
        loaded_placement,
        sources[0].1,
        sources[0].2,
        sources[0].3,
        Some(1),
        crate::jobs::ImportOrigin::Path,
    );

    let undo_modifiers = egui::Modifiers {
        command: true,
        ..Default::default()
    };
    scenario.event(
        "action",
        serde_json::json!({"name": "press Command-Z while both preparations are pending"}),
    );
    for pressed in [true, false] {
        harness.event_modifiers(
            egui::Event::Key {
                key: egui::Key::Z,
                physical_key: Some(egui::Key::Z),
                pressed,
                repeat: false,
                modifiers: undo_modifiers,
            },
            undo_modifiers,
        );
    }
    harness.step();
    scenario.fact(
        "drop.after-undo",
        serde_json::json!({
            "present_count": placement_ids
                .iter()
                .filter(|id| harness.state().canvas.placement_for_id(**id).is_some())
                .count(),
            "pending": harness.state().content_resolution.pending_len(),
            "status": harness.state().status,
        }),
    );
    scenario.assertion(
        "one Undo removes the whole accepted batch while work remains pending",
        placement_ids
            .iter()
            .all(|id| harness.state().canvas.placement_for_id(*id).is_none())
            && harness.state().content_resolution.pending_len() == 2
            && harness.state().status == "Undid Import images",
    );
    scenario.capture_screen("screen-003-undone-while-pending", &mut harness);

    result_tx
        .send(crate::jobs::ImagePrepResult::Loaded(late_loaded))
        .unwrap();
    result_tx
        .send(crate::jobs::ImagePrepResult::Failed {
            target_board_id: Some(harness.state().active_board_id),
            queue_id: Some(2),
            placement_id: Some(placement_ids[1]),
            context: sources[1].0.display().to_string(),
            error: "controlled late decode failure".to_owned(),
        })
        .unwrap();
    scenario.event(
        "wait",
        serde_json::json!({"name": "drain late Loaded and Failed worker results"}),
    );
    harness.step();
    scenario.fact(
        "drop.after-late-results",
        serde_json::json!({
            "present_count": placement_ids
                .iter()
                .filter(|id| harness.state().canvas.placement_for_id(**id).is_some())
                .count(),
            "pending": harness.state().content_resolution.pending_len(),
            "status": harness.state().status,
        }),
    );
    scenario.assertion(
        "late results drain without restoring the undone batch",
        placement_ids
            .iter()
            .all(|id| harness.state().canvas.placement_for_id(*id).is_none())
            && harness.state().content_resolution.pending_len() == 0
            && harness.state().status == "Undid Import images",
    );
    scenario.capture_screen("screen-004-late-results-discarded", &mut harness);

    harness.state_mut().flush_persistence();
    scenario.assertion(
        "the undone batch stays absent from durable Board state",
        library.load_placements().unwrap().is_empty(),
    );

    let report = scenario.finish();
    assert!(
        report.passed,
        "inspect scenario failed; artifact={}",
        report.artifact_dir.display()
    );
}

#[test]
fn missing_drop_event_fallback_is_single_use_after_hover_ends() {
    let mut app = test_app(None);
    let hover_pos = egui::pos2(320.0, 240.0);
    let source = crate::jobs::ImportSource::Path(std::path::PathBuf::from(
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
    let mut app = SnappyRefApp::detached(None, 8192, super::UpdateConfig::for_tests());
    if let Some(library) = &library {
        app.document_persistence = super::DocumentPersistence::queued(library.clone());
    }
    app.library = library;
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

fn app_full_harness(app: SnappyRefApp) -> Harness<'static, SnappyRefApp> {
    app_full_harness_with_step_dt(app, 1.0 / 4.0)
}

fn app_full_harness_with_step_dt(
    app: SnappyRefApp,
    step_dt: f32,
) -> Harness<'static, SnappyRefApp> {
    app_full_harness_with_startup(step_dt, move |_| app)
}

/// Builds the app inside the eframe creation context, before the first frame,
/// like `SnappyRefApp::new`. Use it when startup work (worker creation,
/// session load) must precede the first frame's persistence.
fn app_full_harness_with_startup(
    step_dt: f32,
    startup: impl FnOnce(&egui::Context) -> SnappyRefApp + 'static,
) -> Harness<'static, SnappyRefApp> {
    Harness::builder()
        .with_size(egui::vec2(1200.0, 800.0))
        .with_pixels_per_point(1.0)
        .with_os(egui::os::OperatingSystem::Nix)
        .with_theme(egui::Theme::Dark)
        .with_max_steps(4)
        .with_step_dt(step_dt)
        .build_eframe(move |cc| {
            crate::ui::theme::configure_theme(&cc.egui_ctx);
            startup(&cc.egui_ctx)
        })
}

fn drag_test_pos<State>(harness: &Harness<'_, State>, from: egui::Pos2, to: egui::Pos2) {
    harness.hover_at(from);
    harness.drag_at(from);
    harness.hover_at(to);
    harness.drop_at(to);
}

struct InspectScenario {
    name: String,
    artifact_dir: PathBuf,
    events: Vec<serde_json::Value>,
    assertions: Vec<serde_json::Value>,
    frames: Vec<String>,
    passed: bool,
}

struct InspectReport {
    passed: bool,
    artifact_dir: PathBuf,
}

struct InspectFrameStats {
    width: u32,
    height: u32,
    transparent_pixels: usize,
    unique_colors: usize,
}

struct InspectProgressConfig {
    start_frame_index: usize,
    timeout: Duration,
    poll_interval: Duration,
    fact_name: &'static str,
    timeout_fact_name: &'static str,
    terminal_assertion: &'static str,
    timeout_assertion: &'static str,
}

struct InspectImportSample {
    name: &'static str,
    width: u32,
    height: u32,
    color: [u8; 4],
    center: egui::Pos2,
}

struct InspectSampleImport {
    name: &'static str,
    width: u32,
    height: u32,
    color: [u8; 4],
    reservation: crate::placement::content_resolution::ContentResolutionReservation,
    loading_state: PlacementState,
}

impl InspectScenario {
    fn new(name: impl Into<String>) -> Self {
        let name = name.into();
        let artifact_dir = inspect_artifact_dir(&name);
        Self {
            name,
            artifact_dir,
            events: Vec::new(),
            assertions: Vec::new(),
            frames: Vec::new(),
            passed: true,
        }
    }

    fn action(&mut self, name: &'static str, screen_pos: egui::Pos2) {
        self.event(
            "action",
            serde_json::json!({
                "name": name,
                "screen": inspect_pos(screen_pos),
            }),
        );
    }

    fn target(&mut self, name: &'static str, kind: &'static str, rect: egui::Rect) {
        self.event(
            "observation.target",
            serde_json::json!({
                "name": name,
                "kind": kind,
                "rect": inspect_rect(rect),
            }),
        );
    }

    fn fact(&mut self, name: &'static str, value: serde_json::Value) {
        self.event(
            "observation.fact",
            serde_json::json!({
                "name": name,
                "value": value,
            }),
        );
    }

    fn capture_click_hold_fact(
        &mut self,
        frame_name: &str,
        harness: &Harness<'_, SnappyRefApp>,
        placement_id: i64,
    ) {
        let now_seconds = harness.ctx.input(|input| input.time);
        let placement = harness.state().canvas.placement_for_id(placement_id);
        self.fact(
            "placement-click-hold.frame-state",
            serde_json::json!({
                "frame": frame_name,
                "time_seconds": now_seconds,
                "selected_placement_ids": harness.state().canvas.selected_placement_ids(),
                "placement_center": placement
                    .as_ref()
                    .map(|placement| inspect_pos(placement.state.geometry.center)),
                "selection_border_visible": harness
                    .state()
                    .canvas
                    .selection_border_visible_for_test(now_seconds),
            }),
        );
    }

    fn assertion(&mut self, name: &'static str, passed: bool) {
        self.passed &= passed;
        let event = serde_json::json!({
            "type": "assertion",
            "name": name,
            "passed": passed,
        });
        self.assertions.push(event.clone());
        self.events.push(event);
    }

    fn event(&mut self, event_type: &'static str, payload: serde_json::Value) {
        self.events.push(serde_json::json!({
            "type": event_type,
            "payload": payload,
        }));
    }

    fn capture_board_state_frame(
        &mut self,
        name: &'static str,
        placement: &Placement,
        pointer_screen: egui::Pos2,
    ) {
        let frames_dir = self.artifact_dir.join("frames");
        fs::create_dir_all(&frames_dir).unwrap();
        let path = frames_dir.join(format!("{name}.png"));
        let mut image = image::RgbaImage::from_pixel(
            1200,
            800,
            image::Rgba([
                crate::ui::theme::BACKGROUND.r(),
                crate::ui::theme::BACKGROUND.g(),
                crate::ui::theme::BACKGROUND.b(),
                255,
            ]),
        );
        let center = pointer_screen;
        let size = placement.state.geometry.scaled_size();
        let min_x = (center.x - size.x / 2.0).round().max(0.0) as u32;
        let min_y = (center.y - size.y / 2.0).round().max(0.0) as u32;
        let max_x = (center.x + size.x / 2.0).round().min(1199.0) as u32;
        let max_y = (center.y + size.y / 2.0).round().min(799.0) as u32;
        for y in min_y..=max_y {
            for x in min_x..=max_x {
                let border = x == min_x || x == max_x || y == min_y || y == max_y;
                let color = if border {
                    image::Rgba([235, 238, 245, 255])
                } else {
                    image::Rgba([255, 64, 64, 255])
                };
                image.put_pixel(x, y, color);
            }
        }
        image.save(&path).expect("inspect frame should save");
        self.frames.push(path.display().to_string());
        self.event(
            "frame",
            serde_json::json!({
                "name": name,
                "path": path,
                "source": "board-state-render",
            }),
        );
    }

    fn capture_screen(
        &mut self,
        name: &str,
        harness: &mut Harness<'_, SnappyRefApp>,
    ) -> InspectFrameStats {
        let raw = harness
            .render()
            .expect("inspect screen frame should render");
        let mut transparent_pixels = 0usize;
        let mut unique_colors = std::collections::HashSet::new();
        let mut image = image::RgbaImage::new(raw.width(), raw.height());
        let background = image::Rgba([
            crate::ui::theme::BACKGROUND.r(),
            crate::ui::theme::BACKGROUND.g(),
            crate::ui::theme::BACKGROUND.b(),
            255,
        ]);

        for (x, y, pixel) in raw.enumerate_pixels() {
            if pixel.0[3] < 255 {
                transparent_pixels += 1;
            }
            let composited = composite_pixel_over_background(*pixel, background);
            if unique_colors.len() <= 256 {
                unique_colors.insert(composited.0);
            }
            image.put_pixel(x, y, composited);
        }

        let frames_dir = self.artifact_dir.join("frames");
        fs::create_dir_all(&frames_dir).unwrap();
        let path = frames_dir.join(format!("{name}.png"));
        image.save(&path).expect("inspect frame should save");
        let stats = InspectFrameStats {
            width: image.width(),
            height: image.height(),
            transparent_pixels,
            unique_colors: unique_colors.len(),
        };
        self.frames.push(path.display().to_string());
        self.event(
            "frame",
            serde_json::json!({
                "name": name,
                "path": path,
                "source": "egui-kittest-full-app-render",
                "width": stats.width,
                "height": stats.height,
                "transparent_pixels": stats.transparent_pixels,
                "unique_colors_sample": stats.unique_colors,
            }),
        );
        stats
    }

    fn drive_progress<Status>(
        &mut self,
        harness: &mut Harness<'_, SnappyRefApp>,
        config: InspectProgressConfig,
        mut sample_status: impl FnMut(&SnappyRefApp) -> Status,
        mut status_slug: impl FnMut(&Status) -> String,
        mut is_terminal: impl FnMut(&Status) -> bool,
        mut is_success: impl FnMut(&Status) -> bool,
    ) -> Status
    where
        Status: Clone + PartialEq + std::fmt::Debug,
    {
        let deadline = Instant::now() + config.timeout;
        let mut last_status = sample_status(harness.state());
        let mut frame_index = config.start_frame_index;

        loop {
            harness.step();
            let status = sample_status(harness.state());
            if status != last_status {
                self.fact(
                    config.fact_name,
                    serde_json::json!({
                        "status": format!("{status:?}"),
                    }),
                );
                self.capture_screen(
                    &format!("screen-{frame_index:03}-{}", status_slug(&status)),
                    harness,
                );
                frame_index += 1;
                last_status = status.clone();
            }

            if is_terminal(&status) {
                self.assertion(config.terminal_assertion, is_success(&status));
                return status;
            }

            if Instant::now() >= deadline {
                self.fact(
                    config.timeout_fact_name,
                    serde_json::json!({
                        "status": format!("{status:?}"),
                    }),
                );
                self.capture_screen("screen-timeout", harness);
                self.assertion(config.timeout_assertion, false);
                return status;
            }

            std::thread::sleep(config.poll_interval);
        }
    }

    fn drive_import_lifecycle(
        &mut self,
        harness: &mut Harness<'_, SnappyRefApp>,
        samples: &[InspectImportSample],
    ) {
        self.capture_screen("screen-000-before-import", harness);

        let mut imports = Vec::new();
        for sample in samples {
            let reservation = harness
                .state_mut()
                .reserve_import(
                    Some((sample.width, sample.height)),
                    sample.center,
                    crate::data::ContentResolutionSource::Unknown {
                        description: sample.name.to_owned(),
                    },
                )
                .unwrap();
            let placement_id = reservation
                .placement_id
                .expect("sample import should create loading placement");
            let loading_state = harness
                .state()
                .canvas
                .placement_for_id(placement_id)
                .expect("loading placement should be on canvas")
                .state
                .clone();
            imports.push(InspectSampleImport {
                name: sample.name,
                width: sample.width,
                height: sample.height,
                color: sample.color,
                reservation,
                loading_state,
            });
        }

        harness.state_mut().status = "Queued sample image imports".to_owned();
        self.fact(
            "sample-import.loading",
            serde_json::json!({
                "count": imports.len(),
                "pending": harness.state().content_resolution.pending_len(),
            }),
        );
        harness.step();
        self.capture_screen("screen-001-during-import-loading", harness);
        self.assertion(
            "loading placeholders do not count as painted images",
            harness.state().frame_perf.canvas.image_pass.painted_images == 0,
        );

        let ctx = harness.ctx.clone();
        let active_board_id = harness.state().active_board_id;
        for import in &imports {
            let loaded = inspect_sample_loaded_item(
                import.name,
                import.width,
                import.height,
                import.color,
                import.reservation.queue_id,
                active_board_id,
                import.loading_state.clone(),
            );
            harness.state_mut().integrate_loaded_import(&ctx, loaded);
        }
        self.fact(
            "sample-import.loaded",
            serde_json::json!({
                "count": imports.len(),
                "pending": harness.state().content_resolution.pending_len(),
            }),
        );
        harness.step();
        self.capture_screen("screen-002-after-import-loaded", harness);
        self.assertion(
            "loaded sample textures count as painted images",
            harness.state().frame_perf.canvas.image_pass.painted_images == samples.len(),
        );

        self.assertion(
            "sample imports created loading placements",
            imports.len() == samples.len()
                && harness.state().canvas.all_placement_ids().len() == samples.len(),
        );
        self.assertion(
            "sample imports finished loading",
            harness.state().content_resolution.pending_len() == 0,
        );
    }

    fn finish(self) -> InspectReport {
        let artifact_dir = self.artifact_dir;
        fs::create_dir_all(&artifact_dir).unwrap();

        let events_path = artifact_dir.join("events.ndjson");
        let mut events_file = fs::File::create(&events_path).unwrap();
        for event in &self.events {
            writeln!(events_file, "{event}").unwrap();
        }

        let first_failure = self
            .assertions
            .iter()
            .find(|assertion| assertion["passed"] == false)
            .and_then(|assertion| assertion["name"].as_str())
            .map(str::to_owned);
        let report = serde_json::json!({
            "scenario": self.name,
            "status": if self.passed { "pass" } else { "fail" },
            "artifact_dir": artifact_dir,
            "first_failure": first_failure,
            "assertions": self.assertions,
            "frames": self.frames,
        });
        fs::write(
            artifact_dir.join("report.json"),
            serde_json::to_string_pretty(&report).unwrap(),
        )
        .unwrap();
        fs::write(
            artifact_dir
                .parent()
                .expect("scenario artifact dir should have parent")
                .join("latest.txt"),
            artifact_dir.display().to_string(),
        )
        .unwrap();

        InspectReport {
            passed: self.passed,
            artifact_dir,
        }
    }
}

fn inspect_artifact_dir(scenario: &str) -> PathBuf {
    let target_dir = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target"));
    let target_dir = if target_dir.is_absolute() {
        target_dir
    } else {
        std::env::current_dir().unwrap().join(target_dir)
    };
    target_dir
        .join("snappyref-inspect")
        .join(scenario)
        .join(format!(
            "run-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
}

fn inspect_pos(pos: egui::Pos2) -> serde_json::Value {
    serde_json::json!({
        "x": pos.x,
        "y": pos.y,
    })
}

fn inspect_vec2(vec: egui::Vec2) -> serde_json::Value {
    serde_json::json!({
        "x": vec.x,
        "y": vec.y,
    })
}

fn inspect_rect(rect: egui::Rect) -> serde_json::Value {
    serde_json::json!({
        "min": inspect_pos(rect.min),
        "max": inspect_pos(rect.max),
        "center": inspect_pos(rect.center()),
        "size": inspect_vec2(rect.size()),
    })
}

fn inspect_placement(placement: &Placement) -> serde_json::Value {
    serde_json::json!({
        "id": placement.state.id,
        "asset_id": placement.asset_id(),
        "center": inspect_pos(placement.state.geometry.center),
        "size": inspect_vec2(placement.state.geometry.scaled_size()),
        "scale": placement.state.geometry.scale,
    })
}

fn inspect_status_slug(status: &LocalModelStoreStatus) -> String {
    match status {
        LocalModelStoreStatus::Missing => "missing".to_owned(),
        LocalModelStoreStatus::Downloading { percent } => format!("downloading-{percent:03}"),
        LocalModelStoreStatus::Installed => "installed".to_owned(),
        LocalModelStoreStatus::InstalledViaMigration => "installed-via-migration".to_owned(),
        LocalModelStoreStatus::Failed(_) => "failed".to_owned(),
    }
}

fn inspect_sample_loaded_item(
    name: &str,
    width: u32,
    height: u32,
    color: [u8; 4],
    queue_id: u64,
    target_board_id: i64,
    state: PlacementState,
) -> PreparedPlacementImage {
    let asset_id = state.id;
    let asset = Asset {
        id: asset_id,
        original_path: PathBuf::from(name),
        filename: name.to_owned(),
        mime_type: "image/png".to_owned(),
        width,
        height,
    };
    let pixel_count = width as usize * height as usize;
    PreparedPlacementImage {
        target_board_id,
        queue_id: Some(queue_id),
        preview_rgba: std::iter::repeat_n(color, pixel_count).flatten().collect(),
        preview_width: width,
        preview_height: height,
        origin: crate::jobs::ImportOrigin::Path,
        ..loaded_record(Placement::loaded(state, asset))
    }
}

fn inspect_prepared_loaded_placement(
    placement: Placement,
    color: [u8; 4],
    preview_width: u32,
    preview_height: u32,
    queue_id: Option<u64>,
    origin: crate::jobs::ImportOrigin,
) -> PreparedPlacementImage {
    PreparedPlacementImage {
        queue_id,
        preview_rgba: solid_rgba(color, preview_width, preview_height),
        preview_width,
        preview_height,
        origin,
        ..loaded_record(placement)
    }
}

fn inspect_search_result_entry(placement: &Placement) -> SearchResultAssetEntry {
    let asset = placement.require_loaded_asset();
    SearchResultAssetEntry {
        asset_id: asset.id,
        asset_preview_path: Some(asset.original_path.clone()),
        asset_width: asset.width,
        asset_height: asset.height,
    }
}

fn inspect_search_result_tile_center(index: usize, result_count: usize) -> egui::Pos2 {
    let body_h_pad = 12.0_f32;
    let grid_inner_width = PALETTE_WIDTH - body_h_pad * 2.0;
    let grid_layout = search_result_grid_layout(grid_inner_width, result_count);
    let columns = grid_layout.columns.max(1);
    let row = index / columns;
    let column = index % columns;
    let gap = crate::ui::theme::SPACE_2;
    let overlay_left = test_screen_rect().center().x - PALETTE_WIDTH / 2.0;
    let grid_left = overlay_left + body_h_pad;
    let chrome_height = SEARCH_OVERLAY_TOP_MARGIN + 53.0;
    let grid_top = test_screen_rect().top() + chrome_height + body_h_pad;
    egui::pos2(
        grid_left + column as f32 * (grid_layout.tile_width + gap) + grid_layout.tile_width / 2.0,
        grid_top
            + row as f32 * (search_result_tile_height() + gap)
            + search_result_tile_height() / 2.0,
    )
}

fn inspect_world_to_screen(world: egui::Pos2, rect: egui::Rect) -> egui::Pos2 {
    rect.center() + world.to_vec2()
}

fn inspect_click_at(
    harness: &Harness<'_, SnappyRefApp>,
    pos: egui::Pos2,
    modifiers: egui::Modifiers,
) {
    harness.event_modifiers(
        egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers,
        },
        modifiers,
    );
    harness.event_modifiers(
        egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers,
        },
        modifiers,
    );
}

fn inspect_bright_red_pixels_in_rect(path: &PathBuf, rect: egui::Rect) -> usize {
    let image = image::open(path)
        .expect("inspect frame should exist")
        .to_rgba8();
    let left = rect.left().floor().max(0.0) as u32;
    let top = rect.top().floor().max(0.0) as u32;
    let right = rect.right().ceil().min(image.width() as f32) as u32;
    let bottom = rect.bottom().ceil().min(image.height() as f32) as u32;
    let mut count = 0;
    for y in top..bottom {
        for x in left..right {
            let [r, g, b, a] = image.get_pixel(x, y).0;
            if r >= 240 && g <= 40 && b <= 40 && a >= 200 {
                count += 1;
            }
        }
    }
    count
}

fn inspect_command_modifier() -> egui::Modifiers {
    egui::Modifiers {
        command: true,
        ..Default::default()
    }
}

fn compact_selection_fact(app: &SnappyRefApp) -> serde_json::Value {
    let mut centers = app
        .canvas
        .selected_placement_ids()
        .into_iter()
        .filter_map(|id| {
            app.canvas
                .placement_for_id(id)
                .map(|placement| (id, placement.state.geometry.center))
        })
        .collect::<Vec<_>>();
    centers.sort_by(|left, right| left.0.cmp(&right.0));
    let mut xs = centers
        .iter()
        .map(|(_, center)| center.x)
        .collect::<Vec<_>>();
    let mut ys = centers
        .iter()
        .map(|(_, center)| center.y)
        .collect::<Vec<_>>();
    xs.sort_by(|left, right| left.total_cmp(right));
    ys.sort_by(|left, right| left.total_cmp(right));
    let column_count = compact_axis_bucket_count(xs);
    let row_count = compact_axis_bucket_count(ys);
    let max_column_count = compact_axis_max_bucket_size(
        centers
            .iter()
            .map(|(_, center)| center.x)
            .collect::<Vec<_>>(),
    );
    let bounds = app.canvas.selected_bounds_world();
    let bounds_aspect = bounds
        .map(|bounds| bounds.width().max(bounds.height()) / bounds.width().min(bounds.height()))
        .unwrap_or(1.0);
    serde_json::json!({
        "selected_count": centers.len(),
        "column_count": column_count,
        "row_count": row_count,
        "max_column_count": max_column_count,
        "bounds_aspect": bounds_aspect,
        "bounds_world": bounds.map(inspect_rect),
        "centers": centers
            .iter()
            .map(|(id, center)| serde_json::json!({
                "id": id,
                "center": inspect_pos(*center),
            }))
            .collect::<Vec<_>>(),
    })
}

fn compact_axis_bucket_count(mut values: Vec<f32>) -> usize {
    let mut buckets = 0usize;
    let mut last = None::<f32>;
    for value in values.drain(..) {
        if last.is_none_or(|previous| (value - previous).abs() > 1.0) {
            buckets += 1;
            last = Some(value);
        }
    }
    buckets
}

fn compact_axis_max_bucket_size(mut values: Vec<f32>) -> usize {
    values.sort_by(|left, right| left.total_cmp(right));
    let mut max_bucket = 0usize;
    let mut current_bucket = 0usize;
    let mut last = None::<f32>;
    for value in values {
        if last.is_none_or(|previous| (value - previous).abs() > 1.0) {
            max_bucket = max_bucket.max(current_bucket);
            current_bucket = 1;
            last = Some(value);
        } else {
            current_bucket += 1;
        }
    }
    max_bucket.max(current_bucket)
}

fn add_inspect_compact_image(
    harness: &mut Harness<'_, SnappyRefApp>,
    ctx: &egui::Context,
    id: i64,
    width: u32,
    height: u32,
    color: [u8; 4],
    center: egui::Pos2,
) {
    let asset = Asset {
        id,
        original_path: PathBuf::from(format!("compact-fixture-{id}.png")),
        filename: format!("compact-fixture-{id}.png"),
        mime_type: "image/png".to_owned(),
        width,
        height,
    };
    let placement = Placement::loaded(
        PlacementState::with_display_size(
            id,
            center,
            egui::vec2(width as f32, height as f32),
            1.0,
            0.0,
            id as i32,
            false,
        ),
        asset,
    );
    harness.state_mut().canvas.add_prepared_placement_image(
        ctx,
        PreparedPlacementImage {
            preview_rgba: solid_rgba(color, width, height),
            preview_width: width,
            preview_height: height,
            ..loaded_record(placement)
        },
    );
}

fn solid_rgba(color: [u8; 4], width: u32, height: u32) -> Vec<u8> {
    std::iter::repeat_n(color, width as usize * height as usize)
        .flatten()
        .collect()
}

fn composite_pixel_over_background(
    foreground: image::Rgba<u8>,
    background: image::Rgba<u8>,
) -> image::Rgba<u8> {
    let alpha = foreground.0[3] as f32 / 255.0;
    let inverse_alpha = 1.0 - alpha;
    image::Rgba([
        (foreground.0[0] as f32 * alpha + background.0[0] as f32 * inverse_alpha).round() as u8,
        (foreground.0[1] as f32 * alpha + background.0[1] as f32 * inverse_alpha).round() as u8,
        (foreground.0[2] as f32 * alpha + background.0[2] as f32 * inverse_alpha).round() as u8,
        255,
    ])
}

fn test_viewport_world() -> egui::Rect {
    egui::Rect::from_min_max(egui::pos2(-240.0, -180.0), egui::pos2(240.0, 180.0))
}

fn item_enabled(
    items: &[super::shortcuts::ContextMenuItem],
    action: super::shortcuts::ContextMenuAction,
) -> bool {
    items
        .iter()
        .find(|item| item.action == action)
        .is_some_and(|item| item.enabled)
}

fn test_loaded_item(placement_id: i64, target_board_id: i64) -> PreparedPlacementImage {
    let asset = Asset {
        id: placement_id,
        original_path: PathBuf::from(format!("fixture-{placement_id}.png")),
        filename: format!("fixture-{placement_id}.png"),
        mime_type: "image/png".to_owned(),
        width: 1,
        height: 1,
    };
    let placement = Placement::loaded(
        PlacementState::new(
            placement_id,
            eframe::egui::pos2(0.0, 0.0),
            1.0,
            0.0,
            0,
            false,
        ),
        asset,
    );
    PreparedPlacementImage {
        target_board_id,
        queue_id: Some(placement_id as u64),
        preview_rgba: vec![255, 255, 255, 255],
        origin: crate::jobs::ImportOrigin::Clipboard,
        ..loaded_record(placement)
    }
}

/// Like `test_loaded_item`, but the Asset really exists in `library`, so the
/// queued Document unit for the Placement can satisfy the asset foreign key.
fn persisted_loaded_item(
    library: &Library,
    placement_id: i64,
    target_board_id: i64,
) -> PreparedPlacementImage {
    let stored = library
        .store_rgba_image(
            &format!("fixture-{placement_id}"),
            &[255, 255, 255, 255],
            1,
            1,
        )
        .unwrap();
    let placement = library
        .persist_imported_image_resolving_placement(
            target_board_id,
            Some(placement_id),
            &stored,
            "image/png",
            1,
            1,
            4,
            eframe::egui::pos2(0.0, 0.0),
            1.0,
        )
        .unwrap();
    PreparedPlacementImage {
        placement,
        ..test_loaded_item(placement_id, target_board_id)
    }
}

/// A worker-prepared record for `placement` on board 1 with a 1x1 red
/// preview, as if the existing item had just loaded from the library.
fn loaded_record(placement: Placement) -> PreparedPlacementImage {
    PreparedPlacementImage {
        target_board_id: 1,
        queue_id: None,
        placement,
        preview_rgba: vec![255, 0, 0, 255],
        preview_width: 1,
        preview_height: 1,
        animated_preview: None,
        preview_texture: crate::jobs::ImagePrepPreviewTexture::Cpu,
        loaded_lod: crate::data::CanvasImageLod::Working,
        existing_load_diagnostics: None,
        origin: crate::jobs::ImportOrigin::Existing,
        trace: None,
    }
}

fn add_loaded_record(app: &mut SnappyRefApp, ctx: &egui::Context, placement: Placement) {
    app.canvas
        .add_prepared_placement_image(ctx, loaded_record(placement));
}

fn test_canvas_annotation(id: i64, start: egui::Pos2) -> crate::data::CanvasAnnotation {
    crate::data::CanvasAnnotation::new(
        id,
        CanvasAnnotationKind::Line,
        start,
        egui::pos2(start.x + 40.0, start.y + 30.0),
        String::new(),
        CanvasTextSize::M,
        CanvasStrokeWidth::M,
        crate::data::CanvasAnnotationColor::Gold,
        false,
        false,
    )
}
