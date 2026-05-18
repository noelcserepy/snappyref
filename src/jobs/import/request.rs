use std::path::PathBuf;
use std::time::{Duration, Instant};

use eframe::egui;

use crate::data::{CanvasImageLod, Placement};

use super::preview_texture::ImportedPreviewTexture;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImportOrigin {
    Existing,
    Path,
    Clipboard,
    Url,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ImportPriority {
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
pub struct ImportTrace {
    pub queued_at: Instant,
    pub timings: ImportTimings,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ImportTimings {
    pub queue_wait: Duration,
    pub decode_or_load: Duration,
    pub store_and_persist: Duration,
    pub preview_prepare: Duration,
    pub worker_total: Duration,
    pub ui_integration: Duration,
}

#[derive(Clone, Copy, Debug)]
pub struct StartupPerfReport {
    pub legacy_first_visible_ms: f64,
    pub current_first_visible_ms: f64,
    pub warm_cached_first_visible_ms: f64,
}

#[derive(Clone, Copy, Debug)]
pub struct InteractiveImportPerfReport {
    pub legacy_interactive_import_under_backlog_ms: f64,
    pub current_interactive_import_under_backlog_ms: f64,
}

#[derive(Clone, Copy, Debug)]
pub struct WorkerInteractiveImportPerfReport {
    pub legacy_single_worker_ms: f64,
    pub current_pooled_ms: f64,
    pub current_interactive_workers: usize,
    pub current_background_workers: usize,
}

#[derive(Clone, Debug)]
pub struct FileImportPerfReport {
    pub source_path: PathBuf,
    pub source_file_size: u64,
    pub source_width: u32,
    pub source_height: u32,
    pub preview_width: u32,
    pub preview_height: u32,
    pub canvas_cache_path: Option<PathBuf>,
    pub canvas_cache_file_size: Option<u64>,
    pub timings: ImportTimings,
    pub total_ms: f64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ImportWorkerStats {
    pub queued_startup: usize,
    pub queued_interactive: usize,
    pub queued_background: usize,
    pub in_flight: usize,
}

impl ImportWorkerStats {
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

#[derive(Clone, Debug)]
pub enum ImportRequest {
    ImportPath {
        target_board_id: i64,
        queue_id: u64,
        placement_id: Option<i64>,
        source_path: PathBuf,
        desired_world: egui::Pos2,
        reserved_world: Option<egui::Pos2>,
        viewport_world: egui::Rect,
        max_texture_side: usize,
        canvas_image_settings: CanvasImageSettings,
        auto_scale: bool,
        target_canvas_area: f32,
        import_gap: f32,
        trace: Option<ImportTrace>,
    },
    ImportClipboard {
        target_board_id: i64,
        queue_id: u64,
        placement_id: Option<i64>,
        rgba: Vec<u8>,
        width: u32,
        height: u32,
        desired_world: egui::Pos2,
        reserved_world: Option<egui::Pos2>,
        viewport_world: egui::Rect,
        max_texture_side: usize,
        canvas_image_settings: CanvasImageSettings,
        auto_scale: bool,
        target_canvas_area: f32,
        import_gap: f32,
        trace: Option<ImportTrace>,
    },
    ImportUrl {
        target_board_id: i64,
        queue_id: u64,
        placement_id: Option<i64>,
        url: String,
        desired_world: egui::Pos2,
        reserved_world: Option<egui::Pos2>,
        viewport_world: egui::Rect,
        max_texture_side: usize,
        canvas_image_settings: CanvasImageSettings,
        auto_scale: bool,
        target_canvas_area: f32,
        import_gap: f32,
        trace: Option<ImportTrace>,
    },
    Crop {
        request: crate::canvas::CanvasCropRequest,
        max_texture_side: usize,
        canvas_image_settings: CanvasImageSettings,
    },
    LoadExisting {
        target_board_id: i64,
        queue_id: Option<u64>,
        record: Placement,
        max_texture_side: usize,
        canvas_image_settings: CanvasImageSettings,
        target_lod: CanvasImageLod,
        priority: ExistingLoadPriority,
        trace: Option<ImportTrace>,
    },
    BackfillLod {
        record: Placement,
        canvas_image_settings: CanvasImageSettings,
        lod: CanvasImageLod,
    },
}

#[derive(Debug)]
pub enum ImportResult {
    Loaded(LoadedCanvasItem),
    Cropped(CroppedCanvasItem),
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
        item_id: Option<i64>,
        context: String,
        error: String,
    },
}

#[derive(Debug)]
pub struct LoadedCanvasItem {
    pub target_board_id: i64,
    pub queue_id: Option<u64>,
    pub record: Placement,
    pub preview_rgba: Vec<u8>,
    pub preview_width: u32,
    pub preview_height: u32,
    pub animated_preview: Option<crate::jobs::image_formats::AnimatedPreview>,
    pub preview_texture: ImportedPreviewTexture,
    pub loaded_lod: CanvasImageLod,
    pub existing_load_diagnostics: Option<ExistingLoadDiagnostics>,
    pub origin: ImportOrigin,
    pub trace: Option<ImportTrace>,
}

#[derive(Clone, Debug)]
pub struct CroppedCanvasItem {
    pub item_id: i64,
    pub stored_path: PathBuf,
    pub width: u32,
    pub height: u32,
    pub file_size: u64,
    pub preview_rgba: Vec<u8>,
    pub preview_width: u32,
    pub preview_height: u32,
    pub preview_texture: ImportedPreviewTexture,
    pub loaded_lod: CanvasImageLod,
    pub crop_offset_local: egui::Vec2,
}

#[derive(Clone, Debug)]
pub struct StartupTraceEntry {
    pub record: Placement,
    pub diagnostics: ExistingLoadDiagnostics,
    pub timings: ImportTimings,
    pub preview_width: u32,
    pub preview_height: u32,
    pub startup_elapsed: Duration,
}
