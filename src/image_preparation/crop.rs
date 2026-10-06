use std::path::{Path, PathBuf};

use image::{ImageBuffer, Rgba, RgbaImage};

use crate::canvas::PixelCropRect;
use crate::data::CanvasImageLod;
use crate::image_preparation::image_formats::{DecodedImage, PreviewImage, decode_image_file};
use crate::library::storage::Library;

use super::preview_texture::build_preview_texture;
use super::{
    CanvasImageSettings, INTERACTIVE_CROP_PREVIEW_MAX_SIDE, ImagePrepResult, PreparedCropPreview,
    animated_preview_max_side, original_mime_type_is_svg,
};

pub(super) fn crop_item(
    library: &Library,
    request: crate::canvas::CanvasCropRequest,
    max_texture_side: usize,
    canvas_image_settings: CanvasImageSettings,
    wgpu_render_state: Option<&eframe::egui_wgpu::RenderState>,
) -> ImagePrepResult {
    let source_context = request.source_asset.original_path.display().to_string();
    if original_mime_type_is_svg(&request.source_asset.mime_type) {
        return ImagePrepResult::Failed {
            target_board_id: None,
            queue_id: None,
            placement_id: Some(request.placement_id),
            context: source_context,
            error: "SVG cropping is not supported yet".to_owned(),
        };
    }

    let decoded = match decode_image_file(&request.source_asset.original_path) {
        Ok(decoded) => decoded,
        Err(error) => {
            return ImagePrepResult::Failed {
                target_board_id: None,
                queue_id: None,
                placement_id: Some(request.placement_id),
                context: source_context,
                error,
            };
        }
    };
    let (stored_path, cropped) = match store_cropped_original(
        library,
        &decoded,
        request.crop_rect,
        &request.source_asset.filename,
    ) {
        Ok(stored) => stored,
        Err(error) => {
            return ImagePrepResult::Failed {
                target_board_id: None,
                queue_id: None,
                placement_id: Some(request.placement_id),
                context: source_context,
                error,
            };
        }
    };
    let (width, height) = cropped.dimensions();
    let cropped_rgba = cropped.into_raw();

    let file_size = match std::fs::metadata(&stored_path) {
        Ok(metadata) => metadata.len(),
        Err(error) => {
            let _ = std::fs::remove_file(&stored_path);
            return ImagePrepResult::Failed {
                target_board_id: None,
                queue_id: None,
                placement_id: Some(request.placement_id),
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
            return ImagePrepResult::Failed {
                target_board_id: None,
                queue_id: None,
                placement_id: Some(request.placement_id),
                context: source_context,
                error,
            };
        }
    };
    let preview_texture = match build_preview_texture(wgpu_render_state, &preview) {
        Ok(preview_texture) => preview_texture,
        Err(error) => {
            let _ = std::fs::remove_file(&stored_path);
            return ImagePrepResult::Failed {
                target_board_id: None,
                queue_id: None,
                placement_id: Some(request.placement_id),
                context: source_context,
                error,
            };
        }
    };
    ImagePrepResult::Cropped(PreparedCropPreview {
        placement_id: request.placement_id,
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

/// Crops decoded pixels to `rect`, clamped to the image, and stores them as a new PNG original.
pub(crate) fn store_cropped_original(
    library: &Library,
    decoded: &DecodedImage,
    rect: PixelCropRect,
    source_filename: &str,
) -> Result<(PathBuf, RgbaImage), String> {
    let image = ImageBuffer::<Rgba<u8>, &[u8]>::from_raw(
        decoded.width,
        decoded.height,
        decoded.rgba.as_slice(),
    )
    .ok_or_else(|| "failed to rebuild source image pixels for crop".to_owned())?;
    let (x, y) = (rect.x.min(image.width()), rect.y.min(image.height()));
    let cropped = RgbaImage::from_fn(
        rect.width.min(image.width() - x),
        rect.height.min(image.height() - y),
        |column, row| *image.get_pixel(x + column, y + row),
    );
    let base_name = Path::new(source_filename)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("image");
    let stored_path = library.store_cropped_rgba_image(
        &format!("{base_name}-crop"),
        cropped.as_raw(),
        cropped.width(),
        cropped.height(),
    )?;
    Ok((stored_path, cropped))
}

fn fast_preview_image_from_rgba(
    width: u32,
    height: u32,
    rgba: &[u8],
    max_side: usize,
) -> Result<PreviewImage, String> {
    let (preview_width, preview_height) =
        crate::image_preparation::image_formats::fitted_texture_size(
            width as usize,
            height as usize,
            max_side,
        );

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
