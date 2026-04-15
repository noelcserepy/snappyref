use std::path::Path;

use image::{ImageFormat, ImageReader};

pub const SUPPORTED_IMPORT_FORMATS: &[ImageFormat] = &[
    ImageFormat::Avif,
    ImageFormat::Bmp,
    ImageFormat::Dds,
    ImageFormat::Farbfeld,
    ImageFormat::Gif,
    ImageFormat::Hdr,
    ImageFormat::Ico,
    ImageFormat::Jpeg,
    ImageFormat::OpenExr,
    ImageFormat::Png,
    ImageFormat::Pnm,
    ImageFormat::Qoi,
    ImageFormat::Tga,
    ImageFormat::Tiff,
    ImageFormat::WebP,
];

pub struct DecodedImage {
    pub rgba: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub file_size: u64,
    pub format: ImageFormat,
}

pub fn decode_image_file(path: &Path) -> Result<DecodedImage, String> {
    let file_size = std::fs::metadata(path)
        .map_err(|error| format!("failed to read file metadata: {error}"))?
        .len();

    let reader = ImageReader::open(path)
        .map_err(|error| format!("failed to open image: {error}"))?
        .with_guessed_format()
        .map_err(|error| format!("failed to determine image format: {error}"))?;

    let format = reader
        .format()
        .ok_or_else(|| unsupported_format_error(path, None))?;

    if !SUPPORTED_IMPORT_FORMATS.contains(&format) {
        return Err(unsupported_format_error(path, Some(format)));
    }

    let image = reader
        .decode()
        .map_err(|error| format!("failed to decode {}: {error}", format_name(format)))?
        .to_rgba8();
    let width = image.width();
    let height = image.height();

    Ok(DecodedImage {
        rgba: image.into_raw(),
        width,
        height,
        file_size,
        format,
    })
}

fn unsupported_format_error(path: &Path, format: Option<ImageFormat>) -> String {
    let format_description = format
        .map(format_name)
        .unwrap_or_else(|| "unknown image format".to_owned());

    format!(
        "{} is not supported. Detected {format_description}. Supported formats: {}.",
        path.display(),
        supported_format_list()
    )
}

fn supported_format_list() -> String {
    SUPPORTED_IMPORT_FORMATS
        .iter()
        .map(|format| format_name(*format))
        .collect::<Vec<_>>()
        .join(", ")
}

fn format_name(format: ImageFormat) -> String {
    match format {
        ImageFormat::Avif => "AVIF".to_owned(),
        ImageFormat::Bmp => "BMP".to_owned(),
        ImageFormat::Dds => "DDS".to_owned(),
        ImageFormat::Farbfeld => "farbfeld".to_owned(),
        ImageFormat::Gif => "GIF".to_owned(),
        ImageFormat::Hdr => "HDR".to_owned(),
        ImageFormat::Ico => "ICO".to_owned(),
        ImageFormat::Jpeg => "JPEG".to_owned(),
        ImageFormat::OpenExr => "OpenEXR".to_owned(),
        ImageFormat::Png => "PNG".to_owned(),
        ImageFormat::Pnm => "PNM".to_owned(),
        ImageFormat::Qoi => "QOI".to_owned(),
        ImageFormat::Tga => "TGA".to_owned(),
        ImageFormat::Tiff => "TIFF".to_owned(),
        ImageFormat::WebP => "WebP".to_owned(),
        _ => "unknown image format".to_owned(),
    }
}
