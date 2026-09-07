use crate::placement::display_scale_for_placement_target_area;
use std::io::Read;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui;

use crate::data::CanvasImageLod;
use crate::image_preparation::image_formats::{
    DecodedAnimation, PreviewImage, decode_image_bytes_with_max_side,
    decode_image_file_with_max_side, preview_animation_from_frames, preview_image_from_rgba,
};
use crate::library::storage::{Library, sha256_bytes, sha256_file};

#[cfg(test)]
use super::DEFAULT_PLACEMENT_TARGET_AREA;
use super::decode::{PreparedImage, install_prepared_canvas_cache, prepare_image_with_helper};
use super::preview_texture::build_preview_texture;
use super::{
    CanvasImageSettings, ImagePrepPreviewTexture, ImagePrepResult, ImportRequest, ImportSource,
    PreparedPlacementImage, SVG_MIME_TYPE, animated_preview_max_side,
    canvas_source_image_from_rgba, is_svg_path, original_mime_type_is_svg, parse_svg_document,
    read_svg_document, stable_svg_uri_key,
};
use crate::placement::initial_layout::resolve_import_world;

const URL_IMPORT_MAX_BYTES: u64 = 64 * 1024 * 1024;
const URL_IMPORT_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const URL_IMPORT_REQUEST_TIMEOUT: Duration = Duration::from_secs(45);

struct ImportedImagePersistRequest<'a> {
    library: &'a Library,
    target_board_id: i64,
    placement_id: Option<i64>,
    stored_path: &'a std::path::Path,
    mime_type: &'a str,
    width: u32,
    height: u32,
    file_size: u64,
    original_sha256: &'a str,
    arranged_world: egui::Pos2,
    initial_scale: f32,
}

fn persist_imported_image_with_exact_reuse(
    request: ImportedImagePersistRequest<'_>,
) -> Result<crate::data::Placement, String> {
    let placement = request
        .library
        .persist_imported_image_resolving_placement_with_hash(
            request.target_board_id,
            request.placement_id,
            request.stored_path,
            request.mime_type,
            request.width,
            request.height,
            request.file_size,
            Some(request.original_sha256),
            request.arranged_world,
            request.initial_scale,
        )?;
    if placement.require_loaded_asset().original_path != request.stored_path {
        match std::fs::remove_file(request.stored_path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("failed to remove duplicate original: {error}")),
        }
    }
    Ok(placement)
}

#[cfg(test)]
pub fn import_path_sync(
    library: &Library,
    source_path: PathBuf,
    desired_world: egui::Pos2,
    viewport_world: egui::Rect,
    auto_scale: bool,
    placement_gap: f32,
) -> Result<PreparedPlacementImage, String> {
    let target_board_id = library.load_user_session()?.active_board_id;
    import_sync(
        library,
        ImportRequest {
            target_board_id,
            queue_id: 0,
            placement_id: Some(library.allocate_document_id()),
            source: ImportSource::Path(source_path),
            desired_world,
            reserved_world: None,
            viewport_world,
            max_texture_side: usize::MAX,
            canvas_image_settings: CanvasImageSettings::default(),
            auto_scale,
            placement_target_area: DEFAULT_PLACEMENT_TARGET_AREA,
            placement_gap,
            trace: None,
        },
        None,
    )
}

pub(crate) fn import(
    library: &Library,
    request: ImportRequest,
    wgpu_render_state: Option<&eframe::egui_wgpu::RenderState>,
) -> ImagePrepResult {
    let target_board_id = request.target_board_id;
    let queue_id = request.queue_id;
    let placement_id = request.placement_id;
    let context = request.source.label();
    match import_sync(library, request, wgpu_render_state) {
        Ok(loaded) => ImagePrepResult::Loaded(loaded),
        Err(error) => ImagePrepResult::Failed {
            target_board_id: Some(target_board_id),
            queue_id: Some(queue_id),
            placement_id,
            context,
            error,
        },
    }
}

pub(crate) fn import_sync(
    library: &Library,
    request: ImportRequest,
    wgpu_render_state: Option<&eframe::egui_wgpu::RenderState>,
) -> Result<PreparedPlacementImage, String> {
    let worker_started_at = Instant::now();
    let ImportRequest {
        target_board_id,
        queue_id,
        placement_id,
        source,
        desired_world,
        reserved_world,
        viewport_world,
        max_texture_side,
        canvas_image_settings,
        auto_scale,
        placement_target_area,
        placement_gap,
        mut trace,
    } = request;
    if let Some(trace) = &mut trace {
        trace.timings.queue_wait = worker_started_at.saturating_duration_since(trace.queued_at);
    }
    let origin = source.origin();

    let decode_started_at = Instant::now();
    let decoded = decode_import_source(source, canvas_image_settings)?;
    if let Some(trace) = &mut trace {
        trace.timings.decode_or_load = decode_started_at.elapsed();
    }

    let initial_scale = display_scale_for_placement_target_area(
        decoded.source_width,
        decoded.source_height,
        auto_scale,
        placement_target_area,
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
    let stored = store_original(library, &decoded)?;
    let placement = persist_imported_image_with_exact_reuse(ImportedImagePersistRequest {
        library,
        target_board_id,
        placement_id,
        stored_path: &stored.path,
        mime_type: &decoded.mime_type,
        width: decoded.source_width,
        height: decoded.source_height,
        file_size: stored.file_size,
        original_sha256: &stored.sha256,
        arranged_world,
        initial_scale,
    })?;
    let store_and_persist = store_started_at.elapsed();

    let preview_started_at = Instant::now();
    let asset = placement.require_loaded_asset();
    let (preview, animated_preview, preview_texture) = match decoded.pixels {
        ImportPixels::Svg { bytes } => {
            let preview = PreviewImage {
                rgba: Vec::new(),
                width: decoded.source_width,
                height: decoded.source_height,
            };
            let texture = ImagePrepPreviewTexture::Svg {
                uri: format!(
                    "bytes://asset-svg-{}.svg",
                    stable_svg_uri_key(&asset.original_path)
                ),
                bytes: Arc::<[u8]>::from(bytes),
                size: [
                    decoded.source_width as usize,
                    decoded.source_height as usize,
                ],
            };
            (preview, None, texture)
        }
        ImportPixels::Prepared(prepared) => {
            install_prepared_canvas_cache(
                library,
                asset.id,
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
            let texture = build_preview_texture(wgpu_render_state, &preview)?;
            (preview, None, texture)
        }
        ImportPixels::Raster {
            width,
            height,
            rgba,
            animation,
        } => {
            let canvas_source =
                canvas_source_image_from_rgba(width, height, &rgba, canvas_image_settings)?;
            let _ = library.store_canvas_image_cache(
                asset.id,
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
            let animated_preview = animation
                .as_ref()
                .map(|animation| {
                    preview_animation_from_frames(
                        width,
                        height,
                        &animation.frames,
                        animated_preview_max_side(canvas_image_settings, max_texture_side),
                    )
                })
                .transpose()?;
            let texture = if animated_preview.is_some() {
                ImagePrepPreviewTexture::Cpu
            } else {
                build_preview_texture(wgpu_render_state, &preview)?
            };
            (preview, animated_preview, texture)
        }
    };
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
        animated_preview,
        preview_texture,
        loaded_lod: CanvasImageLod::Working,
        existing_load_diagnostics: None,
        origin,
        trace,
    })
}

/// An import source decoded far enough to size, place, and store it.
struct DecodedImportSource {
    source_width: u32,
    source_height: u32,
    mime_type: String,
    original: OriginalBytes,
    pixels: ImportPixels,
}

/// How the original file reaches the library's originals folder.
enum OriginalBytes {
    CopyFile(PathBuf),
    Write {
        source_name: String,
        extension: String,
        bytes: Vec<u8>,
    },
    /// Encode the raster pixels as PNG; used for clipboard images.
    EncodePng,
}

enum ImportPixels {
    /// Helper output already at canvas size, plus a cache file to adopt.
    Prepared(PreparedImage),
    Raster {
        width: u32,
        height: u32,
        rgba: Vec<u8>,
        animation: Option<DecodedAnimation>,
    },
    Svg {
        bytes: Vec<u8>,
    },
}

fn decode_import_source(
    source: ImportSource,
    canvas_image_settings: CanvasImageSettings,
) -> Result<DecodedImportSource, String> {
    match source {
        ImportSource::Path(source_path) => {
            if is_svg_path(&source_path) {
                let svg = read_svg_document(&source_path)?;
                return Ok(DecodedImportSource {
                    source_width: svg.width,
                    source_height: svg.height,
                    mime_type: SVG_MIME_TYPE.to_owned(),
                    original: OriginalBytes::CopyFile(source_path),
                    pixels: ImportPixels::Svg { bytes: svg.bytes },
                });
            }
            if let Some(prepared) =
                prepare_image_with_helper(&source_path, canvas_image_settings).unwrap_or(None)
            {
                return Ok(DecodedImportSource {
                    source_width: prepared.source_width,
                    source_height: prepared.source_height,
                    mime_type: prepared.mime_type.clone(),
                    original: OriginalBytes::CopyFile(source_path),
                    pixels: ImportPixels::Prepared(prepared),
                });
            }
            let decoded =
                decode_image_file_with_max_side(&source_path, Some(canvas_image_settings.max_side))
                    .map_err(|error| format!("{}: {error}", source_path.display()))?;
            Ok(DecodedImportSource {
                source_width: decoded.source_width,
                source_height: decoded.source_height,
                mime_type: decoded.mime_type,
                original: OriginalBytes::CopyFile(source_path),
                pixels: ImportPixels::Raster {
                    width: decoded.width,
                    height: decoded.height,
                    rgba: decoded.rgba,
                    animation: decoded.animation,
                },
            })
        }
        ImportSource::Url(url) => {
            let downloaded = download_remote_image(&url)?;
            let source_name = remote_image_source_name(&downloaded.url);
            if downloaded_url_image_is_svg(&downloaded) {
                let svg = parse_svg_document(downloaded.bytes)
                    .map_err(|error| format!("{}: {error}", downloaded.url))?;
                return Ok(DecodedImportSource {
                    source_width: svg.width,
                    source_height: svg.height,
                    mime_type: SVG_MIME_TYPE.to_owned(),
                    original: OriginalBytes::Write {
                        source_name,
                        extension: "svg".to_owned(),
                        bytes: svg.bytes.clone(),
                    },
                    pixels: ImportPixels::Svg { bytes: svg.bytes },
                });
            }
            let decoded = decode_image_bytes_with_max_side(
                &downloaded.bytes,
                Some(downloaded.url.as_str().to_owned()),
                downloaded.mime_type.as_deref(),
                Some(canvas_image_settings.max_side),
            )?;
            Ok(DecodedImportSource {
                source_width: decoded.source_width,
                source_height: decoded.source_height,
                mime_type: decoded.mime_type,
                original: OriginalBytes::Write {
                    source_name,
                    extension: decoded.extension,
                    bytes: downloaded.bytes,
                },
                pixels: ImportPixels::Raster {
                    width: decoded.width,
                    height: decoded.height,
                    rgba: decoded.rgba,
                    animation: decoded.animation,
                },
            })
        }
        ImportSource::Clipboard {
            rgba,
            width,
            height,
        } => Ok(DecodedImportSource {
            source_width: width,
            source_height: height,
            mime_type: image::ImageFormat::Png.to_mime_type().to_owned(),
            original: OriginalBytes::EncodePng,
            pixels: ImportPixels::Raster {
                width,
                height,
                rgba,
                animation: None,
            },
        }),
    }
}

struct StoredOriginal {
    path: PathBuf,
    file_size: u64,
    sha256: String,
}

fn store_original(
    library: &Library,
    decoded: &DecodedImportSource,
) -> Result<StoredOriginal, String> {
    match &decoded.original {
        OriginalBytes::CopyFile(source_path) => {
            let path = library
                .store_original(source_path)
                .map_err(|error| format!("{}: {error}", source_path.display()))?;
            stored_file(path)
        }
        OriginalBytes::Write {
            source_name,
            extension,
            bytes,
        } => Ok(StoredOriginal {
            path: library.store_original_bytes(source_name, extension, bytes)?,
            file_size: bytes.len() as u64,
            sha256: sha256_bytes(bytes),
        }),
        OriginalBytes::EncodePng => {
            let ImportPixels::Raster {
                width,
                height,
                rgba,
                ..
            } = &decoded.pixels
            else {
                return Err("clipboard import requires raster pixels".to_owned());
            };
            stored_file(library.store_clipboard_image(rgba, *width, *height)?)
        }
    }
}

fn stored_file(path: PathBuf) -> Result<StoredOriginal, String> {
    let file_size = std::fs::metadata(&path)
        .map_err(|error| format!("failed to read stored original metadata: {error}"))?
        .len();
    let sha256 = sha256_file(&path)?;
    Ok(StoredOriginal {
        path,
        file_size,
        sha256,
    })
}

struct DownloadedUrlImage {
    url: url::Url,
    bytes: Vec<u8>,
    mime_type: Option<String>,
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
