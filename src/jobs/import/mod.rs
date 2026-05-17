use std::io::Read;
use std::path::{Path, PathBuf};
#[cfg(not(test))]
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use eframe::egui;
use image::RgbaImage;
use image::codecs::jpeg::JpegEncoder;
use image::codecs::webp::WebPEncoder;
use image::{ExtendedColorType, ImageEncoder};

use crate::data::{BoardItemWithAsset, CanvasImageLod};
use crate::jobs::image_formats::{
    AnimatedPreview, PreviewImage, decode_image_bytes_with_max_side, decode_image_file,
    decode_image_file_with_max_side, preview_animation_from_frames, preview_image_from_rgba,
};
use crate::library::storage::Library;

mod preview_texture;
mod request;

use preview_texture::build_preview_texture;
pub use preview_texture::{ImportedPreviewTexture, NativeTextureHandle};
pub(super) use request::ImportPriority;
pub use request::{
    CanvasImageSettings, CroppedCanvasItem, ExistingLoadDiagnostics, ExistingLoadPriority,
    ExistingLoadSourceKind, FileImportPerfReport, ImportOrigin, ImportRequest, ImportResult,
    ImportTimings, ImportTrace, ImportWorkerStats, InteractiveImportPerfReport, LoadedCanvasItem,
    StartupPerfReport, StartupTraceEntry, WorkerInteractiveImportPerfReport,
};

pub const DEFAULT_IMPORT_TARGET_CANVAS_AREA: f32 = crate::data::DEFAULT_IMPORT_TARGET_CANVAS_AREA;
const URL_IMPORT_MAX_BYTES: u64 = 64 * 1024 * 1024;
const URL_IMPORT_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const URL_IMPORT_REQUEST_TIMEOUT: Duration = Duration::from_secs(45);
#[cfg(not(test))]
const IMAGE_IMPORT_HELPER_DISABLED_ENV: &str = "SNAPPYREF_DISABLE_IMAGE_IMPORT_HELPER";

pub fn default_import_reference_span() -> f32 {
    DEFAULT_IMPORT_TARGET_CANVAS_AREA.sqrt()
}

#[derive(Clone, Debug)]
struct PreparedImportImage {
    rgba: Vec<u8>,
    width: u32,
    height: u32,
    source_width: u32,
    source_height: u32,
    mime_type: String,
    file_size: u64,
    cache_path: Option<PathBuf>,
    cache_extension: Option<String>,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
struct ImageImportPrepareOutput {
    animated: bool,
    rgba_path: Option<PathBuf>,
    width: u32,
    height: u32,
    source_width: u32,
    source_height: u32,
    mime_type: String,
    file_size: u64,
    cache_path: Option<PathBuf>,
    cache_extension: Option<String>,
}

const OVERVIEW_CACHE_MAX_SIDE: u32 = 256;
const INTERACTIVE_CROP_PREVIEW_MAX_SIDE: u32 = 1024;
const DETAIL_CACHE_MAX_SIDE: u32 = 4096;

mod worker;

pub use worker::ImportWorker;
use worker::ImportWorkerConfig;
#[cfg(test)]
pub(super) use worker::{RequestQueue, WorkerKind};

#[allow(dead_code)]
pub fn import_path_sync(
    library: &Library,
    source_path: PathBuf,
    desired_world: egui::Pos2,
    viewport_world: egui::Rect,
    auto_scale: bool,
    import_gap: f32,
) -> Result<LoadedCanvasItem, String> {
    let target_board_id = library.load_user_session()?.active_board_id;
    import_path_sync_traced(
        library,
        target_board_id,
        None,
        None,
        source_path,
        desired_world,
        None,
        viewport_world,
        usize::MAX,
        CanvasImageSettings::default(),
        auto_scale,
        DEFAULT_IMPORT_TARGET_CANVAS_AREA,
        import_gap,
        None,
        None,
    )
}

pub fn import_clipboard_sync(
    library: &Library,
    rgba: Vec<u8>,
    width: u32,
    height: u32,
    desired_world: egui::Pos2,
    viewport_world: egui::Rect,
    auto_scale: bool,
    import_gap: f32,
) -> Result<LoadedCanvasItem, String> {
    let target_board_id = library.load_user_session()?.active_board_id;
    import_clipboard_sync_traced(
        library,
        target_board_id,
        0,
        None,
        rgba,
        width,
        height,
        desired_world,
        None,
        viewport_world,
        usize::MAX,
        CanvasImageSettings::default(),
        auto_scale,
        DEFAULT_IMPORT_TARGET_CANVAS_AREA,
        import_gap,
        None,
        None,
    )
}

pub fn sort_records_by_viewport_proximity(
    records: &mut [BoardItemWithAsset],
    viewport_world: egui::Rect,
) {
    records.sort_by(|left, right| {
        record_viewport_distance_sq(left, viewport_world)
            .total_cmp(&record_viewport_distance_sq(right, viewport_world))
            .then_with(|| {
                left.item
                    .z_index
                    .cmp(&right.item.z_index)
                    .then_with(|| left.item.id.cmp(&right.item.id))
            })
    });
}

fn import_path_sync_traced(
    library: &Library,
    target_board_id: i64,
    queue_id: Option<u64>,
    placement_id: Option<i64>,
    source_path: PathBuf,
    desired_world: egui::Pos2,
    reserved_world: Option<egui::Pos2>,
    viewport_world: egui::Rect,
    max_texture_side: usize,
    canvas_image_settings: CanvasImageSettings,
    auto_scale: bool,
    target_canvas_area: f32,
    import_gap: f32,
    trace: Option<ImportTrace>,
    wgpu_render_state: Option<&eframe::egui_wgpu::RenderState>,
) -> Result<LoadedCanvasItem, String> {
    let worker_started_at = Instant::now();
    let mut trace = trace;
    if let Some(trace) = &mut trace {
        trace.timings.queue_wait = worker_started_at.saturating_duration_since(trace.queued_at);
    }

    if is_svg_path(&source_path) {
        return import_svg_path_sync_traced(
            library,
            target_board_id,
            queue_id,
            placement_id,
            source_path,
            desired_world,
            reserved_world,
            viewport_world,
            auto_scale,
            target_canvas_area,
            import_gap,
            worker_started_at,
            trace,
        );
    }

    let decode_started_at = Instant::now();
    if let Some(prepared) =
        prepare_import_image_with_helper(&source_path, canvas_image_settings).unwrap_or(None)
    {
        if let Some(trace) = &mut trace {
            trace.timings.decode_or_load = decode_started_at.elapsed();
        }
        let initial_scale = initial_import_scale(
            prepared.source_width,
            prepared.source_height,
            auto_scale,
            target_canvas_area,
        );
        let arranged_world = resolve_import_world(
            library,
            target_board_id,
            desired_world,
            reserved_world,
            viewport_world,
            prepared.source_width as f32 * initial_scale.max(0.01),
            prepared.source_height as f32 * initial_scale.max(0.01),
            import_gap,
        )?;

        let store_started_at = Instant::now();
        let stored_path = library
            .store_original(&source_path)
            .map_err(|error| format!("{}: {error}", source_path.display()))?;

        let record = library
            .persist_imported_image_resolving_placement(
                target_board_id,
                placement_id,
                &stored_path,
                &prepared.mime_type,
                prepared.source_width,
                prepared.source_height,
                prepared.file_size,
                arranged_world,
                initial_scale,
            )
            .map_err(|error| format!("{}: {error}", stored_path.display()))?;
        let store_and_persist = store_started_at.elapsed();
        let preview_started_at = Instant::now();
        install_prepared_canvas_cache(
            library,
            record.asset.id,
            canvas_image_settings.max_side,
            CanvasImageLod::Working,
            &prepared,
        );
        let preview = preview_image_from_rgba(
            prepared.width,
            prepared.height,
            &prepared.rgba,
            max_texture_side,
        )?;
        let preview_texture = build_preview_texture(wgpu_render_state, &preview)?;
        if let Some(trace) = &mut trace {
            trace.timings.store_and_persist = store_and_persist;
            trace.timings.preview_prepare = preview_started_at.elapsed();
            trace.timings.worker_total = worker_started_at.elapsed();
        }

        return Ok(LoadedCanvasItem {
            target_board_id,
            queue_id,
            record,
            preview_rgba: preview.rgba,
            preview_width: preview.width,
            preview_height: preview.height,
            animated_preview: None,
            preview_texture,
            loaded_lod: CanvasImageLod::Working,
            existing_load_diagnostics: None,
            origin: ImportOrigin::Path,
            trace,
        });
    }

    let decode_started_at = Instant::now();
    let decoded =
        decode_image_file_with_max_side(&source_path, Some(canvas_image_settings.max_side))
            .map_err(|error| format!("{}: {error}", source_path.display()))?;
    if let Some(trace) = &mut trace {
        trace.timings.decode_or_load = decode_started_at.elapsed();
    }
    let initial_scale = initial_import_scale(
        decoded.source_width,
        decoded.source_height,
        auto_scale,
        target_canvas_area,
    );
    let arranged_world = resolve_import_world(
        library,
        target_board_id,
        desired_world,
        reserved_world,
        viewport_world,
        decoded.source_width as f32 * initial_scale.max(0.01),
        decoded.source_height as f32 * initial_scale.max(0.01),
        import_gap,
    )?;

    let store_started_at = Instant::now();
    let stored_path = library
        .store_original(&source_path)
        .map_err(|error| format!("{}: {error}", source_path.display()))?;

    let record = library
        .persist_imported_image_resolving_placement(
            target_board_id,
            placement_id,
            &stored_path,
            &decoded.mime_type,
            decoded.source_width,
            decoded.source_height,
            decoded.file_size,
            arranged_world,
            initial_scale,
        )
        .map_err(|error| format!("{}: {error}", stored_path.display()))?;
    let store_and_persist = store_started_at.elapsed();
    let preview_started_at = Instant::now();
    let canvas_source = canvas_source_image_from_rgba(
        decoded.width,
        decoded.height,
        &decoded.rgba,
        canvas_image_settings,
    )?;
    let _ = library.store_canvas_image_cache(
        record.asset.id,
        canvas_image_settings.max_side,
        &canvas_source.rgba,
        canvas_source.width,
        canvas_source.height,
    );
    let preview = preview_image_from_rgba(
        canvas_source.width,
        canvas_source.height,
        &canvas_source.rgba,
        max_texture_side,
    )?;
    let animated_preview = decoded
        .animation
        .as_ref()
        .map(|animation| {
            preview_animation_from_frames(
                decoded.width,
                decoded.height,
                &animation.frames,
                animated_preview_max_side(canvas_image_settings, max_texture_side),
            )
        })
        .transpose()?;
    let preview_texture = if animated_preview.is_some() {
        ImportedPreviewTexture::Cpu
    } else {
        build_preview_texture(wgpu_render_state, &preview)?
    };
    if let Some(trace) = &mut trace {
        trace.timings.store_and_persist = store_and_persist;
        trace.timings.preview_prepare = preview_started_at.elapsed();
        trace.timings.worker_total = worker_started_at.elapsed();
    }

    Ok(LoadedCanvasItem {
        target_board_id,
        queue_id,
        record,
        preview_rgba: preview.rgba,
        preview_width: preview.width,
        preview_height: preview.height,
        animated_preview,
        preview_texture,
        loaded_lod: CanvasImageLod::Working,
        existing_load_diagnostics: None,
        origin: ImportOrigin::Path,
        trace,
    })
}

fn import_svg_path_sync_traced(
    library: &Library,
    target_board_id: i64,
    queue_id: Option<u64>,
    placement_id: Option<i64>,
    source_path: PathBuf,
    desired_world: egui::Pos2,
    reserved_world: Option<egui::Pos2>,
    viewport_world: egui::Rect,
    auto_scale: bool,
    target_canvas_area: f32,
    import_gap: f32,
    worker_started_at: Instant,
    mut trace: Option<ImportTrace>,
) -> Result<LoadedCanvasItem, String> {
    let decode_started_at = Instant::now();
    let svg = read_svg_document(&source_path)?;
    if let Some(trace) = &mut trace {
        trace.timings.decode_or_load = decode_started_at.elapsed();
    }

    let initial_scale = initial_import_scale(svg.width, svg.height, auto_scale, target_canvas_area);
    let arranged_world = resolve_import_world(
        library,
        target_board_id,
        desired_world,
        reserved_world,
        viewport_world,
        svg.width as f32 * initial_scale.max(0.01),
        svg.height as f32 * initial_scale.max(0.01),
        import_gap,
    )?;

    let store_started_at = Instant::now();
    let stored_path = library
        .store_original(&source_path)
        .map_err(|error| format!("{}: {error}", source_path.display()))?;
    let file_size = std::fs::metadata(&stored_path)
        .map_err(|error| format!("failed to read stored SVG metadata: {error}"))?
        .len();
    let record = library
        .persist_imported_image_resolving_placement(
            target_board_id,
            placement_id,
            &stored_path,
            SVG_MIME_TYPE,
            svg.width,
            svg.height,
            file_size,
            arranged_world,
            initial_scale,
        )
        .map_err(|error| format!("{}: {error}", stored_path.display()))?;
    let store_and_persist = store_started_at.elapsed();

    if let Some(trace) = &mut trace {
        trace.timings.store_and_persist = store_and_persist;
        trace.timings.worker_total = worker_started_at.elapsed();
    }

    Ok(LoadedCanvasItem {
        target_board_id,
        queue_id,
        record,
        preview_rgba: Vec::new(),
        preview_width: svg.width,
        preview_height: svg.height,
        animated_preview: None,
        preview_texture: ImportedPreviewTexture::Svg {
            uri: format!("bytes://asset-svg-{}.svg", stable_svg_uri_key(&stored_path)),
            bytes: Arc::<[u8]>::from(svg.bytes),
            size: [svg.width as usize, svg.height as usize],
        },
        loaded_lod: CanvasImageLod::Working,
        existing_load_diagnostics: None,
        origin: ImportOrigin::Path,
        trace,
    })
}

fn import_path(
    library: &Library,
    target_board_id: i64,
    queue_id: u64,
    placement_id: Option<i64>,
    source_path: PathBuf,
    desired_world: egui::Pos2,
    reserved_world: Option<egui::Pos2>,
    viewport_world: egui::Rect,
    max_texture_side: usize,
    canvas_image_settings: CanvasImageSettings,
    auto_scale: bool,
    target_canvas_area: f32,
    import_gap: f32,
    trace: Option<ImportTrace>,
    wgpu_render_state: Option<&eframe::egui_wgpu::RenderState>,
) -> ImportResult {
    match import_path_sync_traced(
        library,
        target_board_id,
        Some(queue_id),
        placement_id,
        source_path.clone(),
        desired_world,
        reserved_world,
        viewport_world,
        max_texture_side,
        canvas_image_settings,
        auto_scale,
        target_canvas_area,
        import_gap,
        trace,
        wgpu_render_state,
    ) {
        Ok(loaded) => ImportResult::Loaded(loaded),
        Err(error) => ImportResult::Failed {
            target_board_id: Some(target_board_id),
            queue_id: Some(queue_id),
            placement_id,
            item_id: None,
            context: source_path.display().to_string(),
            error,
        },
    }
}

fn import_url(
    library: &Library,
    target_board_id: i64,
    queue_id: u64,
    placement_id: Option<i64>,
    url: String,
    desired_world: egui::Pos2,
    reserved_world: Option<egui::Pos2>,
    viewport_world: egui::Rect,
    max_texture_side: usize,
    canvas_image_settings: CanvasImageSettings,
    auto_scale: bool,
    target_canvas_area: f32,
    import_gap: f32,
    trace: Option<ImportTrace>,
    wgpu_render_state: Option<&eframe::egui_wgpu::RenderState>,
) -> ImportResult {
    match import_url_sync_traced(
        library,
        target_board_id,
        Some(queue_id),
        placement_id,
        &url,
        desired_world,
        reserved_world,
        viewport_world,
        max_texture_side,
        canvas_image_settings,
        auto_scale,
        target_canvas_area,
        import_gap,
        trace,
        wgpu_render_state,
    ) {
        Ok(loaded) => ImportResult::Loaded(loaded),
        Err(error) => ImportResult::Failed {
            target_board_id: Some(target_board_id),
            queue_id: Some(queue_id),
            placement_id,
            item_id: None,
            context: url,
            error,
        },
    }
}

fn import_clipboard(
    library: &Library,
    target_board_id: i64,
    queue_id: u64,
    placement_id: Option<i64>,
    rgba: Vec<u8>,
    width: u32,
    height: u32,
    desired_world: egui::Pos2,
    reserved_world: Option<egui::Pos2>,
    viewport_world: egui::Rect,
    max_texture_side: usize,
    canvas_image_settings: CanvasImageSettings,
    auto_scale: bool,
    target_canvas_area: f32,
    import_gap: f32,
    trace: Option<ImportTrace>,
    wgpu_render_state: Option<&eframe::egui_wgpu::RenderState>,
) -> ImportResult {
    match import_clipboard_sync_traced(
        library,
        target_board_id,
        queue_id,
        placement_id,
        rgba,
        width,
        height,
        desired_world,
        reserved_world,
        viewport_world,
        max_texture_side,
        canvas_image_settings,
        auto_scale,
        target_canvas_area,
        import_gap,
        trace,
        wgpu_render_state,
    ) {
        Ok(loaded) => ImportResult::Loaded(loaded),
        Err(error) => ImportResult::Failed {
            target_board_id: Some(target_board_id),
            queue_id: Some(queue_id),
            placement_id,
            item_id: None,
            context: "clipboard".to_owned(),
            error,
        },
    }
}

struct DownloadedUrlImage {
    url: url::Url,
    bytes: Vec<u8>,
    mime_type: Option<String>,
}

fn import_url_sync_traced(
    library: &Library,
    target_board_id: i64,
    queue_id: Option<u64>,
    placement_id: Option<i64>,
    url: &str,
    desired_world: egui::Pos2,
    reserved_world: Option<egui::Pos2>,
    viewport_world: egui::Rect,
    max_texture_side: usize,
    canvas_image_settings: CanvasImageSettings,
    auto_scale: bool,
    target_canvas_area: f32,
    import_gap: f32,
    trace: Option<ImportTrace>,
    wgpu_render_state: Option<&eframe::egui_wgpu::RenderState>,
) -> Result<LoadedCanvasItem, String> {
    let worker_started_at = Instant::now();
    let mut trace = trace;
    if let Some(trace) = &mut trace {
        trace.timings.queue_wait = worker_started_at.saturating_duration_since(trace.queued_at);
    }

    let download_started_at = Instant::now();
    let downloaded = download_remote_image(url)?;
    if let Some(trace) = &mut trace {
        trace.timings.decode_or_load = download_started_at.elapsed();
    }

    if downloaded_url_image_is_svg(&downloaded) {
        return import_downloaded_svg_sync_traced(
            library,
            target_board_id,
            queue_id,
            placement_id,
            downloaded,
            desired_world,
            reserved_world,
            viewport_world,
            auto_scale,
            target_canvas_area,
            import_gap,
            worker_started_at,
            trace,
        );
    }

    let source_label = downloaded.url.as_str().to_owned();
    let decode_started_at = Instant::now();
    let decoded = decode_image_bytes_with_max_side(
        &downloaded.bytes,
        Some(source_label.clone()),
        downloaded.mime_type.as_deref(),
        Some(canvas_image_settings.max_side),
    )?;
    if let Some(trace) = &mut trace {
        trace.timings.decode_or_load += decode_started_at.elapsed();
    }

    let initial_scale = initial_import_scale(
        decoded.source_width,
        decoded.source_height,
        auto_scale,
        target_canvas_area,
    );
    let arranged_world = resolve_import_world(
        library,
        target_board_id,
        desired_world,
        reserved_world,
        viewport_world,
        decoded.source_width as f32 * initial_scale.max(0.01),
        decoded.source_height as f32 * initial_scale.max(0.01),
        import_gap,
    )?;

    let store_started_at = Instant::now();
    let source_name = remote_image_source_name(&downloaded.url);
    let stored_path =
        library.store_original_bytes(&source_name, &decoded.extension, &downloaded.bytes)?;
    let record = library.persist_imported_image_resolving_placement(
        target_board_id,
        placement_id,
        &stored_path,
        &decoded.mime_type,
        decoded.source_width,
        decoded.source_height,
        downloaded.bytes.len() as u64,
        arranged_world,
        initial_scale,
    )?;
    let store_and_persist = store_started_at.elapsed();

    let preview_started_at = Instant::now();
    let canvas_source = canvas_source_image_from_rgba(
        decoded.width,
        decoded.height,
        &decoded.rgba,
        canvas_image_settings,
    )?;
    let _ = library.store_canvas_image_cache(
        record.asset.id,
        canvas_image_settings.max_side,
        &canvas_source.rgba,
        canvas_source.width,
        canvas_source.height,
    );
    let preview = preview_image_from_rgba(
        canvas_source.width,
        canvas_source.height,
        &canvas_source.rgba,
        max_texture_side,
    )?;
    let animated_preview = decoded
        .animation
        .as_ref()
        .map(|animation| {
            preview_animation_from_frames(
                decoded.width,
                decoded.height,
                &animation.frames,
                animated_preview_max_side(canvas_image_settings, max_texture_side),
            )
        })
        .transpose()?;
    let preview_texture = if animated_preview.is_some() {
        ImportedPreviewTexture::Cpu
    } else {
        build_preview_texture(
            wgpu_render_state,
            &PreviewImage {
                rgba: preview.rgba.clone(),
                width: preview.width,
                height: preview.height,
            },
        )?
    };
    let preview_prepare = preview_started_at.elapsed();

    if let Some(trace) = &mut trace {
        trace.timings.store_and_persist = store_and_persist;
        trace.timings.preview_prepare = preview_prepare;
        trace.timings.worker_total = worker_started_at.elapsed();
    }

    Ok(LoadedCanvasItem {
        target_board_id,
        queue_id,
        record,
        preview_rgba: preview.rgba,
        preview_width: preview.width,
        preview_height: preview.height,
        animated_preview,
        preview_texture,
        loaded_lod: CanvasImageLod::Working,
        existing_load_diagnostics: None,
        origin: ImportOrigin::Url,
        trace,
    })
}

fn import_downloaded_svg_sync_traced(
    library: &Library,
    target_board_id: i64,
    queue_id: Option<u64>,
    placement_id: Option<i64>,
    downloaded: DownloadedUrlImage,
    desired_world: egui::Pos2,
    reserved_world: Option<egui::Pos2>,
    viewport_world: egui::Rect,
    auto_scale: bool,
    target_canvas_area: f32,
    import_gap: f32,
    worker_started_at: Instant,
    mut trace: Option<ImportTrace>,
) -> Result<LoadedCanvasItem, String> {
    let source_label = downloaded.url.as_str().to_owned();
    let decode_started_at = Instant::now();
    let svg =
        parse_svg_document(downloaded.bytes).map_err(|error| format!("{source_label}: {error}"))?;
    if let Some(trace) = &mut trace {
        trace.timings.decode_or_load += decode_started_at.elapsed();
    }

    let initial_scale = initial_import_scale(svg.width, svg.height, auto_scale, target_canvas_area);
    let arranged_world = resolve_import_world(
        library,
        target_board_id,
        desired_world,
        reserved_world,
        viewport_world,
        svg.width as f32 * initial_scale.max(0.01),
        svg.height as f32 * initial_scale.max(0.01),
        import_gap,
    )?;

    let store_started_at = Instant::now();
    let source_name = remote_image_source_name(&downloaded.url);
    let stored_path = library.store_original_bytes(&source_name, "svg", &svg.bytes)?;
    let record = library.persist_imported_image_resolving_placement(
        target_board_id,
        placement_id,
        &stored_path,
        SVG_MIME_TYPE,
        svg.width,
        svg.height,
        svg.bytes.len() as u64,
        arranged_world,
        initial_scale,
    )?;
    let store_and_persist = store_started_at.elapsed();

    if let Some(trace) = &mut trace {
        trace.timings.store_and_persist = store_and_persist;
        trace.timings.worker_total = worker_started_at.elapsed();
    }

    Ok(LoadedCanvasItem {
        target_board_id,
        queue_id,
        record,
        preview_rgba: Vec::new(),
        preview_width: svg.width,
        preview_height: svg.height,
        animated_preview: None,
        preview_texture: ImportedPreviewTexture::Svg {
            uri: format!("bytes://asset-svg-{}.svg", stable_svg_uri_key(&stored_path)),
            bytes: Arc::<[u8]>::from(svg.bytes),
            size: [svg.width as usize, svg.height as usize],
        },
        loaded_lod: CanvasImageLod::Working,
        existing_load_diagnostics: None,
        origin: ImportOrigin::Url,
        trace,
    })
}

fn prepare_import_image_with_helper(
    source_path: &Path,
    canvas_image_settings: CanvasImageSettings,
) -> Result<Option<PreparedImportImage>, String> {
    #[cfg(test)]
    {
        let _ = (source_path, canvas_image_settings);
        return Ok(None);
    }

    #[cfg(not(test))]
    {
        if std::env::var_os(IMAGE_IMPORT_HELPER_DISABLED_ENV).is_some() {
            return Ok(None);
        }

        let rgba_path = unique_image_import_helper_rgba_path();
        let output = Command::new(std::env::current_exe().map_err(|error| {
            format!("failed to locate image import helper executable: {error}")
        })?)
        .arg("--image-import-prepare")
        .arg(format!("--source={}", source_path.display()))
        .arg(format!("--rgba-output={}", rgba_path.display()))
        .arg(format!(
            "--canvas-max-side={}",
            canvas_image_settings.max_side
        ))
        .output()
        .map_err(|error| format!("failed to run image import helper: {error}"))?;

        if !output.status.success() {
            let _ = std::fs::remove_file(&rgba_path);
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!(
                "image import helper failed for {}: {}",
                source_path.display(),
                stderr.trim()
            ));
        }

        let prepared: ImageImportPrepareOutput = serde_json::from_slice(&output.stdout)
            .map_err(|error| format!("failed to decode image import helper output: {error}"))?;
        if prepared.animated {
            let _ = std::fs::remove_file(&rgba_path);
            return Ok(None);
        }

        let output_path = prepared.rgba_path.as_ref().unwrap_or(&rgba_path);
        let rgba = std::fs::read(output_path)
            .map_err(|error| format!("failed to read image import helper pixels: {error}"))?;
        let _ = std::fs::remove_file(output_path);
        let expected_len = prepared.width as usize * prepared.height as usize * 4;
        if rgba.len() != expected_len {
            return Err(format!(
                "image import helper returned {} bytes for {}x{} RGBA image; expected {}",
                rgba.len(),
                prepared.width,
                prepared.height,
                expected_len
            ));
        }

        Ok(Some(PreparedImportImage {
            rgba,
            width: prepared.width,
            height: prepared.height,
            source_width: prepared.source_width,
            source_height: prepared.source_height,
            mime_type: prepared.mime_type,
            file_size: prepared.file_size,
            cache_path: prepared.cache_path,
            cache_extension: prepared.cache_extension,
        }))
    }
}

fn install_prepared_canvas_cache(
    library: &Library,
    asset_id: i64,
    max_side: u32,
    lod: CanvasImageLod,
    prepared: &PreparedImportImage,
) -> Option<PathBuf> {
    let (Some(cache_path), Some(cache_extension)) = (
        prepared.cache_path.as_ref(),
        prepared.cache_extension.as_deref(),
    ) else {
        return None;
    };
    let destination = match library.canvas_image_cache_destination_for_extension(
        asset_id,
        max_side,
        lod,
        cache_extension,
    ) {
        Ok(destination) => destination,
        Err(_) => {
            let _ = std::fs::remove_file(cache_path);
            return None;
        }
    };
    match std::fs::rename(cache_path, &destination) {
        Ok(()) => Some(destination),
        Err(_) => {
            let _ = std::fs::remove_file(cache_path);
            None
        }
    }
}

#[cfg(not(test))]
fn unique_image_import_helper_rgba_path() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!(
        "snappyref-import-prepare-{}-{nanos}.rgba",
        std::process::id()
    ))
}

pub(crate) fn print_image_import_prepare_output_from_args() -> Result<(), String> {
    let mut source_path = None;
    let mut rgba_output_path = None;
    let mut canvas_max_side = None;
    for argument in std::env::args().skip(1) {
        if let Some(value) = argument.strip_prefix("--source=") {
            source_path = Some(PathBuf::from(value));
        } else if let Some(value) = argument.strip_prefix("--rgba-output=") {
            rgba_output_path = Some(PathBuf::from(value));
        } else if let Some(value) = argument.strip_prefix("--canvas-max-side=") {
            canvas_max_side = Some(
                value
                    .parse::<u32>()
                    .map_err(|error| format!("invalid --canvas-max-side value: {error}"))?,
            );
        }
    }
    let source_path = source_path.ok_or_else(|| "missing --source".to_owned())?;
    let rgba_output_path = rgba_output_path.ok_or_else(|| "missing --rgba-output".to_owned())?;
    let canvas_max_side = canvas_max_side.ok_or_else(|| "missing --canvas-max-side".to_owned())?;
    print_image_import_prepare_output(
        &source_path,
        &rgba_output_path,
        CanvasImageSettings {
            max_side: canvas_max_side,
        },
    )
}

fn print_image_import_prepare_output(
    source_path: &Path,
    rgba_output_path: &Path,
    canvas_image_settings: CanvasImageSettings,
) -> Result<(), String> {
    let decoded =
        decode_image_file_with_max_side(source_path, Some(canvas_image_settings.max_side))
            .map_err(|error| format!("{}: {error}", source_path.display()))?;
    if decoded.animation.is_some() {
        serde_json::to_writer(
            std::io::stdout(),
            &ImageImportPrepareOutput {
                animated: true,
                rgba_path: None,
                width: 0,
                height: 0,
                source_width: decoded.source_width,
                source_height: decoded.source_height,
                mime_type: decoded.mime_type,
                file_size: decoded.file_size,
                cache_path: None,
                cache_extension: None,
            },
        )
        .map_err(|error| format!("failed to write image import helper output: {error}"))?;
        return Ok(());
    }

    let canvas_source = canvas_source_image_from_rgba(
        decoded.width,
        decoded.height,
        &decoded.rgba,
        canvas_image_settings,
    )?;
    std::fs::write(rgba_output_path, &canvas_source.rgba)
        .map_err(|error| format!("failed to write image import helper pixels: {error}"))?;
    let (cache_path, cache_extension) = write_prepared_canvas_cache(
        rgba_output_path,
        &canvas_source.rgba,
        canvas_source.width,
        canvas_source.height,
    )?;
    serde_json::to_writer(
        std::io::stdout(),
        &ImageImportPrepareOutput {
            animated: false,
            rgba_path: Some(rgba_output_path.to_path_buf()),
            width: canvas_source.width,
            height: canvas_source.height,
            source_width: decoded.source_width,
            source_height: decoded.source_height,
            mime_type: decoded.mime_type,
            file_size: decoded.file_size,
            cache_path: Some(cache_path),
            cache_extension: Some(cache_extension.to_owned()),
        },
    )
    .map_err(|error| format!("failed to write image import helper output: {error}"))?;
    Ok(())
}

fn write_prepared_canvas_cache(
    rgba_output_path: &Path,
    rgba: &[u8],
    width: u32,
    height: u32,
) -> Result<(PathBuf, &'static str), String> {
    let (cache_path, extension) = if rgba.chunks_exact(4).all(|pixel| pixel[3] == u8::MAX) {
        (rgba_output_path.with_extension("jpg"), "jpg")
    } else {
        (rgba_output_path.with_extension("webp"), "webp")
    };
    let file = std::fs::File::create(&cache_path)
        .map_err(|error| format!("failed to create image import helper cache: {error}"))?;
    let writer = std::io::BufWriter::new(file);
    match extension {
        "jpg" => {
            let rgb = rgba_to_rgb_for_import_cache(rgba);
            JpegEncoder::new_with_quality(writer, 82)
                .write_image(&rgb, width, height, ExtendedColorType::Rgb8)
                .map_err(|error| {
                    format!("failed to write image import helper JPEG cache: {error}")
                })?;
        }
        "webp" => {
            WebPEncoder::new_lossless(writer)
                .write_image(rgba, width, height, ExtendedColorType::Rgba8)
                .map_err(|error| {
                    format!("failed to write image import helper WebP cache: {error}")
                })?;
        }
        _ => unreachable!("cache extension is selected locally"),
    }
    Ok((cache_path, extension))
}

fn rgba_to_rgb_for_import_cache(rgba: &[u8]) -> Vec<u8> {
    let mut rgb = Vec::with_capacity(rgba.len() / 4 * 3);
    for pixel in rgba.chunks_exact(4) {
        rgb.extend_from_slice(&pixel[..3]);
    }
    rgb
}

fn import_clipboard_sync_traced(
    library: &Library,
    target_board_id: i64,
    queue_id: u64,
    placement_id: Option<i64>,
    rgba: Vec<u8>,
    width: u32,
    height: u32,
    desired_world: egui::Pos2,
    reserved_world: Option<egui::Pos2>,
    viewport_world: egui::Rect,
    max_texture_side: usize,
    canvas_image_settings: CanvasImageSettings,
    auto_scale: bool,
    target_canvas_area: f32,
    import_gap: f32,
    trace: Option<ImportTrace>,
    wgpu_render_state: Option<&eframe::egui_wgpu::RenderState>,
) -> Result<LoadedCanvasItem, String> {
    let worker_started_at = Instant::now();
    let mut trace = trace;
    if let Some(trace) = &mut trace {
        trace.timings.queue_wait = worker_started_at.saturating_duration_since(trace.queued_at);
    }

    let store_started_at = Instant::now();
    let stored_path = library.store_clipboard_image(&rgba, width, height)?;
    let file_size = std::fs::metadata(&stored_path)
        .map_err(|error| format!("failed to read clipboard image metadata: {error}"))?
        .len();
    let initial_scale = initial_import_scale(width, height, auto_scale, target_canvas_area);
    let arranged_world = resolve_import_world(
        library,
        target_board_id,
        desired_world,
        reserved_world,
        viewport_world,
        width as f32 * initial_scale.max(0.01),
        height as f32 * initial_scale.max(0.01),
        import_gap,
    )?;
    let record = library.persist_imported_image_resolving_placement(
        target_board_id,
        placement_id,
        &stored_path,
        image::ImageFormat::Png.to_mime_type(),
        width,
        height,
        file_size,
        arranged_world,
        initial_scale,
    )?;
    let store_and_persist = store_started_at.elapsed();
    let preview_started_at = Instant::now();
    let canvas_source = canvas_source_image_from_rgba(width, height, &rgba, canvas_image_settings)?;
    let _ = library.store_canvas_image_cache(
        record.asset.id,
        canvas_image_settings.max_side,
        &canvas_source.rgba,
        canvas_source.width,
        canvas_source.height,
    );
    let preview = preview_image_from_rgba(
        canvas_source.width,
        canvas_source.height,
        &canvas_source.rgba,
        max_texture_side,
    )?;
    let preview_texture = build_preview_texture(wgpu_render_state, &preview)?;
    if let Some(trace) = &mut trace {
        trace.timings.store_and_persist = store_and_persist;
        trace.timings.preview_prepare = preview_started_at.elapsed();
        trace.timings.worker_total = worker_started_at.elapsed();
    }

    Ok(LoadedCanvasItem {
        target_board_id,
        queue_id: Some(queue_id),
        record,
        preview_rgba: preview.rgba,
        preview_width: preview.width,
        preview_height: preview.height,
        animated_preview: None,
        preview_texture,
        loaded_lod: CanvasImageLod::Working,
        existing_load_diagnostics: None,
        origin: ImportOrigin::Clipboard,
        trace,
    })
}

pub(crate) fn load_existing(
    library: &Library,
    target_board_id: i64,
    queue_id: Option<u64>,
    record: BoardItemWithAsset,
    max_texture_side: usize,
    canvas_image_settings: CanvasImageSettings,
    target_lod: CanvasImageLod,
    trace: Option<ImportTrace>,
    wgpu_render_state: Option<&eframe::egui_wgpu::RenderState>,
) -> ImportResult {
    let worker_started_at = Instant::now();
    let mut trace = trace;
    if let Some(trace) = &mut trace {
        trace.timings.queue_wait = worker_started_at.saturating_duration_since(trace.queued_at);
    }

    if original_mime_type_is_svg(&record.asset.mime_type) {
        let decode_started_at = Instant::now();
        match read_svg_document(&record.asset.original_path) {
            Ok(svg) => {
                if let Some(trace) = &mut trace {
                    trace.timings.decode_or_load = decode_started_at.elapsed();
                    trace.timings.worker_total = worker_started_at.elapsed();
                }
                return ImportResult::Loaded(LoadedCanvasItem {
                    target_board_id,
                    queue_id,
                    preview_rgba: Vec::new(),
                    preview_width: svg.width,
                    preview_height: svg.height,
                    preview_texture: ImportedPreviewTexture::Svg {
                        uri: format!(
                            "bytes://asset-svg-{}.svg",
                            stable_svg_uri_key(&record.asset.original_path)
                        ),
                        bytes: Arc::<[u8]>::from(svg.bytes),
                        size: [record.asset.width as usize, record.asset.height as usize],
                    },
                    record,
                    animated_preview: None,
                    loaded_lod: target_lod,
                    existing_load_diagnostics: None,
                    origin: ImportOrigin::Existing,
                    trace,
                });
            }
            Err(error) => {
                return ImportResult::Failed {
                    target_board_id: Some(target_board_id),
                    queue_id,
                    placement_id: None,
                    item_id: Some(record.item.id),
                    context: record.asset.original_path.display().to_string(),
                    error,
                };
            }
        }
    }

    let decode_started_at = Instant::now();
    let preview_started_at = Instant::now();
    match load_existing_preview(
        library,
        &record,
        canvas_image_settings,
        target_lod,
        max_texture_side,
    ) {
        Ok((preview, animated_preview, diagnostics)) => {
            let decode_or_load = decode_started_at.elapsed();
            let preview_texture = if animated_preview.is_some() {
                ImportedPreviewTexture::Cpu
            } else {
                match build_preview_texture(wgpu_render_state, &preview) {
                    Ok(preview_texture) => preview_texture,
                    Err(error) => {
                        return ImportResult::Failed {
                            target_board_id: Some(target_board_id),
                            queue_id,
                            placement_id: None,
                            item_id: Some(record.item.id),
                            context: record.asset.original_path.display().to_string(),
                            error,
                        };
                    }
                }
            };
            if let Some(trace) = &mut trace {
                trace.timings.decode_or_load = decode_or_load;
                trace.timings.preview_prepare = preview_started_at.elapsed();
                trace.timings.worker_total = worker_started_at.elapsed();
            }
            ImportResult::Loaded(LoadedCanvasItem {
                target_board_id,
                queue_id,
                record,
                preview_rgba: preview.rgba,
                preview_width: preview.width,
                preview_height: preview.height,
                animated_preview,
                preview_texture,
                loaded_lod: target_lod,
                existing_load_diagnostics: Some(diagnostics),
                origin: ImportOrigin::Existing,
                trace,
            })
        }
        Err(error) => ImportResult::Failed {
            target_board_id: Some(target_board_id),
            queue_id,
            placement_id: None,
            item_id: Some(record.item.id),
            context: record.asset.original_path.display().to_string(),
            error,
        },
    }
}

fn record_viewport_distance_sq(record: &BoardItemWithAsset, viewport_world: egui::Rect) -> f32 {
    let rect = record_world_rect(record);
    if rect.intersects(viewport_world) {
        return 0.0;
    }
    let center = rect.center();
    let clamped = egui::pos2(
        center
            .x
            .clamp(viewport_world.left(), viewport_world.right()),
        center
            .y
            .clamp(viewport_world.top(), viewport_world.bottom()),
    );
    center.distance_sq(clamped)
}

fn record_intersects_viewport_world(
    record: &BoardItemWithAsset,
    viewport_world: egui::Rect,
) -> bool {
    record_world_rect(record).intersects(viewport_world)
}

fn record_world_rect(record: &BoardItemWithAsset) -> egui::Rect {
    let size = egui::vec2(record.asset.width as f32, record.asset.height as f32)
        * record.item.scale.max(0.01);
    egui::Rect::from_center_size(egui::pos2(record.item.x, record.item.y), size)
}

fn crop_item(
    library: &Library,
    request: crate::canvas::CanvasCropRequest,
    max_texture_side: usize,
    canvas_image_settings: CanvasImageSettings,
    wgpu_render_state: Option<&eframe::egui_wgpu::RenderState>,
) -> ImportResult {
    let source_context = request.source_asset.original_path.display().to_string();
    if original_mime_type_is_svg(&request.source_asset.mime_type) {
        return ImportResult::Failed {
            target_board_id: None,
            queue_id: None,
            placement_id: None,
            item_id: Some(request.item_id),
            context: source_context,
            error: "SVG cropping is not supported yet".to_owned(),
        };
    }

    let decoded = match decode_image_file(&request.source_asset.original_path) {
        Ok(decoded) => decoded,
        Err(error) => {
            return ImportResult::Failed {
                target_board_id: None,
                queue_id: None,
                placement_id: None,
                item_id: Some(request.item_id),
                context: source_context,
                error,
            };
        }
    };
    let image = match RgbaImage::from_vec(decoded.width, decoded.height, decoded.rgba) {
        Some(image) => image,
        None => {
            return ImportResult::Failed {
                target_board_id: None,
                queue_id: None,
                placement_id: None,
                item_id: Some(request.item_id),
                context: source_context,
                error: "failed to rebuild source image pixels for crop".to_owned(),
            };
        }
    };

    let cropped = image::imageops::crop_imm(
        &image,
        request.crop_rect.x,
        request.crop_rect.y,
        request.crop_rect.width,
        request.crop_rect.height,
    )
    .to_image();
    let cropped_rgba = cropped.into_raw();
    let width = request.crop_rect.width;
    let height = request.crop_rect.height;

    let base_name = std::path::Path::new(&request.source_asset.filename)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("image");
    let stored_path = match library.store_cropped_rgba_image(
        &format!("{base_name}-crop"),
        &cropped_rgba,
        width,
        height,
    ) {
        Ok(path) => path,
        Err(error) => {
            return ImportResult::Failed {
                target_board_id: None,
                queue_id: None,
                placement_id: None,
                item_id: Some(request.item_id),
                context: source_context,
                error,
            };
        }
    };

    let file_size = match std::fs::metadata(&stored_path) {
        Ok(metadata) => metadata.len(),
        Err(error) => {
            let _ = std::fs::remove_file(&stored_path);
            return ImportResult::Failed {
                target_board_id: None,
                queue_id: None,
                placement_id: None,
                item_id: Some(request.item_id),
                context: stored_path.display().to_string(),
                error: format!("failed to read cropped image metadata: {error}"),
            };
        }
    };
    let preview_max_side = animated_preview_max_side(
        CanvasImageSettings {
            max_side: INTERACTIVE_CROP_PREVIEW_MAX_SIDE.min(canvas_image_settings.max_side),
        },
        max_texture_side,
    );
    let preview = match fast_preview_image_from_rgba(width, height, &cropped_rgba, preview_max_side)
    {
        Ok(preview) => preview,
        Err(error) => {
            let _ = std::fs::remove_file(&stored_path);
            return ImportResult::Failed {
                target_board_id: None,
                queue_id: None,
                placement_id: None,
                item_id: Some(request.item_id),
                context: source_context,
                error,
            };
        }
    };
    let preview_texture = match build_preview_texture(wgpu_render_state, &preview) {
        Ok(preview_texture) => preview_texture,
        Err(error) => {
            let _ = std::fs::remove_file(&stored_path);
            return ImportResult::Failed {
                target_board_id: None,
                queue_id: None,
                placement_id: None,
                item_id: Some(request.item_id),
                context: source_context,
                error,
            };
        }
    };
    ImportResult::Cropped(CroppedCanvasItem {
        item_id: request.item_id,
        stored_path,
        width,
        height,
        file_size,
        preview_rgba: preview.rgba,
        preview_width: preview.width,
        preview_height: preview.height,
        preview_texture,
        loaded_lod: CanvasImageLod::Overview,
        crop_offset_local: request.crop_offset_local,
    })
}

fn lod_max_side(canvas_image_settings: CanvasImageSettings, lod: CanvasImageLod) -> u32 {
    match lod {
        CanvasImageLod::Overview => OVERVIEW_CACHE_MAX_SIDE.min(canvas_image_settings.max_side),
        CanvasImageLod::Working => canvas_image_settings.max_side,
        CanvasImageLod::Detail => DETAIL_CACHE_MAX_SIDE.max(canvas_image_settings.max_side),
    }
}

fn lod_settings(
    canvas_image_settings: CanvasImageSettings,
    lod: CanvasImageLod,
) -> CanvasImageSettings {
    CanvasImageSettings {
        max_side: lod_max_side(canvas_image_settings, lod),
    }
}

fn load_existing_preview(
    library: &Library,
    record: &BoardItemWithAsset,
    canvas_image_settings: CanvasImageSettings,
    target_lod: CanvasImageLod,
    max_texture_side: usize,
) -> Result<
    (
        PreviewImage,
        Option<AnimatedPreview>,
        ExistingLoadDiagnostics,
    ),
    String,
> {
    if original_mime_type_is_svg(&record.asset.mime_type) {
        let svg = read_svg_document(&record.asset.original_path)?;
        let diagnostics = ExistingLoadDiagnostics {
            source_kind: ExistingLoadSourceKind::Original,
            source_path: record.asset.original_path.clone(),
            source_file_size: svg.bytes.len() as u64,
            source_width: svg.width,
            source_height: svg.height,
            backfilled_cache: false,
        };
        return Ok((
            PreviewImage {
                rgba: Vec::new(),
                width: svg.width,
                height: svg.height,
            },
            None,
            diagnostics,
        ));
    }

    let lod_settings = lod_settings(canvas_image_settings, target_lod);
    let (canvas_source, animated_preview, diagnostics) =
        if original_mime_type_supports_animation(&record.asset.mime_type) {
            let decoded = decode_image_file(&record.asset.original_path)?;
            let source_file_size = decoded.file_size;
            (
                preview_image_from_rgba(
                    decoded.width,
                    decoded.height,
                    &decoded.rgba,
                    lod_settings.max_side.min(max_texture_side as u32) as usize,
                )?,
                decoded
                    .animation
                    .as_ref()
                    .map(|animation| {
                        preview_animation_from_frames(
                            decoded.width,
                            decoded.height,
                            &animation.frames,
                            lod_settings.max_side.min(max_texture_side as u32) as usize,
                        )
                    })
                    .transpose()?,
                ExistingLoadDiagnostics {
                    source_kind: ExistingLoadSourceKind::Original,
                    source_path: record.asset.original_path.clone(),
                    source_file_size,
                    source_width: decoded.source_width,
                    source_height: decoded.source_height,
                    backfilled_cache: false,
                },
            )
        } else if let Some(cache_path) = library.canvas_image_cache_path_for_lod(
            record.asset.id,
            lod_settings.max_side,
            target_lod,
        ) && cache_path.is_file()
        {
            match decode_image_file(&cache_path) {
                Ok(decoded) => {
                    let source_file_size = decoded.file_size;
                    (
                        PreviewImage {
                            rgba: decoded.rgba,
                            width: decoded.width,
                            height: decoded.height,
                        },
                        None,
                        ExistingLoadDiagnostics {
                            source_kind: ExistingLoadSourceKind::Cache,
                            source_path: cache_path,
                            source_file_size,
                            source_width: decoded.width,
                            source_height: decoded.height,
                            backfilled_cache: false,
                        },
                    )
                }
                Err(_) => {
                    let _ = std::fs::remove_file(&cache_path);
                    let (preview, diagnostics) =
                        backfill_canvas_image_cache(library, record, lod_settings, target_lod)?;
                    (preview, None, diagnostics)
                }
            }
        } else {
            let (preview, diagnostics) =
                backfill_canvas_image_cache(library, record, lod_settings, target_lod)?;
            (preview, None, diagnostics)
        };

    let preview = if let Some(animated_preview) = &animated_preview {
        PreviewImage {
            rgba: animated_preview.frames[0].rgba.clone(),
            width: animated_preview.width,
            height: animated_preview.height,
        }
    } else if canvas_source.width <= max_texture_side as u32
        && canvas_source.height <= max_texture_side as u32
    {
        canvas_source
    } else {
        preview_image_from_rgba(
            canvas_source.width,
            canvas_source.height,
            &canvas_source.rgba,
            max_texture_side,
        )?
    };

    Ok((preview, animated_preview, diagnostics))
}

pub fn load_canvas_snapshot_pixels(
    library: &Library,
    record: &BoardItemWithAsset,
    target_width: u32,
    target_height: u32,
) -> Result<(Vec<u8>, u32, u32), String> {
    if target_width == 0 || target_height == 0 {
        return Err("cannot cache zero-sized canvas pixels".to_owned());
    }
    if original_mime_type_is_svg(&record.asset.mime_type) {
        return Err("cannot cache SVG canvas pixels".to_owned());
    }

    for path in
        canvas_snapshot_cache_candidates(library, record.asset.id, target_width, target_height)
    {
        if !path.is_file() {
            continue;
        }
        match decode_image_file(&path) {
            Ok(decoded) => {
                return resize_rgba_exact(
                    decoded.rgba,
                    decoded.width,
                    decoded.height,
                    target_width,
                    target_height,
                );
            }
            Err(_) => {
                let _ = std::fs::remove_file(&path);
            }
        }
    }

    let decoded = decode_image_file_with_max_side(
        &record.asset.original_path,
        Some(target_width.max(target_height)),
    )?;
    resize_rgba_exact(
        decoded.rgba,
        decoded.width,
        decoded.height,
        target_width,
        target_height,
    )
}

fn canvas_snapshot_cache_candidates(
    library: &Library,
    asset_id: i64,
    target_width: u32,
    target_height: u32,
) -> Vec<PathBuf> {
    let max_side = target_width.max(target_height);
    let mut paths = Vec::new();
    for lod in [
        CanvasImageLod::Working,
        CanvasImageLod::Overview,
        CanvasImageLod::Detail,
    ] {
        if let Some(path) = library.canvas_image_cache_path_for_lod(asset_id, max_side, lod) {
            paths.push(path);
        }
    }
    paths
}

fn resize_rgba_exact(
    rgba: Vec<u8>,
    width: u32,
    height: u32,
    target_width: u32,
    target_height: u32,
) -> Result<(Vec<u8>, u32, u32), String> {
    if width == target_width && height == target_height {
        return Ok((rgba, width, height));
    }

    let image = RgbaImage::from_raw(width, height, rgba)
        .ok_or_else(|| "failed to rebuild RGBA image for canvas pixel cache".to_owned())?;
    let resized = image::imageops::resize(
        &image,
        target_width,
        target_height,
        image::imageops::FilterType::Triangle,
    );
    Ok((resized.into_raw(), target_width, target_height))
}

fn original_mime_type_supports_animation(mime_type: &str) -> bool {
    mime_type == image::ImageFormat::Gif.to_mime_type()
        || mime_type == image::ImageFormat::WebP.to_mime_type()
}

const SVG_MIME_TYPE: &str = "image/svg+xml";

struct SvgDocument {
    bytes: Vec<u8>,
    width: u32,
    height: u32,
}

fn is_svg_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("svg"))
}

fn original_mime_type_is_svg(mime_type: &str) -> bool {
    mime_type
        .split(';')
        .next()
        .unwrap_or(mime_type)
        .trim()
        .eq_ignore_ascii_case(SVG_MIME_TYPE)
}

fn read_svg_document(path: &Path) -> Result<SvgDocument, String> {
    let bytes = std::fs::read(path).map_err(|error| format!("failed to open SVG: {error}"))?;
    parse_svg_document(bytes)
}

fn parse_svg_document(bytes: Vec<u8>) -> Result<SvgDocument, String> {
    let tree = usvg::Tree::from_data(&bytes, &usvg::Options::default())
        .map_err(|error| format!("failed to parse SVG: {error}"))?;
    let size = tree.size();
    let width = size.width().round().max(1.0) as u32;
    let height = size.height().round().max(1.0) as u32;
    Ok(SvgDocument {
        bytes,
        width,
        height,
    })
}

fn downloaded_url_image_is_svg(downloaded: &DownloadedUrlImage) -> bool {
    downloaded
        .mime_type
        .as_deref()
        .is_some_and(original_mime_type_is_svg)
        || downloaded
            .url
            .path_segments()
            .and_then(|mut segments| segments.next_back())
            .is_some_and(|name| {
                Path::new(name)
                    .extension()
                    .and_then(|extension| extension.to_str())
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("svg"))
            })
}

fn stable_svg_uri_key(path: &Path) -> u64 {
    use std::hash::{DefaultHasher, Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    path.hash(&mut hasher);
    hasher.finish()
}

fn canvas_source_image_from_rgba(
    width: u32,
    height: u32,
    rgba: &[u8],
    canvas_image_settings: CanvasImageSettings,
) -> Result<PreviewImage, String> {
    preview_image_from_rgba(width, height, rgba, canvas_image_settings.max_side as usize)
}

fn fast_preview_image_from_rgba(
    width: u32,
    height: u32,
    rgba: &[u8],
    max_side: usize,
) -> Result<PreviewImage, String> {
    let (preview_width, preview_height) =
        crate::jobs::image_formats::fitted_texture_size(width as usize, height as usize, max_side);

    if preview_width == width as usize && preview_height == height as usize {
        return Ok(PreviewImage {
            rgba: rgba.to_vec(),
            width,
            height,
        });
    }

    let image = RgbaImage::from_raw(width, height, rgba.to_vec())
        .ok_or_else(|| "failed to rebuild RGBA image for crop preview resize".to_owned())?;
    let resized = image::imageops::resize(
        &image,
        preview_width as u32,
        preview_height as u32,
        image::imageops::FilterType::Nearest,
    );

    Ok(PreviewImage {
        rgba: resized.into_raw(),
        width: preview_width as u32,
        height: preview_height as u32,
    })
}

fn animated_preview_max_side(
    canvas_image_settings: CanvasImageSettings,
    max_texture_side: usize,
) -> usize {
    (canvas_image_settings.max_side as usize).min(max_texture_side)
}

fn backfill_canvas_image_cache(
    library: &Library,
    record: &BoardItemWithAsset,
    canvas_image_settings: CanvasImageSettings,
    lod: CanvasImageLod,
) -> Result<(PreviewImage, ExistingLoadDiagnostics), String> {
    let decode_path = preferred_backfill_source_path(library, record, canvas_image_settings, lod)
        .unwrap_or_else(|| record.asset.original_path.clone());
    if let Some(prepared) =
        prepare_import_image_with_helper(&decode_path, canvas_image_settings).unwrap_or(None)
    {
        let source_path = install_prepared_canvas_cache(
            library,
            record.asset.id,
            canvas_image_settings.max_side,
            lod,
            &prepared,
        )
        .unwrap_or_else(|| decode_path.clone());
        let source_file_size = std::fs::metadata(&source_path)
            .map(|metadata| metadata.len())
            .unwrap_or(0);
        return Ok((
            PreviewImage {
                rgba: prepared.rgba,
                width: prepared.width,
                height: prepared.height,
            },
            ExistingLoadDiagnostics {
                source_kind: ExistingLoadSourceKind::Cache,
                source_path,
                source_file_size,
                source_width: prepared.width,
                source_height: prepared.height,
                backfilled_cache: true,
            },
        ));
    }

    let decoded =
        decode_image_file_with_max_side(&decode_path, Some(canvas_image_settings.max_side))?;
    let canvas_source = canvas_source_image_from_rgba(
        decoded.width,
        decoded.height,
        &decoded.rgba,
        canvas_image_settings,
    )?;
    let source_path = library
        .store_canvas_image_cache_for_lod(
            record.asset.id,
            canvas_image_settings.max_side,
            lod,
            &canvas_source.rgba,
            canvas_source.width,
            canvas_source.height,
        )?
        .unwrap_or_else(|| record.asset.original_path.clone());
    let source_file_size = std::fs::metadata(&source_path)
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    let source_width = canvas_source.width;
    let source_height = canvas_source.height;
    Ok((
        canvas_source,
        ExistingLoadDiagnostics {
            source_kind: ExistingLoadSourceKind::Cache,
            source_path,
            source_file_size,
            source_width,
            source_height,
            backfilled_cache: true,
        },
    ))
}

fn preferred_backfill_source_path(
    library: &Library,
    record: &BoardItemWithAsset,
    canvas_image_settings: CanvasImageSettings,
    lod: CanvasImageLod,
) -> Option<PathBuf> {
    match lod {
        CanvasImageLod::Working => None,
        CanvasImageLod::Overview => library.canvas_image_cache_path_for_lod(
            record.asset.id,
            canvas_image_settings.max_side,
            CanvasImageLod::Working,
        ),
        CanvasImageLod::Detail => None,
    }
}

fn backfill_cache_lod(
    library: &Library,
    record: BoardItemWithAsset,
    canvas_image_settings: CanvasImageSettings,
    lod: CanvasImageLod,
) -> ImportResult {
    let lod_settings = lod_settings(canvas_image_settings, lod);
    match backfill_canvas_image_cache(library, &record, lod_settings, lod) {
        Ok(_) => ImportResult::CacheLodBackfilled {
            asset_id: record.asset.id,
            lod,
        },
        Err(error) => ImportResult::CacheLodBackfillFailed {
            asset_id: record.asset.id,
            lod,
            error,
        },
    }
}

pub fn initial_import_scale(
    asset_width: u32,
    asset_height: u32,
    auto_scale: bool,
    target_canvas_area: f32,
) -> f32 {
    if !auto_scale || asset_width == 0 || asset_height == 0 {
        return 1.0;
    }

    let target_canvas_area = target_canvas_area.max(1.0);
    let asset_area = asset_width as f32 * asset_height as f32;
    (target_canvas_area / asset_area).sqrt().min(1.0)
}

mod arrangement;

pub use arrangement::arrange_rect_world;
#[cfg(test)]
pub(super) use arrangement::rects_overlap_with_gap;
use arrangement::resolve_import_world;

fn download_remote_image(url: &str) -> Result<DownloadedUrlImage, String> {
    let parsed = url::Url::parse(url).map_err(|error| format!("invalid image URL: {error}"))?;
    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        return Err("only http and https image URLs are supported".to_owned());
    }

    let client = reqwest::blocking::Client::builder()
        .connect_timeout(URL_IMPORT_CONNECT_TIMEOUT)
        .timeout(URL_IMPORT_REQUEST_TIMEOUT)
        .build()
        .map_err(|error| format!("failed to build HTTP client: {error}"))?;
    let mut response = client
        .get(parsed.clone())
        .send()
        .and_then(reqwest::blocking::Response::error_for_status)
        .map_err(|error| format!("failed to download image: {error}"))?;

    let content_length = response.content_length();
    if let Some(content_length) = content_length
        && content_length > URL_IMPORT_MAX_BYTES
    {
        return Err(format!(
            "remote image is too large ({content_length} bytes > {URL_IMPORT_MAX_BYTES} byte limit)"
        ));
    }

    let mime_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);

    let mut bytes = Vec::new();
    let limit = URL_IMPORT_MAX_BYTES + 1;
    std::io::Read::take(&mut response, limit)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("failed to read downloaded image: {error}"))?;
    if bytes.len() as u64 > URL_IMPORT_MAX_BYTES {
        return Err(format!(
            "remote image exceeded the {} byte download limit",
            URL_IMPORT_MAX_BYTES
        ));
    }

    Ok(DownloadedUrlImage {
        url: parsed,
        bytes,
        mime_type,
    })
}

fn remote_image_source_name(url: &url::Url) -> String {
    let path_name = url
        .path_segments()
        .and_then(Iterator::last)
        .and_then(|segment| {
            segment
                .rsplit_once('.')
                .map(|(stem, _)| stem)
                .or(Some(segment))
        })
        .filter(|segment| !segment.is_empty())
        .map(str::to_owned);

    path_name.unwrap_or_else(|| {
        let host = url.host_str().unwrap_or("remote-image").replace('.', "-");
        format!("{host}-image")
    })
}

#[cfg(test)]
mod tests;

mod perf;

pub use perf::{
    file_import_perf_report, interactive_import_perf_report, startup_perf_report, startup_trace,
    worker_interactive_import_perf_report,
};
