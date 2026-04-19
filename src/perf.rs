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

    Ok(())
}
