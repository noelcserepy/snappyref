use eframe::egui;

#[cfg(feature = "semantic-search")]
#[derive(Clone, Debug)]
struct EmbeddingSourceTiming {
    asset_id: i64,
    source_kind: &'static str,
    path: std::path::PathBuf,
    file_size: u64,
    timing: crate::search::semantic::ImageEmbeddingTiming,
}

#[cfg(feature = "semantic-search")]
#[derive(Clone, Debug)]
struct EmbeddingPairTiming {
    asset_id: i64,
    original: EmbeddingSourceTiming,
    working_lod: EmbeddingSourceTiming,
}

pub fn print_file_decode_report(path: &std::path::Path) -> Result<(), String> {
    let report = crate::jobs::decode_image_file_perf_report(path)?;
    let frames = report
        .animated_frame_count
        .map(|count| format!(" animated_frames={count}"))
        .unwrap_or_default();

    println!(
        "decode source={} format={} source_mb={:.2} dims={}x{} megapixels={:.2} rgba_mb={:.2} decode_ms={:.2} ms_per_mp={:.2}{}",
        report.source_path.display(),
        report.format_name,
        report.source_file_size as f64 / 1024.0 / 1024.0,
        report.width,
        report.height,
        report.megapixels,
        report.rgba_bytes as f64 / 1024.0 / 1024.0,
        report.decode_ms,
        report.ms_per_megapixel,
        frames,
    );

    Ok(())
}

pub fn print_file_import_report(path: &std::path::Path) -> Result<(), String> {
    let report = crate::jobs::file_import_perf_report(path.to_path_buf())?;
    let cache = match (&report.canvas_cache_path, report.canvas_cache_file_size) {
        (Some(path), Some(bytes)) => format!(
            "{} ({:.2} MB)",
            path.display(),
            bytes as f64 / 1024.0 / 1024.0
        ),
        (Some(path), None) => path.display().to_string(),
        (None, _) => "none".to_owned(),
    };

    println!(
        "file_import source={} source_mb={:.2} source_dims={}x{} preview_dims={}x{} cache={} total_ms={:.2} decode_ms={:.2} store_ms={:.2} preview_ms={:.2} worker_ms={:.2}",
        report.source_path.display(),
        report.source_file_size as f64 / 1024.0 / 1024.0,
        report.source_width,
        report.source_height,
        report.preview_width,
        report.preview_height,
        cache,
        report.total_ms,
        report.timings.decode_or_load.as_secs_f64() * 1000.0,
        report.timings.store_and_persist.as_secs_f64() * 1000.0,
        report.timings.preview_prepare.as_secs_f64() * 1000.0,
        report.timings.worker_total.as_secs_f64() * 1000.0,
    );

    Ok(())
}

#[cfg(feature = "semantic-search")]
pub fn print_embedding_lod_report(
    library_root: &std::path::Path,
    limit: usize,
) -> Result<(), String> {
    use crate::search::semantic::{DEFAULT_SEMANTIC_SEARCH_MODEL, OrtDualEncoderModel};

    let library = crate::library::storage::Library::initialize(library_root.to_path_buf())?;
    let settings = library.load_user_session().unwrap_or_default().settings;
    let model =
        OrtDualEncoderModel::load_with_progress(DEFAULT_SEMANTIC_SEARCH_MODEL, |message| {
            eprintln!("[embedding-lod-bench] {message}");
        })?;
    let jobs =
        library.missing_live_canvas_asset_jobs_for_model("__snappyref_perf_embedding_lod__")?;
    let pairs = jobs
        .into_iter()
        .filter_map(|job| {
            let working_lod = library.canvas_image_cache_path_for_lod(
                job.asset_id,
                settings.canvas_image_max_side,
                crate::data::CanvasImageLod::Working,
            )?;
            job.original_path
                .is_file()
                .then_some((job.asset_id, job.original_path, working_lod))
        })
        .take(limit)
        .collect::<Vec<_>>();

    if pairs.is_empty() {
        return Err(format!(
            "no assets with original and working LOD found in {}",
            library_root.display()
        ));
    }

    let warmup_path = pairs[0].2.clone();
    let _ = model.embed_image_path_timed(&warmup_path)?;

    let mut reports = Vec::with_capacity(pairs.len());
    let mut skipped = 0_usize;
    for (index, (asset_id, original_path, working_lod_path)) in pairs.into_iter().enumerate() {
        let pair = if index % 2 == 0 {
            benchmark_embedding_source(&model, asset_id, "original", original_path).and_then(
                |original| {
                    benchmark_embedding_source(&model, asset_id, "working_lod", working_lod_path)
                        .map(|working_lod| (original, working_lod))
                },
            )
        } else {
            benchmark_embedding_source(&model, asset_id, "working_lod", working_lod_path).and_then(
                |working_lod| {
                    benchmark_embedding_source(&model, asset_id, "original", original_path)
                        .map(|original| (original, working_lod))
                },
            )
        };

        match pair {
            Ok((original, working_lod)) => reports.push(EmbeddingPairTiming {
                asset_id,
                original,
                working_lod,
            }),
            Err(error) => {
                skipped += 1;
                eprintln!("[embedding-lod-bench] skipped asset_id={asset_id}: {error}");
            }
        }
    }

    if reports.is_empty() {
        return Err("no readable original/working-LOD pairs found".to_owned());
    }

    print_embedding_lod_summary(&reports, settings.canvas_image_max_side, skipped);
    for report in &reports {
        print_embedding_source_line(&report.original);
        print_embedding_source_line(&report.working_lod);
        println!(
            "embedding_lod_delta asset_id={} total_saved_ms={:.2} total_speedup={:.2}x preprocess_saved_ms={:.2} preprocess_speedup={:.2}x model_saved_ms={:.2} model_speedup={:.2}x",
            report.asset_id,
            ms(report.original.timing.total) - ms(report.working_lod.timing.total),
            speedup(
                report.original.timing.total,
                report.working_lod.timing.total
            ),
            preprocess_ms(report.original.timing) - preprocess_ms(report.working_lod.timing),
            speedup_duration(
                preprocess_duration(report.original.timing),
                preprocess_duration(report.working_lod.timing)
            ),
            ms(report.original.timing.model_run) - ms(report.working_lod.timing.model_run),
            speedup(
                report.original.timing.model_run,
                report.working_lod.timing.model_run
            ),
        );
    }

    Ok(())
}

#[cfg(not(feature = "semantic-search"))]
pub fn print_embedding_lod_report(
    _library_root: &std::path::Path,
    _limit: usize,
) -> Result<(), String> {
    Err("semantic-search feature is disabled".to_owned())
}

#[cfg(feature = "semantic-search")]
fn benchmark_embedding_source(
    model: &crate::search::semantic::OrtDualEncoderModel,
    asset_id: i64,
    source_kind: &'static str,
    path: std::path::PathBuf,
) -> Result<EmbeddingSourceTiming, String> {
    let file_size = path
        .metadata()
        .map_err(|error| format!("failed to read metadata for {}: {error}", path.display()))?
        .len();
    let (_embedding, timing) = model.embed_image_path_timed(&path)?;
    Ok(EmbeddingSourceTiming {
        asset_id,
        source_kind,
        path,
        file_size,
        timing,
    })
}

#[cfg(feature = "semantic-search")]
fn print_embedding_source_line(source: &EmbeddingSourceTiming) {
    println!(
        "embedding_source asset_id={} kind={} file_mb={:.2} dims={}x{} total_ms={:.2} preprocess_ms={:.2} decode_rgb_ms={:.2} resize_ms={:.2} tensor_ms={:.2} bind_ms={:.2} model_ms={:.2} extract_ms={:.2} normalize_ms={:.2} path={}",
        source.asset_id,
        source.source_kind,
        source.file_size as f64 / 1024.0 / 1024.0,
        source.timing.source_width,
        source.timing.source_height,
        ms(source.timing.total),
        preprocess_ms(source.timing),
        ms(source.timing.decode_rgb),
        ms(source.timing.resize_crop),
        ms(source.timing.tensor_prepare),
        ms(source.timing.bind_input),
        ms(source.timing.model_run),
        ms(source.timing.extract_output),
        ms(source.timing.normalize_output),
        source.path.display(),
    );
}

#[cfg(feature = "semantic-search")]
fn print_embedding_lod_summary(
    reports: &[EmbeddingPairTiming],
    canvas_image_max_side: u32,
    skipped: usize,
) {
    let original_total = reports
        .iter()
        .map(|report| report.original.timing.total)
        .sum();
    let lod_total = reports
        .iter()
        .map(|report| report.working_lod.timing.total)
        .sum();
    let original_preprocess = reports
        .iter()
        .map(|report| preprocess_duration(report.original.timing))
        .sum();
    let lod_preprocess = reports
        .iter()
        .map(|report| preprocess_duration(report.working_lod.timing))
        .sum();
    let original_model = reports
        .iter()
        .map(|report| report.original.timing.model_run)
        .sum();
    let lod_model = reports
        .iter()
        .map(|report| report.working_lod.timing.model_run)
        .sum();
    println!(
        "embedding_lod_summary assets={} skipped={} working_lod_max_side={} original_total_ms={:.2} lod_total_ms={:.2} total_saved_ms={:.2} total_speedup={:.2}x original_preprocess_ms={:.2} lod_preprocess_ms={:.2} preprocess_saved_ms={:.2} preprocess_speedup={:.2}x original_model_ms={:.2} lod_model_ms={:.2} model_saved_ms={:.2} model_speedup={:.2}x",
        reports.len(),
        skipped,
        canvas_image_max_side,
        ms(original_total),
        ms(lod_total),
        ms(original_total) - ms(lod_total),
        speedup_duration(original_total, lod_total),
        ms(original_preprocess),
        ms(lod_preprocess),
        ms(original_preprocess) - ms(lod_preprocess),
        speedup_duration(original_preprocess, lod_preprocess),
        ms(original_model),
        ms(lod_model),
        ms(original_model) - ms(lod_model),
        speedup_duration(original_model, lod_model),
    );
}

#[cfg(feature = "semantic-search")]
fn preprocess_duration(
    timing: crate::search::semantic::ImageEmbeddingTiming,
) -> std::time::Duration {
    timing.decode_rgb + timing.resize_crop + timing.tensor_prepare
}

#[cfg(feature = "semantic-search")]
fn preprocess_ms(timing: crate::search::semantic::ImageEmbeddingTiming) -> f64 {
    ms(preprocess_duration(timing))
}

#[cfg(feature = "semantic-search")]
fn ms(duration: std::time::Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}

#[cfg(feature = "semantic-search")]
fn speedup(before: std::time::Duration, after: std::time::Duration) -> f64 {
    speedup_duration(before, after)
}

#[cfg(feature = "semantic-search")]
fn speedup_duration(before: std::time::Duration, after: std::time::Duration) -> f64 {
    if after.is_zero() {
        return f64::INFINITY;
    }
    before.as_secs_f64() / after.as_secs_f64()
}

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

pub fn print_startup_trace(library_root: &std::path::Path) -> Result<(), String> {
    let library = crate::library::storage::Library::initialize(library_root.to_path_buf())?;
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
            max_side: session.settings.canvas_image_max_side,
        })
        .unwrap_or_default();

    let entries = crate::jobs::startup_trace(&library, viewport_world, settings)?;
    println!(
        "startup_trace items={} max_side={} lod=overview",
        entries.len(),
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
