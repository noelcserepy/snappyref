use crate::data::CanvasImageLod;
use image::codecs::{jpeg::JpegEncoder, webp::WebPEncoder};
use image::{ExtendedColorType, ImageEncoder};
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CanvasImageCacheEncoding {
    Jpeg,
    WebpLossless,
}

impl CanvasImageCacheEncoding {
    pub(crate) const ALL: [Self; 2] = [Self::Jpeg, Self::WebpLossless];

    pub(crate) fn from_extension(extension: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|encoding| encoding.extension() == extension)
    }

    pub(crate) fn extension(self) -> &'static str {
        match self {
            Self::Jpeg => "jpg",
            Self::WebpLossless => "webp",
        }
    }
}

fn canvas_image_cache_quality(lod: CanvasImageLod) -> u8 {
    match lod {
        CanvasImageLod::Overview => 72,
        CanvasImageLod::Working => 82,
        CanvasImageLod::Detail => 88,
    }
}

fn rgba_to_rgb(rgba: &[u8]) -> Vec<u8> {
    let mut rgb = Vec::with_capacity(rgba.len() / 4 * 3);
    for pixel in rgba.as_chunks::<4>().0 {
        rgb.extend_from_slice(&pixel[..3]);
    }
    rgb
}

impl CanvasImageCacheEncoding {
    pub(crate) fn for_pixels(rgba: &[u8]) -> Self {
        if rgba
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| pixel[3] == u8::MAX)
        {
            Self::Jpeg
        } else {
            Self::WebpLossless
        }
    }

    pub(crate) fn write(
        self,
        path: &Path,
        rgba: &[u8],
        width: u32,
        height: u32,
        lod: CanvasImageLod,
    ) -> Result<(), String> {
        let file = std::fs::File::create(path)
            .map_err(|error| format!("failed to create canvas cache: {error}"))?;
        let writer = std::io::BufWriter::new(file);
        let result = match self {
            Self::Jpeg => JpegEncoder::new_with_quality(writer, canvas_image_cache_quality(lod))
                .write_image(&rgba_to_rgb(rgba), width, height, ExtendedColorType::Rgb8),
            Self::WebpLossless => WebPEncoder::new_lossless(writer).write_image(
                rgba,
                width,
                height,
                ExtendedColorType::Rgba8,
            ),
        };
        result.map_err(|error| format!("failed to write canvas cache: {error}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_encoding_keeps_transparency_and_uses_jpeg_for_opaque_pixels() {
        let root = crate::test_support::TempRoot::new("canvas-cache-encoding");
        for rgba in [[40, 80, 120, 17], [40, 80, 120, 255]] {
            let encoding = CanvasImageCacheEncoding::for_pixels(&rgba);
            let path = root.join(format!("cache.{}", encoding.extension()));
            encoding
                .write(&path, &rgba, 1, 1, CanvasImageLod::Working)
                .unwrap();
            let decoded = image::open(&path).unwrap().to_rgba8();
            assert_eq!(decoded.dimensions(), (1, 1));
            if rgba[3] < 255 {
                assert_eq!(encoding, CanvasImageCacheEncoding::WebpLossless);
                assert_eq!(decoded.as_raw(), &rgba);
            } else {
                assert_eq!(encoding, CanvasImageCacheEncoding::Jpeg);
                assert_eq!(decoded.as_raw()[3], 255);
            }
        }
    }
}
