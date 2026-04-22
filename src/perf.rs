use eframe::egui;

pub fn print_report() -> Result<(), String> {
    let startup = crate::jobs::startup_perf_report()?;
    let interactive = crate::jobs::interactive_import_perf_report()?;
    let worker_interactive = crate::jobs::worker_interactive_import_perf_report()?;
    let paint = crate::canvas::paint_perf_report();

    println!(
        "startup_first_visible_ms legacy={:.2} current={:.2} delta={:.2}",
        startup.legacy_first_visible_ms,
        startup.current_first_visible_ms,
        startup.current_first_visible_ms - startup.legacy_first_visible_ms,
    );
    println!(
        "startup_first_visible_cached_ms cold={:.2} warm={:.2} delta={:.2}",
        startup.current_first_visible_ms,
        startup.warm_cached_first_visible_ms,
        startup.warm_cached_first_visible_ms - startup.current_first_visible_ms,
    );
    println!(
        "interactive_import_under_backlog_ms legacy={:.2} current={:.2} delta={:.2}",
        interactive.legacy_interactive_import_under_backlog_ms,
        interactive.current_interactive_import_under_backlog_ms,
        interactive.current_interactive_import_under_backlog_ms
            - interactive.legacy_interactive_import_under_backlog_ms,
    );
    println!(
        "worker_interactive_import_under_backlog_ms legacy={:.2} current={:.2} delta={:.2} interactive_workers={} background_workers={}",
        worker_interactive.legacy_single_worker_ms,
        worker_interactive.current_pooled_ms,
        worker_interactive.current_pooled_ms - worker_interactive.legacy_single_worker_ms,
        worker_interactive.current_interactive_workers,
        worker_interactive.current_background_workers,
    );
    println!(
        "paint_visibility_pass_ms proxy legacy={:.4} current={:.4} delta={:.4}",
        paint.legacy_visibility_pass_ms,
        paint.current_visibility_pass_ms,
        paint.current_visibility_pass_ms - paint.legacy_visibility_pass_ms,
    );
    println!(
        "paint_counts legacy_considered={} legacy_drawn={} current_considered={} current_drawn={}",
        paint.legacy_considered, paint.legacy_drawn, paint.current_considered, paint.current_drawn,
    );
    println!(
        "image_pass_groups current_texture_groups={}",
        paint.current_texture_groups,
    );

    Ok(())
}

pub fn print_startup_trace() -> Result<(), String> {
    let library_root = std::env::current_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."))
        .join("library");
    let library = crate::storage::Library::initialize(library_root)?;
    let mut canvas = crate::canvas::CanvasState::default();
    if let Ok(session) = library.load_user_session() {
        canvas.apply_viewport_state(&session.viewport);
    }
    let screen_rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(960.0, 640.0));
    let viewport_world = egui::Rect::from_min_max(
        canvas.screen_to_world(screen_rect.min, screen_rect),
        canvas.screen_to_world(screen_rect.max, screen_rect),
    );
    let settings = library
        .load_user_session()
        .map(|session| crate::jobs::CanvasImageSettings {
            mode: session.settings.canvas_image_mode,
            max_side: session.settings.canvas_image_max_side,
        })
        .unwrap_or_default();

    let entries = crate::jobs::startup_trace(&library, viewport_world, settings)?;
    println!(
        "startup_trace items={} mode={} max_side={}",
        entries.len(),
        settings.mode.as_str(),
        settings.max_side
    );
    for entry in &entries {
        println!(
            "startup_item startup_ms={:.2} item_id={} asset_id={} file={} source_kind={:?} source_mb={:.2} backfilled_cache={} source_dims={}x{} preview_dims={}x{} decode_ms={:.2} preview_ms={:.2} worker_ms={:.2} source={}",
            entry.startup_elapsed.as_secs_f64() * 1000.0,
            entry.record.item.id,
            entry.record.asset.id,
            entry.record.asset.filename,
            entry.diagnostics.source_kind,
            entry.diagnostics.source_file_size as f64 / 1024.0 / 1024.0,
            entry.diagnostics.backfilled_cache,
            entry.diagnostics.source_width,
            entry.diagnostics.source_height,
            entry.preview_width,
            entry.preview_height,
            entry.timings.decode_or_load.as_secs_f64() * 1000.0,
            entry.timings.preview_prepare.as_secs_f64() * 1000.0,
            entry.timings.worker_total.as_secs_f64() * 1000.0,
            entry.diagnostics.source_path.display(),
        );
    }

    Ok(())
}
