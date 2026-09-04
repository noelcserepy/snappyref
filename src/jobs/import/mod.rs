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
pub(crate) use decode::print_imageprep_output_from_args;
use existing_load::backfill_cache_lod;
pub use existing_load::load_canvas_snapshot_pixels;
pub(crate) use existing_load::load_existing;
pub use persist::display_scale_for_placement_target_area;
pub(super) use persist::import;
#[cfg(test)]
pub(super) use persist::{import_path_sync, import_sync};
#[cfg(test)]
use preview_texture::build_preview_texture;
pub use preview_texture::{ImagePrepPreviewTexture, NativeTextureHandle};
pub(super) use request::ImagePrepPriority;
pub use request::{
    CanvasImageSettings, ExistingLoadDiagnostics, ExistingLoadPriority, ExistingLoadSourceKind,
    ImagePrepRequest, ImagePrepResult, ImagePrepTimings, ImagePrepTrace, ImagePrepWorkerStats,
    ImportOrigin, ImportRequest, ImportSource, PreparedCropPreview, PreparedPlacementImage,
};

pub const DEFAULT_PLACEMENT_TARGET_AREA: f32 = crate::data::DEFAULT_PLACEMENT_TARGET_AREA;
pub fn placement_target_reference_span() -> f32 {
    DEFAULT_PLACEMENT_TARGET_AREA.sqrt()
}

const INTERACTIVE_CROP_PREVIEW_MAX_SIDE: u32 = 1024;

mod worker;

pub use worker::ImagePrepWorker;
#[cfg(test)]
pub(super) use worker::{ImagePrepWorkerConfig, RequestQueue, WorkerKind};

pub fn sort_records_by_viewport_proximity(
    placements: &mut [Placement],
    viewport_world: egui::Rect,
) {
    placements.sort_by(|left, right| {
        placement_viewport_distance_sq(left, viewport_world)
            .total_cmp(&placement_viewport_distance_sq(right, viewport_world))
            .then_with(|| {
                left.state
                    .geometry
                    .z_index
                    .cmp(&right.state.geometry.z_index)
                    .then_with(|| left.state.id.cmp(&right.state.id))
            })
    });
}

fn placement_viewport_distance_sq(placement: &Placement, viewport_world: egui::Rect) -> f32 {
    let rect = placement_world_rect(placement);
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

fn placement_world_rect(placement: &Placement) -> egui::Rect {
    let size = egui::vec2(
        placement.require_loaded_asset().width as f32,
        placement.require_loaded_asset().height as f32,
    ) * placement.state.geometry.scale.max(0.01);
    egui::Rect::from_center_size(
        egui::pos2(
            placement.state.geometry.center.x,
            placement.state.geometry.center.y,
        ),
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
