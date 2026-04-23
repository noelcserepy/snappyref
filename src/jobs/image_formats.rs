use std::path::Path;

use image::{AnimationDecoder, ImageFormat, RgbaImage};

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
    pub animation: Option<DecodedAnimation>,
}

#[derive(Clone, Debug)]
pub struct DecodedAnimationFrame {
    pub rgba: Vec<u8>,
    pub delay_seconds: f32,
}

#[derive(Clone, Debug)]
pub struct DecodedAnimation {
    pub frames: Vec<DecodedAnimationFrame>,
}

pub fn decode_image_file(path: &Path) -> Result<DecodedImage, String> {
    let bytes = std::fs::read(path).map_err(|error| format!("failed to open image: {error}"))?;
    decode_image_bytes(&bytes, Some(path.display().to_string()), None)
}

pub fn decode_image_bytes(
    bytes: &[u8],
    source_label: Option<String>,
    mime_hint: Option<&str>,
) -> Result<DecodedImage, String> {
    let file_size = bytes.len() as u64;
    let format = guess_image_format(bytes, mime_hint)
        .map_err(|error| format!("{}: {error}", source_label.as_deref().unwrap_or("image")))?;

    if !SUPPORTED_IMPORT_FORMATS.contains(&format) {
        let source_path = source_label
            .as_deref()
            .map(Path::new)
            .unwrap_or(Path::new("image"));
        return Err(unsupported_format_error(source_path, Some(format)));
    }

    #[cfg(feature = "turbojpeg-native")]
    let decoded = if format == ImageFormat::Gif {
        decode_gif(&bytes, file_size)
    } else if format == ImageFormat::Jpeg {
        decode_jpeg_with_turbojpeg(&bytes, file_size)
            .or_else(|_| decode_with_image_crate(&bytes, file_size, format))
    } else {
        decode_with_image_crate(&bytes, file_size, format)
    }?;

    #[cfg(not(feature = "turbojpeg-native"))]
    let decoded = if format == ImageFormat::Gif {
        decode_gif(&bytes, file_size)?
    } else {
        decode_with_image_crate(&bytes, file_size, format)?
    };

    Ok(decoded)
}

fn guess_image_format(bytes: &[u8], mime_hint: Option<&str>) -> Result<ImageFormat, String> {
    match image::guess_format(bytes) {
        Ok(format) => Ok(format),
        Err(primary_error) => {
            if let Some(format) = mime_hint.and_then(image_format_from_mime_hint) {
                Ok(format)
            } else {
                Err(format!("failed to determine image format: {primary_error}"))
            }
        }
    }
}

pub fn image_format_extension(format: ImageFormat) -> &'static str {
    match format {
        ImageFormat::Avif => "avif",
        ImageFormat::Bmp => "bmp",
        ImageFormat::Dds => "dds",
        ImageFormat::Farbfeld => "ff",
        ImageFormat::Gif => "gif",
        ImageFormat::Hdr => "hdr",
        ImageFormat::Ico => "ico",
        ImageFormat::Jpeg => "jpg",
        ImageFormat::OpenExr => "exr",
        ImageFormat::Png => "png",
        ImageFormat::Pnm => "pnm",
        ImageFormat::Qoi => "qoi",
        ImageFormat::Tga => "tga",
        ImageFormat::Tiff => "tiff",
        ImageFormat::WebP => "webp",
        _ => "img",
    }
}

fn image_format_from_mime_hint(mime_hint: &str) -> Option<ImageFormat> {
    let mime = mime_hint
        .split(';')
        .next()
        .unwrap_or(mime_hint)
        .trim()
        .to_ascii_lowercase();
    match mime.as_str() {
        "image/avif" => Some(ImageFormat::Avif),
        "image/bmp" => Some(ImageFormat::Bmp),
        "image/gif" => Some(ImageFormat::Gif),
        "image/vnd.microsoft.icon" | "image/x-icon" => Some(ImageFormat::Ico),
        "image/jpeg" | "image/jpg" => Some(ImageFormat::Jpeg),
        "image/exr" | "image/x-exr" => Some(ImageFormat::OpenExr),
        "image/png" => Some(ImageFormat::Png),
        "image/x-portable-anymap"
        | "image/x-portable-bitmap"
        | "image/x-portable-graymap"
        | "image/x-portable-pixmap" => Some(ImageFormat::Pnm),
        "image/tga" | "image/x-tga" => Some(ImageFormat::Tga),
        "image/tiff" => Some(ImageFormat::Tiff),
        "image/webp" => Some(ImageFormat::WebP),
        _ => None,
    }
}

pub struct PreviewImage {
    pub rgba: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Debug)]
pub struct AnimatedPreviewFrame {
    pub rgba: Vec<u8>,
    pub delay_seconds: f32,
}

#[derive(Clone, Debug)]
pub struct AnimatedPreview {
    pub frames: Vec<AnimatedPreviewFrame>,
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

pub fn preview_animation_from_frames(
    width: u32,
    height: u32,
    frames: &[DecodedAnimationFrame],
    max_side: usize,
) -> Result<AnimatedPreview, String> {
    let (preview_width, preview_height) =
        fitted_texture_size(width as usize, height as usize, max_side);

    let mut preview_frames = Vec::with_capacity(frames.len());
    for frame in frames {
        let rgba = if preview_width == width as usize && preview_height == height as usize {
            frame.rgba.clone()
        } else {
            let image = RgbaImage::from_raw(width, height, frame.rgba.clone())
                .ok_or_else(|| "failed to rebuild RGBA image for animation resize".to_owned())?;
            image::imageops::resize(
                &image,
                preview_width as u32,
                preview_height as u32,
                image::imageops::FilterType::Triangle,
            )
            .into_raw()
        };
        preview_frames.push(AnimatedPreviewFrame {
            rgba,
            delay_seconds: frame.delay_seconds.max(0.01),
        });
    }

    Ok(AnimatedPreview {
        frames: preview_frames,
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
        animation: None,
    })
}

fn decode_gif(bytes: &[u8], file_size: u64) -> Result<DecodedImage, String> {
    let decoder = image::codecs::gif::GifDecoder::new(std::io::Cursor::new(bytes))
        .map_err(|error| format!("failed to decode GIF: {error}"))?;
    let frames = decoder
        .into_frames()
        .collect_frames()
        .map_err(|error| format!("failed to read GIF frames: {error}"))?;

    let first = frames
        .first()
        .ok_or_else(|| "GIF did not contain any frames".to_owned())?;
    let width = first.buffer().width();
    let height = first.buffer().height();
    let rgba = first.buffer().clone().into_raw();

    let animation = (frames.len() > 1).then(|| DecodedAnimation {
        frames: frames
            .into_iter()
            .map(|frame| {
                let (numerator_ms, denominator) = frame.delay().numer_denom_ms();
                let delay_seconds = if denominator == 0 {
                    0.1
                } else {
                    (numerator_ms as f32 / denominator as f32 / 1000.0).max(0.01)
                };
                DecodedAnimationFrame {
                    rgba: frame.into_buffer().into_raw(),
                    delay_seconds,
                }
            })
            .collect(),
    });

    Ok(DecodedImage {
        rgba,
        width,
        height,
        file_size,
        format: ImageFormat::Gif,
        animation,
    })
}

#[cfg(feature = "turbojpeg-native")]
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
        animation: None,
    })
}

#[cfg(test)]
mod tests {
    use super::{decode_image_file, fitted_texture_size, preview_image_from_rgba};
    use image::codecs::gif::{GifEncoder, Repeat};
    use image::codecs::jpeg::JpegEncoder;
    use image::{Delay, ExtendedColorType, Frame, ImageEncoder, RgbaImage};

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

    #[test]
    fn decode_image_file_detects_animated_gif_frames() {
        let root = std::env::temp_dir().join(format!(
            "reference-decode-gif-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("sample.gif");
        let file = std::fs::File::create(&path).unwrap();
        let mut encoder = GifEncoder::new(std::io::BufWriter::new(file));
        encoder.set_repeat(Repeat::Infinite).unwrap();
        let first = RgbaImage::from_raw(2, 1, vec![255, 0, 0, 255, 255, 0, 0, 255]).unwrap();
        let second = RgbaImage::from_raw(2, 1, vec![0, 0, 255, 255, 0, 0, 255, 255]).unwrap();
        encoder
            .encode_frames(
                vec![
                    Frame::from_parts(first, 0, 0, Delay::from_numer_denom_ms(120, 1)),
                    Frame::from_parts(second, 0, 0, Delay::from_numer_denom_ms(120, 1)),
                ]
                .into_iter(),
            )
            .unwrap();
        drop(encoder);

        let decoded = decode_image_file(&path).unwrap();
        assert_eq!(decoded.format, image::ImageFormat::Gif);
        let animation = decoded.animation.expect("expected animation metadata");
        assert_eq!(animation.frames.len(), 2);
        assert!(animation.frames[0].delay_seconds > 0.0);

        let _ = std::fs::remove_dir_all(root);
    }
}
