use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use image::RgbaImage;

use crate::data::{CanvasImageLod, Placement};
use crate::image_preparation::image_formats::{
    AnimatedPreview, PreviewImage, decode_image_file, decode_image_file_with_max_side,
    preview_animation_from_frames, preview_image_from_rgba,
};
use crate::library::storage::Library;

use super::decode::{ImagePrepHelper, install_prepared_canvas_cache};
use super::preview_texture::build_preview_texture;
use super::{
    CanvasImageSettings, ExistingLoadDiagnostics, ExistingLoadSourceKind, ImagePrepPreviewTexture,
    ImagePrepResult, ImagePrepTrace, ImportOrigin, PreparedPlacementImage,
    canvas_source_image_from_rgba, original_mime_type_is_svg,
    original_mime_type_supports_animation, read_svg_document, stable_svg_uri_key,
};

const OVERVIEW_CACHE_MAX_SIDE: u32 = 256;
const DETAIL_CACHE_MAX_SIDE: u32 = 4096;

pub(crate) fn load_existing(
    library: &Library,
    target_board_id: i64,
    queue_id: Option<u64>,
    placement: Placement,
    max_texture_side: usize,
    canvas_image_settings: CanvasImageSettings,
    target_lod: CanvasImageLod,
    trace: Option<ImagePrepTrace>,
    wgpu_render_state: Option<&eframe::egui_wgpu::RenderState>,
) -> ImagePrepResult {
    let worker_started_at = Instant::now();
    let mut trace = trace;
    if let Some(trace) = &mut trace {
        trace.timings.queue_wait = worker_started_at.saturating_duration_since(trace.queued_at);
    }

    if original_mime_type_is_svg(&placement.require_loaded_asset().mime_type) {
        let decode_started_at = Instant::now();
        match read_svg_document(&placement.require_loaded_asset().original_path) {
            Ok(svg) => {
                if let Some(trace) = &mut trace {
                    trace.timings.decode_or_load = decode_started_at.elapsed();
                    trace.timings.worker_total = worker_started_at.elapsed();
                }
                return ImagePrepResult::Loaded(Box::new(PreparedPlacementImage {
                    target_board_id,
                    queue_id,
                    preview_rgba: Vec::new(),
                    preview_width: svg.width,
                    preview_height: svg.height,
                    preview_texture: ImagePrepPreviewTexture::Svg {
                        uri: format!(
                            "bytes://asset-svg-{}.svg",
                            stable_svg_uri_key(&placement.require_loaded_asset().original_path)
                        ),
                        bytes: Arc::<[u8]>::from(svg.bytes),
                        size: [
                            placement.require_loaded_asset().width as usize,
                            placement.require_loaded_asset().height as usize,
                        ],
                    },
                    placement,
                    animated_preview: None,
                    loaded_lod: target_lod,
                    existing_load_diagnostics: None,
                    origin: ImportOrigin::Existing,
                    trace,
                }));
            }
            Err(error) => {
                return ImagePrepResult::Failed {
                    target_board_id: Some(target_board_id),
                    queue_id,
                    placement_id: Some(placement.state.id),
                    context: placement
                        .require_loaded_asset()
                        .original_path
                        .display()
                        .to_string(),
                    error,
                };
            }
        }
    }

    let decode_started_at = Instant::now();
    let preview_started_at = Instant::now();
    match load_existing_preview(
        library,
        &placement,
        canvas_image_settings,
        target_lod,
        max_texture_side,
    ) {
        Ok((preview, animated_preview, diagnostics)) => {
            let decode_or_load = decode_started_at.elapsed();
            let preview_texture = if animated_preview.is_some() {
                ImagePrepPreviewTexture::Cpu
            } else {
                match build_preview_texture(wgpu_render_state, &preview) {
                    Ok(preview_texture) => preview_texture,
                    Err(error) => {
                        return ImagePrepResult::Failed {
                            target_board_id: Some(target_board_id),
                            queue_id,
                            placement_id: Some(placement.state.id),
                            context: placement
                                .require_loaded_asset()
                                .original_path
                                .display()
                                .to_string(),
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
            ImagePrepResult::Loaded(Box::new(PreparedPlacementImage {
                target_board_id,
                queue_id,
                placement,
                preview_rgba: preview.rgba,
                preview_width: preview.width,
                preview_height: preview.height,
                animated_preview,
                preview_texture,
                loaded_lod: target_lod,
                existing_load_diagnostics: Some(diagnostics),
                origin: ImportOrigin::Existing,
                trace,
            }))
        }
        Err(error) => ImagePrepResult::Failed {
            target_board_id: Some(target_board_id),
            queue_id,
            placement_id: Some(placement.state.id),
            context: placement
                .require_loaded_asset()
                .original_path
                .display()
                .to_string(),
            error,
        },
    }
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
    placement: &Placement,
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
    if original_mime_type_is_svg(&placement.require_loaded_asset().mime_type) {
        let svg = read_svg_document(&placement.require_loaded_asset().original_path)?;
        let diagnostics = ExistingLoadDiagnostics {
            source_kind: ExistingLoadSourceKind::Original,
            source_path: placement.require_loaded_asset().original_path.clone(),
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
        if original_mime_type_supports_animation(&placement.require_loaded_asset().mime_type) {
            let decoded = decode_image_file(&placement.require_loaded_asset().original_path)?;
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
                    source_path: placement.require_loaded_asset().original_path.clone(),
                    source_file_size,
                    source_width: decoded.source_width,
                    source_height: decoded.source_height,
                    backfilled_cache: false,
                },
            )
        } else if let Some(cache_path) = library.canvas_image_cache_path_for_lod(
            placement.require_loaded_asset().id,
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
                    let (preview, diagnostics) = backfill_canvas_image_cache(
                        library,
                        placement,
                        lod_settings,
                        target_lod,
                        &ImagePrepHelper::default(),
                    )?;
                    (preview, None, diagnostics)
                }
            }
        } else {
            let (preview, diagnostics) = backfill_canvas_image_cache(
                library,
                placement,
                lod_settings,
                target_lod,
                &ImagePrepHelper::default(),
            )?;
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
    placement: &Placement,
    target_width: u32,
    target_height: u32,
) -> Result<(Vec<u8>, u32, u32), String> {
    if target_width == 0 || target_height == 0 {
        return Err("cannot cache zero-sized canvas pixels".to_owned());
    }
    if original_mime_type_is_svg(&placement.require_loaded_asset().mime_type) {
        return Err("cannot cache SVG canvas pixels".to_owned());
    }

    for path in canvas_snapshot_cache_candidates(
        library,
        placement.require_loaded_asset().id,
        target_width,
        target_height,
    ) {
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
        &placement.require_loaded_asset().original_path,
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

fn backfill_canvas_image_cache(
    library: &Library,
    placement: &Placement,
    canvas_image_settings: CanvasImageSettings,
    lod: CanvasImageLod,
    helper: &ImagePrepHelper,
) -> Result<(PreviewImage, ExistingLoadDiagnostics), String> {
    let decode_path =
        preferred_backfill_source_path(library, placement, canvas_image_settings, lod)
            .unwrap_or_else(|| placement.require_loaded_asset().original_path.clone());
    if let Some(prepared) = helper.prepare(&decode_path, canvas_image_settings)? {
        let source_path = install_prepared_canvas_cache(
            library,
            placement.require_loaded_asset().id,
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
    let source_path = library.store_canvas_image_cache_for_lod(
        placement.require_loaded_asset().id,
        canvas_image_settings.max_side,
        lod,
        &canvas_source.rgba,
        canvas_source.width,
        canvas_source.height,
    )?;
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
    placement: &Placement,
    canvas_image_settings: CanvasImageSettings,
    lod: CanvasImageLod,
) -> Option<PathBuf> {
    match lod {
        CanvasImageLod::Working => None,
        CanvasImageLod::Overview => library.canvas_image_cache_path_for_lod(
            placement.require_loaded_asset().id,
            canvas_image_settings.max_side,
            CanvasImageLod::Working,
        ),
        CanvasImageLod::Detail => None,
    }
}

pub(super) fn backfill_cache_lod(
    library: &Library,
    placement: Placement,
    canvas_image_settings: CanvasImageSettings,
    lod: CanvasImageLod,
) -> ImagePrepResult {
    let lod_settings = lod_settings(canvas_image_settings, lod);
    match backfill_canvas_image_cache(
        library,
        &placement,
        lod_settings,
        lod,
        &ImagePrepHelper::default(),
    ) {
        Ok(_) => ImagePrepResult::CacheLodBackfilled {
            asset_id: placement.require_loaded_asset().id,
            lod,
        },
        Err(error) => ImagePrepResult::CacheLodBackfillFailed {
            asset_id: placement.require_loaded_asset().id,
            lod,
            error,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helper_failure_does_not_retry_cache_backfill_in_process() {
        let root = crate::test_support::TempRoot::new("helper-backfill-failure");
        let library = Library::initialize(root.to_path_buf()).unwrap();
        let source = root.join("crash.png");
        image::RgbaImage::from_pixel(1, 1, image::Rgba([30, 100, 220, 255]))
            .save(&source)
            .unwrap();
        let imported =
            crate::image_preparation::import_path_sync(&library, source, eframe::egui::Pos2::ZERO)
                .unwrap();
        let helper = ImagePrepHelper {
            executable: Some(crate::image_preparation::tests::helper_fixture().to_path_buf()),
            timeout: std::time::Duration::from_millis(300),
        };
        let error = backfill_canvas_image_cache(
            &library,
            &imported.placement,
            CanvasImageSettings::default(),
            CanvasImageLod::Detail,
            &helper,
        )
        .err()
        .expect("helper crash must fail the cache backfill");
        assert!(error.starts_with("imageprep helper failed"), "{error}");
        let source = root.join("valid.png");
        image::RgbaImage::from_pixel(1, 1, image::Rgba([10, 20, 30, 255]))
            .save(&source)
            .unwrap();
        let imported =
            crate::image_preparation::import_path_sync(&library, source, eframe::egui::Pos2::ZERO)
                .unwrap();
        let (preview, _) = backfill_canvas_image_cache(
            &library,
            &imported.placement,
            CanvasImageSettings::default(),
            CanvasImageLod::Detail,
            &helper,
        )
        .unwrap();
        assert_eq!(
            (preview.width, preview.height, preview.rgba),
            (1, 1, vec![30, 100, 220, 255])
        );
    }
}
