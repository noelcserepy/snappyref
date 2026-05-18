use std::io::Read;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui;

use crate::data::CanvasImageLod;
use crate::jobs::image_formats::{
    PreviewImage, decode_image_bytes_with_max_side, decode_image_file_with_max_side,
    preview_animation_from_frames, preview_image_from_rgba,
};
use crate::library::storage::Library;

use super::arrangement::resolve_import_world;
use super::decode::{install_prepared_canvas_cache, prepare_image_with_helper};
use super::preview_texture::build_preview_texture;
use super::{
    CanvasImageSettings, DEFAULT_IMPORT_TARGET_CANVAS_AREA, ImagePrepPreviewTexture,
    ImagePrepResult, ImagePrepTrace, ImportOrigin, PreparedPlacementImage, SVG_MIME_TYPE,
    animated_preview_max_side, canvas_source_image_from_rgba, is_svg_path,
    original_mime_type_is_svg, parse_svg_document, read_svg_document, stable_svg_uri_key,
};

const URL_IMPORT_MAX_BYTES: u64 = 64 * 1024 * 1024;
const URL_IMPORT_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const URL_IMPORT_REQUEST_TIMEOUT: Duration = Duration::from_secs(45);

#[cfg_attr(not(test), allow(dead_code))]
pub fn import_path_sync(
    library: &Library,
    source_path: PathBuf,
    desired_world: egui::Pos2,
    viewport_world: egui::Rect,
    auto_scale: bool,
    placement_gap: f32,
) -> Result<PreparedPlacementImage, String> {
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
        placement_gap,
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
    placement_gap: f32,
) -> Result<PreparedPlacementImage, String> {
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
        placement_gap,
        None,
        None,
    )
}

pub(crate) fn import_path_sync_traced(
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
    placement_gap: f32,
    trace: Option<ImagePrepTrace>,
    wgpu_render_state: Option<&eframe::egui_wgpu::RenderState>,
) -> Result<PreparedPlacementImage, String> {
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
            placement_gap,
            worker_started_at,
            trace,
        );
    }

    let decode_started_at = Instant::now();
    if let Some(prepared) =
        prepare_image_with_helper(&source_path, canvas_image_settings).unwrap_or(None)
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
            placement_gap,
        )?;

        let store_started_at = Instant::now();
        let stored_path = library
            .store_original(&source_path)
            .map_err(|error| format!("{}: {error}", source_path.display()))?;

        let placement = library
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
            placement.asset.as_ref().expect("loaded placement asset").id,
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

        return Ok(PreparedPlacementImage {
            target_board_id,
            queue_id,
            placement,
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
        placement_gap,
    )?;

    let store_started_at = Instant::now();
    let stored_path = library
        .store_original(&source_path)
        .map_err(|error| format!("{}: {error}", source_path.display()))?;

    let placement = library
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
        placement.asset.as_ref().expect("loaded placement asset").id,
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
        ImagePrepPreviewTexture::Cpu
    } else {
        build_preview_texture(wgpu_render_state, &preview)?
    };
    if let Some(trace) = &mut trace {
        trace.timings.store_and_persist = store_and_persist;
        trace.timings.preview_prepare = preview_started_at.elapsed();
        trace.timings.worker_total = worker_started_at.elapsed();
    }

    Ok(PreparedPlacementImage {
        target_board_id,
        queue_id,
        placement,
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
    placement_gap: f32,
    worker_started_at: Instant,
    mut trace: Option<ImagePrepTrace>,
) -> Result<PreparedPlacementImage, String> {
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
        placement_gap,
    )?;

    let store_started_at = Instant::now();
    let stored_path = library
        .store_original(&source_path)
        .map_err(|error| format!("{}: {error}", source_path.display()))?;
    let file_size = std::fs::metadata(&stored_path)
        .map_err(|error| format!("failed to read stored SVG metadata: {error}"))?
        .len();
    let placement = library
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

    Ok(PreparedPlacementImage {
        target_board_id,
        queue_id,
        placement,
        preview_rgba: Vec::new(),
        preview_width: svg.width,
        preview_height: svg.height,
        animated_preview: None,
        preview_texture: ImagePrepPreviewTexture::Svg {
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

pub(crate) fn import_path(
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
    placement_gap: f32,
    trace: Option<ImagePrepTrace>,
    wgpu_render_state: Option<&eframe::egui_wgpu::RenderState>,
) -> ImagePrepResult {
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
        placement_gap,
        trace,
        wgpu_render_state,
    ) {
        Ok(loaded) => ImagePrepResult::Loaded(loaded),
        Err(error) => ImagePrepResult::Failed {
            target_board_id: Some(target_board_id),
            queue_id: Some(queue_id),
            placement_id,
            context: source_path.display().to_string(),
            error,
        },
    }
}

pub(crate) fn import_url(
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
    placement_gap: f32,
    trace: Option<ImagePrepTrace>,
    wgpu_render_state: Option<&eframe::egui_wgpu::RenderState>,
) -> ImagePrepResult {
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
        placement_gap,
        trace,
        wgpu_render_state,
    ) {
        Ok(loaded) => ImagePrepResult::Loaded(loaded),
        Err(error) => ImagePrepResult::Failed {
            target_board_id: Some(target_board_id),
            queue_id: Some(queue_id),
            placement_id,
            context: url,
            error,
        },
    }
}

pub(crate) fn import_clipboard(
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
    placement_gap: f32,
    trace: Option<ImagePrepTrace>,
    wgpu_render_state: Option<&eframe::egui_wgpu::RenderState>,
) -> ImagePrepResult {
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
        placement_gap,
        trace,
        wgpu_render_state,
    ) {
        Ok(loaded) => ImagePrepResult::Loaded(loaded),
        Err(error) => ImagePrepResult::Failed {
            target_board_id: Some(target_board_id),
            queue_id: Some(queue_id),
            placement_id,
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

pub(crate) fn import_url_sync_traced(
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
    placement_gap: f32,
    trace: Option<ImagePrepTrace>,
    wgpu_render_state: Option<&eframe::egui_wgpu::RenderState>,
) -> Result<PreparedPlacementImage, String> {
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
            placement_gap,
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
        placement_gap,
    )?;

    let store_started_at = Instant::now();
    let source_name = remote_image_source_name(&downloaded.url);
    let stored_path =
        library.store_original_bytes(&source_name, &decoded.extension, &downloaded.bytes)?;
    let placement = library.persist_imported_image_resolving_placement(
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
        placement.asset.as_ref().expect("loaded placement asset").id,
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
        ImagePrepPreviewTexture::Cpu
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

    Ok(PreparedPlacementImage {
        target_board_id,
        queue_id,
        placement,
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
    placement_gap: f32,
    worker_started_at: Instant,
    mut trace: Option<ImagePrepTrace>,
) -> Result<PreparedPlacementImage, String> {
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
        placement_gap,
    )?;

    let store_started_at = Instant::now();
    let source_name = remote_image_source_name(&downloaded.url);
    let stored_path = library.store_original_bytes(&source_name, "svg", &svg.bytes)?;
    let placement = library.persist_imported_image_resolving_placement(
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

    Ok(PreparedPlacementImage {
        target_board_id,
        queue_id,
        placement,
        preview_rgba: Vec::new(),
        preview_width: svg.width,
        preview_height: svg.height,
        animated_preview: None,
        preview_texture: ImagePrepPreviewTexture::Svg {
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

pub(crate) fn import_clipboard_sync_traced(
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
    placement_gap: f32,
    trace: Option<ImagePrepTrace>,
    wgpu_render_state: Option<&eframe::egui_wgpu::RenderState>,
) -> Result<PreparedPlacementImage, String> {
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
        placement_gap,
    )?;
    let placement = library.persist_imported_image_resolving_placement(
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
        placement.asset.as_ref().expect("loaded placement asset").id,
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

    Ok(PreparedPlacementImage {
        target_board_id,
        queue_id: Some(queue_id),
        placement,
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
                std::path::Path::new(name)
                    .extension()
                    .and_then(|extension| extension.to_str())
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("svg"))
            })
}

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
