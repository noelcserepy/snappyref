use super::connection::Library;
use crate::data::CanvasImageLod;
use crate::image_preparation::cache_encoding::CanvasImageCacheEncoding;
use image::ColorType;
use image::ExtendedColorType;
use image::ImageEncoder;
use image::ImageFormat;
use image::codecs::png::CompressionType;
use image::codecs::png::FilterType;
use image::codecs::png::PngEncoder;
use std::fs;
use std::fs::File;
use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

static DESTINATION_PATH_COUNTER: AtomicU64 = AtomicU64::new(0);

pub(super) fn unix_timestamp_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0)
}

pub(super) fn sanitize_filename(input: &str) -> String {
    let sanitized: String = input
        .chars()
        .map(|character| match character {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' => character,
            _ => '-',
        })
        .collect();

    let sanitized = sanitized.trim_matches('-');
    if sanitized.is_empty() {
        "image".to_owned()
    } else {
        sanitized.to_owned()
    }
}

impl Library {
    pub fn store_original(&self, source_path: &Path) -> Result<PathBuf, String> {
        let original_name = source_path
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("image");
        let extension = source_path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("png");
        let destination = self.destination_path(original_name, extension);

        fs::copy(source_path, &destination)
            .map_err(|error| format!("failed to copy original: {error}"))?;
        Ok(destination)
    }

    pub fn store_original_bytes(
        &self,
        source_name: &str,
        extension: &str,
        bytes: &[u8],
    ) -> Result<PathBuf, String> {
        let destination = self.destination_path(source_name, extension);
        fs::write(&destination, bytes)
            .map_err(|error| format!("failed to write original bytes: {error}"))?;
        Ok(destination)
    }

    pub fn store_clipboard_image(
        &self,
        rgba: &[u8],
        width: u32,
        height: u32,
    ) -> Result<PathBuf, String> {
        self.store_rgba_image("clipboard", rgba, width, height)
    }

    pub fn store_rgba_image(
        &self,
        source_name: &str,
        rgba: &[u8],
        width: u32,
        height: u32,
    ) -> Result<PathBuf, String> {
        let destination = self.destination_path(source_name, "png");
        image::save_buffer_with_format(
            &destination,
            rgba,
            width,
            height,
            ColorType::Rgba8,
            ImageFormat::Png,
        )
        .map_err(|error| format!("failed to write clipboard image: {error}"))?;
        Ok(destination)
    }

    pub fn store_cropped_rgba_image(
        &self,
        source_name: &str,
        rgba: &[u8],
        width: u32,
        height: u32,
    ) -> Result<PathBuf, String> {
        let destination = self.destination_path(source_name, "png");
        let output = File::create(&destination)
            .map_err(|error| format!("failed to create cropped image: {error}"))?;
        PngEncoder::new_with_quality(output, CompressionType::Fast, FilterType::NoFilter)
            .write_image(rgba, width, height, ExtendedColorType::Rgba8)
            .map_err(|error| format!("failed to write cropped image: {error}"))?;
        Ok(destination)
    }

    fn canvas_image_cache_destination_for_lod(
        &self,
        asset_id: i64,
        max_side: u32,
        lod: CanvasImageLod,
        encoding: CanvasImageCacheEncoding,
    ) -> PathBuf {
        self.thumbs_dir.join(format!(
            "asset-{asset_id}-canvas-v3-downscale-{}-{max_side}.{}",
            lod.as_str(),
            encoding.extension()
        ))
    }

    #[cfg(test)]
    pub fn canvas_image_cache_path(&self, asset_id: i64, max_side: u32) -> Option<PathBuf> {
        self.canvas_image_cache_path_for_lod(asset_id, max_side, CanvasImageLod::Working)
    }

    pub fn canvas_image_cache_path_for_lod(
        &self,
        asset_id: i64,
        max_side: u32,
        lod: CanvasImageLod,
    ) -> Option<PathBuf> {
        CanvasImageCacheEncoding::ALL
            .into_iter()
            .map(|encoding| {
                self.canvas_image_cache_destination_for_lod(asset_id, max_side, lod, encoding)
            })
            .find(|path| path.is_file())
    }

    pub fn asset_search_preview_paths_for_assets(
        &self,
        asset_ids: &[i64],
    ) -> std::collections::HashMap<i64, PathBuf> {
        asset_ids
            .iter()
            .filter_map(|asset_id| {
                self.canvas_image_cache_path_for_lod(
                    *asset_id,
                    crate::data::DEFAULT_CANVAS_IMAGE_MAX_SIDE,
                    CanvasImageLod::Overview,
                )
                .or_else(|| {
                    self.canvas_image_cache_path_for_lod(
                        *asset_id,
                        crate::data::DEFAULT_CANVAS_IMAGE_MAX_SIDE,
                        CanvasImageLod::Working,
                    )
                })
                .map(|path| (*asset_id, path))
            })
            .collect()
    }

    pub fn canvas_image_cache_destination_for_extension(
        &self,
        asset_id: i64,
        max_side: u32,
        lod: CanvasImageLod,
        extension: &str,
    ) -> Result<PathBuf, String> {
        let encoding = CanvasImageCacheEncoding::from_extension(extension)
            .ok_or_else(|| format!("unsupported canvas image cache extension {extension:?}"))?;
        Ok(self.canvas_image_cache_destination_for_lod(asset_id, max_side, lod, encoding))
    }

    pub fn store_canvas_image_cache(
        &self,
        asset_id: i64,
        max_side: u32,
        rgba: &[u8],
        width: u32,
        height: u32,
    ) -> Result<PathBuf, String> {
        self.store_canvas_image_cache_for_lod(
            asset_id,
            max_side,
            CanvasImageLod::Working,
            rgba,
            width,
            height,
        )
    }

    pub fn store_canvas_image_cache_for_lod(
        &self,
        asset_id: i64,
        max_side: u32,
        lod: CanvasImageLod,
        rgba: &[u8],
        width: u32,
        height: u32,
    ) -> Result<PathBuf, String> {
        let encoding = CanvasImageCacheEncoding::for_pixels(rgba);
        let destination =
            self.canvas_image_cache_destination_for_lod(asset_id, max_side, lod, encoding);
        encoding.write(&destination, rgba, width, height, lod)?;
        Ok(destination)
    }

    pub fn remove_asset_canvas_caches(&self, asset_id: i64) -> Result<(), String> {
        let prefix = format!("asset-{asset_id}-");
        let entries = fs::read_dir(&self.thumbs_dir)
            .map_err(|error| format!("failed to read thumbs dir: {error}"))?;
        for entry in entries {
            let entry = entry.map_err(|error| format!("failed to read thumbs entry: {error}"))?;
            let file_name = entry.file_name();
            let file_name = file_name.to_string_lossy();
            if !file_name.starts_with(&prefix) {
                continue;
            }
            match fs::remove_file(entry.path()) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(format!("failed to remove asset preview: {error}")),
            }
        }
        Ok(())
    }

    fn destination_path(&self, source_name: &str, extension: &str) -> PathBuf {
        let timestamp = unix_timestamp_millis();
        let sequence = DESTINATION_PATH_COUNTER.fetch_add(1, Ordering::Relaxed);
        let safe_name = sanitize_filename(source_name);
        self.originals_dir
            .join(format!("{timestamp}-{sequence}-{safe_name}.{extension}"))
    }
}

pub(super) fn unix_timestamp_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}
