use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use eframe::egui;

use crate::data::{CanvasImageLod, Placement};
use crate::library::storage::Library;

use super::*;

const PERF_STARTUP_IMAGE_SIZE: u32 = 1536;
const PERF_BACKLOG_IMAGE_SIZE: u32 = 1024;

pub fn startup_perf_report() -> Result<StartupPerfReport, String> {
    let legacy_root = temp_library_root("startup-perf-legacy");
    let current_root = temp_library_root("startup-perf-current");
    let legacy_library = Library::initialize(legacy_root.clone())?;
    let current_library = Library::initialize(current_root.clone())?;
    let viewport_world =
        egui::Rect::from_min_max(egui::pos2(-240.0, -180.0), egui::pos2(240.0, 180.0));
    let legacy_paths = [
        write_png(
            &legacy_root,
            "offscreen-a.png",
            PERF_STARTUP_IMAGE_SIZE,
            PERF_STARTUP_IMAGE_SIZE,
        )?,
        write_png(
            &legacy_root,
            "offscreen-b.png",
            PERF_STARTUP_IMAGE_SIZE,
            PERF_STARTUP_IMAGE_SIZE,
        )?,
        write_png(
            &legacy_root,
            "visible-a.png",
            PERF_STARTUP_IMAGE_SIZE,
            PERF_STARTUP_IMAGE_SIZE,
        )?,
        write_png(
            &legacy_root,
            "visible-b.png",
            PERF_STARTUP_IMAGE_SIZE,
            PERF_STARTUP_IMAGE_SIZE,
        )?,
    ];
    let legacy_records = vec![
        perf_record(
            1,
            &legacy_paths[0],
            egui::pos2(-2200.0, -2200.0),
            PERF_STARTUP_IMAGE_SIZE,
            PERF_STARTUP_IMAGE_SIZE,
        ),
        perf_record(
            2,
            &legacy_paths[1],
            egui::pos2(2200.0, 2200.0),
            PERF_STARTUP_IMAGE_SIZE,
            PERF_STARTUP_IMAGE_SIZE,
        ),
        perf_record(
            3,
            &legacy_paths[2],
            egui::pos2(0.0, 0.0),
            PERF_STARTUP_IMAGE_SIZE,
            PERF_STARTUP_IMAGE_SIZE,
        ),
        perf_record(
            4,
            &legacy_paths[3],
            egui::pos2(120.0, 60.0),
            PERF_STARTUP_IMAGE_SIZE,
            PERF_STARTUP_IMAGE_SIZE,
        ),
    ];
    let current_paths = [
        write_png(
            &current_root,
            "offscreen-a.png",
            PERF_STARTUP_IMAGE_SIZE,
            PERF_STARTUP_IMAGE_SIZE,
        )?,
        write_png(
            &current_root,
            "offscreen-b.png",
            PERF_STARTUP_IMAGE_SIZE,
            PERF_STARTUP_IMAGE_SIZE,
        )?,
        write_png(
            &current_root,
            "visible-a.png",
            PERF_STARTUP_IMAGE_SIZE,
            PERF_STARTUP_IMAGE_SIZE,
        )?,
        write_png(
            &current_root,
            "visible-b.png",
            PERF_STARTUP_IMAGE_SIZE,
            PERF_STARTUP_IMAGE_SIZE,
        )?,
    ];
    let current_records = vec![
        perf_record(
            1,
            &current_paths[0],
            egui::pos2(-2200.0, -2200.0),
            PERF_STARTUP_IMAGE_SIZE,
            PERF_STARTUP_IMAGE_SIZE,
        ),
        perf_record(
            2,
            &current_paths[1],
            egui::pos2(2200.0, 2200.0),
            PERF_STARTUP_IMAGE_SIZE,
            PERF_STARTUP_IMAGE_SIZE,
        ),
        perf_record(
            3,
            &current_paths[2],
            egui::pos2(0.0, 0.0),
            PERF_STARTUP_IMAGE_SIZE,
            PERF_STARTUP_IMAGE_SIZE,
        ),
        perf_record(
            4,
            &current_paths[3],
            egui::pos2(120.0, 60.0),
            PERF_STARTUP_IMAGE_SIZE,
            PERF_STARTUP_IMAGE_SIZE,
        ),
    ];
    let legacy_first_visible_ms =
        measure_first_visible_load_ms(&legacy_library, legacy_records, viewport_world, false)?;
    let current_first_visible_ms = measure_first_visible_load_ms(
        &current_library,
        current_records.clone(),
        viewport_world,
        true,
    )?;
    let warm_cached_first_visible_ms =
        measure_first_visible_load_ms(&current_library, current_records, viewport_world, true)?;
    let _ = std::fs::remove_dir_all(legacy_root);
    let _ = std::fs::remove_dir_all(current_root);

    Ok(StartupPerfReport {
        legacy_first_visible_ms,
        current_first_visible_ms,
        warm_cached_first_visible_ms,
    })
}

pub fn startup_trace(
    library: &Library,
    viewport_world: egui::Rect,
    canvas_image_settings: CanvasImageSettings,
) -> Result<Vec<StartupTraceEntry>, String> {
    let mut placements = library.load_placements()?;
    sort_records_by_viewport_proximity(&mut placements, viewport_world);
    let started_at = Instant::now();
    let mut entries = Vec::with_capacity(placements.len());
    for placement in placements {
        let trace = ImagePrepTrace {
            queued_at: Instant::now(),
            timings: ImagePrepTimings::default(),
        };
        match load_existing(
            library,
            1,
            None,
            placement.clone(),
            usize::MAX,
            canvas_image_settings,
            CanvasImageLod::Overview,
            Some(trace),
            None,
        ) {
            ImagePrepResult::Loaded(loaded) => {
                let diagnostics = loaded
                    .existing_load_diagnostics
                    .ok_or_else(|| "missing existing-load diagnostics".to_owned())?;
                let timings = loaded
                    .trace
                    .map(|trace| trace.timings)
                    .ok_or_else(|| "missing existing-load trace".to_owned())?;
                entries.push(StartupTraceEntry {
                    placement: loaded.placement,
                    diagnostics,
                    timings,
                    preview_width: loaded.preview_width,
                    preview_height: loaded.preview_height,
                    startup_elapsed: started_at.elapsed(),
                });
            }
            ImagePrepResult::Failed { error, .. } => return Err(error),
            ImagePrepResult::Cropped(_)
            | ImagePrepResult::CacheLodBackfilled { .. }
            | ImagePrepResult::CacheLodBackfillFailed { .. } => {}
        }
    }
    Ok(entries)
}

pub fn interactive_import_perf_report() -> Result<InteractiveImportPerfReport, String> {
    let root = temp_library_root("interactive-import-perf");
    let library = Library::initialize(root.clone())?;
    let viewport_world =
        egui::Rect::from_min_max(egui::pos2(-240.0, -180.0), egui::pos2(240.0, 180.0));
    let mut backlog = Vec::new();
    for index in 0..6 {
        let path = write_png(
            &root,
            &format!("backlog-{index}.png"),
            PERF_BACKLOG_IMAGE_SIZE,
            PERF_BACKLOG_IMAGE_SIZE,
        )?;
        backlog.push(perf_record(
            index as i64 + 1,
            &path,
            egui::pos2(2500.0 + index as f32 * 40.0, 2500.0),
            PERF_BACKLOG_IMAGE_SIZE,
            PERF_BACKLOG_IMAGE_SIZE,
        ));
    }

    let legacy_interactive_import_under_backlog_ms = measure_interactive_import_ms(
        &library,
        &backlog,
        viewport_world,
        solid_rgba(1200, 900),
        false,
    )?;
    let current_interactive_import_under_backlog_ms = measure_interactive_import_ms(
        &library,
        &backlog,
        viewport_world,
        solid_rgba(1200, 900),
        true,
    )?;

    let _ = std::fs::remove_dir_all(root);
    Ok(InteractiveImportPerfReport {
        legacy_interactive_import_under_backlog_ms,
        current_interactive_import_under_backlog_ms,
    })
}

pub fn worker_interactive_import_perf_report() -> Result<WorkerInteractiveImportPerfReport, String>
{
    let parallelism = thread::available_parallelism()
        .map(|available| available.get())
        .unwrap_or(1);
    let current_config = ImagePrepWorkerConfig::for_parallelism(parallelism);

    let legacy_single_worker_ms =
        measure_worker_interactive_import_ms(ImagePrepWorkerConfig::legacy_single_worker())?;
    let current_pooled_ms = measure_worker_interactive_import_ms(current_config)?;

    Ok(WorkerInteractiveImportPerfReport {
        legacy_single_worker_ms,
        current_pooled_ms,
        current_interactive_workers: current_config.interactive_workers,
        current_background_workers: current_config.background_workers,
    })
}

pub fn file_import_perf_report(source_path: PathBuf) -> Result<FileImportPerfReport, String> {
    let root = temp_library_root("file-import-perf");
    let library = Library::initialize(root.clone())?;
    let viewport_world =
        egui::Rect::from_min_max(egui::pos2(-480.0, -320.0), egui::pos2(480.0, 320.0));
    let settings = CanvasImageSettings::default();
    let trace = ImagePrepTrace {
        queued_at: Instant::now(),
        timings: ImagePrepTimings::default(),
    };

    let started_at = Instant::now();
    let result = import_path_sync_traced(
        &library,
        1,
        None,
        None,
        source_path.clone(),
        egui::Pos2::ZERO,
        None,
        viewport_world,
        usize::MAX,
        settings,
        true,
        DEFAULT_PLACEMENT_TARGET_AREA,
        crate::data::DEFAULT_PLACEMENT_GAP,
        Some(trace),
        None,
    );
    let total_ms = started_at.elapsed().as_secs_f64() * 1000.0;

    let loaded = match result {
        Ok(loaded) => loaded,
        Err(error) => {
            let _ = std::fs::remove_dir_all(root);
            return Err(error);
        }
    };
    let timings = loaded
        .trace
        .map(|trace| trace.timings)
        .ok_or_else(|| "missing import trace".to_owned())?;
    let canvas_cache_path = library.canvas_image_cache_path(
        loaded.placement.require_loaded_asset().id,
        settings.max_side,
    );
    let canvas_cache_file_size = canvas_cache_path
        .as_ref()
        .and_then(|path| std::fs::metadata(path).ok())
        .map(|metadata| metadata.len());
    let source_file_size = std::fs::metadata(&source_path)
        .map(|metadata| metadata.len())
        .unwrap_or(loaded.placement.require_loaded_asset().file_size);

    let report = FileImportPerfReport {
        source_path,
        source_file_size,
        source_width: loaded.placement.require_loaded_asset().width,
        source_height: loaded.placement.require_loaded_asset().height,
        preview_width: loaded.preview_width,
        preview_height: loaded.preview_height,
        canvas_cache_path,
        canvas_cache_file_size,
        timings,
        total_ms,
    };

    let _ = std::fs::remove_dir_all(root);
    Ok(report)
}

fn measure_first_visible_load_ms(
    library: &Library,
    mut placements: Vec<Placement>,
    viewport_world: egui::Rect,
    prioritize: bool,
) -> Result<f64, String> {
    if prioritize {
        sort_records_by_viewport_proximity(&mut placements, viewport_world);
    }
    let started_at = Instant::now();
    for placement in placements {
        let loaded = load_existing(
            library,
            1,
            None,
            placement.clone(),
            usize::MAX,
            CanvasImageSettings::default(),
            CanvasImageLod::Working,
            None,
            None,
        );
        match loaded {
            ImagePrepResult::Loaded(loaded) => {
                if record_intersects_viewport_world(&loaded.placement, viewport_world) {
                    return Ok(started_at.elapsed().as_secs_f64() * 1000.0);
                }
            }
            ImagePrepResult::Failed { error, .. } => return Err(error),
            ImagePrepResult::Cropped(_)
            | ImagePrepResult::CacheLodBackfilled { .. }
            | ImagePrepResult::CacheLodBackfillFailed { .. } => {}
        }
    }
    Ok(started_at.elapsed().as_secs_f64() * 1000.0)
}

fn measure_interactive_import_ms(
    library: &Library,
    backlog: &[Placement],
    viewport_world: egui::Rect,
    clipboard_rgba: Vec<u8>,
    prioritize: bool,
) -> Result<f64, String> {
    let started_at = Instant::now();
    if prioritize {
        import_clipboard_sync(
            library,
            clipboard_rgba,
            1200,
            900,
            egui::Pos2::ZERO,
            viewport_world,
            true,
            crate::data::DEFAULT_PLACEMENT_GAP,
        )?;
    } else {
        for placement in backlog {
            match load_existing(
                library,
                1,
                None,
                placement.clone(),
                usize::MAX,
                CanvasImageSettings::default(),
                CanvasImageLod::Working,
                None,
                None,
            ) {
                ImagePrepResult::Loaded(_) => {}
                ImagePrepResult::Failed { error, .. } => return Err(error),
                ImagePrepResult::Cropped(_)
                | ImagePrepResult::CacheLodBackfilled { .. }
                | ImagePrepResult::CacheLodBackfillFailed { .. } => {}
            }
        }
        import_clipboard_sync(
            library,
            clipboard_rgba,
            1200,
            900,
            egui::Pos2::ZERO,
            viewport_world,
            true,
            crate::data::DEFAULT_PLACEMENT_GAP,
        )?;
    }
    Ok(started_at.elapsed().as_secs_f64() * 1000.0)
}

fn measure_worker_interactive_import_ms(config: ImagePrepWorkerConfig) -> Result<f64, String> {
    let root = temp_library_root("worker-interactive-import-perf");
    let library = Library::initialize(root.clone())?;
    let worker = ImagePrepWorker::new_with_config(library, egui::Context::default(), None, config);
    let viewport_world =
        egui::Rect::from_min_max(egui::pos2(-240.0, -180.0), egui::pos2(240.0, 180.0));

    for index in 0..6 {
        let path = write_png(
            &root,
            &format!("worker-backlog-{index}.png"),
            PERF_BACKLOG_IMAGE_SIZE,
            PERF_BACKLOG_IMAGE_SIZE,
        )?;
        worker.enqueue(ImagePrepRequest::LoadExisting {
            target_board_id: 1,
            queue_id: None,
            placement: perf_record(
                index as i64 + 1,
                &path,
                egui::pos2(2500.0 + index as f32 * 40.0, 2500.0),
                PERF_BACKLOG_IMAGE_SIZE,
                PERF_BACKLOG_IMAGE_SIZE,
            ),
            max_texture_side: usize::MAX,
            canvas_image_settings: CanvasImageSettings::default(),
            target_lod: CanvasImageLod::Working,
            priority: ExistingLoadPriority::Background,
            trace: None,
        })?;
    }

    let started_at = Instant::now();
    worker.enqueue(ImagePrepRequest::ImportClipboard {
        target_board_id: 1,
        queue_id: 1,
        placement_id: None,
        rgba: solid_rgba(1200, 900),
        width: 1200,
        height: 900,
        desired_world: egui::Pos2::ZERO,
        reserved_world: None,
        viewport_world,
        max_texture_side: usize::MAX,
        canvas_image_settings: CanvasImageSettings::default(),
        auto_scale: true,
        placement_target_area: DEFAULT_PLACEMENT_TARGET_AREA,
        placement_gap: crate::data::DEFAULT_PLACEMENT_GAP,
        trace: Some(ImagePrepTrace {
            queued_at: started_at,
            timings: ImagePrepTimings::default(),
        }),
    })?;

    let deadline = Instant::now() + Duration::from_secs(10);
    let result = loop {
        let mut interactive_result = None;
        for result in worker.try_recv_up_to(8) {
            match result {
                ImagePrepResult::Loaded(loaded) if loaded.origin == ImportOrigin::Clipboard => {
                    interactive_result = Some(Ok(started_at.elapsed().as_secs_f64() * 1000.0));
                    break;
                }
                ImagePrepResult::Failed { context, error, .. } if context == "clipboard" => {
                    interactive_result = Some(Err(error));
                    break;
                }
                _ => {}
            }
        }
        if let Some(result) = interactive_result {
            break result;
        }

        if Instant::now() >= deadline {
            break Err("timed out waiting for worker-backed interactive import".to_owned());
        }

        thread::sleep(Duration::from_millis(1));
    };
    drop(worker);
    let _ = std::fs::remove_dir_all(&root);
    result
}

fn perf_record(
    placement_id: i64,
    path: &Path,
    center: egui::Pos2,
    width: u32,
    height: u32,
) -> Placement {
    let item = crate::data::PlacementState::new(
        placement_id,
        Some(placement_id),
        center,
        1.0,
        0.0,
        placement_id as i32,
        false,
    );
    let asset = crate::data::Asset {
        id: placement_id,
        original_path: path.to_path_buf(),
        filename: path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("image.png")
            .to_owned(),
        mime_type: "image/png".to_owned(),
        width,
        height,
        file_size: std::fs::metadata(path)
            .map(|metadata| metadata.len())
            .unwrap_or(0),
        imported_at: 0,
    };
    Placement::loaded(item, asset)
}

fn solid_rgba(width: u32, height: u32) -> Vec<u8> {
    let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
    for y in 0..height {
        for x in 0..width {
            let channel = ((x ^ y) & 0xFF) as u8;
            rgba.extend_from_slice(&[channel, channel.wrapping_add(47), 255, 255]);
        }
    }
    rgba
}

fn temp_library_root(label: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("snappyref-{label}-{unique}"))
}

fn write_png(root: &Path, filename: &str, width: u32, height: u32) -> Result<PathBuf, String> {
    std::fs::create_dir_all(root)
        .map_err(|error| format!("failed to create temp perf root: {error}"))?;
    let path = root.join(filename);
    let rgba = solid_rgba(width, height);
    image::save_buffer_with_format(
        &path,
        &rgba,
        width,
        height,
        image::ColorType::Rgba8,
        image::ImageFormat::Png,
    )
    .map_err(|error| format!("failed to write perf fixture image: {error}"))?;
    Ok(path)
}
