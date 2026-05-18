use image::RgbaImage;

use crate::data::CanvasImageLod;
use crate::jobs::image_formats::{PreviewImage, decode_image_file};
use crate::library::storage::Library;

use super::preview_texture::build_preview_texture;
use super::{
    CanvasImageSettings, INTERACTIVE_CROP_PREVIEW_MAX_SIDE, ImagePreparationResult,
    PreparedCropPreview, animated_preview_max_side, original_mime_type_is_svg,
};

pub(super) fn crop_item(
    library: &Library,
    request: crate::canvas::CanvasCropRequest,
    max_texture_side: usize,
    canvas_image_settings: CanvasImageSettings,
    wgpu_render_state: Option<&eframe::egui_wgpu::RenderState>,
) -> ImagePreparationResult {
    let source_context = request.source_asset.original_path.display().to_string();
    if original_mime_type_is_svg(&request.source_asset.mime_type) {
        return ImagePreparationResult::Failed {
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
            return ImagePreparationResult::Failed {
                target_board_id: None,
                queue_id: None,
                placement_id: Some(request.placement_id),
                context: source_context,
                error,
            };
        }
    };
    let image = match RgbaImage::from_vec(decoded.width, decoded.height, decoded.rgba) {
        Some(image) => image,
        None => {
            return ImagePreparationResult::Failed {
                target_board_id: None,
                queue_id: None,
                placement_id: Some(request.placement_id),
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
            return ImagePreparationResult::Failed {
                target_board_id: None,
                queue_id: None,
                placement_id: Some(request.placement_id),
                context: source_context,
                error,
            };
        }
    };

    let file_size = match std::fs::metadata(&stored_path) {
        Ok(metadata) => metadata.len(),
        Err(error) => {
            let _ = std::fs::remove_file(&stored_path);
            return ImagePreparationResult::Failed {
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
            return ImagePreparationResult::Failed {
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
            return ImagePreparationResult::Failed {
                target_board_id: None,
                queue_id: None,
                placement_id: Some(request.placement_id),
                context: source_context,
                error,
            };
        }
    };
    ImagePreparationResult::Cropped(PreparedCropPreview {
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
