use std::path::{Path, PathBuf};

#[cfg(not(test))]
use std::process::Command;
#[cfg(not(test))]
use std::time::{SystemTime, UNIX_EPOCH};

use image::codecs::jpeg::JpegEncoder;
use image::codecs::webp::WebPEncoder;
use image::{ExtendedColorType, ImageEncoder};

use crate::data::CanvasImageLod;
use crate::jobs::image_formats::decode_image_file_with_max_side;
use crate::library::storage::Library;

use super::{CanvasImageSettings, canvas_source_image_from_rgba};

#[cfg(not(test))]
const IMAGE_IMPORT_HELPER_DISABLED_ENV: &str = "SNAPPYREF_DISABLE_IMAGE_IMPORT_HELPER";

#[derive(Clone, Debug)]
pub(super) struct PreparedImportImage {
    pub(super) rgba: Vec<u8>,
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) source_width: u32,
    pub(super) source_height: u32,
    pub(super) mime_type: String,
    pub(super) file_size: u64,
    pub(super) cache_path: Option<PathBuf>,
    pub(super) cache_extension: Option<String>,
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

pub(super) fn prepare_import_image_with_helper(
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

pub(super) fn install_prepared_canvas_cache(
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
