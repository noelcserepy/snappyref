use std::path::Path;

use crate::data::Placement;
use crate::jobs::image_formats::{PreviewImage, preview_image_from_rgba};
use eframe::egui;

mod crop;
mod decode;
mod existing_load;
mod persist;
mod preview_texture;
mod request;

use crop::crop_item;
pub(crate) use decode::print_image_import_prepare_output_from_args;
use existing_load::backfill_cache_lod;
pub use existing_load::load_canvas_snapshot_pixels;
pub(crate) use existing_load::load_existing;
#[cfg(test)]
pub(super) use persist::import_path_sync;
#[cfg(test)]
pub(super) use persist::import_url_sync_traced;
pub(super) use persist::{import_clipboard, import_path, import_path_sync_traced, import_url};
pub use persist::{import_clipboard_sync, initial_import_scale};
#[cfg(test)]
use preview_texture::build_preview_texture;
pub use preview_texture::{ImportedPreviewTexture, NativeTextureHandle};
pub(super) use request::ImportPriority;
pub use request::{
    CanvasImageSettings, CroppedCanvasItem, ExistingLoadDiagnostics, ExistingLoadPriority,
    ExistingLoadSourceKind, FileImportPerfReport, ImportOrigin, ImportRequest, ImportResult,
    ImportTimings, ImportTrace, ImportWorkerStats, InteractiveImportPerfReport, LoadedCanvasItem,
    StartupPerfReport, StartupTraceEntry, WorkerInteractiveImportPerfReport,
};

pub const DEFAULT_IMPORT_TARGET_CANVAS_AREA: f32 = crate::data::DEFAULT_IMPORT_TARGET_CANVAS_AREA;
pub fn default_import_reference_span() -> f32 {
    DEFAULT_IMPORT_TARGET_CANVAS_AREA.sqrt()
}

const INTERACTIVE_CROP_PREVIEW_MAX_SIDE: u32 = 1024;

mod worker;

pub use worker::ImportWorker;
use worker::ImportWorkerConfig;
#[cfg(test)]
pub(super) use worker::{RequestQueue, WorkerKind};

pub fn sort_records_by_viewport_proximity(records: &mut [Placement], viewport_world: egui::Rect) {
    records.sort_by(|left, right| {
        record_viewport_distance_sq(left, viewport_world)
            .total_cmp(&record_viewport_distance_sq(right, viewport_world))
            .then_with(|| {
                left.state
                    .z_index
                    .cmp(&right.state.z_index)
                    .then_with(|| left.state.id.cmp(&right.state.id))
            })
    });
}

fn record_viewport_distance_sq(record: &Placement, viewport_world: egui::Rect) -> f32 {
    let rect = record_world_rect(record);
    if rect.intersects(viewport_world) {
        return 0.0;
    }
    let center = rect.center();
    let clamped = egui::pos2(
        center
            .x
            .clamp(viewport_world.left(), viewport_world.right()),
        center
            .y
            .clamp(viewport_world.top(), viewport_world.bottom()),
    );
    center.distance_sq(clamped)
}

fn record_intersects_viewport_world(record: &Placement, viewport_world: egui::Rect) -> bool {
    record_world_rect(record).intersects(viewport_world)
}

fn record_world_rect(record: &Placement) -> egui::Rect {
    let size = egui::vec2(
        record.asset.as_ref().expect("loaded placement asset").width as f32,
        record
            .asset
            .as_ref()
            .expect("loaded placement asset")
            .height as f32,
    ) * record.state.scale.max(0.01);
    egui::Rect::from_center_size(
        egui::pos2(record.state.center.x, record.state.center.y),
        size,
    )
}

pub(super) fn original_mime_type_supports_animation(mime_type: &str) -> bool {
    mime_type == image::ImageFormat::Gif.to_mime_type()
        || mime_type == image::ImageFormat::WebP.to_mime_type()
}

const SVG_MIME_TYPE: &str = "image/svg+xml";

pub(super) struct SvgDocument {
    pub(super) bytes: Vec<u8>,
    pub(super) width: u32,
    pub(super) height: u32,
}

fn is_svg_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("svg"))
}

pub(super) fn original_mime_type_is_svg(mime_type: &str) -> bool {
    mime_type
        .split(';')
        .next()
        .unwrap_or(mime_type)
        .trim()
        .eq_ignore_ascii_case(SVG_MIME_TYPE)
}

pub(super) fn read_svg_document(path: &Path) -> Result<SvgDocument, String> {
    let bytes = std::fs::read(path).map_err(|error| format!("failed to open SVG: {error}"))?;
    parse_svg_document(bytes)
}

pub(super) fn parse_svg_document(bytes: Vec<u8>) -> Result<SvgDocument, String> {
    let tree = usvg::Tree::from_data(&bytes, &usvg::Options::default())
        .map_err(|error| format!("failed to parse SVG: {error}"))?;
    let size = tree.size();
    let width = size.width().round().max(1.0) as u32;
    let height = size.height().round().max(1.0) as u32;
    Ok(SvgDocument {
        bytes,
        width,
        height,
    })
}

pub(super) fn stable_svg_uri_key(path: &Path) -> u64 {
    use std::hash::{DefaultHasher, Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    path.hash(&mut hasher);
    hasher.finish()
}

pub(super) fn canvas_source_image_from_rgba(
    width: u32,
    height: u32,
    rgba: &[u8],
    canvas_image_settings: CanvasImageSettings,
) -> Result<PreviewImage, String> {
    preview_image_from_rgba(width, height, rgba, canvas_image_settings.max_side as usize)
}

pub(super) fn animated_preview_max_side(
    canvas_image_settings: CanvasImageSettings,
    max_texture_side: usize,
) -> usize {
    (canvas_image_settings.max_side as usize).min(max_texture_side)
}

mod arrangement;

pub use arrangement::arrange_rect_world;
#[cfg(test)]
pub(super) use arrangement::rects_overlap_with_gap;
#[cfg(test)]
mod tests;

mod perf;

pub use perf::{
    file_import_perf_report, interactive_import_perf_report, startup_perf_report, startup_trace,
    worker_interactive_import_perf_report,
};
