use crate::placement::display_scale_for_placement_target_area;
use std::io::Read;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

#[cfg(test)]
use eframe::egui;

use crate::data::CanvasImageLod;
use crate::image_preparation::image_formats::{
    DecodedAnimation, PreviewImage, decode_image_bytes_with_max_side,
    decode_image_file_with_max_side, preview_animation_from_frames, preview_image_from_rgba,
};
use crate::library::storage::{Library, sha256_bytes, sha256_file};

use super::decode::{PreparedImage, install_prepared_canvas_cache, prepare_image_with_helper};
use super::preview_texture::build_preview_texture;
use super::{
    CanvasImageSettings, ImagePrepPreviewTexture, ImagePrepResult, ImportRequest, ImportSource,
    PreparedPlacementImage, SVG_MIME_TYPE, animated_preview_max_side,
    canvas_source_image_from_rgba, is_svg_path, original_mime_type_is_svg, parse_svg_document,
    read_svg_document, stable_svg_uri_key,
};
#[cfg(test)]
use crate::placement::DEFAULT_PLACEMENT_TARGET_AREA;

const URL_IMPORT_MAX_BYTES: u64 = 64 * 1024 * 1024;
const URL_IMPORT_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const URL_IMPORT_REQUEST_TIMEOUT: Duration = Duration::from_secs(45);

#[cfg(test)]
pub fn import_path_sync(
    library: &Library,
    source_path: PathBuf,
    desired_world: egui::Pos2,
) -> Result<PreparedPlacementImage, String> {
    let target_board_id = library.load_user_session()?.active_board_id;
    import_sync(
        library,
        super::tests::reserve_import_fixture(
            library,
            ImportRequest {
                target_board_id,
                queue_id: 0,
                placement_id: library.allocate_document_id(),
                source: ImportSource::Path(source_path),
                reserved_world: desired_world,
                max_texture_side: usize::MAX,
                canvas_image_settings: CanvasImageSettings::default(),
                placement_target_area: DEFAULT_PLACEMENT_TARGET_AREA,
                trace: None,
            },
        ),
        None,
    )
}

pub(crate) fn import(
    library: &Library,
    request: ImportRequest,
    wgpu_render_state: Option<&eframe::egui_wgpu::RenderState>,
) -> ImagePrepResult {
    let target_board_id = request.target_board_id;
    let queue_id = request.queue_id;
    let placement_id = request.placement_id;
    let context = request.source.label();
    match import_sync(library, request, wgpu_render_state) {
        Ok(loaded) => ImagePrepResult::Loaded(Box::new(loaded)),
        Err(error) => ImagePrepResult::Failed {
            target_board_id: Some(target_board_id),
            queue_id: Some(queue_id),
            placement_id: Some(placement_id),
            context,
            error,
        },
    }
}

pub(crate) fn import_sync(
    library: &Library,
    request: ImportRequest,
    wgpu_render_state: Option<&eframe::egui_wgpu::RenderState>,
) -> Result<PreparedPlacementImage, String> {
    let worker_started_at = Instant::now();
    let ImportRequest {
        target_board_id,
        queue_id,
        placement_id,
        source,
        reserved_world,
        max_texture_side,
        canvas_image_settings,
        placement_target_area,
        mut trace,
    } = request;
    if let Some(trace) = &mut trace {
        trace.timings.queue_wait = worker_started_at.saturating_duration_since(trace.queued_at);
    }
    let origin = source.origin();

    let decode_started_at = Instant::now();
    let decoded = decode_import_source(source, canvas_image_settings)?;
    if let Some(trace) = &mut trace {
        trace.timings.decode_or_load = decode_started_at.elapsed();
    }

    let initial_scale = display_scale_for_placement_target_area(
        decoded.source_width,
        decoded.source_height,
        placement_target_area,
    );

    let store_started_at = Instant::now();
    let stored = store_original(library, &decoded)?;
    let asset = library.register_original(
        &stored.path,
        &decoded.mime_type,
        decoded.source_width,
        decoded.source_height,
        stored.file_size,
        Some(&stored.sha256),
    )?;
    let placement = crate::data::Placement::loaded(
        crate::data::PlacementState::with_display_size(
            placement_id,
            reserved_world,
            eframe::egui::vec2(decoded.source_width as f32, decoded.source_height as f32),
            initial_scale,
            0.0,
            0,
            false,
        ),
        asset,
    );
    let store_and_persist = store_started_at.elapsed();

    let preview_started_at = Instant::now();
    let asset = placement.require_loaded_asset();
    let (preview, animated_preview, preview_texture) = match decoded.pixels {
        ImportPixels::Svg { bytes } => {
            let preview = PreviewImage {
                rgba: Vec::new(),
                width: decoded.source_width,
                height: decoded.source_height,
            };
            let texture = ImagePrepPreviewTexture::Svg {
                uri: format!(
                    "bytes://asset-svg-{}.svg",
                    stable_svg_uri_key(&asset.original_path)
                ),
                bytes: Arc::<[u8]>::from(bytes),
                size: [
                    decoded.source_width as usize,
                    decoded.source_height as usize,
                ],
            };
            (preview, None, texture)
        }
        ImportPixels::Prepared(prepared) => {
            install_prepared_canvas_cache(
                library,
                asset.id,
                canvas_image_settings.max_side,
                CanvasImageLod::Working,
                &prepared,
            );
            let preview = preview_image_from_rgba(
                prepared.width,
                prepared.height,
                &prepared.rgba,
                max_texture_side,
            )?;
            let texture = build_preview_texture(wgpu_render_state, &preview)?;
            (preview, None, texture)
        }
        ImportPixels::Raster {
            width,
            height,
            rgba,
            animation,
        } => {
            let canvas_source =
                canvas_source_image_from_rgba(width, height, &rgba, canvas_image_settings)?;
            let _ = library.store_canvas_image_cache(
                asset.id,
                canvas_image_settings.max_side,
                &canvas_source.rgba,
                canvas_source.width,
                canvas_source.height,
            );
            let preview = preview_image_from_rgba(
                canvas_source.width,
                canvas_source.height,
                &canvas_source.rgba,
                max_texture_side,
            )?;
            let animated_preview = animation
                .as_ref()
                .map(|animation| {
                    preview_animation_from_frames(
                        width,
                        height,
                        &animation.frames,
                        animated_preview_max_side(canvas_image_settings, max_texture_side),
                    )
                })
                .transpose()?;
            let texture = if animated_preview.is_some() {
                ImagePrepPreviewTexture::Cpu
            } else {
                build_preview_texture(wgpu_render_state, &preview)?
            };
            (preview, animated_preview, texture)
        }
    };
    if let Some(trace) = &mut trace {
        trace.timings.store_and_persist = store_and_persist;
        trace.timings.preview_prepare = preview_started_at.elapsed();
        trace.timings.worker_total = worker_started_at.elapsed();
    }

    Ok(PreparedPlacementImage {
        target_board_id,
        queue_id: Some(queue_id),
        placement,
        preview_rgba: preview.rgba,
        preview_width: preview.width,
        preview_height: preview.height,
        animated_preview,
        preview_texture,
        loaded_lod: CanvasImageLod::Working,
        existing_load_diagnostics: None,
        origin,
        trace,
    })
}

/// An import source decoded far enough to size, place, and store it.
struct DecodedImportSource {
    source_width: u32,
    source_height: u32,
    mime_type: String,
    original: OriginalBytes,
    pixels: ImportPixels,
}

/// How the original file reaches the library's originals folder.
enum OriginalBytes {
    CopyFile(PathBuf),
    Write {
        source_name: String,
        extension: String,
        bytes: Vec<u8>,
    },
    /// Encode the raster pixels as PNG; used for clipboard images.
    EncodePng,
}

enum ImportPixels {
    /// Helper output already at canvas size, plus a cache file to adopt.
    Prepared(PreparedImage),
    Raster {
        width: u32,
        height: u32,
        rgba: Vec<u8>,
        animation: Option<DecodedAnimation>,
    },
    Svg {
        bytes: Vec<u8>,
    },
}

fn decode_import_source(
    source: ImportSource,
    canvas_image_settings: CanvasImageSettings,
) -> Result<DecodedImportSource, String> {
    match source {
        ImportSource::Path(source_path) => {
            if is_svg_path(&source_path) {
                let svg = read_svg_document(&source_path)?;
                return Ok(DecodedImportSource {
                    source_width: svg.width,
                    source_height: svg.height,
                    mime_type: SVG_MIME_TYPE.to_owned(),
                    original: OriginalBytes::CopyFile(source_path),
                    pixels: ImportPixels::Svg { bytes: svg.bytes },
                });
            }
            if let Some(prepared) =
                prepare_image_with_helper(&source_path, canvas_image_settings).unwrap_or(None)
            {
                return Ok(DecodedImportSource {
                    source_width: prepared.source_width,
                    source_height: prepared.source_height,
                    mime_type: prepared.mime_type.clone(),
                    original: OriginalBytes::CopyFile(source_path),
                    pixels: ImportPixels::Prepared(prepared),
                });
            }
            let decoded =
                decode_image_file_with_max_side(&source_path, Some(canvas_image_settings.max_side))
                    .map_err(|error| format!("{}: {error}", source_path.display()))?;
            Ok(DecodedImportSource {
                source_width: decoded.source_width,
                source_height: decoded.source_height,
                mime_type: decoded.mime_type,
                original: OriginalBytes::CopyFile(source_path),
                pixels: ImportPixels::Raster {
                    width: decoded.width,
                    height: decoded.height,
                    rgba: decoded.rgba,
                    animation: decoded.animation,
                },
            })
        }
        ImportSource::Url(url) => {
            let downloaded = download_remote_image(&url)?;
            let source_name = remote_image_source_name(&downloaded.url);
            if downloaded_url_image_is_svg(&downloaded) {
                let svg = parse_svg_document(downloaded.bytes)
                    .map_err(|error| format!("{}: {error}", downloaded.url))?;
                return Ok(DecodedImportSource {
                    source_width: svg.width,
                    source_height: svg.height,
                    mime_type: SVG_MIME_TYPE.to_owned(),
                    original: OriginalBytes::Write {
                        source_name,
                        extension: "svg".to_owned(),
                        bytes: svg.bytes.clone(),
                    },
                    pixels: ImportPixels::Svg { bytes: svg.bytes },
                });
            }
            let decoded = decode_image_bytes_with_max_side(
                &downloaded.bytes,
                Some(downloaded.url.as_str().to_owned()),
                downloaded.mime_type.as_deref(),
                Some(canvas_image_settings.max_side),
            )?;
            Ok(DecodedImportSource {
                source_width: decoded.source_width,
                source_height: decoded.source_height,
                mime_type: decoded.mime_type,
                original: OriginalBytes::Write {
                    source_name,
                    extension: decoded.extension,
                    bytes: downloaded.bytes,
                },
                pixels: ImportPixels::Raster {
                    width: decoded.width,
                    height: decoded.height,
                    rgba: decoded.rgba,
                    animation: decoded.animation,
                },
            })
        }
        ImportSource::Clipboard {
            rgba,
            width,
            height,
        } => Ok(DecodedImportSource {
            source_width: width,
            source_height: height,
            mime_type: image::ImageFormat::Png.to_mime_type().to_owned(),
            original: OriginalBytes::EncodePng,
            pixels: ImportPixels::Raster {
                width,
                height,
                rgba,
                animation: None,
            },
        }),
    }
}

struct StoredOriginal {
    path: PathBuf,
    file_size: u64,
    sha256: String,
}

fn store_original(
    library: &Library,
    decoded: &DecodedImportSource,
) -> Result<StoredOriginal, String> {
    match &decoded.original {
        OriginalBytes::CopyFile(source_path) => {
            let path = library
                .store_original(source_path)
                .map_err(|error| format!("{}: {error}", source_path.display()))?;
            stored_file(path)
        }
        OriginalBytes::Write {
            source_name,
            extension,
            bytes,
        } => Ok(StoredOriginal {
            path: library.store_original_bytes(source_name, extension, bytes)?,
            file_size: bytes.len() as u64,
            sha256: sha256_bytes(bytes),
        }),
        OriginalBytes::EncodePng => {
            let ImportPixels::Raster {
                width,
                height,
                rgba,
                ..
            } = &decoded.pixels
            else {
                return Err("clipboard import requires raster pixels".to_owned());
            };
            stored_file(library.store_clipboard_image(rgba, *width, *height)?)
        }
    }
}

fn stored_file(path: PathBuf) -> Result<StoredOriginal, String> {
    let file_size = std::fs::metadata(&path)
        .map_err(|error| format!("failed to read stored original metadata: {error}"))?
        .len();
    let sha256 = sha256_file(&path)?;
    Ok(StoredOriginal {
        path,
        file_size,
        sha256,
    })
}

struct DownloadedUrlImage {
    url: url::Url,
    bytes: Vec<u8>,
    mime_type: Option<String>,
}

fn downloaded_url_image_is_svg(downloaded: &DownloadedUrlImage) -> bool {
    downloaded
        .mime_type
        .as_deref()
        .is_some_and(original_mime_type_is_svg)
        || downloaded
            .url
            .path_segments()
            .and_then(|mut segments| segments.next_back())
            .is_some_and(|name| {
                std::path::Path::new(name)
                    .extension()
                    .and_then(|extension| extension.to_str())
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("svg"))
            })
}

fn download_remote_image(url: &str) -> Result<DownloadedUrlImage, String> {
    download_remote_image_with_limits(
        url,
        URL_IMPORT_MAX_BYTES,
        URL_IMPORT_CONNECT_TIMEOUT,
        URL_IMPORT_REQUEST_TIMEOUT,
    )
}

fn download_remote_image_with_limits(
    url: &str,
    max_bytes: u64,
    connect_timeout: Duration,
    request_timeout: Duration,
) -> Result<DownloadedUrlImage, String> {
    let parsed = url::Url::parse(url).map_err(|error| format!("invalid image URL: {error}"))?;
    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        return Err("only http and https image URLs are supported".to_owned());
    }

    let client = reqwest::blocking::Client::builder()
        .connect_timeout(connect_timeout)
        .timeout(request_timeout)
        .build()
        .map_err(|error| format!("failed to build HTTP client: {error}"))?;
    let mut response = client
        .get(parsed.clone())
        .send()
        .and_then(reqwest::blocking::Response::error_for_status)
        .map_err(|error| format!("failed to download image: {error}"))?;

    let content_length = response.content_length();
    if let Some(content_length) = content_length
        && content_length > max_bytes
    {
        return Err(format!(
            "remote image is too large ({content_length} bytes > {max_bytes} byte limit)"
        ));
    }

    let mime_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);

    let mut bytes = Vec::new();
    let limit = max_bytes + 1;
    std::io::Read::take(&mut response, limit)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("failed to read downloaded image: {error}"))?;
    if bytes.len() as u64 > max_bytes {
        return Err(format!(
            "remote image exceeded the {} byte download limit",
            max_bytes
        ));
    }

    Ok(DownloadedUrlImage {
        url: parsed,
        bytes,
        mime_type,
    })
}

fn remote_image_source_name(url: &url::Url) -> String {
    let path_name = url
        .path_segments()
        .and_then(Iterator::last)
        .and_then(|segment| {
            segment
                .rsplit_once('.')
                .map(|(stem, _)| stem)
                .or(Some(segment))
        })
        .filter(|segment| !segment.is_empty())
        .map(str::to_owned);

    path_name.unwrap_or_else(|| {
        let host = url.host_str().unwrap_or("remote-image").replace('.', "-");
        format!("{host}-image")
    })
}

#[cfg(test)]
mod download_tests {
    use super::download_remote_image_with_limits;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::mpsc;
    use std::thread;
    use std::time::{Duration, Instant};

    fn serve_once(
        response: &'static [u8],
        hold_body: bool,
    ) -> (String, mpsc::Sender<()>, thread::JoinHandle<()>) {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
        let url = format!(
            "http://{}/reference.png?source=board",
            listener.local_addr().unwrap()
        );
        let (release_tx, release_rx) = mpsc::channel();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") {
                let mut byte = [0];
                stream.read_exact(&mut byte).unwrap();
                request.push(byte[0]);
            }
            stream.write_all(response).unwrap();
            if hold_body {
                let _ = release_rx.recv_timeout(Duration::from_secs(2));
            }
        });
        (url, release_tx, server)
    }

    #[test]
    fn url_download_returns_bytes_and_metadata_and_rejects_oversized_responses() {
        let (url, _, server) = serve_once(
            b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\nContent-Type: image/png\r\nConnection: close\r\n\r\nPNG!",
            false,
        );
        let downloaded = download_remote_image_with_limits(
            &url,
            4,
            Duration::from_secs(1),
            Duration::from_secs(1),
        )
        .unwrap();
        server.join().unwrap();
        assert_eq!(downloaded.bytes, b"PNG!");
        assert_eq!(downloaded.mime_type.as_deref(), Some("image/png"));
        assert_eq!(downloaded.url.as_str(), url);

        for (response, expected) in [
            (
                b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\n".as_slice(),
                "remote image is too large (5 bytes > 4 byte limit)",
            ),
            (
                b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n3\r\nPNG\r\n2\r\n!!\r\n0\r\n\r\n".as_slice(),
                "remote image exceeded the 4 byte download limit",
            ),
        ] {
            let (url, _, server) = serve_once(response, false);
            let error = download_remote_image_with_limits(
                &url, 4, Duration::from_secs(1), Duration::from_secs(1),
            ).err().expect("oversized response must fail");
            server.join().unwrap();
            assert_eq!(error, expected);
        }
    }

    #[test]
    fn url_download_times_out_while_waiting_for_the_response_body() {
        let (url, release, server) = serve_once(
            b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\n",
            true,
        );
        let started = Instant::now();
        let error = download_remote_image_with_limits(
            &url,
            4,
            Duration::from_secs(1),
            Duration::from_millis(250),
        )
        .err()
        .expect("stalled body must time out");
        let elapsed = started.elapsed();
        let _ = release.send(());
        server.join().unwrap();
        assert!(
            error.starts_with("failed to read downloaded image:"),
            "{error}"
        );
        assert!(elapsed < Duration::from_secs(2), "{elapsed:?}");
    }
}
