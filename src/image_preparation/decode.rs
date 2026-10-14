use std::path::{Path, PathBuf};

use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::data::CanvasImageLod;
use crate::image_preparation::image_formats::decode_image_file_with_max_side;
use crate::library::storage::Library;

use super::{CanvasImageSettings, canvas_source_image_from_rgba};

#[derive(Clone, Debug)]
pub(super) struct PreparedImage {
    pub(super) rgba: Vec<u8>,
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) source_width: u32,
    pub(super) source_height: u32,
    pub(super) mime_type: String,
    pub(super) cache_path: Option<PathBuf>,
    pub(super) cache_extension: Option<String>,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
struct ImagePrepOutput {
    animated: bool,
    rgba_path: Option<PathBuf>,
    width: u32,
    height: u32,
    source_width: u32,
    source_height: u32,
    mime_type: String,
    cache_path: Option<PathBuf>,
    cache_extension: Option<String>,
}

/// Executes the per-image decode subprocess. Tests can supply a small
/// executable to exercise the same process protocol and deadline.
#[derive(Clone)]
pub(crate) struct ImagePrepHelper {
    pub(super) executable: Option<PathBuf>,
    pub(super) timeout: std::time::Duration,
}

impl Default for ImagePrepHelper {
    fn default() -> Self {
        Self {
            #[cfg(not(test))]
            executable: std::env::current_exe().ok(),
            #[cfg(test)]
            executable: None,
            timeout: std::time::Duration::from_secs(60),
        }
    }
}

impl ImagePrepHelper {
    pub(super) fn prepare(
        &self,
        source_path: &Path,
        canvas_image_settings: CanvasImageSettings,
    ) -> Result<Option<PreparedImage>, String> {
        let Some(executable) = &self.executable else {
            return Ok(None);
        };
        let mut files = HelperOutputFiles {
            rgba_path: unique_imageprep_rgba_path(),
            keep_cache: false,
        };
        let rgba_path = files.rgba_path.clone();
        let mut child = Command::new(executable)
            .arg("--imageprep")
            .arg(format!("--source={}", source_path.display()))
            .arg(format!("--rgba-output={}", rgba_path.display()))
            .arg(format!(
                "--canvas-max-side={}",
                canvas_image_settings.max_side
            ))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| format!("failed to run imageprep helper: {error}"))?;
        // Drain both pipes while waiting, so a decoder's diagnostics cannot
        // fill a pipe and prevent its exit or our deadline from being observed.
        let stdout = child.stdout.take().expect("piped helper stdout");
        let stderr = child.stderr.take().expect("piped helper stderr");
        let stdout = std::thread::spawn(move || read_helper_output(stdout));
        let stderr = std::thread::spawn(move || read_helper_output(stderr));
        let started = Instant::now();
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break Ok(status),
                Ok(None) if started.elapsed() < self.timeout => {
                    std::thread::sleep(Duration::from_millis(5));
                }
                result => {
                    let _ = child.kill();
                    let _ = child.wait();
                    break Err(match result {
                        Err(error) => format!("failed to wait for imageprep helper: {error}"),
                        _ => format!(
                            "imageprep helper timed out after {} ms",
                            self.timeout.as_millis()
                        ),
                    });
                }
            }
        };
        let stdout = stdout
            .join()
            .map_err(|_| "imageprep stdout reader panicked".to_owned())?;
        let stderr = stderr
            .join()
            .map_err(|_| "imageprep stderr reader panicked".to_owned())?;
        // Every failed attempt removes partial pixels and any cache the helper
        // wrote before it failed. Successful cache output is adopted below.
        let status = status?;
        if !status.success() {
            let stderr = stderr?;
            return Err(format!(
                "imageprep helper failed for {} ({status}): {}",
                source_path.display(),
                String::from_utf8_lossy(&stderr).trim()
            ));
        }
        let stdout = stdout?;

        let prepared: ImagePrepOutput = serde_json::from_slice(&stdout)
            .map_err(|error| format!("failed to decode imageprep helper output: {error}"))?;
        if prepared.animated {
            return Ok(None);
        }

        let expected_len = (prepared.width as usize)
            .checked_mul(prepared.height as usize)
            .and_then(|pixels| pixels.checked_mul(4))
            .filter(|_| {
                prepared.width > 0
                    && prepared.height > 0
                    && prepared.source_width > 0
                    && prepared.source_height > 0
            })
            .ok_or_else(|| "imageprep helper returned invalid image dimensions".to_owned())?;
        let output_path = prepared.rgba_path.as_ref().unwrap_or(&rgba_path);
        let rgba = std::fs::read(output_path)
            .map_err(|error| format!("failed to read imageprep helper pixels: {error}"))?;
        let _ = std::fs::remove_file(output_path);
        if rgba.len() != expected_len {
            return Err(format!(
                "imageprep helper returned {} bytes for {}x{} RGBA image; expected {}",
                rgba.len(),
                prepared.width,
                prepared.height,
                expected_len
            ));
        }

        files.keep_cache = true;
        Ok(Some(PreparedImage {
            rgba,
            width: prepared.width,
            height: prepared.height,
            source_width: prepared.source_width,
            source_height: prepared.source_height,
            mime_type: prepared.mime_type,
            cache_path: prepared.cache_path,
            cache_extension: prepared.cache_extension,
        }))
    }
}

fn read_helper_output(mut pipe: impl Read) -> Result<Vec<u8>, String> {
    const MAX_OUTPUT_BYTES: u64 = 1024 * 1024;
    let mut bytes = Vec::new();
    pipe.by_ref()
        .take(MAX_OUTPUT_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("failed to read imageprep helper output: {error}"))?;
    std::io::copy(&mut pipe, &mut std::io::sink())
        .map_err(|error| format!("failed to drain imageprep helper output: {error}"))?;
    if bytes.len() as u64 > MAX_OUTPUT_BYTES {
        return Err("imageprep helper output exceeded 1 MiB".to_owned());
    }
    Ok(bytes)
}

struct HelperOutputFiles {
    rgba_path: PathBuf,
    keep_cache: bool,
}

impl Drop for HelperOutputFiles {
    fn drop(&mut self) {
        if self.keep_cache {
            let _ = std::fs::remove_file(&self.rgba_path);
        } else {
            cleanup_helper_files(&self.rgba_path);
        }
    }
}

fn cleanup_helper_files(rgba_path: &Path) {
    let _ = std::fs::remove_file(rgba_path);
    for encoding in super::cache_encoding::CanvasImageCacheEncoding::ALL {
        let _ = std::fs::remove_file(rgba_path.with_extension(encoding.extension()));
    }
}

pub(super) fn install_prepared_canvas_cache(
    library: &Library,
    asset_id: i64,
    max_side: u32,
    lod: CanvasImageLod,
    prepared: &PreparedImage,
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

fn unique_imageprep_rgba_path() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!(
        "snappyref-imageprep-{}-{nanos}.rgba",
        std::process::id()
    ))
}

pub(crate) fn print_imageprep_output_from_args() -> Result<(), String> {
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
    print_imageprep_output(
        &source_path,
        &rgba_output_path,
        CanvasImageSettings {
            max_side: canvas_max_side,
        },
    )
}

fn print_imageprep_output(
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
            &ImagePrepOutput {
                animated: true,
                rgba_path: None,
                width: 0,
                height: 0,
                source_width: decoded.source_width,
                source_height: decoded.source_height,
                mime_type: decoded.mime_type,
                cache_path: None,
                cache_extension: None,
            },
        )
        .map_err(|error| format!("failed to write imageprep helper output: {error}"))?;
        return Ok(());
    }

    let canvas_source = canvas_source_image_from_rgba(
        decoded.width,
        decoded.height,
        &decoded.rgba,
        canvas_image_settings,
    )?;
    std::fs::write(rgba_output_path, &canvas_source.rgba)
        .map_err(|error| format!("failed to write imageprep helper pixels: {error}"))?;
    let (cache_path, cache_extension) = write_prepared_canvas_cache(
        rgba_output_path,
        &canvas_source.rgba,
        canvas_source.width,
        canvas_source.height,
    )?;
    serde_json::to_writer(
        std::io::stdout(),
        &ImagePrepOutput {
            animated: false,
            rgba_path: Some(rgba_output_path.to_path_buf()),
            width: canvas_source.width,
            height: canvas_source.height,
            source_width: decoded.source_width,
            source_height: decoded.source_height,
            mime_type: decoded.mime_type,
            cache_path: Some(cache_path),
            cache_extension: Some(cache_extension.to_owned()),
        },
    )
    .map_err(|error| format!("failed to write imageprep helper output: {error}"))?;
    Ok(())
}

fn write_prepared_canvas_cache(
    rgba_output_path: &Path,
    rgba: &[u8],
    width: u32,
    height: u32,
) -> Result<(PathBuf, &'static str), String> {
    let encoding = super::cache_encoding::CanvasImageCacheEncoding::for_pixels(rgba);
    let extension = encoding.extension();
    let cache_path = rgba_output_path.with_extension(extension);
    encoding.write(&cache_path, rgba, width, height, CanvasImageLod::Working)?;
    Ok((cache_path, extension))
}
