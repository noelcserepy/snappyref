use std::path::Path;

use image::{ImageFormat, RgbaImage};

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
    let bytes = std::fs::read(path).map_err(|error| format!("failed to open image: {error}"))?;
    let file_size = bytes.len() as u64;
    let format = image::guess_format(&bytes)
        .map_err(|error| format!("failed to determine image format: {error}"))?;

    if !SUPPORTED_IMPORT_FORMATS.contains(&format) {
        return Err(unsupported_format_error(path, Some(format)));
    }

    let decoded = if format == ImageFormat::Jpeg {
        decode_jpeg_with_turbojpeg(&bytes, file_size)
            .or_else(|_| decode_with_image_crate(&bytes, file_size, format))
    } else {
        decode_with_image_crate(&bytes, file_size, format)
    }?;

    Ok(decoded)
}

pub struct PreviewImage {
    pub rgba: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

pub fn preview_image_from_rgba(
    width: u32,
    height: u32,
    rgba: &[u8],
    max_side: usize,
) -> Result<PreviewImage, String> {
    let (preview_width, preview_height) =
        fitted_texture_size(width as usize, height as usize, max_side);

    if preview_width == width as usize && preview_height == height as usize {
        return Ok(PreviewImage {
            rgba: rgba.to_vec(),
            width,
            height,
        });
    }

    let image = RgbaImage::from_raw(width, height, rgba.to_vec())
        .ok_or_else(|| "failed to rebuild RGBA image for preview resize".to_owned())?;
    let resized = image::imageops::resize(
        &image,
        preview_width as u32,
        preview_height as u32,
        image::imageops::FilterType::Triangle,
    );

    Ok(PreviewImage {
        rgba: resized.into_raw(),
        width: preview_width as u32,
        height: preview_height as u32,
    })
}

pub fn fitted_texture_size(width: usize, height: usize, max_side: usize) -> (usize, usize) {
    if width == 0 || height == 0 || max_side == 0 {
        return (width.max(1), height.max(1));
    }
    if width <= max_side && height <= max_side {
        return (width, height);
    }

    let scale = (max_side as f64 / width as f64).min(max_side as f64 / height as f64);
    let scaled_width = ((width as f64 * scale).floor() as usize).clamp(1, max_side);
    let scaled_height = ((height as f64 * scale).floor() as usize).clamp(1, max_side);
    (scaled_width, scaled_height)
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

fn decode_with_image_crate(
    bytes: &[u8],
    file_size: u64,
    format: ImageFormat,
) -> Result<DecodedImage, String> {
    let image = image::load_from_memory_with_format(bytes, format)
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

fn decode_jpeg_with_turbojpeg(bytes: &[u8], file_size: u64) -> Result<DecodedImage, String> {
    let mut decompressor = turbojpeg::Decompressor::new()
        .map_err(|error| format!("failed to initialize TurboJPEG: {error}"))?;
    let header = decompressor
        .read_header(bytes)
        .map_err(|error| format!("failed to read JPEG header with TurboJPEG: {error}"))?;
    let mut image = turbojpeg::Image {
        pixels: vec![0; 4 * header.width * header.height],
        width: header.width,
        pitch: 4 * header.width,
        height: header.height,
        format: turbojpeg::PixelFormat::RGBA,
    };
    decompressor
        .decompress(bytes, image.as_deref_mut())
        .map_err(|error| format!("failed to decode JPEG with TurboJPEG: {error}"))?;

    Ok(DecodedImage {
        rgba: image.pixels,
        width: header.width as u32,
        height: header.height as u32,
        file_size,
        format: ImageFormat::Jpeg,
    })
}

#[cfg(test)]
mod tests {
    use super::{decode_image_file, fitted_texture_size, preview_image_from_rgba};
    use image::codecs::jpeg::JpegEncoder;
    use image::{ExtendedColorType, ImageEncoder};

    #[test]
    fn fitted_texture_size_clamps_to_max_texture_side() {
        assert_eq!(fitted_texture_size(8403, 3952, 8192), (8192, 3852));
        assert_eq!(fitted_texture_size(3952, 8403, 8192), (3852, 8192));
        assert_eq!(fitted_texture_size(1024, 768, 8192), (1024, 768));
    }

    #[test]
    fn preview_image_from_rgba_resizes_to_requested_bound() {
        let rgba = vec![255; 400 * 200 * 4];
        let preview = preview_image_from_rgba(400, 200, &rgba, 100).unwrap();

        assert_eq!(preview.width, 100);
        assert_eq!(preview.height, 50);
        assert_eq!(preview.rgba.len(), 100 * 50 * 4);
    }

    #[test]
    fn decode_image_file_decodes_jpeg() {
        let root = std::env::temp_dir().join(format!(
            "reference-decode-jpeg-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("sample.jpg");
        let rgba = vec![
            255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255,
        ];
        let file = std::fs::File::create(&path).unwrap();
        let writer = std::io::BufWriter::new(file);
        let encoder = JpegEncoder::new_with_quality(writer, 85);
        encoder
            .write_image(&rgba[..12], 2, 2, ExtendedColorType::Rgb8)
            .unwrap();

        let decoded = decode_image_file(&path).unwrap();
        assert_eq!(decoded.format, image::ImageFormat::Jpeg);
        assert_eq!(decoded.width, 2);
        assert_eq!(decoded.height, 2);
        assert_eq!(decoded.rgba.len(), 16);

        let _ = std::fs::remove_dir_all(root);
    }
}
