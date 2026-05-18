use super::*;

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

    sanitized.trim_matches('-').to_owned().if_empty("image")
}

trait StringFallback {
    fn if_empty(self, fallback: &str) -> String;
}

impl StringFallback for String {
    fn if_empty(self, fallback: &str) -> String {
        if self.is_empty() {
            fallback.to_owned()
        } else {
            self
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CanvasImageCacheEncoding {
    Jpeg,
    WebpLossless,
}

impl CanvasImageCacheEncoding {
    fn extension(self) -> &'static str {
        match self {
            Self::Jpeg => "jpg",
            Self::WebpLossless => "webp",
        }
    }
}

fn choose_canvas_image_cache_encoding(rgba: &[u8]) -> CanvasImageCacheEncoding {
    if rgba.chunks_exact(4).all(|pixel| pixel[3] == u8::MAX) {
        CanvasImageCacheEncoding::Jpeg
    } else {
        CanvasImageCacheEncoding::WebpLossless
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
    for pixel in rgba.chunks_exact(4) {
        rgb.extend_from_slice(&pixel[..3]);
    }
    rgb
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

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn asset_thumb_path(&self, asset_id: i64) -> PathBuf {
        self.thumbs_dir.join(format!("asset-{asset_id}.png"))
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn canvas_image_cache_path(&self, asset_id: i64, max_side: u32) -> Option<PathBuf> {
        self.canvas_image_cache_path_for_lod(asset_id, max_side, CanvasImageLod::Working)
    }

    pub fn canvas_image_cache_path_for_lod(
        &self,
        asset_id: i64,
        max_side: u32,
        lod: CanvasImageLod,
    ) -> Option<PathBuf> {
        [
            CanvasImageCacheEncoding::Jpeg,
            CanvasImageCacheEncoding::WebpLossless,
        ]
        .into_iter()
        .map(|encoding| {
            self.canvas_image_cache_destination_for_lod(asset_id, max_side, lod, encoding)
        })
        .find(|path| path.is_file())
    }

    #[cfg_attr(not(feature = "semantic-search"), allow(dead_code))]
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
        let encoding = match extension {
            "jpg" => CanvasImageCacheEncoding::Jpeg,
            "webp" => CanvasImageCacheEncoding::WebpLossless,
            _ => {
                return Err(format!(
                    "unsupported canvas image cache extension {extension:?}"
                ));
            }
        };
        Ok(self.canvas_image_cache_destination_for_lod(asset_id, max_side, lod, encoding))
    }

    pub fn store_canvas_image_cache(
        &self,
        asset_id: i64,
        max_side: u32,
        rgba: &[u8],
        width: u32,
        height: u32,
    ) -> Result<Option<PathBuf>, String> {
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
    ) -> Result<Option<PathBuf>, String> {
        let encoding = choose_canvas_image_cache_encoding(rgba);
        let destination =
            self.canvas_image_cache_destination_for_lod(asset_id, max_side, lod, encoding);
        let file = File::create(&destination)
            .map_err(|error| format!("failed to create asset preview: {error}"))?;
        let writer = BufWriter::new(file);
        match encoding {
            CanvasImageCacheEncoding::Jpeg => {
                let rgb = rgba_to_rgb(rgba);
                let encoder =
                    JpegEncoder::new_with_quality(writer, canvas_image_cache_quality(lod));
                encoder
                    .write_image(&rgb, width, height, ExtendedColorType::Rgb8)
                    .map_err(|error| format!("failed to write asset preview: {error}"))?;
            }
            CanvasImageCacheEncoding::WebpLossless => {
                let encoder = WebPEncoder::new_lossless(writer);
                encoder
                    .write_image(rgba, width, height, ExtendedColorType::Rgba8)
                    .map_err(|error| format!("failed to write asset preview: {error}"))?;
            }
        }
        Ok(Some(destination))
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
