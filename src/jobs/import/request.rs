use std::path::PathBuf;
use std::time::{Duration, Instant};

use eframe::egui;

use crate::data::{CanvasImageLod, Placement};

use super::preview_texture::ImagePrepPreviewTexture;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImportOrigin {
    Existing,
    Path,
    Clipboard,
    Url,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ImagePrepPriority {
    Startup,
    Interactive,
    Background,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExistingLoadPriority {
    Startup,
    Background,
}

#[derive(Clone, Copy, Debug)]
pub struct ImagePrepTrace {
    pub queued_at: Instant,
    pub timings: ImagePrepTimings,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ImagePrepTimings {
    pub queue_wait: Duration,
    pub decode_or_load: Duration,
    pub store_and_persist: Duration,
    pub preview_prepare: Duration,
    pub worker_total: Duration,
    pub ui_integration: Duration,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ImagePrepWorkerStats {
    pub queued_startup: usize,
    pub queued_interactive: usize,
    pub queued_background: usize,
    pub in_flight: usize,
}

impl ImagePrepWorkerStats {
    pub fn has_work(self) -> bool {
        self.queued_startup > 0
            || self.queued_interactive > 0
            || self.queued_background > 0
            || self.in_flight > 0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CanvasImageSettings {
    pub max_side: u32,
}

impl Default for CanvasImageSettings {
    fn default() -> Self {
        Self {
            max_side: crate::data::DEFAULT_CANVAS_IMAGE_MAX_SIDE,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExistingLoadSourceKind {
    Original,
    Cache,
}

#[derive(Clone, Debug)]
pub struct ExistingLoadDiagnostics {
    pub source_kind: ExistingLoadSourceKind,
    pub source_path: PathBuf,
    pub source_file_size: u64,
    pub source_width: u32,
    pub source_height: u32,
    pub backfilled_cache: bool,
}

/// Where an import's pixels come from. Everything else about an import is
/// shared, so the worker decodes the source and then runs one common tail.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ImportSource {
    Path(PathBuf),
    Url(String),
    Clipboard {
        rgba: Vec<u8>,
        width: u32,
        height: u32,
    },
}

impl ImportSource {
    pub fn origin(&self) -> ImportOrigin {
        match self {
            Self::Path(_) => ImportOrigin::Path,
            Self::Url(_) => ImportOrigin::Url,
            Self::Clipboard { .. } => ImportOrigin::Clipboard,
        }
    }

    /// Human-readable name used in status messages and failure context.
    pub fn label(&self) -> String {
        match self {
            Self::Path(path) => path.display().to_string(),
            Self::Url(url) => url.clone(),
            Self::Clipboard { .. } => "clipboard".to_owned(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct ImportRequest {
    pub target_board_id: i64,
    pub queue_id: u64,
    pub placement_id: Option<i64>,
    pub source: ImportSource,
    pub desired_world: egui::Pos2,
    pub reserved_world: Option<egui::Pos2>,
    pub viewport_world: egui::Rect,
    pub max_texture_side: usize,
    pub canvas_image_settings: CanvasImageSettings,
    pub auto_scale: bool,
    pub placement_target_area: f32,
    pub placement_gap: f32,
    pub trace: Option<ImagePrepTrace>,
}

#[derive(Clone, Debug)]
pub enum ImagePrepRequest {
    Import(ImportRequest),
    Crop {
        request: crate::canvas::CanvasCropRequest,
        max_texture_side: usize,
        canvas_image_settings: CanvasImageSettings,
    },
    LoadExisting {
        target_board_id: i64,
        queue_id: Option<u64>,
        placement: Placement,
        max_texture_side: usize,
        canvas_image_settings: CanvasImageSettings,
        target_lod: CanvasImageLod,
        priority: ExistingLoadPriority,
        trace: Option<ImagePrepTrace>,
    },
    BackfillLod {
        placement: Placement,
        canvas_image_settings: CanvasImageSettings,
        lod: CanvasImageLod,
    },
}

#[derive(Debug)]
pub enum ImagePrepResult {
    Loaded(PreparedPlacementImage),
    Cropped(PreparedCropPreview),
    CacheLodBackfilled {
        asset_id: i64,
        lod: CanvasImageLod,
    },
    CacheLodBackfillFailed {
        asset_id: i64,
        lod: CanvasImageLod,
        error: String,
    },
    Failed {
        target_board_id: Option<i64>,
        queue_id: Option<u64>,
        placement_id: Option<i64>,
        context: String,
        error: String,
    },
}

#[derive(Debug)]
pub struct PreparedPlacementImage {
    pub target_board_id: i64,
    pub queue_id: Option<u64>,
    pub placement: Placement,
    pub preview_rgba: Vec<u8>,
    pub preview_width: u32,
    pub preview_height: u32,
    pub animated_preview: Option<crate::jobs::image_formats::AnimatedPreview>,
    pub preview_texture: ImagePrepPreviewTexture,
    pub loaded_lod: CanvasImageLod,
    pub existing_load_diagnostics: Option<ExistingLoadDiagnostics>,
    pub origin: ImportOrigin,
    pub trace: Option<ImagePrepTrace>,
}

#[derive(Clone, Debug)]
pub struct PreparedCropPreview {
    pub placement_id: i64,
    pub stored_path: PathBuf,
    pub width: u32,
    pub height: u32,
    pub file_size: u64,
    pub preview_rgba: Vec<u8>,
    pub preview_width: u32,
    pub preview_height: u32,
    pub preview_texture: ImagePrepPreviewTexture,
    pub loaded_lod: CanvasImageLod,
    pub crop_offset_local: egui::Vec2,
}
