use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use image::{AnimationDecoder, DynamicImage, ImageFormat, RgbaImage};

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
    pub source_width: u32,
    pub source_height: u32,
    pub file_size: u64,
    #[allow(dead_code)]
    pub format: ImageFormat,
    pub format_name: String,
    pub mime_type: String,
    pub extension: String,
    pub animation: Option<DecodedAnimation>,
}

impl DecodedImage {
    fn with_source_dimensions(mut self, source_width: u32, source_height: u32) -> Self {
        self.source_width = source_width;
        self.source_height = source_height;
        self
    }
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

#[derive(Clone, Debug)]
pub struct DecodePerfReport {
    pub source_path: PathBuf,
    pub source_file_size: u64,
    pub format_name: String,
    pub width: u32,
    pub height: u32,
    pub rgba_bytes: usize,
    pub megapixels: f64,
    pub decode_ms: f64,
    pub ms_per_megapixel: f64,
    pub animated_frame_count: Option<usize>,
}

pub fn decode_image_file(path: &Path) -> Result<DecodedImage, String> {
    let bytes = std::fs::read(path).map_err(|error| format!("failed to open image: {error}"))?;
    if path_has_raw_like_extension(path) {
        return decode_image_file_with_native_fallback(path, &bytes);
    }
    match decode_image_bytes_with_path_hint(
        &bytes,
        Some(path.display().to_string()),
        None,
        Some(path),
    ) {
        Ok(decoded) => Ok(decoded),
        Err(error) => decode_image_file_with_native_fallback(path, &bytes)
            .map_err(|fallback_error| format!("{error}; native fallback failed: {fallback_error}")),
    }
}

pub fn decode_image_file_perf_report(path: &Path) -> Result<DecodePerfReport, String> {
    let started_at = Instant::now();
    let decoded = decode_image_file(path)?;
    let decode_ms = started_at.elapsed().as_secs_f64() * 1000.0;
    let megapixels = decoded.width as f64 * decoded.height as f64 / 1_000_000.0;
    let ms_per_megapixel = if megapixels > 0.0 {
        decode_ms / megapixels
    } else {
        0.0
    };

    Ok(DecodePerfReport {
        source_path: path.to_path_buf(),
        source_file_size: decoded.file_size,
        format_name: decoded.format_name,
        width: decoded.width,
        height: decoded.height,
        rgba_bytes: decoded.rgba.len(),
        megapixels,
        decode_ms,
        ms_per_megapixel,
        animated_frame_count: decoded
            .animation
            .as_ref()
            .map(|animation| animation.frames.len()),
    })
}

pub fn decode_image_bytes(
    bytes: &[u8],
    source_label: Option<String>,
    mime_hint: Option<&str>,
) -> Result<DecodedImage, String> {
    decode_image_bytes_with_path_hint(bytes, source_label, mime_hint, None)
}

fn decode_image_bytes_with_path_hint(
    bytes: &[u8],
    source_label: Option<String>,
    mime_hint: Option<&str>,
    path_hint: Option<&Path>,
) -> Result<DecodedImage, String> {
    let file_size = bytes.len() as u64;
    let source_label_path_hint = source_label.as_deref().map(Path::new);
    let effective_path_hint = path_hint.or(source_label_path_hint);

    if effective_path_hint.is_some_and(|path| path_extension_is(path, "xbm"))
        || mime_hint.is_some_and(mime_hint_is_xbm)
    {
        return decode_xbm(bytes, file_size);
    }
    if effective_path_hint.is_some_and(|path| path_extension_is(path, "xpm"))
        || mime_hint.is_some_and(mime_hint_is_xpm)
    {
        return decode_xpm(bytes, file_size);
    }
    if effective_path_hint.is_some_and(|path| path_extension_is(path, "jxl"))
        || mime_hint.is_some_and(mime_hint_is_jxl)
    {
        return decode_jxl(bytes, file_size);
    }
    let format = guess_image_format(bytes, mime_hint, effective_path_hint)
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
    } else if format == ImageFormat::WebP {
        decode_webp(&bytes, file_size)
    } else if format == ImageFormat::OpenExr {
        decode_exr(&bytes, file_size)
    } else if format == ImageFormat::Jpeg {
        decode_jpeg_with_turbojpeg(&bytes, file_size)
            .or_else(|_| decode_with_image_crate(&bytes, file_size, format))
    } else {
        decode_with_image_crate(&bytes, file_size, format)
    }?;

    #[cfg(not(feature = "turbojpeg-native"))]
    let decoded = if format == ImageFormat::Gif {
        decode_gif(&bytes, file_size)
    } else if format == ImageFormat::WebP {
        decode_webp(&bytes, file_size)
    } else if format == ImageFormat::OpenExr {
        decode_exr(&bytes, file_size)
    } else {
        decode_with_image_crate(&bytes, file_size, format)
    }?;

    Ok(decoded)
}

fn path_extension_is(path: &Path, expected: &str) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case(expected))
}

fn path_has_raw_like_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "raw" | "dng" | "cr2" | "cr3" | "nef" | "arw" | "orf" | "rw2"
            )
        })
}

fn mime_hint_is_xbm(mime_hint: &str) -> bool {
    let mime = mime_hint
        .split(';')
        .next()
        .unwrap_or(mime_hint)
        .trim()
        .to_ascii_lowercase();
    matches!(mime.as_str(), "image/x-xbitmap" | "image/xbm")
}

fn mime_hint_is_xpm(mime_hint: &str) -> bool {
    let mime = mime_hint
        .split(';')
        .next()
        .unwrap_or(mime_hint)
        .trim()
        .to_ascii_lowercase();
    matches!(mime.as_str(), "image/x-xpixmap" | "image/xpm")
}

fn mime_hint_is_jxl(mime_hint: &str) -> bool {
    let mime = mime_hint
        .split(';')
        .next()
        .unwrap_or(mime_hint)
        .trim()
        .to_ascii_lowercase();
    mime == "image/jxl"
}

fn guess_image_format(
    bytes: &[u8],
    mime_hint: Option<&str>,
    path_hint: Option<&Path>,
) -> Result<ImageFormat, String> {
    match image::guess_format(bytes) {
        Ok(format) => Ok(format),
        Err(primary_error) => {
            if let Some(format) = mime_hint.and_then(image_format_from_mime_hint) {
                Ok(format)
            } else if let Some(format) = path_hint.and_then(image_format_from_path_hint) {
                Ok(format)
            } else {
                Err(format!("failed to determine image format: {primary_error}"))
            }
        }
    }
}

fn image_format_from_path_hint(path: &Path) -> Option<ImageFormat> {
    let extension = path.extension()?.to_str()?;
    ImageFormat::from_extension(extension)
}

#[cfg(target_os = "macos")]
fn decode_image_file_with_native_fallback(
    path: &Path,
    original_bytes: &[u8],
) -> Result<DecodedImage, String> {
    let Some(metadata) = native_fallback_metadata_for_path(path) else {
        return Err("no native fallback registered for extension".to_owned());
    };

    if metadata.extension == "jp2"
        && let Ok(decoded) = decode_jpeg2000_quicklook_preview(path, original_bytes, &metadata)
    {
        return Ok(decoded);
    }

    let temp_path = std::env::temp_dir().join(format!(
        "snappyref-native-decode-{}-{}.png",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0)
    ));
    let output = std::process::Command::new("sips")
        .arg("-s")
        .arg("format")
        .arg("png")
        .arg(path)
        .arg("--out")
        .arg(&temp_path)
        .output()
        .map_err(|error| format!("failed to run sips: {error}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let _ = std::fs::remove_file(&temp_path);
        return Err(format!("sips failed: {}", stderr.trim()));
    }

    let png = std::fs::read(&temp_path)
        .map_err(|error| format!("failed to read native PNG output: {error}"))?;
    let _ = std::fs::remove_file(&temp_path);
    let image = image::load_from_memory_with_format(&png, ImageFormat::Png)
        .map_err(|error| format!("failed to decode native PNG output: {error}"))?
        .to_rgba8();
    let (source_width, source_height) =
        native_image_dimensions(path).unwrap_or((image.width(), image.height()));
    Ok(decoded_custom_image_from_rgba(
        image.as_raw().clone(),
        image.width(),
        image.height(),
        original_bytes.len() as u64,
        metadata.format_name,
        metadata.mime_type,
        &metadata.extension,
    )
    .with_source_dimensions(source_width, source_height))
}

#[cfg(target_os = "macos")]
fn decode_jpeg2000_quicklook_preview(
    path: &Path,
    original_bytes: &[u8],
    metadata: &NativeFallbackMetadata,
) -> Result<DecodedImage, String> {
    let temp_dir = std::env::temp_dir().join(format!(
        "snappyref-quicklook-decode-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&temp_dir)
        .map_err(|error| format!("failed to create Quick Look temp dir: {error}"))?;
    let output = std::process::Command::new("qlmanage")
        .arg("-t")
        .arg("-s")
        .arg(crate::data::DEFAULT_CANVAS_IMAGE_MAX_SIDE.to_string())
        .arg("-o")
        .arg(&temp_dir)
        .arg(path)
        .output()
        .map_err(|error| format!("failed to run qlmanage: {error}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let _ = std::fs::remove_dir_all(&temp_dir);
        return Err(format!("qlmanage failed: {}", stderr.trim()));
    }

    let preview_path = path
        .file_name()
        .map(|file_name| temp_dir.join(format!("{}.png", file_name.to_string_lossy())))
        .ok_or_else(|| "JPEG 2000 path did not have a file name".to_owned())?;
    let png = std::fs::read(&preview_path)
        .map_err(|error| format!("failed to read Quick Look PNG output: {error}"))?;
    let _ = std::fs::remove_dir_all(&temp_dir);
    let image = image::load_from_memory_with_format(&png, ImageFormat::Png)
        .map_err(|error| format!("failed to decode Quick Look PNG output: {error}"))?
        .to_rgba8();
    let (source_width, source_height) =
        native_image_dimensions(path).unwrap_or((image.width(), image.height()));

    Ok(decoded_custom_image_from_rgba_with_source_dimensions(
        image.as_raw().clone(),
        image.width(),
        image.height(),
        source_width,
        source_height,
        original_bytes.len() as u64,
        metadata.format_name,
        metadata.mime_type,
        &metadata.extension,
    ))
}

#[cfg(target_os = "macos")]
fn native_image_dimensions(path: &Path) -> Option<(u32, u32)> {
    let output = std::process::Command::new("sips")
        .arg("-g")
        .arg("pixelWidth")
        .arg("-g")
        .arg("pixelHeight")
        .arg(path)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut width = None;
    let mut height = None;
    for line in stdout.lines() {
        let line = line.trim();
        if let Some(value) = line.strip_prefix("pixelWidth:") {
            width = value.trim().parse::<u32>().ok();
        } else if let Some(value) = line.strip_prefix("pixelHeight:") {
            height = value.trim().parse::<u32>().ok();
        }
    }
    Some((width?, height?))
}

#[cfg(not(target_os = "macos"))]
fn decode_image_file_with_native_fallback(
    _path: &Path,
    _original_bytes: &[u8],
) -> Result<DecodedImage, String> {
    Err("native fallback is not available on this platform".to_owned())
}

struct NativeFallbackMetadata {
    format_name: &'static str,
    mime_type: &'static str,
    extension: String,
}

fn native_fallback_metadata_for_path(path: &Path) -> Option<NativeFallbackMetadata> {
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    match extension.as_str() {
        "avif" => Some(NativeFallbackMetadata {
            format_name: "AVIF",
            mime_type: "image/avif",
            extension: "avif".to_owned(),
        }),
        "heic" => Some(NativeFallbackMetadata {
            format_name: "HEIC",
            mime_type: "image/heic",
            extension: "heic".to_owned(),
        }),
        "heif" => Some(NativeFallbackMetadata {
            format_name: "HEIF",
            mime_type: "image/heif",
            extension: "heif".to_owned(),
        }),
        "jp2" | "j2k" | "j2c" => Some(NativeFallbackMetadata {
            format_name: "JP2",
            mime_type: "image/jp2",
            extension: "jp2".to_owned(),
        }),
        "jxl" => Some(NativeFallbackMetadata {
            format_name: "JXL",
            mime_type: "image/jxl",
            extension: "jxl".to_owned(),
        }),
        "psd" => Some(NativeFallbackMetadata {
            format_name: "PSD",
            mime_type: "image/vnd.adobe.photoshop",
            extension: "psd".to_owned(),
        }),
        "exr" => Some(NativeFallbackMetadata {
            format_name: "OpenEXR",
            mime_type: "image/exr",
            extension: "exr".to_owned(),
        }),
        "raw" | "dng" | "cr2" | "cr3" | "nef" | "arw" | "orf" | "rw2" => {
            Some(NativeFallbackMetadata {
                format_name: "RAW",
                mime_type: "image/x-raw",
                extension,
            })
        }
        _ => None,
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

fn image_format_mime_type(format: ImageFormat) -> &'static str {
    match format {
        ImageFormat::Avif => "image/avif",
        ImageFormat::Bmp => "image/bmp",
        ImageFormat::Dds => "image/vnd-ms.dds",
        ImageFormat::Farbfeld => "image/x-farbfeld",
        ImageFormat::Gif => "image/gif",
        ImageFormat::Hdr => "image/vnd.radiance",
        ImageFormat::Ico => "image/vnd.microsoft.icon",
        ImageFormat::Jpeg => "image/jpeg",
        ImageFormat::OpenExr => "image/exr",
        ImageFormat::Png => "image/png",
        ImageFormat::Pnm => "image/x-portable-anymap",
        ImageFormat::Qoi => "image/qoi",
        ImageFormat::Tga => "image/x-tga",
        ImageFormat::Tiff => "image/tiff",
        ImageFormat::WebP => "image/webp",
        _ => "application/octet-stream",
    }
}

fn decoded_image_from_rgba(
    rgba: Vec<u8>,
    width: u32,
    height: u32,
    file_size: u64,
    format: ImageFormat,
    animation: Option<DecodedAnimation>,
) -> DecodedImage {
    DecodedImage {
        rgba,
        width,
        height,
        source_width: width,
        source_height: height,
        file_size,
        format,
        format_name: format_name(format),
        mime_type: image_format_mime_type(format).to_owned(),
        extension: image_format_extension(format).to_owned(),
        animation,
    }
}

fn decoded_custom_image_from_rgba(
    rgba: Vec<u8>,
    width: u32,
    height: u32,
    file_size: u64,
    format_name: &str,
    mime_type: &str,
    extension: &str,
) -> DecodedImage {
    DecodedImage {
        rgba,
        width,
        height,
        source_width: width,
        source_height: height,
        file_size,
        format: ImageFormat::Png,
        format_name: format_name.to_owned(),
        mime_type: mime_type.to_owned(),
        extension: extension.to_owned(),
        animation: None,
    }
}

fn decoded_custom_image_from_rgba_with_source_dimensions(
    rgba: Vec<u8>,
    width: u32,
    height: u32,
    source_width: u32,
    source_height: u32,
    file_size: u64,
    format_name: &str,
    mime_type: &str,
    extension: &str,
) -> DecodedImage {
    DecodedImage {
        rgba,
        width,
        height,
        source_width,
        source_height,
        file_size,
        format: ImageFormat::Png,
        format_name: format_name.to_owned(),
        mime_type: mime_type.to_owned(),
        extension: extension.to_owned(),
        animation: None,
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

const MIN_ANIMATION_FRAME_DELAY_SECONDS: f32 = 0.02;
const DEFAULT_TINY_ANIMATION_FRAME_DELAY_SECONDS: f32 = 0.1;

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
            delay_seconds: normalized_animation_frame_delay(frame.delay_seconds),
        });
    }

    Ok(AnimatedPreview {
        frames: preview_frames,
        width: preview_width as u32,
        height: preview_height as u32,
    })
}

fn normalized_animation_frame_delay(delay_seconds: f32) -> f32 {
    if delay_seconds <= 0.01 {
        DEFAULT_TINY_ANIMATION_FRAME_DELAY_SECONDS
    } else {
        delay_seconds.max(MIN_ANIMATION_FRAME_DELAY_SECONDS)
    }
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

    Ok(decoded_image_from_rgba(
        image.into_raw(),
        width,
        height,
        file_size,
        format,
        None,
    ))
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

    Ok(decoded_image_from_rgba(
        rgba,
        width,
        height,
        file_size,
        ImageFormat::Gif,
        animation,
    ))
}

fn decode_webp(bytes: &[u8], file_size: u64) -> Result<DecodedImage, String> {
    let decoder = image::codecs::webp::WebPDecoder::new(std::io::Cursor::new(bytes))
        .map_err(|error| format!("failed to decode WebP: {error}"))?;
    if !decoder.has_animation() {
        return decode_with_image_crate(bytes, file_size, ImageFormat::WebP);
    }

    let frames = decoder
        .into_frames()
        .collect_frames()
        .map_err(|error| format!("failed to read WebP frames: {error}"))?;
    let first = frames
        .first()
        .ok_or_else(|| "WebP did not contain any frames".to_owned())?;
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

    Ok(decoded_image_from_rgba(
        rgba,
        width,
        height,
        file_size,
        ImageFormat::WebP,
        animation,
    ))
}

fn decode_jxl(bytes: &[u8], file_size: u64) -> Result<DecodedImage, String> {
    let decoder = jxl_oxide::integration::JxlDecoder::new(std::io::Cursor::new(bytes))
        .map_err(|error| format!("failed to read JPEG XL header: {error}"))?;
    let image = DynamicImage::from_decoder(decoder)
        .map_err(|error| format!("failed to decode JPEG XL: {error}"))?
        .to_rgba8();

    Ok(decoded_custom_image_from_rgba(
        image.as_raw().clone(),
        image.width(),
        image.height(),
        file_size,
        "JXL",
        "image/jxl",
        "jxl",
    ))
}

fn decode_exr(bytes: &[u8], file_size: u64) -> Result<DecodedImage, String> {
    use exr::prelude::{
        Image, Layer, ReadChannels, ReadLayers, RgbaChannels, SpecificChannels,
        read as read_exr_image,
    };

    let reader = read_exr_image()
        .no_deep_data()
        .largest_resolution_level()
        .rgba_channels(
            |resolution, _channels: &RgbaChannels| -> RgbaImage {
                RgbaImage::new(resolution.width() as u32, resolution.height() as u32)
            },
            |image, position, (red, green, blue, alpha): (f32, f32, f32, f32)| {
                image.put_pixel(
                    position.x() as u32,
                    position.y() as u32,
                    image::Rgba([
                        exr_sample_to_u8(red),
                        exr_sample_to_u8(green),
                        exr_sample_to_u8(blue),
                        exr_alpha_to_u8(alpha),
                    ]),
                );
            },
        )
        .first_valid_layer()
        .all_attributes();
    let image: Image<Layer<SpecificChannels<RgbaImage, RgbaChannels>>> = reader
        .from_buffered(std::io::Cursor::new(bytes))
        .map_err(|error| format!("failed to decode OpenEXR: {error}"))?;
    let rgba = image.layer_data.channel_data.pixels;
    let width = rgba.width();
    let height = rgba.height();

    Ok(decoded_custom_image_from_rgba(
        rgba.into_raw(),
        width,
        height,
        file_size,
        "OpenEXR",
        "image/exr",
        "exr",
    ))
}

fn exr_sample_to_u8(value: f32) -> u8 {
    let tone_mapped = (value - 0.5).tanh() * 0.5 + 0.5;
    (tone_mapped.clamp(0.0, 1.0) * 255.0).round() as u8
}

fn exr_alpha_to_u8(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

fn decode_xbm(bytes: &[u8], file_size: u64) -> Result<DecodedImage, String> {
    let text =
        std::str::from_utf8(bytes).map_err(|error| format!("failed to read XBM: {error}"))?;
    let width =
        xbm_define_value(text, "_width").ok_or_else(|| "failed to read XBM width".to_owned())?;
    let height =
        xbm_define_value(text, "_height").ok_or_else(|| "failed to read XBM height".to_owned())?;
    let data = text
        .split_once('{')
        .and_then(|(_, rest)| rest.split_once('}').map(|(body, _)| body))
        .ok_or_else(|| "failed to read XBM bitmap data".to_owned())?;
    let bytes = parse_c_integer_list(data)?;
    let required = (width as usize * height as usize).div_ceil(8);
    if bytes.len() < required {
        return Err(format!(
            "XBM bitmap data too short: expected at least {required} bytes, found {}",
            bytes.len()
        ));
    }

    let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
    for y in 0..height {
        for x in 0..width {
            let bit_index = y as usize * width as usize + x as usize;
            let value = (bytes[bit_index / 8] >> (bit_index % 8)) & 1;
            if value == 1 {
                rgba.extend_from_slice(&[0, 0, 0, 255]);
            } else {
                rgba.extend_from_slice(&[255, 255, 255, 255]);
            }
        }
    }

    Ok(decoded_custom_image_from_rgba(
        rgba,
        width,
        height,
        file_size,
        "XBM",
        "image/x-xbitmap",
        "xbm",
    ))
}

fn xbm_define_value(text: &str, suffix: &str) -> Option<u32> {
    text.lines().find_map(|line| {
        let mut parts = line.split_whitespace();
        (parts.next()? == "#define").then_some(())?;
        let name = parts.next()?;
        name.ends_with(suffix).then_some(())?;
        parts.next()?.parse::<u32>().ok()
    })
}

fn parse_c_integer_list(text: &str) -> Result<Vec<u8>, String> {
    let mut values = Vec::new();
    for raw in text.split(|character: char| character == ',' || character.is_whitespace()) {
        let value = raw.trim();
        if value.is_empty() {
            continue;
        }
        let value = value.trim_end_matches(['u', 'U']);
        let parsed = if let Some(hex) = value
            .strip_prefix("0x")
            .or_else(|| value.strip_prefix("0X"))
        {
            u8::from_str_radix(hex, 16)
        } else {
            value.parse::<u8>()
        }
        .map_err(|error| format!("failed to parse XBM byte {value:?}: {error}"))?;
        values.push(parsed);
    }
    Ok(values)
}

fn decode_xpm(bytes: &[u8], file_size: u64) -> Result<DecodedImage, String> {
    let text =
        std::str::from_utf8(bytes).map_err(|error| format!("failed to read XPM: {error}"))?;
    let strings = quoted_c_strings(text);
    let header = strings
        .first()
        .ok_or_else(|| "failed to read XPM header".to_owned())?;
    let mut header_parts = header.split_whitespace();
    let width = header_parts
        .next()
        .ok_or_else(|| "missing XPM width".to_owned())?
        .parse::<u32>()
        .map_err(|error| format!("invalid XPM width: {error}"))?;
    let height = header_parts
        .next()
        .ok_or_else(|| "missing XPM height".to_owned())?
        .parse::<u32>()
        .map_err(|error| format!("invalid XPM height: {error}"))?;
    let color_count = header_parts
        .next()
        .ok_or_else(|| "missing XPM color count".to_owned())?
        .parse::<usize>()
        .map_err(|error| format!("invalid XPM color count: {error}"))?;
    let chars_per_pixel = header_parts
        .next()
        .ok_or_else(|| "missing XPM chars-per-pixel".to_owned())?
        .parse::<usize>()
        .map_err(|error| format!("invalid XPM chars-per-pixel: {error}"))?;
    if chars_per_pixel == 0 {
        return Err("XPM chars-per-pixel must be positive".to_owned());
    }
    if strings.len() < 1 + color_count + height as usize {
        return Err("XPM did not contain enough color or pixel rows".to_owned());
    }

    let mut colors = std::collections::HashMap::with_capacity(color_count);
    for line in strings.iter().skip(1).take(color_count) {
        if line.len() < chars_per_pixel {
            return Err("XPM color key shorter than chars-per-pixel".to_owned());
        }
        let key = line[..chars_per_pixel].to_owned();
        colors.insert(key, parse_xpm_color(line[chars_per_pixel..].trim())?);
    }

    let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
    for row in strings.iter().skip(1 + color_count).take(height as usize) {
        if row.len() < width as usize * chars_per_pixel {
            return Err("XPM pixel row shorter than expected".to_owned());
        }
        for x in 0..width as usize {
            let start = x * chars_per_pixel;
            let key = &row[start..start + chars_per_pixel];
            let color = colors
                .get(key)
                .ok_or_else(|| format!("XPM pixel references unknown color key {key:?}"))?;
            rgba.extend_from_slice(color);
        }
    }

    Ok(decoded_custom_image_from_rgba(
        rgba,
        width,
        height,
        file_size,
        "XPM",
        "image/x-xpixmap",
        "xpm",
    ))
}

fn quoted_c_strings(text: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut chars = text.chars().peekable();
    while let Some(character) = chars.next() {
        if character != '"' {
            continue;
        }
        let mut value = String::new();
        let mut escaped = false;
        for character in chars.by_ref() {
            if escaped {
                value.push(character);
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                break;
            } else {
                value.push(character);
            }
        }
        values.push(value);
    }
    values
}

fn parse_xpm_color(definition: &str) -> Result<[u8; 4], String> {
    let mut parts = definition.split_whitespace();
    while let Some(key) = parts.next() {
        let Some(value) = parts.next() else {
            break;
        };
        if key != "c" {
            continue;
        }
        if value.eq_ignore_ascii_case("none") {
            return Ok([0, 0, 0, 0]);
        }
        let hex = value
            .strip_prefix('#')
            .ok_or_else(|| format!("unsupported XPM color value {value:?}"))?;
        if hex.len() != 6 {
            return Err(format!("unsupported XPM color value {value:?}"));
        }
        let red = u8::from_str_radix(&hex[0..2], 16)
            .map_err(|error| format!("invalid XPM red channel: {error}"))?;
        let green = u8::from_str_radix(&hex[2..4], 16)
            .map_err(|error| format!("invalid XPM green channel: {error}"))?;
        let blue = u8::from_str_radix(&hex[4..6], 16)
            .map_err(|error| format!("invalid XPM blue channel: {error}"))?;
        return Ok([red, green, blue, 255]);
    }
    Err(format!("unsupported XPM color definition {definition:?}"))
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

    Ok(decoded_image_from_rgba(
        image.pixels,
        header.width as u32,
        header.height as u32,
        file_size,
        ImageFormat::Jpeg,
        None,
    ))
}

#[cfg(test)]
mod tests {
    use super::{
        decode_image_bytes, decode_image_file, decode_image_file_perf_report, fitted_texture_size,
        native_fallback_metadata_for_path, preview_image_from_rgba,
    };
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
            "snappyref-decode-jpeg-test-{}",
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
    fn decode_image_file_decodes_checked_in_ppm_smoke_fixture() {
        let path = smoke_fixture_path("ppm-2x2.ppm");

        let decoded = decode_image_file(&path).unwrap();

        assert_eq!(decoded.format, image::ImageFormat::Pnm);
        assert_eq!(decoded.width, 2);
        assert_eq!(decoded.height, 2);
        assert_eq!(decoded.rgba.len(), 2 * 2 * 4);
        assert_eq!(&decoded.rgba[..4], &[255, 0, 0, 255]);
    }

    #[test]
    fn decode_image_file_decodes_generated_common_static_formats() {
        let root = temp_test_dir("snappyref-decode-common-formats-test");
        let rgba = sample_rgba_2x2();
        let rgb = sample_rgb_2x2();
        let mut cases = Vec::new();

        let png = root.join("sample.png");
        image::codecs::png::PngEncoder::new(std::fs::File::create(&png).unwrap())
            .write_image(&rgba, 2, 2, ExtendedColorType::Rgba8)
            .unwrap();
        cases.push((png, "PNG"));

        let jpg = root.join("sample.jpg");
        JpegEncoder::new_with_quality(std::fs::File::create(&jpg).unwrap(), 85)
            .write_image(&rgb, 2, 2, ExtendedColorType::Rgb8)
            .unwrap();
        cases.push((jpg, "JPEG"));

        let bmp = root.join("sample.bmp");
        let mut bmp_file = std::fs::File::create(&bmp).unwrap();
        image::codecs::bmp::BmpEncoder::new(&mut bmp_file)
            .write_image(&rgba, 2, 2, ExtendedColorType::Rgba8)
            .unwrap();
        cases.push((bmp, "BMP"));

        let tiff = root.join("sample.tiff");
        image::codecs::tiff::TiffEncoder::new(std::fs::File::create(&tiff).unwrap())
            .write_image(&rgba, 2, 2, ExtendedColorType::Rgba8)
            .unwrap();
        cases.push((tiff, "TIFF"));

        let qoi = root.join("sample.qoi");
        image::codecs::qoi::QoiEncoder::new(std::fs::File::create(&qoi).unwrap())
            .write_image(&rgba, 2, 2, ExtendedColorType::Rgba8)
            .unwrap();
        cases.push((qoi, "QOI"));

        let tga = root.join("sample.tga");
        image::codecs::tga::TgaEncoder::new(std::fs::File::create(&tga).unwrap())
            .write_image(&rgba, 2, 2, ExtendedColorType::Rgba8)
            .unwrap();
        cases.push((tga, "TGA"));

        for (path, format_name) in cases {
            let decoded = decode_image_file(&path).unwrap();
            assert_eq!(decoded.format_name, format_name, "{}", path.display());
            assert_eq!(decoded.width, 2, "{}", path.display());
            assert_eq!(decoded.height, 2, "{}", path.display());
            assert_eq!(decoded.rgba.len(), 2 * 2 * 4, "{}", path.display());
        }

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn decode_image_file_decodes_smoke_fixture_manifest() {
        let manifest_path = smoke_fixture_path("manifest.json");
        let manifest = std::fs::read_to_string(&manifest_path).unwrap();
        let fixtures: Vec<SmokeFixture> = serde_json::from_str(&manifest).unwrap();

        assert!(!fixtures.is_empty());
        for fixture in fixtures {
            let decoded = decode_image_file(&smoke_fixture_path(&fixture.file)).unwrap();
            assert_eq!(decoded.width, fixture.width, "{}", fixture.file);
            assert_eq!(decoded.height, fixture.height, "{}", fixture.file);
            assert_eq!(
                decoded.format_name,
                fixture.expected_format(),
                "{}",
                fixture.file
            );
            if let Some(animation_frame_count) = fixture.animation_frame_count {
                assert_eq!(
                    decoded
                        .animation
                        .as_ref()
                        .map(|animation| animation.frames.len()),
                    Some(animation_frame_count),
                    "{}",
                    fixture.file
                );
            }
        }
    }

    #[test]
    fn decode_image_file_decodes_xbm_smoke_fixture() {
        let decoded = decode_image_file(&smoke_fixture_path("xbm-2x2.xbm")).unwrap();

        assert_eq!(decoded.format_name, "XBM");
        assert_eq!(decoded.mime_type, "image/x-xbitmap");
        assert_eq!(decoded.extension, "xbm");
        assert_eq!(decoded.width, 2);
        assert_eq!(decoded.height, 2);
        assert_eq!(
            decoded.rgba,
            vec![
                0, 0, 0, 255, 255, 255, 255, 255, 255, 255, 255, 255, 0, 0, 0, 255
            ]
        );
    }

    #[test]
    fn decode_image_file_decodes_xpm_smoke_fixture() {
        let decoded = decode_image_file(&smoke_fixture_path("xpm-2x2.xpm")).unwrap();

        assert_eq!(decoded.format_name, "XPM");
        assert_eq!(decoded.mime_type, "image/x-xpixmap");
        assert_eq!(decoded.extension, "xpm");
        assert_eq!(decoded.width, 2);
        assert_eq!(decoded.height, 2);
        assert_eq!(
            decoded.rgba,
            vec![255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 0, 0, 255, 0, 0, 255]
        );
    }

    #[test]
    fn decode_image_file_decodes_jxl_with_dedicated_decoder_when_cjxl_is_available() {
        let Some(cjxl) = std::env::var_os("SNAPPYREF_CJXL")
            .map(std::path::PathBuf::from)
            .or_else(|| find_command_in_path("cjxl"))
        else {
            eprintln!("skipping JPEG XL decode smoke test: cjxl not found");
            return;
        };
        let root = std::env::temp_dir().join(format!(
            "snappyref-decode-jxl-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let ppm = root.join("sample.ppm");
        let jxl = root.join("sample.jxl");
        std::fs::write(
            &ppm,
            [
                b"P6\n2 2\n255\n".as_slice(),
                &[255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255],
            ]
            .concat(),
        )
        .unwrap();
        let output = std::process::Command::new(cjxl)
            .arg(&ppm)
            .arg(&jxl)
            .arg("--quiet")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "cjxl failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );

        let decoded = decode_image_file(&jxl).unwrap();

        assert_eq!(decoded.format_name, "JXL");
        assert_eq!(decoded.mime_type, "image/jxl");
        assert_eq!(decoded.extension, "jxl");
        assert_eq!(decoded.width, 2);
        assert_eq!(decoded.height, 2);
        assert_eq!(decoded.rgba.len(), 2 * 2 * 4);

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn decode_image_file_decodes_external_exr_fixture_when_available() {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/external/files/image-rs/overexposed-gradient.exr");
        if !path.exists() {
            eprintln!(
                "skipping OpenEXR external fixture test: run scripts/fetch-image-fixtures.sh"
            );
            return;
        }

        let decoded = decode_image_file(&path).unwrap();

        assert_eq!(decoded.format_name, "OpenEXR");
        assert_eq!(decoded.mime_type, "image/exr");
        assert_eq!(decoded.extension, "exr");
        assert_eq!(decoded.width, 20);
        assert_eq!(decoded.height, 12);
        assert_eq!(decoded.rgba.len(), 20 * 12 * 4);
    }

    #[test]
    fn decode_image_file_decodes_external_jp2_fixture_when_available() {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/external/files/jpeg2k/file6.jp2");
        if !path.exists() {
            eprintln!(
                "skipping JPEG 2000 external fixture test: run scripts/fetch-image-fixtures.sh"
            );
            return;
        }

        let decoded = decode_image_file(&path).unwrap();

        assert_eq!(decoded.format_name, "JP2");
        assert_eq!(decoded.mime_type, "image/jp2");
        assert_eq!(decoded.extension, "jp2");
        assert_eq!(decoded.source_width, 768);
        assert_eq!(decoded.source_height, 512);
        assert_eq!(decoded.width, 768);
        assert_eq!(decoded.height, 512);
        assert_eq!(decoded.rgba.len(), 768 * 512 * 4);
    }

    #[test]
    fn decode_image_file_decodes_external_psd_fixture_when_available() {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/external/files/pillow/hopper_merged.psd");
        if !path.exists() {
            eprintln!("skipping PSD external fixture test: run scripts/fetch-image-fixtures.sh");
            return;
        }

        let decoded = decode_image_file(&path).unwrap();

        assert_eq!(decoded.format_name, "PSD");
        assert_eq!(decoded.mime_type, "image/vnd.adobe.photoshop");
        assert_eq!(decoded.extension, "psd");
        assert_eq!(decoded.source_width, 128);
        assert_eq!(decoded.source_height, 128);
        assert_eq!(decoded.width, 128);
        assert_eq!(decoded.height, 128);
        assert_eq!(decoded.rgba.len(), 128 * 128 * 4);
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn decode_image_file_decodes_external_dng_fixture_as_raw_when_available() {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(
            "tests/fixtures/external/files/raw-pixls-us/Canon-PowerShot-A410/CANON_A410_CHDK_CRW_0741.DNG",
        );
        if !path.exists() {
            eprintln!("skipping RAW external fixture test: run scripts/fetch-image-fixtures.sh");
            return;
        }

        let decoded = decode_image_file(&path).unwrap();

        assert_eq!(decoded.format_name, "RAW");
        assert_eq!(decoded.mime_type, "image/x-raw");
        assert_eq!(decoded.extension, "dng");
        assert_eq!(decoded.source_width, 2048);
        assert_eq!(decoded.source_height, 1536);
        assert_eq!(decoded.width, 2048);
        assert_eq!(decoded.height, 1536);
        assert_eq!(decoded.rgba.len(), 2048 * 1536 * 4);
    }

    #[test]
    fn decode_image_bytes_uses_source_label_extension_for_custom_formats() {
        let xbm = br#"#define sample_width 2
#define sample_height 2
static unsigned char sample_bits[] = { 0x01 };
"#;

        let decoded = decode_image_bytes(
            xbm,
            Some("https://example.test/sample.xbm".to_owned()),
            None,
        )
        .unwrap();

        assert_eq!(decoded.format_name, "XBM");
        assert_eq!(decoded.width, 2);
        assert_eq!(decoded.height, 2);
    }

    #[test]
    fn native_fallback_metadata_covers_specialty_formats() {
        for (file, format_name, mime_type) in [
            ("sample.avif", "AVIF", "image/avif"),
            ("sample.heic", "HEIC", "image/heic"),
            ("sample.heif", "HEIF", "image/heif"),
            ("sample.jp2", "JP2", "image/jp2"),
            ("sample.jxl", "JXL", "image/jxl"),
            ("sample.psd", "PSD", "image/vnd.adobe.photoshop"),
            ("sample.exr", "OpenEXR", "image/exr"),
            ("sample.dng", "RAW", "image/x-raw"),
        ] {
            let metadata = native_fallback_metadata_for_path(std::path::Path::new(file)).unwrap();
            assert_eq!(metadata.format_name, format_name, "{file}");
            assert_eq!(metadata.mime_type, mime_type, "{file}");
        }
    }

    #[test]
    fn decode_image_file_uses_extension_when_magic_is_ambiguous() {
        let root = std::env::temp_dir().join(format!(
            "snappyref-decode-tga-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("sample.tga");
        let tga_2x2_rgb = [
            0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2, 0, 2, 0, 24, 0x20, 0, 0, 255, 0, 255, 0, 255, 0,
            0, 255, 255, 255,
        ];
        std::fs::write(&path, tga_2x2_rgb).unwrap();

        let decoded = decode_image_file(&path).unwrap();

        assert_eq!(decoded.format, image::ImageFormat::Tga);
        assert_eq!(decoded.width, 2);
        assert_eq!(decoded.height, 2);
        assert_eq!(decoded.rgba.len(), 2 * 2 * 4);

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn decode_image_file_perf_report_decodes_jpeg_to_rgba_and_reports_timing() {
        let root = std::env::temp_dir().join(format!(
            "snappyref-decode-perf-jpeg-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("sample.jpg");
        let rgb = vec![255; 8 * 6 * 3];
        let file = std::fs::File::create(&path).unwrap();
        let writer = std::io::BufWriter::new(file);
        let encoder = JpegEncoder::new_with_quality(writer, 85);
        encoder
            .write_image(&rgb, 8, 6, ExtendedColorType::Rgb8)
            .unwrap();

        let report = decode_image_file_perf_report(&path).unwrap();

        assert_eq!(report.source_path, path);
        assert_eq!(report.format_name, "JPEG");
        assert_eq!(report.width, 8);
        assert_eq!(report.height, 6);
        assert_eq!(report.rgba_bytes, 8 * 6 * 4);
        assert!(report.source_file_size > 0);
        assert!(report.megapixels > 0.0);
        assert!(report.decode_ms >= 0.0);
        assert!(report.ms_per_megapixel >= 0.0);

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn decode_image_file_detects_animated_gif_frames() {
        let root = std::env::temp_dir().join(format!(
            "snappyref-decode-gif-test-{}",
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

    #[test]
    fn decode_image_file_detects_animated_webp_frames() {
        let root = std::env::temp_dir().join(format!(
            "snappyref-decode-webp-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("sample.webp");
        std::fs::write(&path, ANIMATED_WEBP_2X2_2_FRAMES).unwrap();

        let decoded = decode_image_file(&path).unwrap();
        assert_eq!(decoded.format, image::ImageFormat::WebP);
        assert_eq!(decoded.width, 2);
        assert_eq!(decoded.height, 2);
        let animation = decoded.animation.expect("expected animation metadata");
        assert_eq!(animation.frames.len(), 2);
        assert!(animation.frames[0].delay_seconds > 0.0);

        let _ = std::fs::remove_dir_all(root);
    }

    const ANIMATED_WEBP_2X2_2_FRAMES: &[u8] = &[
        0x52, 0x49, 0x46, 0x46, 0x84, 0x00, 0x00, 0x00, 0x57, 0x45, 0x42, 0x50, 0x56, 0x50, 0x38,
        0x58, 0x0a, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x01, 0x00, 0x00,
        0x41, 0x4e, 0x49, 0x4d, 0x06, 0x00, 0x00, 0x00, 0x00, 0xff, 0x00, 0xff, 0x00, 0x00, 0x41,
        0x4e, 0x4d, 0x46, 0x28, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00,
        0x00, 0x01, 0x00, 0x00, 0x78, 0x00, 0x00, 0x02, 0x56, 0x50, 0x38, 0x4c, 0x0f, 0x00, 0x00,
        0x00, 0x2f, 0x01, 0x40, 0x00, 0x00, 0x07, 0x10, 0xe5, 0x8f, 0xfe, 0x07, 0x22, 0xa2, 0xff,
        0x01, 0x00, 0x41, 0x4e, 0x4d, 0x46, 0x28, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x01, 0x00, 0x00, 0x01, 0x00, 0x00, 0x78, 0x00, 0x00, 0x00, 0x56, 0x50, 0x38, 0x4c,
        0x0f, 0x00, 0x00, 0x00, 0x2f, 0x01, 0x40, 0x00, 0x00, 0x07, 0x10, 0xd1, 0xfe, 0xfe, 0x07,
        0x22, 0xa2, 0xff, 0x01, 0x00,
    ];

    fn temp_test_dir(prefix: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "{prefix}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    fn sample_rgba_2x2() -> Vec<u8> {
        vec![
            255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 128,
        ]
    }

    fn sample_rgb_2x2() -> Vec<u8> {
        vec![255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255]
    }

    fn smoke_fixture_path(name: &str) -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/images/smoke")
            .join(name)
    }

    fn find_command_in_path(command: &str) -> Option<std::path::PathBuf> {
        let path = std::env::var_os("PATH")?;
        for directory in std::env::split_paths(&path) {
            let candidate = directory.join(command);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
        None
    }

    #[derive(serde::Deserialize)]
    struct SmokeFixture {
        file: String,
        format: String,
        width: u32,
        height: u32,
        animation_frame_count: Option<usize>,
    }

    impl SmokeFixture {
        fn expected_format(&self) -> &str {
            match self.format.as_str() {
                "dds" => "DDS",
                "pnm" => "PNM",
                "webp" => "WebP",
                "xbm" => "XBM",
                "xpm" => "XPM",
                other => panic!("unknown smoke fixture format {other}"),
            }
        }
    }
}
