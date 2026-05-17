use std::collections::VecDeque;
use std::fmt;
use std::io::Read;
use std::path::{Path, PathBuf};
#[cfg(not(test))]
use std::process::Command;
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use eframe::egui;
use image::RgbaImage;
use image::codecs::jpeg::JpegEncoder;
use image::codecs::webp::WebPEncoder;
use image::{ExtendedColorType, ImageEncoder};

use crate::data::{BoardItemWithAsset, BoardLayoutItem, CanvasImageLod};
use crate::jobs::image_formats::{
    AnimatedPreview, PreviewImage, decode_image_bytes_with_max_side, decode_image_file,
    decode_image_file_with_max_side, preview_animation_from_frames, preview_image_from_rgba,
};
use crate::library::storage::Library;

pub const DEFAULT_IMPORT_TARGET_CANVAS_AREA: f32 = crate::data::DEFAULT_IMPORT_TARGET_CANVAS_AREA;
const URL_IMPORT_MAX_BYTES: u64 = 64 * 1024 * 1024;
const URL_IMPORT_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const URL_IMPORT_REQUEST_TIMEOUT: Duration = Duration::from_secs(45);
#[cfg(not(test))]
const IMAGE_IMPORT_HELPER_DISABLED_ENV: &str = "SNAPPYREF_DISABLE_IMAGE_IMPORT_HELPER";

pub fn default_import_reference_span() -> f32 {
    DEFAULT_IMPORT_TARGET_CANVAS_AREA.sqrt()
}

const PERF_STARTUP_IMAGE_SIZE: u32 = 1536;
const PERF_BACKLOG_IMAGE_SIZE: u32 = 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImportOrigin {
    Existing,
    Path,
    Clipboard,
    Url,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ImportPriority {
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

#[derive(Clone, Debug)]
struct PreparedImportImage {
    rgba: Vec<u8>,
    width: u32,
    height: u32,
    source_width: u32,
    source_height: u32,
    mime_type: String,
    file_size: u64,
    cache_path: Option<PathBuf>,
    cache_extension: Option<String>,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
struct ImageImportPrepareOutput {
    animated: bool,
    rgba_path: Option<PathBuf>,
    width: u32,
    height: u32,
    source_width: u32,
    source_height: u32,
    mime_type: String,
    file_size: u64,
    cache_path: Option<PathBuf>,
    cache_extension: Option<String>,
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

const OVERVIEW_CACHE_MAX_SIDE: u32 = 256;
const INTERACTIVE_CROP_PREVIEW_MAX_SIDE: u32 = 1024;
const DETAIL_CACHE_MAX_SIDE: u32 = 4096;

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
        record: BoardItemWithAsset,
        max_texture_side: usize,
        canvas_image_settings: CanvasImageSettings,
        target_lod: CanvasImageLod,
        priority: ExistingLoadPriority,
        trace: Option<ImportTrace>,
    },
    BackfillLod {
        record: BoardItemWithAsset,
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

#[derive(Clone)]
pub enum ImportedPreviewTexture {
    Cpu,
    Native(Arc<NativeTextureHandle>),
    Svg {
        uri: String,
        bytes: Arc<[u8]>,
        size: [usize; 2],
    },
}

impl fmt::Debug for ImportedPreviewTexture {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cpu => f.write_str("Cpu"),
            Self::Native(handle) => f.debug_tuple("Native").field(handle).finish(),
            Self::Svg { uri, size, .. } => f
                .debug_struct("Svg")
                .field("uri", uri)
                .field("size", size)
                .finish(),
        }
    }
}

pub struct NativeTextureHandle {
    texture_id: egui::TextureId,
    size: [usize; 2],
    cleanup: Arc<dyn Fn(egui::TextureId) + Send + Sync>,
    _texture: Option<eframe::wgpu::Texture>,
    view: Option<Arc<eframe::wgpu::TextureView>>,
}

impl fmt::Debug for NativeTextureHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NativeTextureHandle")
            .field("texture_id", &self.texture_id)
            .field("size", &self.size)
            .finish()
    }
}

impl NativeTextureHandle {
    fn from_registered_texture(
        render_state: eframe::egui_wgpu::RenderState,
        texture_id: egui::TextureId,
        width: u32,
        height: u32,
        texture: eframe::wgpu::Texture,
        view: eframe::wgpu::TextureView,
    ) -> Self {
        let cleanup = Arc::new(move |texture_id| {
            render_state.renderer.write().free_texture(&texture_id);
        });
        Self {
            texture_id,
            size: [width as usize, height as usize],
            cleanup,
            _texture: Some(texture),
            view: Some(Arc::new(view)),
        }
    }

    pub fn texture_id(&self) -> egui::TextureId {
        self.texture_id
    }

    pub fn size(&self) -> [usize; 2] {
        self.size
    }

    pub fn create_bind_group(
        &self,
        device: &eframe::wgpu::Device,
        layout: &eframe::wgpu::BindGroupLayout,
        sampler: &eframe::wgpu::Sampler,
    ) -> Option<eframe::wgpu::BindGroup> {
        let view = self.view.as_ref()?;
        Some(
            device.create_bind_group(&eframe::wgpu::BindGroupDescriptor {
                label: Some("snappyref_canvas_image_bind_group"),
                layout,
                entries: &[
                    eframe::wgpu::BindGroupEntry {
                        binding: 0,
                        resource: eframe::wgpu::BindingResource::TextureView(view.as_ref()),
                    },
                    eframe::wgpu::BindGroupEntry {
                        binding: 1,
                        resource: eframe::wgpu::BindingResource::Sampler(sampler),
                    },
                ],
            }),
        )
    }

    #[cfg(test)]
    pub(crate) fn for_tests(
        texture_id: egui::TextureId,
        size: [usize; 2],
        cleanup: impl Fn(egui::TextureId) + Send + Sync + 'static,
    ) -> Arc<Self> {
        Arc::new(Self {
            texture_id,
            size,
            cleanup: Arc::new(cleanup),
            _texture: None,
            view: None,
        })
    }
}

impl Drop for NativeTextureHandle {
    fn drop(&mut self) {
        (self.cleanup)(self.texture_id);
    }
}

#[derive(Debug)]
pub struct LoadedCanvasItem {
    pub target_board_id: i64,
    pub queue_id: Option<u64>,
    pub record: BoardItemWithAsset,
    pub preview_rgba: Vec<u8>,
    pub preview_width: u32,
    pub preview_height: u32,
    pub animated_preview: Option<AnimatedPreview>,
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
    pub record: BoardItemWithAsset,
    pub diagnostics: ExistingLoadDiagnostics,
    pub timings: ImportTimings,
    pub preview_width: u32,
    pub preview_height: u32,
    pub startup_elapsed: Duration,
}

pub struct ImportWorker {
    queue: Arc<RequestQueue>,
    receiver: Receiver<ImportResult>,
    workers: Vec<JoinHandle<()>>,
}

struct RequestQueue {
    state: Mutex<RequestQueueState>,
    ready: Condvar,
}

#[derive(Default)]
struct RequestQueueState {
    startup: VecDeque<ImportRequest>,
    interactive: VecDeque<ImportRequest>,
    background: VecDeque<ImportRequest>,
    in_flight: usize,
    shutting_down: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WorkerKind {
    General,
    Interactive,
    Background,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ImportWorkerConfig {
    general_workers: usize,
    interactive_workers: usize,
    background_workers: usize,
}

impl ImportWorker {
    pub fn new(
        library: Library,
        ctx: egui::Context,
        wgpu_render_state: Option<eframe::egui_wgpu::RenderState>,
    ) -> Self {
        let parallelism = thread::available_parallelism()
            .map(|available| available.get())
            .unwrap_or(1);
        Self::new_with_config(
            library,
            ctx,
            wgpu_render_state,
            ImportWorkerConfig::for_parallelism(parallelism),
        )
    }

    fn new_with_config(
        library: Library,
        ctx: egui::Context,
        wgpu_render_state: Option<eframe::egui_wgpu::RenderState>,
        config: ImportWorkerConfig,
    ) -> Self {
        let (result_tx, result_rx) = mpsc::channel::<ImportResult>();
        let queue = Arc::new(RequestQueue::default());
        let mut workers = Vec::with_capacity(config.total_workers());

        for kind in config.worker_kinds() {
            let worker_queue = queue.clone();
            let worker_library = library.clone();
            let worker_ctx = ctx.clone();
            let worker_render_state = wgpu_render_state.clone();
            let worker_result_tx = result_tx.clone();
            workers.push(thread::spawn(move || {
                while let Some(request) = worker_queue.pop(kind) {
                    let result = match request {
                        ImportRequest::ImportPath {
                            target_board_id,
                            queue_id,
                            placement_id,
                            source_path,
                            desired_world,
                            reserved_world,
                            viewport_world,
                            max_texture_side,
                            canvas_image_settings,
                            auto_scale,
                            target_canvas_area,
                            import_gap,
                            trace,
                        } => import_path(
                            &worker_library,
                            target_board_id,
                            queue_id,
                            placement_id,
                            source_path,
                            desired_world,
                            reserved_world,
                            viewport_world,
                            max_texture_side,
                            canvas_image_settings,
                            auto_scale,
                            target_canvas_area,
                            import_gap,
                            trace,
                            worker_render_state.as_ref(),
                        ),
                        ImportRequest::ImportClipboard {
                            target_board_id,
                            queue_id,
                            placement_id,
                            rgba,
                            width,
                            height,
                            desired_world,
                            reserved_world,
                            viewport_world,
                            max_texture_side,
                            canvas_image_settings,
                            auto_scale,
                            target_canvas_area,
                            import_gap,
                            trace,
                        } => import_clipboard(
                            &worker_library,
                            target_board_id,
                            queue_id,
                            placement_id,
                            rgba,
                            width,
                            height,
                            desired_world,
                            reserved_world,
                            viewport_world,
                            max_texture_side,
                            canvas_image_settings,
                            auto_scale,
                            target_canvas_area,
                            import_gap,
                            trace,
                            worker_render_state.as_ref(),
                        ),
                        ImportRequest::ImportUrl {
                            target_board_id,
                            queue_id,
                            placement_id,
                            url,
                            desired_world,
                            reserved_world,
                            viewport_world,
                            max_texture_side,
                            canvas_image_settings,
                            auto_scale,
                            target_canvas_area,
                            import_gap,
                            trace,
                        } => import_url(
                            &worker_library,
                            target_board_id,
                            queue_id,
                            placement_id,
                            url,
                            desired_world,
                            reserved_world,
                            viewport_world,
                            max_texture_side,
                            canvas_image_settings,
                            auto_scale,
                            target_canvas_area,
                            import_gap,
                            trace,
                            worker_render_state.as_ref(),
                        ),
                        ImportRequest::Crop {
                            request,
                            max_texture_side,
                            canvas_image_settings,
                        } => crop_item(
                            &worker_library,
                            request,
                            max_texture_side,
                            canvas_image_settings,
                            worker_render_state.as_ref(),
                        ),
                        ImportRequest::LoadExisting {
                            target_board_id,
                            queue_id,
                            record,
                            max_texture_side,
                            canvas_image_settings,
                            target_lod,
                            priority: _,
                            trace,
                        } => load_existing(
                            &worker_library,
                            target_board_id,
                            queue_id,
                            record,
                            max_texture_side,
                            canvas_image_settings,
                            target_lod,
                            trace,
                            worker_render_state.as_ref(),
                        ),
                        ImportRequest::BackfillLod {
                            record,
                            canvas_image_settings,
                            lod,
                        } => {
                            backfill_cache_lod(&worker_library, record, canvas_image_settings, lod)
                        }
                    };

                    worker_queue.finish_one();
                    if worker_result_tx.send(result).is_ok() {
                        worker_ctx.request_repaint();
                    }
                }
            }));
        }
        drop(result_tx);

        Self {
            queue,
            receiver: result_rx,
            workers,
        }
    }

    pub fn enqueue(&self, request: ImportRequest) -> Result<(), String> {
        self.queue.push(request)
    }

    pub fn try_recv_up_to(&self, limit: usize) -> Vec<ImportResult> {
        self.receiver.try_iter().take(limit).collect()
    }

    pub fn stats(&self) -> ImportWorkerStats {
        self.queue.stats()
    }
}

impl Default for RequestQueue {
    fn default() -> Self {
        Self {
            state: Mutex::new(RequestQueueState::default()),
            ready: Condvar::new(),
        }
    }
}

impl RequestQueue {
    fn push(&self, request: ImportRequest) -> Result<(), String> {
        let mut state = self.state.lock().expect("request queue poisoned");
        if state.shutting_down {
            return Err("import worker is shutting down".to_owned());
        }
        match request.priority() {
            ImportPriority::Startup => state.startup.push_back(request),
            ImportPriority::Interactive => {
                insert_interactive_sorted_by_bytes(&mut state.interactive, request);
            }
            ImportPriority::Background => state.background.push_back(request),
        }
        self.ready.notify_all();
        Ok(())
    }

    #[cfg(test)]
    fn try_pop_one(&self, kind: WorkerKind) -> Option<ImportRequest> {
        let mut state = self.state.lock().expect("request queue poisoned");
        match kind {
            WorkerKind::General | WorkerKind::Interactive => state
                .interactive
                .pop_front()
                .or_else(|| state.startup.pop_front())
                .or_else(|| {
                    if matches!(kind, WorkerKind::General) {
                        state.background.pop_front()
                    } else {
                        None
                    }
                }),
            WorkerKind::Background => state
                .startup
                .pop_front()
                .or_else(|| state.background.pop_front()),
        }
    }

    fn pop(&self, kind: WorkerKind) -> Option<ImportRequest> {
        let mut state = self.state.lock().expect("request queue poisoned");
        loop {
            if state.shutting_down {
                return None;
            }
            match kind {
                WorkerKind::General => {
                    if let Some(request) = state.interactive.pop_front() {
                        state.in_flight += 1;
                        return Some(request);
                    }
                    if let Some(request) = state.startup.pop_front() {
                        state.in_flight += 1;
                        return Some(request);
                    }
                    if let Some(request) = state.background.pop_front() {
                        state.in_flight += 1;
                        return Some(request);
                    }
                }
                WorkerKind::Interactive => {
                    if let Some(request) = state.interactive.pop_front() {
                        state.in_flight += 1;
                        return Some(request);
                    }
                    if let Some(request) = state.startup.pop_front() {
                        state.in_flight += 1;
                        return Some(request);
                    }
                }
                WorkerKind::Background => {
                    if let Some(request) = state.startup.pop_front() {
                        state.in_flight += 1;
                        return Some(request);
                    }
                    if let Some(request) = state.background.pop_front() {
                        state.in_flight += 1;
                        return Some(request);
                    }
                }
            }
            state = self.ready.wait(state).expect("request queue poisoned");
        }
    }

    fn shutdown(&self) {
        let mut state = self.state.lock().expect("request queue poisoned");
        state.shutting_down = true;
        self.ready.notify_all();
    }

    fn finish_one(&self) {
        let mut state = self.state.lock().expect("request queue poisoned");
        state.in_flight = state.in_flight.saturating_sub(1);
    }

    fn stats(&self) -> ImportWorkerStats {
        let state = self.state.lock().expect("request queue poisoned");
        ImportWorkerStats {
            queued_startup: state.startup.len(),
            queued_interactive: state.interactive.len(),
            queued_background: state.background.len(),
            in_flight: state.in_flight,
        }
    }
}

impl ImportWorkerConfig {
    fn for_parallelism(parallelism: usize) -> Self {
        if parallelism <= 1 {
            return Self::legacy_single_worker();
        }

        Self {
            general_workers: 0,
            interactive_workers: parallelism.saturating_sub(1).clamp(1, 4),
            background_workers: 1,
        }
    }

    fn legacy_single_worker() -> Self {
        Self {
            general_workers: 1,
            interactive_workers: 0,
            background_workers: 0,
        }
    }

    fn total_workers(self) -> usize {
        self.general_workers + self.interactive_workers + self.background_workers
    }

    fn worker_kinds(self) -> Vec<WorkerKind> {
        let mut kinds = Vec::with_capacity(self.total_workers());
        kinds.extend(std::iter::repeat_n(
            WorkerKind::General,
            self.general_workers,
        ));
        kinds.extend(std::iter::repeat_n(
            WorkerKind::Interactive,
            self.interactive_workers,
        ));
        kinds.extend(std::iter::repeat_n(
            WorkerKind::Background,
            self.background_workers,
        ));
        kinds
    }
}

impl Drop for ImportWorker {
    fn drop(&mut self) {
        self.queue.shutdown();
        while let Some(worker) = self.workers.pop() {
            let _ = worker.join();
        }
    }
}

impl ImportRequest {
    fn priority(&self) -> ImportPriority {
        match self {
            ImportRequest::LoadExisting { priority, .. } => match priority {
                ExistingLoadPriority::Startup => ImportPriority::Startup,
                ExistingLoadPriority::Background => ImportPriority::Background,
            },
            ImportRequest::BackfillLod { .. } => ImportPriority::Background,
            ImportRequest::ImportPath { .. }
            | ImportRequest::ImportClipboard { .. }
            | ImportRequest::ImportUrl { .. }
            | ImportRequest::Crop { .. } => ImportPriority::Interactive,
        }
    }

    /// File-size hint used to prioritize **smaller imports first** in a mixed
    /// batch so a single large asset cannot block many small assets from
    /// reaching **Placement Visible**. Returns `None` for variants whose size
    /// is unknown at enqueue time (URL downloads) or which do not participate
    /// in mixed-batch import ordering (`Crop`, `LoadExisting`, `BackfillLod`).
    fn priority_bytes(&self) -> Option<u64> {
        match self {
            ImportRequest::ImportPath { source_path, .. } => std::fs::metadata(source_path)
                .ok()
                .map(|metadata| metadata.len()),
            ImportRequest::ImportClipboard { rgba, .. } => Some(rgba.len() as u64),
            ImportRequest::ImportUrl { .. }
            | ImportRequest::Crop { .. }
            | ImportRequest::LoadExisting { .. }
            | ImportRequest::BackfillLod { .. } => None,
        }
    }
}

/// Insert `request` into the **Interactive** queue so requests with smaller
/// `priority_bytes` pop first, falling back to FIFO arrival order when sizes
/// tie or are unknown. Stable across equal keys: ties keep their relative
/// arrival order, so canvas-placement reservations made at enqueue time stay
/// laid out as the user dropped them.
fn insert_interactive_sorted_by_bytes(
    interactive: &mut VecDeque<ImportRequest>,
    request: ImportRequest,
) {
    // `None` sorts to the back so we never starve known-small assets when a
    // size cannot be measured (e.g. a URL import whose payload size is not
    // known until the download completes).
    let incoming = request.priority_bytes().unwrap_or(u64::MAX);
    // Linear scan from the back is appropriate: a typical mixed-batch
    // import is well under 100 items so a stable in-place insert is faster
    // and simpler than a binary heap, and it preserves FIFO on ties.
    let mut insert_at = interactive.len();
    while insert_at > 0 {
        let existing = interactive[insert_at - 1]
            .priority_bytes()
            .unwrap_or(u64::MAX);
        if existing <= incoming {
            break;
        }
        insert_at -= 1;
    }
    interactive.insert(insert_at, request);
}

#[allow(dead_code)]
pub fn import_path_sync(
    library: &Library,
    source_path: PathBuf,
    desired_world: egui::Pos2,
    viewport_world: egui::Rect,
    auto_scale: bool,
    import_gap: f32,
) -> Result<LoadedCanvasItem, String> {
    let target_board_id = library.load_user_session()?.active_board_id;
    import_path_sync_traced(
        library,
        target_board_id,
        None,
        None,
        source_path,
        desired_world,
        None,
        viewport_world,
        usize::MAX,
        CanvasImageSettings::default(),
        auto_scale,
        DEFAULT_IMPORT_TARGET_CANVAS_AREA,
        import_gap,
        None,
        None,
    )
}

pub fn import_clipboard_sync(
    library: &Library,
    rgba: Vec<u8>,
    width: u32,
    height: u32,
    desired_world: egui::Pos2,
    viewport_world: egui::Rect,
    auto_scale: bool,
    import_gap: f32,
) -> Result<LoadedCanvasItem, String> {
    let target_board_id = library.load_user_session()?.active_board_id;
    import_clipboard_sync_traced(
        library,
        target_board_id,
        0,
        None,
        rgba,
        width,
        height,
        desired_world,
        None,
        viewport_world,
        usize::MAX,
        CanvasImageSettings::default(),
        auto_scale,
        DEFAULT_IMPORT_TARGET_CANVAS_AREA,
        import_gap,
        None,
        None,
    )
}

pub fn sort_records_by_viewport_proximity(
    records: &mut [BoardItemWithAsset],
    viewport_world: egui::Rect,
) {
    records.sort_by(|left, right| {
        record_viewport_distance_sq(left, viewport_world)
            .total_cmp(&record_viewport_distance_sq(right, viewport_world))
            .then_with(|| {
                left.item
                    .z_index
                    .cmp(&right.item.z_index)
                    .then_with(|| left.item.id.cmp(&right.item.id))
            })
    });
}

pub fn startup_perf_report() -> Result<StartupPerfReport, String> {
    let legacy_root = temp_library_root("startup-perf-legacy");
    let current_root = temp_library_root("startup-perf-current");
    let legacy_library = Library::initialize(legacy_root.clone())?;
    let current_library = Library::initialize(current_root.clone())?;
    let viewport_world =
        egui::Rect::from_min_max(egui::pos2(-240.0, -180.0), egui::pos2(240.0, 180.0));
    let legacy_paths = [
        write_png(
            &legacy_root,
            "offscreen-a.png",
            PERF_STARTUP_IMAGE_SIZE,
            PERF_STARTUP_IMAGE_SIZE,
        )?,
        write_png(
            &legacy_root,
            "offscreen-b.png",
            PERF_STARTUP_IMAGE_SIZE,
            PERF_STARTUP_IMAGE_SIZE,
        )?,
        write_png(
            &legacy_root,
            "visible-a.png",
            PERF_STARTUP_IMAGE_SIZE,
            PERF_STARTUP_IMAGE_SIZE,
        )?,
        write_png(
            &legacy_root,
            "visible-b.png",
            PERF_STARTUP_IMAGE_SIZE,
            PERF_STARTUP_IMAGE_SIZE,
        )?,
    ];
    let legacy_records = vec![
        perf_record(
            1,
            &legacy_paths[0],
            egui::pos2(-2200.0, -2200.0),
            PERF_STARTUP_IMAGE_SIZE,
            PERF_STARTUP_IMAGE_SIZE,
        ),
        perf_record(
            2,
            &legacy_paths[1],
            egui::pos2(2200.0, 2200.0),
            PERF_STARTUP_IMAGE_SIZE,
            PERF_STARTUP_IMAGE_SIZE,
        ),
        perf_record(
            3,
            &legacy_paths[2],
            egui::pos2(0.0, 0.0),
            PERF_STARTUP_IMAGE_SIZE,
            PERF_STARTUP_IMAGE_SIZE,
        ),
        perf_record(
            4,
            &legacy_paths[3],
            egui::pos2(120.0, 60.0),
            PERF_STARTUP_IMAGE_SIZE,
            PERF_STARTUP_IMAGE_SIZE,
        ),
    ];
    let current_paths = [
        write_png(
            &current_root,
            "offscreen-a.png",
            PERF_STARTUP_IMAGE_SIZE,
            PERF_STARTUP_IMAGE_SIZE,
        )?,
        write_png(
            &current_root,
            "offscreen-b.png",
            PERF_STARTUP_IMAGE_SIZE,
            PERF_STARTUP_IMAGE_SIZE,
        )?,
        write_png(
            &current_root,
            "visible-a.png",
            PERF_STARTUP_IMAGE_SIZE,
            PERF_STARTUP_IMAGE_SIZE,
        )?,
        write_png(
            &current_root,
            "visible-b.png",
            PERF_STARTUP_IMAGE_SIZE,
            PERF_STARTUP_IMAGE_SIZE,
        )?,
    ];
    let current_records = vec![
        perf_record(
            1,
            &current_paths[0],
            egui::pos2(-2200.0, -2200.0),
            PERF_STARTUP_IMAGE_SIZE,
            PERF_STARTUP_IMAGE_SIZE,
        ),
        perf_record(
            2,
            &current_paths[1],
            egui::pos2(2200.0, 2200.0),
            PERF_STARTUP_IMAGE_SIZE,
            PERF_STARTUP_IMAGE_SIZE,
        ),
        perf_record(
            3,
            &current_paths[2],
            egui::pos2(0.0, 0.0),
            PERF_STARTUP_IMAGE_SIZE,
            PERF_STARTUP_IMAGE_SIZE,
        ),
        perf_record(
            4,
            &current_paths[3],
            egui::pos2(120.0, 60.0),
            PERF_STARTUP_IMAGE_SIZE,
            PERF_STARTUP_IMAGE_SIZE,
        ),
    ];
    let legacy_first_visible_ms =
        measure_first_visible_load_ms(&legacy_library, legacy_records, viewport_world, false)?;
    let current_first_visible_ms = measure_first_visible_load_ms(
        &current_library,
        current_records.clone(),
        viewport_world,
        true,
    )?;
    let warm_cached_first_visible_ms =
        measure_first_visible_load_ms(&current_library, current_records, viewport_world, true)?;
    let _ = std::fs::remove_dir_all(legacy_root);
    let _ = std::fs::remove_dir_all(current_root);

    Ok(StartupPerfReport {
        legacy_first_visible_ms,
        current_first_visible_ms,
        warm_cached_first_visible_ms,
    })
}

pub fn startup_trace(
    library: &Library,
    viewport_world: egui::Rect,
    canvas_image_settings: CanvasImageSettings,
) -> Result<Vec<StartupTraceEntry>, String> {
    let mut records = library.load_board_items()?;
    sort_records_by_viewport_proximity(&mut records, viewport_world);
    let started_at = Instant::now();
    let mut entries = Vec::with_capacity(records.len());
    for record in records {
        let trace = ImportTrace {
            queued_at: Instant::now(),
            timings: ImportTimings::default(),
        };
        match load_existing(
            library,
            1,
            None,
            record.clone(),
            usize::MAX,
            canvas_image_settings,
            CanvasImageLod::Overview,
            Some(trace),
            None,
        ) {
            ImportResult::Loaded(loaded) => {
                let diagnostics = loaded
                    .existing_load_diagnostics
                    .ok_or_else(|| "missing existing-load diagnostics".to_owned())?;
                let timings = loaded
                    .trace
                    .map(|trace| trace.timings)
                    .ok_or_else(|| "missing existing-load trace".to_owned())?;
                entries.push(StartupTraceEntry {
                    record: loaded.record,
                    diagnostics,
                    timings,
                    preview_width: loaded.preview_width,
                    preview_height: loaded.preview_height,
                    startup_elapsed: started_at.elapsed(),
                });
            }
            ImportResult::Failed { error, .. } => return Err(error),
            ImportResult::Cropped(_)
            | ImportResult::CacheLodBackfilled { .. }
            | ImportResult::CacheLodBackfillFailed { .. } => {}
        }
    }
    Ok(entries)
}

pub fn interactive_import_perf_report() -> Result<InteractiveImportPerfReport, String> {
    let root = temp_library_root("interactive-import-perf");
    let library = Library::initialize(root.clone())?;
    let viewport_world =
        egui::Rect::from_min_max(egui::pos2(-240.0, -180.0), egui::pos2(240.0, 180.0));
    let mut backlog = Vec::new();
    for index in 0..6 {
        let path = write_png(
            &root,
            &format!("backlog-{index}.png"),
            PERF_BACKLOG_IMAGE_SIZE,
            PERF_BACKLOG_IMAGE_SIZE,
        )?;
        backlog.push(perf_record(
            index as i64 + 1,
            &path,
            egui::pos2(2500.0 + index as f32 * 40.0, 2500.0),
            PERF_BACKLOG_IMAGE_SIZE,
            PERF_BACKLOG_IMAGE_SIZE,
        ));
    }

    let legacy_interactive_import_under_backlog_ms = measure_interactive_import_ms(
        &library,
        &backlog,
        viewport_world,
        solid_rgba(1200, 900),
        false,
    )?;
    let current_interactive_import_under_backlog_ms = measure_interactive_import_ms(
        &library,
        &backlog,
        viewport_world,
        solid_rgba(1200, 900),
        true,
    )?;

    let _ = std::fs::remove_dir_all(root);
    Ok(InteractiveImportPerfReport {
        legacy_interactive_import_under_backlog_ms,
        current_interactive_import_under_backlog_ms,
    })
}

pub fn worker_interactive_import_perf_report() -> Result<WorkerInteractiveImportPerfReport, String>
{
    let parallelism = thread::available_parallelism()
        .map(|available| available.get())
        .unwrap_or(1);
    let current_config = ImportWorkerConfig::for_parallelism(parallelism);

    let legacy_single_worker_ms =
        measure_worker_interactive_import_ms(ImportWorkerConfig::legacy_single_worker())?;
    let current_pooled_ms = measure_worker_interactive_import_ms(current_config)?;

    Ok(WorkerInteractiveImportPerfReport {
        legacy_single_worker_ms,
        current_pooled_ms,
        current_interactive_workers: current_config.interactive_workers,
        current_background_workers: current_config.background_workers,
    })
}

pub fn file_import_perf_report(source_path: PathBuf) -> Result<FileImportPerfReport, String> {
    let root = temp_library_root("file-import-perf");
    let library = Library::initialize(root.clone())?;
    let viewport_world =
        egui::Rect::from_min_max(egui::pos2(-480.0, -320.0), egui::pos2(480.0, 320.0));
    let settings = CanvasImageSettings::default();
    let trace = ImportTrace {
        queued_at: Instant::now(),
        timings: ImportTimings::default(),
    };

    let started_at = Instant::now();
    let result = import_path_sync_traced(
        &library,
        1,
        None,
        None,
        source_path.clone(),
        egui::Pos2::ZERO,
        None,
        viewport_world,
        usize::MAX,
        settings,
        true,
        DEFAULT_IMPORT_TARGET_CANVAS_AREA,
        crate::data::DEFAULT_IMPORT_GAP,
        Some(trace),
        None,
    );
    let total_ms = started_at.elapsed().as_secs_f64() * 1000.0;

    let loaded = match result {
        Ok(loaded) => loaded,
        Err(error) => {
            let _ = std::fs::remove_dir_all(root);
            return Err(error);
        }
    };
    let timings = loaded
        .trace
        .map(|trace| trace.timings)
        .ok_or_else(|| "missing import trace".to_owned())?;
    let canvas_cache_path =
        library.canvas_image_cache_path(loaded.record.asset.id, settings.max_side);
    let canvas_cache_file_size = canvas_cache_path
        .as_ref()
        .and_then(|path| std::fs::metadata(path).ok())
        .map(|metadata| metadata.len());
    let source_file_size = std::fs::metadata(&source_path)
        .map(|metadata| metadata.len())
        .unwrap_or(loaded.record.asset.file_size);

    let report = FileImportPerfReport {
        source_path,
        source_file_size,
        source_width: loaded.record.asset.width,
        source_height: loaded.record.asset.height,
        preview_width: loaded.preview_width,
        preview_height: loaded.preview_height,
        canvas_cache_path,
        canvas_cache_file_size,
        timings,
        total_ms,
    };

    let _ = std::fs::remove_dir_all(root);
    Ok(report)
}

fn import_path_sync_traced(
    library: &Library,
    target_board_id: i64,
    queue_id: Option<u64>,
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
    wgpu_render_state: Option<&eframe::egui_wgpu::RenderState>,
) -> Result<LoadedCanvasItem, String> {
    let worker_started_at = Instant::now();
    let mut trace = trace;
    if let Some(trace) = &mut trace {
        trace.timings.queue_wait = worker_started_at.saturating_duration_since(trace.queued_at);
    }

    if is_svg_path(&source_path) {
        return import_svg_path_sync_traced(
            library,
            target_board_id,
            queue_id,
            placement_id,
            source_path,
            desired_world,
            reserved_world,
            viewport_world,
            auto_scale,
            target_canvas_area,
            import_gap,
            worker_started_at,
            trace,
        );
    }

    let decode_started_at = Instant::now();
    if let Some(prepared) =
        prepare_import_image_with_helper(&source_path, canvas_image_settings).unwrap_or(None)
    {
        if let Some(trace) = &mut trace {
            trace.timings.decode_or_load = decode_started_at.elapsed();
        }
        let initial_scale = initial_import_scale(
            prepared.source_width,
            prepared.source_height,
            auto_scale,
            target_canvas_area,
        );
        let arranged_world = resolve_import_world(
            library,
            target_board_id,
            desired_world,
            reserved_world,
            viewport_world,
            prepared.source_width as f32 * initial_scale.max(0.01),
            prepared.source_height as f32 * initial_scale.max(0.01),
            import_gap,
        )?;

        let store_started_at = Instant::now();
        let stored_path = library
            .store_original(&source_path)
            .map_err(|error| format!("{}: {error}", source_path.display()))?;

        let record = library
            .persist_imported_image_resolving_placement(
                target_board_id,
                placement_id,
                &stored_path,
                &prepared.mime_type,
                prepared.source_width,
                prepared.source_height,
                prepared.file_size,
                arranged_world,
                initial_scale,
            )
            .map_err(|error| format!("{}: {error}", stored_path.display()))?;
        let store_and_persist = store_started_at.elapsed();
        let preview_started_at = Instant::now();
        install_prepared_canvas_cache(
            library,
            record.asset.id,
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
        let preview_texture = build_preview_texture(wgpu_render_state, &preview)?;
        if let Some(trace) = &mut trace {
            trace.timings.store_and_persist = store_and_persist;
            trace.timings.preview_prepare = preview_started_at.elapsed();
            trace.timings.worker_total = worker_started_at.elapsed();
        }

        return Ok(LoadedCanvasItem {
            target_board_id,
            queue_id,
            record,
            preview_rgba: preview.rgba,
            preview_width: preview.width,
            preview_height: preview.height,
            animated_preview: None,
            preview_texture,
            loaded_lod: CanvasImageLod::Working,
            existing_load_diagnostics: None,
            origin: ImportOrigin::Path,
            trace,
        });
    }

    let decode_started_at = Instant::now();
    let decoded =
        decode_image_file_with_max_side(&source_path, Some(canvas_image_settings.max_side))
            .map_err(|error| format!("{}: {error}", source_path.display()))?;
    if let Some(trace) = &mut trace {
        trace.timings.decode_or_load = decode_started_at.elapsed();
    }
    let initial_scale = initial_import_scale(
        decoded.source_width,
        decoded.source_height,
        auto_scale,
        target_canvas_area,
    );
    let arranged_world = resolve_import_world(
        library,
        target_board_id,
        desired_world,
        reserved_world,
        viewport_world,
        decoded.source_width as f32 * initial_scale.max(0.01),
        decoded.source_height as f32 * initial_scale.max(0.01),
        import_gap,
    )?;

    let store_started_at = Instant::now();
    let stored_path = library
        .store_original(&source_path)
        .map_err(|error| format!("{}: {error}", source_path.display()))?;

    let record = library
        .persist_imported_image_resolving_placement(
            target_board_id,
            placement_id,
            &stored_path,
            &decoded.mime_type,
            decoded.source_width,
            decoded.source_height,
            decoded.file_size,
            arranged_world,
            initial_scale,
        )
        .map_err(|error| format!("{}: {error}", stored_path.display()))?;
    let store_and_persist = store_started_at.elapsed();
    let preview_started_at = Instant::now();
    let canvas_source = canvas_source_image_from_rgba(
        decoded.width,
        decoded.height,
        &decoded.rgba,
        canvas_image_settings,
    )?;
    let _ = library.store_canvas_image_cache(
        record.asset.id,
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
    let animated_preview = decoded
        .animation
        .as_ref()
        .map(|animation| {
            preview_animation_from_frames(
                decoded.width,
                decoded.height,
                &animation.frames,
                animated_preview_max_side(canvas_image_settings, max_texture_side),
            )
        })
        .transpose()?;
    let preview_texture = if animated_preview.is_some() {
        ImportedPreviewTexture::Cpu
    } else {
        build_preview_texture(wgpu_render_state, &preview)?
    };
    if let Some(trace) = &mut trace {
        trace.timings.store_and_persist = store_and_persist;
        trace.timings.preview_prepare = preview_started_at.elapsed();
        trace.timings.worker_total = worker_started_at.elapsed();
    }

    Ok(LoadedCanvasItem {
        target_board_id,
        queue_id,
        record,
        preview_rgba: preview.rgba,
        preview_width: preview.width,
        preview_height: preview.height,
        animated_preview,
        preview_texture,
        loaded_lod: CanvasImageLod::Working,
        existing_load_diagnostics: None,
        origin: ImportOrigin::Path,
        trace,
    })
}

fn import_svg_path_sync_traced(
    library: &Library,
    target_board_id: i64,
    queue_id: Option<u64>,
    placement_id: Option<i64>,
    source_path: PathBuf,
    desired_world: egui::Pos2,
    reserved_world: Option<egui::Pos2>,
    viewport_world: egui::Rect,
    auto_scale: bool,
    target_canvas_area: f32,
    import_gap: f32,
    worker_started_at: Instant,
    mut trace: Option<ImportTrace>,
) -> Result<LoadedCanvasItem, String> {
    let decode_started_at = Instant::now();
    let svg = read_svg_document(&source_path)?;
    if let Some(trace) = &mut trace {
        trace.timings.decode_or_load = decode_started_at.elapsed();
    }

    let initial_scale = initial_import_scale(svg.width, svg.height, auto_scale, target_canvas_area);
    let arranged_world = resolve_import_world(
        library,
        target_board_id,
        desired_world,
        reserved_world,
        viewport_world,
        svg.width as f32 * initial_scale.max(0.01),
        svg.height as f32 * initial_scale.max(0.01),
        import_gap,
    )?;

    let store_started_at = Instant::now();
    let stored_path = library
        .store_original(&source_path)
        .map_err(|error| format!("{}: {error}", source_path.display()))?;
    let file_size = std::fs::metadata(&stored_path)
        .map_err(|error| format!("failed to read stored SVG metadata: {error}"))?
        .len();
    let record = library
        .persist_imported_image_resolving_placement(
            target_board_id,
            placement_id,
            &stored_path,
            SVG_MIME_TYPE,
            svg.width,
            svg.height,
            file_size,
            arranged_world,
            initial_scale,
        )
        .map_err(|error| format!("{}: {error}", stored_path.display()))?;
    let store_and_persist = store_started_at.elapsed();

    if let Some(trace) = &mut trace {
        trace.timings.store_and_persist = store_and_persist;
        trace.timings.worker_total = worker_started_at.elapsed();
    }

    Ok(LoadedCanvasItem {
        target_board_id,
        queue_id,
        record,
        preview_rgba: Vec::new(),
        preview_width: svg.width,
        preview_height: svg.height,
        animated_preview: None,
        preview_texture: ImportedPreviewTexture::Svg {
            uri: format!("bytes://asset-svg-{}.svg", stable_svg_uri_key(&stored_path)),
            bytes: Arc::<[u8]>::from(svg.bytes),
            size: [svg.width as usize, svg.height as usize],
        },
        loaded_lod: CanvasImageLod::Working,
        existing_load_diagnostics: None,
        origin: ImportOrigin::Path,
        trace,
    })
}

fn import_path(
    library: &Library,
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
    wgpu_render_state: Option<&eframe::egui_wgpu::RenderState>,
) -> ImportResult {
    match import_path_sync_traced(
        library,
        target_board_id,
        Some(queue_id),
        placement_id,
        source_path.clone(),
        desired_world,
        reserved_world,
        viewport_world,
        max_texture_side,
        canvas_image_settings,
        auto_scale,
        target_canvas_area,
        import_gap,
        trace,
        wgpu_render_state,
    ) {
        Ok(loaded) => ImportResult::Loaded(loaded),
        Err(error) => ImportResult::Failed {
            target_board_id: Some(target_board_id),
            queue_id: Some(queue_id),
            placement_id,
            item_id: None,
            context: source_path.display().to_string(),
            error,
        },
    }
}

fn import_url(
    library: &Library,
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
    wgpu_render_state: Option<&eframe::egui_wgpu::RenderState>,
) -> ImportResult {
    match import_url_sync_traced(
        library,
        target_board_id,
        Some(queue_id),
        placement_id,
        &url,
        desired_world,
        reserved_world,
        viewport_world,
        max_texture_side,
        canvas_image_settings,
        auto_scale,
        target_canvas_area,
        import_gap,
        trace,
        wgpu_render_state,
    ) {
        Ok(loaded) => ImportResult::Loaded(loaded),
        Err(error) => ImportResult::Failed {
            target_board_id: Some(target_board_id),
            queue_id: Some(queue_id),
            placement_id,
            item_id: None,
            context: url,
            error,
        },
    }
}

fn import_clipboard(
    library: &Library,
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
    wgpu_render_state: Option<&eframe::egui_wgpu::RenderState>,
) -> ImportResult {
    match import_clipboard_sync_traced(
        library,
        target_board_id,
        queue_id,
        placement_id,
        rgba,
        width,
        height,
        desired_world,
        reserved_world,
        viewport_world,
        max_texture_side,
        canvas_image_settings,
        auto_scale,
        target_canvas_area,
        import_gap,
        trace,
        wgpu_render_state,
    ) {
        Ok(loaded) => ImportResult::Loaded(loaded),
        Err(error) => ImportResult::Failed {
            target_board_id: Some(target_board_id),
            queue_id: Some(queue_id),
            placement_id,
            item_id: None,
            context: "clipboard".to_owned(),
            error,
        },
    }
}

struct DownloadedUrlImage {
    url: url::Url,
    bytes: Vec<u8>,
    mime_type: Option<String>,
}

fn import_url_sync_traced(
    library: &Library,
    target_board_id: i64,
    queue_id: Option<u64>,
    placement_id: Option<i64>,
    url: &str,
    desired_world: egui::Pos2,
    reserved_world: Option<egui::Pos2>,
    viewport_world: egui::Rect,
    max_texture_side: usize,
    canvas_image_settings: CanvasImageSettings,
    auto_scale: bool,
    target_canvas_area: f32,
    import_gap: f32,
    trace: Option<ImportTrace>,
    wgpu_render_state: Option<&eframe::egui_wgpu::RenderState>,
) -> Result<LoadedCanvasItem, String> {
    let worker_started_at = Instant::now();
    let mut trace = trace;
    if let Some(trace) = &mut trace {
        trace.timings.queue_wait = worker_started_at.saturating_duration_since(trace.queued_at);
    }

    let download_started_at = Instant::now();
    let downloaded = download_remote_image(url)?;
    if let Some(trace) = &mut trace {
        trace.timings.decode_or_load = download_started_at.elapsed();
    }

    if downloaded_url_image_is_svg(&downloaded) {
        return import_downloaded_svg_sync_traced(
            library,
            target_board_id,
            queue_id,
            placement_id,
            downloaded,
            desired_world,
            reserved_world,
            viewport_world,
            auto_scale,
            target_canvas_area,
            import_gap,
            worker_started_at,
            trace,
        );
    }

    let source_label = downloaded.url.as_str().to_owned();
    let decode_started_at = Instant::now();
    let decoded = decode_image_bytes_with_max_side(
        &downloaded.bytes,
        Some(source_label.clone()),
        downloaded.mime_type.as_deref(),
        Some(canvas_image_settings.max_side),
    )?;
    if let Some(trace) = &mut trace {
        trace.timings.decode_or_load += decode_started_at.elapsed();
    }

    let initial_scale = initial_import_scale(
        decoded.source_width,
        decoded.source_height,
        auto_scale,
        target_canvas_area,
    );
    let arranged_world = resolve_import_world(
        library,
        target_board_id,
        desired_world,
        reserved_world,
        viewport_world,
        decoded.source_width as f32 * initial_scale.max(0.01),
        decoded.source_height as f32 * initial_scale.max(0.01),
        import_gap,
    )?;

    let store_started_at = Instant::now();
    let source_name = remote_image_source_name(&downloaded.url);
    let stored_path =
        library.store_original_bytes(&source_name, &decoded.extension, &downloaded.bytes)?;
    let record = library.persist_imported_image_resolving_placement(
        target_board_id,
        placement_id,
        &stored_path,
        &decoded.mime_type,
        decoded.source_width,
        decoded.source_height,
        downloaded.bytes.len() as u64,
        arranged_world,
        initial_scale,
    )?;
    let store_and_persist = store_started_at.elapsed();

    let preview_started_at = Instant::now();
    let canvas_source = canvas_source_image_from_rgba(
        decoded.width,
        decoded.height,
        &decoded.rgba,
        canvas_image_settings,
    )?;
    let _ = library.store_canvas_image_cache(
        record.asset.id,
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
    let animated_preview = decoded
        .animation
        .as_ref()
        .map(|animation| {
            preview_animation_from_frames(
                decoded.width,
                decoded.height,
                &animation.frames,
                animated_preview_max_side(canvas_image_settings, max_texture_side),
            )
        })
        .transpose()?;
    let preview_texture = if animated_preview.is_some() {
        ImportedPreviewTexture::Cpu
    } else {
        build_preview_texture(
            wgpu_render_state,
            &PreviewImage {
                rgba: preview.rgba.clone(),
                width: preview.width,
                height: preview.height,
            },
        )?
    };
    let preview_prepare = preview_started_at.elapsed();

    if let Some(trace) = &mut trace {
        trace.timings.store_and_persist = store_and_persist;
        trace.timings.preview_prepare = preview_prepare;
        trace.timings.worker_total = worker_started_at.elapsed();
    }

    Ok(LoadedCanvasItem {
        target_board_id,
        queue_id,
        record,
        preview_rgba: preview.rgba,
        preview_width: preview.width,
        preview_height: preview.height,
        animated_preview,
        preview_texture,
        loaded_lod: CanvasImageLod::Working,
        existing_load_diagnostics: None,
        origin: ImportOrigin::Url,
        trace,
    })
}

fn import_downloaded_svg_sync_traced(
    library: &Library,
    target_board_id: i64,
    queue_id: Option<u64>,
    placement_id: Option<i64>,
    downloaded: DownloadedUrlImage,
    desired_world: egui::Pos2,
    reserved_world: Option<egui::Pos2>,
    viewport_world: egui::Rect,
    auto_scale: bool,
    target_canvas_area: f32,
    import_gap: f32,
    worker_started_at: Instant,
    mut trace: Option<ImportTrace>,
) -> Result<LoadedCanvasItem, String> {
    let source_label = downloaded.url.as_str().to_owned();
    let decode_started_at = Instant::now();
    let svg =
        parse_svg_document(downloaded.bytes).map_err(|error| format!("{source_label}: {error}"))?;
    if let Some(trace) = &mut trace {
        trace.timings.decode_or_load += decode_started_at.elapsed();
    }

    let initial_scale = initial_import_scale(svg.width, svg.height, auto_scale, target_canvas_area);
    let arranged_world = resolve_import_world(
        library,
        target_board_id,
        desired_world,
        reserved_world,
        viewport_world,
        svg.width as f32 * initial_scale.max(0.01),
        svg.height as f32 * initial_scale.max(0.01),
        import_gap,
    )?;

    let store_started_at = Instant::now();
    let source_name = remote_image_source_name(&downloaded.url);
    let stored_path = library.store_original_bytes(&source_name, "svg", &svg.bytes)?;
    let record = library.persist_imported_image_resolving_placement(
        target_board_id,
        placement_id,
        &stored_path,
        SVG_MIME_TYPE,
        svg.width,
        svg.height,
        svg.bytes.len() as u64,
        arranged_world,
        initial_scale,
    )?;
    let store_and_persist = store_started_at.elapsed();

    if let Some(trace) = &mut trace {
        trace.timings.store_and_persist = store_and_persist;
        trace.timings.worker_total = worker_started_at.elapsed();
    }

    Ok(LoadedCanvasItem {
        target_board_id,
        queue_id,
        record,
        preview_rgba: Vec::new(),
        preview_width: svg.width,
        preview_height: svg.height,
        animated_preview: None,
        preview_texture: ImportedPreviewTexture::Svg {
            uri: format!("bytes://asset-svg-{}.svg", stable_svg_uri_key(&stored_path)),
            bytes: Arc::<[u8]>::from(svg.bytes),
            size: [svg.width as usize, svg.height as usize],
        },
        loaded_lod: CanvasImageLod::Working,
        existing_load_diagnostics: None,
        origin: ImportOrigin::Url,
        trace,
    })
}

fn prepare_import_image_with_helper(
    source_path: &Path,
    canvas_image_settings: CanvasImageSettings,
) -> Result<Option<PreparedImportImage>, String> {
    #[cfg(test)]
    {
        let _ = (source_path, canvas_image_settings);
        return Ok(None);
    }

    #[cfg(not(test))]
    {
        if std::env::var_os(IMAGE_IMPORT_HELPER_DISABLED_ENV).is_some() {
            return Ok(None);
        }

        let rgba_path = unique_image_import_helper_rgba_path();
        let output = Command::new(std::env::current_exe().map_err(|error| {
            format!("failed to locate image import helper executable: {error}")
        })?)
        .arg("--image-import-prepare")
        .arg(format!("--source={}", source_path.display()))
        .arg(format!("--rgba-output={}", rgba_path.display()))
        .arg(format!(
            "--canvas-max-side={}",
            canvas_image_settings.max_side
        ))
        .output()
        .map_err(|error| format!("failed to run image import helper: {error}"))?;

        if !output.status.success() {
            let _ = std::fs::remove_file(&rgba_path);
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!(
                "image import helper failed for {}: {}",
                source_path.display(),
                stderr.trim()
            ));
        }

        let prepared: ImageImportPrepareOutput = serde_json::from_slice(&output.stdout)
            .map_err(|error| format!("failed to decode image import helper output: {error}"))?;
        if prepared.animated {
            let _ = std::fs::remove_file(&rgba_path);
            return Ok(None);
        }

        let output_path = prepared.rgba_path.as_ref().unwrap_or(&rgba_path);
        let rgba = std::fs::read(output_path)
            .map_err(|error| format!("failed to read image import helper pixels: {error}"))?;
        let _ = std::fs::remove_file(output_path);
        let expected_len = prepared.width as usize * prepared.height as usize * 4;
        if rgba.len() != expected_len {
            return Err(format!(
                "image import helper returned {} bytes for {}x{} RGBA image; expected {}",
                rgba.len(),
                prepared.width,
                prepared.height,
                expected_len
            ));
        }

        Ok(Some(PreparedImportImage {
            rgba,
            width: prepared.width,
            height: prepared.height,
            source_width: prepared.source_width,
            source_height: prepared.source_height,
            mime_type: prepared.mime_type,
            file_size: prepared.file_size,
            cache_path: prepared.cache_path,
            cache_extension: prepared.cache_extension,
        }))
    }
}

fn install_prepared_canvas_cache(
    library: &Library,
    asset_id: i64,
    max_side: u32,
    lod: CanvasImageLod,
    prepared: &PreparedImportImage,
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

#[cfg(not(test))]
fn unique_image_import_helper_rgba_path() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!(
        "snappyref-import-prepare-{}-{nanos}.rgba",
        std::process::id()
    ))
}

pub(crate) fn print_image_import_prepare_output_from_args() -> Result<(), String> {
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
    print_image_import_prepare_output(
        &source_path,
        &rgba_output_path,
        CanvasImageSettings {
            max_side: canvas_max_side,
        },
    )
}

fn print_image_import_prepare_output(
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
            &ImageImportPrepareOutput {
                animated: true,
                rgba_path: None,
                width: 0,
                height: 0,
                source_width: decoded.source_width,
                source_height: decoded.source_height,
                mime_type: decoded.mime_type,
                file_size: decoded.file_size,
                cache_path: None,
                cache_extension: None,
            },
        )
        .map_err(|error| format!("failed to write image import helper output: {error}"))?;
        return Ok(());
    }

    let canvas_source = canvas_source_image_from_rgba(
        decoded.width,
        decoded.height,
        &decoded.rgba,
        canvas_image_settings,
    )?;
    std::fs::write(rgba_output_path, &canvas_source.rgba)
        .map_err(|error| format!("failed to write image import helper pixels: {error}"))?;
    let (cache_path, cache_extension) = write_prepared_canvas_cache(
        rgba_output_path,
        &canvas_source.rgba,
        canvas_source.width,
        canvas_source.height,
    )?;
    serde_json::to_writer(
        std::io::stdout(),
        &ImageImportPrepareOutput {
            animated: false,
            rgba_path: Some(rgba_output_path.to_path_buf()),
            width: canvas_source.width,
            height: canvas_source.height,
            source_width: decoded.source_width,
            source_height: decoded.source_height,
            mime_type: decoded.mime_type,
            file_size: decoded.file_size,
            cache_path: Some(cache_path),
            cache_extension: Some(cache_extension.to_owned()),
        },
    )
    .map_err(|error| format!("failed to write image import helper output: {error}"))?;
    Ok(())
}

fn write_prepared_canvas_cache(
    rgba_output_path: &Path,
    rgba: &[u8],
    width: u32,
    height: u32,
) -> Result<(PathBuf, &'static str), String> {
    let (cache_path, extension) = if rgba.chunks_exact(4).all(|pixel| pixel[3] == u8::MAX) {
        (rgba_output_path.with_extension("jpg"), "jpg")
    } else {
        (rgba_output_path.with_extension("webp"), "webp")
    };
    let file = std::fs::File::create(&cache_path)
        .map_err(|error| format!("failed to create image import helper cache: {error}"))?;
    let writer = std::io::BufWriter::new(file);
    match extension {
        "jpg" => {
            let rgb = rgba_to_rgb_for_import_cache(rgba);
            JpegEncoder::new_with_quality(writer, 82)
                .write_image(&rgb, width, height, ExtendedColorType::Rgb8)
                .map_err(|error| {
                    format!("failed to write image import helper JPEG cache: {error}")
                })?;
        }
        "webp" => {
            WebPEncoder::new_lossless(writer)
                .write_image(rgba, width, height, ExtendedColorType::Rgba8)
                .map_err(|error| {
                    format!("failed to write image import helper WebP cache: {error}")
                })?;
        }
        _ => unreachable!("cache extension is selected locally"),
    }
    Ok((cache_path, extension))
}

fn rgba_to_rgb_for_import_cache(rgba: &[u8]) -> Vec<u8> {
    let mut rgb = Vec::with_capacity(rgba.len() / 4 * 3);
    for pixel in rgba.chunks_exact(4) {
        rgb.extend_from_slice(&pixel[..3]);
    }
    rgb
}

fn import_clipboard_sync_traced(
    library: &Library,
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
    wgpu_render_state: Option<&eframe::egui_wgpu::RenderState>,
) -> Result<LoadedCanvasItem, String> {
    let worker_started_at = Instant::now();
    let mut trace = trace;
    if let Some(trace) = &mut trace {
        trace.timings.queue_wait = worker_started_at.saturating_duration_since(trace.queued_at);
    }

    let store_started_at = Instant::now();
    let stored_path = library.store_clipboard_image(&rgba, width, height)?;
    let file_size = std::fs::metadata(&stored_path)
        .map_err(|error| format!("failed to read clipboard image metadata: {error}"))?
        .len();
    let initial_scale = initial_import_scale(width, height, auto_scale, target_canvas_area);
    let arranged_world = resolve_import_world(
        library,
        target_board_id,
        desired_world,
        reserved_world,
        viewport_world,
        width as f32 * initial_scale.max(0.01),
        height as f32 * initial_scale.max(0.01),
        import_gap,
    )?;
    let record = library.persist_imported_image_resolving_placement(
        target_board_id,
        placement_id,
        &stored_path,
        image::ImageFormat::Png.to_mime_type(),
        width,
        height,
        file_size,
        arranged_world,
        initial_scale,
    )?;
    let store_and_persist = store_started_at.elapsed();
    let preview_started_at = Instant::now();
    let canvas_source = canvas_source_image_from_rgba(width, height, &rgba, canvas_image_settings)?;
    let _ = library.store_canvas_image_cache(
        record.asset.id,
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
    let preview_texture = build_preview_texture(wgpu_render_state, &preview)?;
    if let Some(trace) = &mut trace {
        trace.timings.store_and_persist = store_and_persist;
        trace.timings.preview_prepare = preview_started_at.elapsed();
        trace.timings.worker_total = worker_started_at.elapsed();
    }

    Ok(LoadedCanvasItem {
        target_board_id,
        queue_id: Some(queue_id),
        record,
        preview_rgba: preview.rgba,
        preview_width: preview.width,
        preview_height: preview.height,
        animated_preview: None,
        preview_texture,
        loaded_lod: CanvasImageLod::Working,
        existing_load_diagnostics: None,
        origin: ImportOrigin::Clipboard,
        trace,
    })
}

pub(crate) fn load_existing(
    library: &Library,
    target_board_id: i64,
    queue_id: Option<u64>,
    record: BoardItemWithAsset,
    max_texture_side: usize,
    canvas_image_settings: CanvasImageSettings,
    target_lod: CanvasImageLod,
    trace: Option<ImportTrace>,
    wgpu_render_state: Option<&eframe::egui_wgpu::RenderState>,
) -> ImportResult {
    let worker_started_at = Instant::now();
    let mut trace = trace;
    if let Some(trace) = &mut trace {
        trace.timings.queue_wait = worker_started_at.saturating_duration_since(trace.queued_at);
    }

    if original_mime_type_is_svg(&record.asset.mime_type) {
        let decode_started_at = Instant::now();
        match read_svg_document(&record.asset.original_path) {
            Ok(svg) => {
                if let Some(trace) = &mut trace {
                    trace.timings.decode_or_load = decode_started_at.elapsed();
                    trace.timings.worker_total = worker_started_at.elapsed();
                }
                return ImportResult::Loaded(LoadedCanvasItem {
                    target_board_id,
                    queue_id,
                    preview_rgba: Vec::new(),
                    preview_width: svg.width,
                    preview_height: svg.height,
                    preview_texture: ImportedPreviewTexture::Svg {
                        uri: format!(
                            "bytes://asset-svg-{}.svg",
                            stable_svg_uri_key(&record.asset.original_path)
                        ),
                        bytes: Arc::<[u8]>::from(svg.bytes),
                        size: [record.asset.width as usize, record.asset.height as usize],
                    },
                    record,
                    animated_preview: None,
                    loaded_lod: target_lod,
                    existing_load_diagnostics: None,
                    origin: ImportOrigin::Existing,
                    trace,
                });
            }
            Err(error) => {
                return ImportResult::Failed {
                    target_board_id: Some(target_board_id),
                    queue_id,
                    placement_id: None,
                    item_id: Some(record.item.id),
                    context: record.asset.original_path.display().to_string(),
                    error,
                };
            }
        }
    }

    let decode_started_at = Instant::now();
    let preview_started_at = Instant::now();
    match load_existing_preview(
        library,
        &record,
        canvas_image_settings,
        target_lod,
        max_texture_side,
    ) {
        Ok((preview, animated_preview, diagnostics)) => {
            let decode_or_load = decode_started_at.elapsed();
            let preview_texture = if animated_preview.is_some() {
                ImportedPreviewTexture::Cpu
            } else {
                match build_preview_texture(wgpu_render_state, &preview) {
                    Ok(preview_texture) => preview_texture,
                    Err(error) => {
                        return ImportResult::Failed {
                            target_board_id: Some(target_board_id),
                            queue_id,
                            placement_id: None,
                            item_id: Some(record.item.id),
                            context: record.asset.original_path.display().to_string(),
                            error,
                        };
                    }
                }
            };
            if let Some(trace) = &mut trace {
                trace.timings.decode_or_load = decode_or_load;
                trace.timings.preview_prepare = preview_started_at.elapsed();
                trace.timings.worker_total = worker_started_at.elapsed();
            }
            ImportResult::Loaded(LoadedCanvasItem {
                target_board_id,
                queue_id,
                record,
                preview_rgba: preview.rgba,
                preview_width: preview.width,
                preview_height: preview.height,
                animated_preview,
                preview_texture,
                loaded_lod: target_lod,
                existing_load_diagnostics: Some(diagnostics),
                origin: ImportOrigin::Existing,
                trace,
            })
        }
        Err(error) => ImportResult::Failed {
            target_board_id: Some(target_board_id),
            queue_id,
            placement_id: None,
            item_id: Some(record.item.id),
            context: record.asset.original_path.display().to_string(),
            error,
        },
    }
}

fn measure_first_visible_load_ms(
    library: &Library,
    mut records: Vec<BoardItemWithAsset>,
    viewport_world: egui::Rect,
    prioritize: bool,
) -> Result<f64, String> {
    if prioritize {
        sort_records_by_viewport_proximity(&mut records, viewport_world);
    }
    let started_at = Instant::now();
    for record in records {
        let loaded = load_existing(
            library,
            1,
            None,
            record.clone(),
            usize::MAX,
            CanvasImageSettings::default(),
            CanvasImageLod::Working,
            None,
            None,
        );
        match loaded {
            ImportResult::Loaded(loaded) => {
                if record_intersects_viewport_world(&loaded.record, viewport_world) {
                    return Ok(started_at.elapsed().as_secs_f64() * 1000.0);
                }
            }
            ImportResult::Failed { error, .. } => return Err(error),
            ImportResult::Cropped(_)
            | ImportResult::CacheLodBackfilled { .. }
            | ImportResult::CacheLodBackfillFailed { .. } => {}
        }
    }
    Ok(started_at.elapsed().as_secs_f64() * 1000.0)
}

fn measure_interactive_import_ms(
    library: &Library,
    backlog: &[BoardItemWithAsset],
    viewport_world: egui::Rect,
    clipboard_rgba: Vec<u8>,
    prioritize: bool,
) -> Result<f64, String> {
    let started_at = Instant::now();
    if prioritize {
        import_clipboard_sync(
            library,
            clipboard_rgba,
            1200,
            900,
            egui::Pos2::ZERO,
            viewport_world,
            true,
            crate::data::DEFAULT_IMPORT_GAP,
        )?;
    } else {
        for record in backlog {
            match load_existing(
                library,
                1,
                None,
                record.clone(),
                usize::MAX,
                CanvasImageSettings::default(),
                CanvasImageLod::Working,
                None,
                None,
            ) {
                ImportResult::Loaded(_) => {}
                ImportResult::Failed { error, .. } => return Err(error),
                ImportResult::Cropped(_)
                | ImportResult::CacheLodBackfilled { .. }
                | ImportResult::CacheLodBackfillFailed { .. } => {}
            }
        }
        import_clipboard_sync(
            library,
            clipboard_rgba,
            1200,
            900,
            egui::Pos2::ZERO,
            viewport_world,
            true,
            crate::data::DEFAULT_IMPORT_GAP,
        )?;
    }
    Ok(started_at.elapsed().as_secs_f64() * 1000.0)
}

fn measure_worker_interactive_import_ms(config: ImportWorkerConfig) -> Result<f64, String> {
    let root = temp_library_root("worker-interactive-import-perf");
    let library = Library::initialize(root.clone())?;
    let worker = ImportWorker::new_with_config(library, egui::Context::default(), None, config);
    let viewport_world =
        egui::Rect::from_min_max(egui::pos2(-240.0, -180.0), egui::pos2(240.0, 180.0));

    for index in 0..6 {
        let path = write_png(
            &root,
            &format!("worker-backlog-{index}.png"),
            PERF_BACKLOG_IMAGE_SIZE,
            PERF_BACKLOG_IMAGE_SIZE,
        )?;
        worker.enqueue(ImportRequest::LoadExisting {
            target_board_id: 1,
            queue_id: None,
            record: perf_record(
                index as i64 + 1,
                &path,
                egui::pos2(2500.0 + index as f32 * 40.0, 2500.0),
                PERF_BACKLOG_IMAGE_SIZE,
                PERF_BACKLOG_IMAGE_SIZE,
            ),
            max_texture_side: usize::MAX,
            canvas_image_settings: CanvasImageSettings::default(),
            target_lod: CanvasImageLod::Working,
            priority: ExistingLoadPriority::Background,
            trace: None,
        })?;
    }

    let started_at = Instant::now();
    worker.enqueue(ImportRequest::ImportClipboard {
        target_board_id: 1,
        queue_id: 1,
        placement_id: None,
        rgba: solid_rgba(1200, 900),
        width: 1200,
        height: 900,
        desired_world: egui::Pos2::ZERO,
        reserved_world: None,
        viewport_world,
        max_texture_side: usize::MAX,
        canvas_image_settings: CanvasImageSettings::default(),
        auto_scale: true,
        target_canvas_area: DEFAULT_IMPORT_TARGET_CANVAS_AREA,
        import_gap: crate::data::DEFAULT_IMPORT_GAP,
        trace: Some(ImportTrace {
            queued_at: started_at,
            timings: ImportTimings::default(),
        }),
    })?;

    let deadline = Instant::now() + Duration::from_secs(10);
    let result = loop {
        let mut interactive_result = None;
        for result in worker.try_recv_up_to(8) {
            match result {
                ImportResult::Loaded(loaded) if loaded.origin == ImportOrigin::Clipboard => {
                    interactive_result = Some(Ok(started_at.elapsed().as_secs_f64() * 1000.0));
                    break;
                }
                ImportResult::Failed { context, error, .. } if context == "clipboard" => {
                    interactive_result = Some(Err(error));
                    break;
                }
                _ => {}
            }
        }
        if let Some(result) = interactive_result {
            break result;
        }

        if Instant::now() >= deadline {
            break Err("timed out waiting for worker-backed interactive import".to_owned());
        }

        thread::sleep(Duration::from_millis(1));
    };
    drop(worker);
    let _ = std::fs::remove_dir_all(&root);
    result
}

fn record_viewport_distance_sq(record: &BoardItemWithAsset, viewport_world: egui::Rect) -> f32 {
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

fn record_intersects_viewport_world(
    record: &BoardItemWithAsset,
    viewport_world: egui::Rect,
) -> bool {
    record_world_rect(record).intersects(viewport_world)
}

fn record_world_rect(record: &BoardItemWithAsset) -> egui::Rect {
    let size = egui::vec2(record.asset.width as f32, record.asset.height as f32)
        * record.item.scale.max(0.01);
    egui::Rect::from_center_size(egui::pos2(record.item.x, record.item.y), size)
}

fn perf_record(
    item_id: i64,
    path: &Path,
    center: egui::Pos2,
    width: u32,
    height: u32,
) -> BoardItemWithAsset {
    let item = crate::data::BoardItem {
        id: item_id,
        asset_id: item_id,
        x: center.x,
        y: center.y,
        scale: 1.0,
        rotation: 0.0,
        z_index: item_id as i32,
        archived: false,
    };
    let asset = crate::data::ImageAsset {
        id: item_id,
        original_path: path.to_path_buf(),
        filename: path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("image.png")
            .to_owned(),
        mime_type: "image/png".to_owned(),
        width,
        height,
        file_size: std::fs::metadata(path)
            .map(|metadata| metadata.len())
            .unwrap_or(0),
        imported_at: 0,
    };
    BoardItemWithAsset::loaded(item, asset)
}

fn solid_rgba(width: u32, height: u32) -> Vec<u8> {
    let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
    for y in 0..height {
        for x in 0..width {
            let channel = ((x ^ y) & 0xFF) as u8;
            rgba.extend_from_slice(&[channel, channel.wrapping_add(47), 255, 255]);
        }
    }
    rgba
}

fn temp_library_root(label: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("snappyref-{label}-{unique}"))
}

fn write_png(root: &Path, filename: &str, width: u32, height: u32) -> Result<PathBuf, String> {
    std::fs::create_dir_all(root)
        .map_err(|error| format!("failed to create temp perf root: {error}"))?;
    let path = root.join(filename);
    let rgba = solid_rgba(width, height);
    image::save_buffer_with_format(
        &path,
        &rgba,
        width,
        height,
        image::ColorType::Rgba8,
        image::ImageFormat::Png,
    )
    .map_err(|error| format!("failed to write perf fixture image: {error}"))?;
    Ok(path)
}

fn crop_item(
    library: &Library,
    request: crate::canvas::CanvasCropRequest,
    max_texture_side: usize,
    canvas_image_settings: CanvasImageSettings,
    wgpu_render_state: Option<&eframe::egui_wgpu::RenderState>,
) -> ImportResult {
    let source_context = request.source_asset.original_path.display().to_string();
    if original_mime_type_is_svg(&request.source_asset.mime_type) {
        return ImportResult::Failed {
            target_board_id: None,
            queue_id: None,
            placement_id: None,
            item_id: Some(request.item_id),
            context: source_context,
            error: "SVG cropping is not supported yet".to_owned(),
        };
    }

    let decoded = match decode_image_file(&request.source_asset.original_path) {
        Ok(decoded) => decoded,
        Err(error) => {
            return ImportResult::Failed {
                target_board_id: None,
                queue_id: None,
                placement_id: None,
                item_id: Some(request.item_id),
                context: source_context,
                error,
            };
        }
    };
    let image = match RgbaImage::from_vec(decoded.width, decoded.height, decoded.rgba) {
        Some(image) => image,
        None => {
            return ImportResult::Failed {
                target_board_id: None,
                queue_id: None,
                placement_id: None,
                item_id: Some(request.item_id),
                context: source_context,
                error: "failed to rebuild source image pixels for crop".to_owned(),
            };
        }
    };

    let cropped = image::imageops::crop_imm(
        &image,
        request.crop_rect.x,
        request.crop_rect.y,
        request.crop_rect.width,
        request.crop_rect.height,
    )
    .to_image();
    let cropped_rgba = cropped.into_raw();
    let width = request.crop_rect.width;
    let height = request.crop_rect.height;

    let base_name = std::path::Path::new(&request.source_asset.filename)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("image");
    let stored_path = match library.store_cropped_rgba_image(
        &format!("{base_name}-crop"),
        &cropped_rgba,
        width,
        height,
    ) {
        Ok(path) => path,
        Err(error) => {
            return ImportResult::Failed {
                target_board_id: None,
                queue_id: None,
                placement_id: None,
                item_id: Some(request.item_id),
                context: source_context,
                error,
            };
        }
    };

    let file_size = match std::fs::metadata(&stored_path) {
        Ok(metadata) => metadata.len(),
        Err(error) => {
            let _ = std::fs::remove_file(&stored_path);
            return ImportResult::Failed {
                target_board_id: None,
                queue_id: None,
                placement_id: None,
                item_id: Some(request.item_id),
                context: stored_path.display().to_string(),
                error: format!("failed to read cropped image metadata: {error}"),
            };
        }
    };
    let preview_max_side = animated_preview_max_side(
        CanvasImageSettings {
            max_side: INTERACTIVE_CROP_PREVIEW_MAX_SIDE.min(canvas_image_settings.max_side),
        },
        max_texture_side,
    );
    let preview = match fast_preview_image_from_rgba(width, height, &cropped_rgba, preview_max_side)
    {
        Ok(preview) => preview,
        Err(error) => {
            let _ = std::fs::remove_file(&stored_path);
            return ImportResult::Failed {
                target_board_id: None,
                queue_id: None,
                placement_id: None,
                item_id: Some(request.item_id),
                context: source_context,
                error,
            };
        }
    };
    let preview_texture = match build_preview_texture(wgpu_render_state, &preview) {
        Ok(preview_texture) => preview_texture,
        Err(error) => {
            let _ = std::fs::remove_file(&stored_path);
            return ImportResult::Failed {
                target_board_id: None,
                queue_id: None,
                placement_id: None,
                item_id: Some(request.item_id),
                context: source_context,
                error,
            };
        }
    };
    ImportResult::Cropped(CroppedCanvasItem {
        item_id: request.item_id,
        stored_path,
        width,
        height,
        file_size,
        preview_rgba: preview.rgba,
        preview_width: preview.width,
        preview_height: preview.height,
        preview_texture,
        loaded_lod: CanvasImageLod::Overview,
        crop_offset_local: request.crop_offset_local,
    })
}

fn build_preview_texture(
    wgpu_render_state: Option<&eframe::egui_wgpu::RenderState>,
    preview: &crate::jobs::image_formats::PreviewImage,
) -> Result<ImportedPreviewTexture, String> {
    let Some(render_state) = wgpu_render_state else {
        return Ok(ImportedPreviewTexture::Cpu);
    };

    if preview.rgba.len() != preview.width as usize * preview.height as usize * 4 {
        return Err("preview pixel buffer did not match preview dimensions".to_owned());
    }

    let size = eframe::wgpu::Extent3d {
        width: preview.width,
        height: preview.height,
        depth_or_array_layers: 1,
    };
    let texture = render_state
        .device
        .create_texture(&eframe::wgpu::TextureDescriptor {
            label: Some("snappyref_import_preview"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: eframe::wgpu::TextureDimension::D2,
            format: eframe::wgpu::TextureFormat::Rgba8Unorm,
            usage: eframe::wgpu::TextureUsages::TEXTURE_BINDING
                | eframe::wgpu::TextureUsages::COPY_DST,
            view_formats: &[eframe::wgpu::TextureFormat::Rgba8Unorm],
        });
    render_state.queue.write_texture(
        texture.as_image_copy(),
        &preview.rgba,
        eframe::wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(4 * preview.width),
            rows_per_image: Some(preview.height),
        },
        size,
    );

    let view = texture.create_view(&eframe::wgpu::TextureViewDescriptor::default());
    let texture_id = render_state.renderer.write().register_native_texture(
        &render_state.device,
        &view,
        eframe::wgpu::FilterMode::Linear,
    );

    Ok(ImportedPreviewTexture::Native(Arc::new(
        NativeTextureHandle::from_registered_texture(
            render_state.clone(),
            texture_id,
            preview.width,
            preview.height,
            texture,
            view,
        ),
    )))
}

fn lod_max_side(canvas_image_settings: CanvasImageSettings, lod: CanvasImageLod) -> u32 {
    match lod {
        CanvasImageLod::Overview => OVERVIEW_CACHE_MAX_SIDE.min(canvas_image_settings.max_side),
        CanvasImageLod::Working => canvas_image_settings.max_side,
        CanvasImageLod::Detail => DETAIL_CACHE_MAX_SIDE.max(canvas_image_settings.max_side),
    }
}

fn lod_settings(
    canvas_image_settings: CanvasImageSettings,
    lod: CanvasImageLod,
) -> CanvasImageSettings {
    CanvasImageSettings {
        max_side: lod_max_side(canvas_image_settings, lod),
    }
}

fn load_existing_preview(
    library: &Library,
    record: &BoardItemWithAsset,
    canvas_image_settings: CanvasImageSettings,
    target_lod: CanvasImageLod,
    max_texture_side: usize,
) -> Result<
    (
        PreviewImage,
        Option<AnimatedPreview>,
        ExistingLoadDiagnostics,
    ),
    String,
> {
    if original_mime_type_is_svg(&record.asset.mime_type) {
        let svg = read_svg_document(&record.asset.original_path)?;
        let diagnostics = ExistingLoadDiagnostics {
            source_kind: ExistingLoadSourceKind::Original,
            source_path: record.asset.original_path.clone(),
            source_file_size: svg.bytes.len() as u64,
            source_width: svg.width,
            source_height: svg.height,
            backfilled_cache: false,
        };
        return Ok((
            PreviewImage {
                rgba: Vec::new(),
                width: svg.width,
                height: svg.height,
            },
            None,
            diagnostics,
        ));
    }

    let lod_settings = lod_settings(canvas_image_settings, target_lod);
    let (canvas_source, animated_preview, diagnostics) =
        if original_mime_type_supports_animation(&record.asset.mime_type) {
            let decoded = decode_image_file(&record.asset.original_path)?;
            let source_file_size = decoded.file_size;
            (
                preview_image_from_rgba(
                    decoded.width,
                    decoded.height,
                    &decoded.rgba,
                    lod_settings.max_side.min(max_texture_side as u32) as usize,
                )?,
                decoded
                    .animation
                    .as_ref()
                    .map(|animation| {
                        preview_animation_from_frames(
                            decoded.width,
                            decoded.height,
                            &animation.frames,
                            lod_settings.max_side.min(max_texture_side as u32) as usize,
                        )
                    })
                    .transpose()?,
                ExistingLoadDiagnostics {
                    source_kind: ExistingLoadSourceKind::Original,
                    source_path: record.asset.original_path.clone(),
                    source_file_size,
                    source_width: decoded.source_width,
                    source_height: decoded.source_height,
                    backfilled_cache: false,
                },
            )
        } else if let Some(cache_path) = library.canvas_image_cache_path_for_lod(
            record.asset.id,
            lod_settings.max_side,
            target_lod,
        ) && cache_path.is_file()
        {
            match decode_image_file(&cache_path) {
                Ok(decoded) => {
                    let source_file_size = decoded.file_size;
                    (
                        PreviewImage {
                            rgba: decoded.rgba,
                            width: decoded.width,
                            height: decoded.height,
                        },
                        None,
                        ExistingLoadDiagnostics {
                            source_kind: ExistingLoadSourceKind::Cache,
                            source_path: cache_path,
                            source_file_size,
                            source_width: decoded.width,
                            source_height: decoded.height,
                            backfilled_cache: false,
                        },
                    )
                }
                Err(_) => {
                    let _ = std::fs::remove_file(&cache_path);
                    let (preview, diagnostics) =
                        backfill_canvas_image_cache(library, record, lod_settings, target_lod)?;
                    (preview, None, diagnostics)
                }
            }
        } else {
            let (preview, diagnostics) =
                backfill_canvas_image_cache(library, record, lod_settings, target_lod)?;
            (preview, None, diagnostics)
        };

    let preview = if let Some(animated_preview) = &animated_preview {
        PreviewImage {
            rgba: animated_preview.frames[0].rgba.clone(),
            width: animated_preview.width,
            height: animated_preview.height,
        }
    } else if canvas_source.width <= max_texture_side as u32
        && canvas_source.height <= max_texture_side as u32
    {
        canvas_source
    } else {
        preview_image_from_rgba(
            canvas_source.width,
            canvas_source.height,
            &canvas_source.rgba,
            max_texture_side,
        )?
    };

    Ok((preview, animated_preview, diagnostics))
}

pub fn load_canvas_snapshot_pixels(
    library: &Library,
    record: &BoardItemWithAsset,
    target_width: u32,
    target_height: u32,
) -> Result<(Vec<u8>, u32, u32), String> {
    if target_width == 0 || target_height == 0 {
        return Err("cannot cache zero-sized canvas pixels".to_owned());
    }
    if original_mime_type_is_svg(&record.asset.mime_type) {
        return Err("cannot cache SVG canvas pixels".to_owned());
    }

    for path in
        canvas_snapshot_cache_candidates(library, record.asset.id, target_width, target_height)
    {
        if !path.is_file() {
            continue;
        }
        match decode_image_file(&path) {
            Ok(decoded) => {
                return resize_rgba_exact(
                    decoded.rgba,
                    decoded.width,
                    decoded.height,
                    target_width,
                    target_height,
                );
            }
            Err(_) => {
                let _ = std::fs::remove_file(&path);
            }
        }
    }

    let decoded = decode_image_file_with_max_side(
        &record.asset.original_path,
        Some(target_width.max(target_height)),
    )?;
    resize_rgba_exact(
        decoded.rgba,
        decoded.width,
        decoded.height,
        target_width,
        target_height,
    )
}

fn canvas_snapshot_cache_candidates(
    library: &Library,
    asset_id: i64,
    target_width: u32,
    target_height: u32,
) -> Vec<PathBuf> {
    let max_side = target_width.max(target_height);
    let mut paths = Vec::new();
    for lod in [
        CanvasImageLod::Working,
        CanvasImageLod::Overview,
        CanvasImageLod::Detail,
    ] {
        if let Some(path) = library.canvas_image_cache_path_for_lod(asset_id, max_side, lod) {
            paths.push(path);
        }
    }
    paths
}

fn resize_rgba_exact(
    rgba: Vec<u8>,
    width: u32,
    height: u32,
    target_width: u32,
    target_height: u32,
) -> Result<(Vec<u8>, u32, u32), String> {
    if width == target_width && height == target_height {
        return Ok((rgba, width, height));
    }

    let image = RgbaImage::from_raw(width, height, rgba)
        .ok_or_else(|| "failed to rebuild RGBA image for canvas pixel cache".to_owned())?;
    let resized = image::imageops::resize(
        &image,
        target_width,
        target_height,
        image::imageops::FilterType::Triangle,
    );
    Ok((resized.into_raw(), target_width, target_height))
}

fn original_mime_type_supports_animation(mime_type: &str) -> bool {
    mime_type == image::ImageFormat::Gif.to_mime_type()
        || mime_type == image::ImageFormat::WebP.to_mime_type()
}

const SVG_MIME_TYPE: &str = "image/svg+xml";

struct SvgDocument {
    bytes: Vec<u8>,
    width: u32,
    height: u32,
}

fn is_svg_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("svg"))
}

fn original_mime_type_is_svg(mime_type: &str) -> bool {
    mime_type
        .split(';')
        .next()
        .unwrap_or(mime_type)
        .trim()
        .eq_ignore_ascii_case(SVG_MIME_TYPE)
}

fn read_svg_document(path: &Path) -> Result<SvgDocument, String> {
    let bytes = std::fs::read(path).map_err(|error| format!("failed to open SVG: {error}"))?;
    parse_svg_document(bytes)
}

fn parse_svg_document(bytes: Vec<u8>) -> Result<SvgDocument, String> {
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
                Path::new(name)
                    .extension()
                    .and_then(|extension| extension.to_str())
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("svg"))
            })
}

fn stable_svg_uri_key(path: &Path) -> u64 {
    use std::hash::{DefaultHasher, Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    path.hash(&mut hasher);
    hasher.finish()
}

fn canvas_source_image_from_rgba(
    width: u32,
    height: u32,
    rgba: &[u8],
    canvas_image_settings: CanvasImageSettings,
) -> Result<PreviewImage, String> {
    preview_image_from_rgba(width, height, rgba, canvas_image_settings.max_side as usize)
}

fn fast_preview_image_from_rgba(
    width: u32,
    height: u32,
    rgba: &[u8],
    max_side: usize,
) -> Result<PreviewImage, String> {
    let (preview_width, preview_height) =
        crate::jobs::image_formats::fitted_texture_size(width as usize, height as usize, max_side);

    if preview_width == width as usize && preview_height == height as usize {
        return Ok(PreviewImage {
            rgba: rgba.to_vec(),
            width,
            height,
        });
    }

    let image = RgbaImage::from_raw(width, height, rgba.to_vec())
        .ok_or_else(|| "failed to rebuild RGBA image for crop preview resize".to_owned())?;
    let resized = image::imageops::resize(
        &image,
        preview_width as u32,
        preview_height as u32,
        image::imageops::FilterType::Nearest,
    );

    Ok(PreviewImage {
        rgba: resized.into_raw(),
        width: preview_width as u32,
        height: preview_height as u32,
    })
}

fn animated_preview_max_side(
    canvas_image_settings: CanvasImageSettings,
    max_texture_side: usize,
) -> usize {
    (canvas_image_settings.max_side as usize).min(max_texture_side)
}

fn backfill_canvas_image_cache(
    library: &Library,
    record: &BoardItemWithAsset,
    canvas_image_settings: CanvasImageSettings,
    lod: CanvasImageLod,
) -> Result<(PreviewImage, ExistingLoadDiagnostics), String> {
    let decode_path = preferred_backfill_source_path(library, record, canvas_image_settings, lod)
        .unwrap_or_else(|| record.asset.original_path.clone());
    if let Some(prepared) =
        prepare_import_image_with_helper(&decode_path, canvas_image_settings).unwrap_or(None)
    {
        let source_path = install_prepared_canvas_cache(
            library,
            record.asset.id,
            canvas_image_settings.max_side,
            lod,
            &prepared,
        )
        .unwrap_or_else(|| decode_path.clone());
        let source_file_size = std::fs::metadata(&source_path)
            .map(|metadata| metadata.len())
            .unwrap_or(0);
        return Ok((
            PreviewImage {
                rgba: prepared.rgba,
                width: prepared.width,
                height: prepared.height,
            },
            ExistingLoadDiagnostics {
                source_kind: ExistingLoadSourceKind::Cache,
                source_path,
                source_file_size,
                source_width: prepared.width,
                source_height: prepared.height,
                backfilled_cache: true,
            },
        ));
    }

    let decoded =
        decode_image_file_with_max_side(&decode_path, Some(canvas_image_settings.max_side))?;
    let canvas_source = canvas_source_image_from_rgba(
        decoded.width,
        decoded.height,
        &decoded.rgba,
        canvas_image_settings,
    )?;
    let source_path = library
        .store_canvas_image_cache_for_lod(
            record.asset.id,
            canvas_image_settings.max_side,
            lod,
            &canvas_source.rgba,
            canvas_source.width,
            canvas_source.height,
        )?
        .unwrap_or_else(|| record.asset.original_path.clone());
    let source_file_size = std::fs::metadata(&source_path)
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    let source_width = canvas_source.width;
    let source_height = canvas_source.height;
    Ok((
        canvas_source,
        ExistingLoadDiagnostics {
            source_kind: ExistingLoadSourceKind::Cache,
            source_path,
            source_file_size,
            source_width,
            source_height,
            backfilled_cache: true,
        },
    ))
}

fn preferred_backfill_source_path(
    library: &Library,
    record: &BoardItemWithAsset,
    canvas_image_settings: CanvasImageSettings,
    lod: CanvasImageLod,
) -> Option<PathBuf> {
    match lod {
        CanvasImageLod::Working => None,
        CanvasImageLod::Overview => library.canvas_image_cache_path_for_lod(
            record.asset.id,
            canvas_image_settings.max_side,
            CanvasImageLod::Working,
        ),
        CanvasImageLod::Detail => None,
    }
}

fn backfill_cache_lod(
    library: &Library,
    record: BoardItemWithAsset,
    canvas_image_settings: CanvasImageSettings,
    lod: CanvasImageLod,
) -> ImportResult {
    let lod_settings = lod_settings(canvas_image_settings, lod);
    match backfill_canvas_image_cache(library, &record, lod_settings, lod) {
        Ok(_) => ImportResult::CacheLodBackfilled {
            asset_id: record.asset.id,
            lod,
        },
        Err(error) => ImportResult::CacheLodBackfillFailed {
            asset_id: record.asset.id,
            lod,
            error,
        },
    }
}

pub fn initial_import_scale(
    asset_width: u32,
    asset_height: u32,
    auto_scale: bool,
    target_canvas_area: f32,
) -> f32 {
    if !auto_scale || asset_width == 0 || asset_height == 0 {
        return 1.0;
    }

    let target_canvas_area = target_canvas_area.max(1.0);
    let asset_area = asset_width as f32 * asset_height as f32;
    (target_canvas_area / asset_area).sqrt().min(1.0)
}

#[derive(Clone, Copy, Debug)]
struct LayoutRect {
    id: i64,
    rect: egui::Rect,
}

#[derive(Clone, Copy, Debug)]
struct PlacementCandidate {
    center: egui::Pos2,
    generation_index: usize,
}

#[derive(Clone, Copy, Debug)]
pub struct ArrangementCandidate {
    pub center: egui::Pos2,
}

pub fn arrange_rect_world(
    library: &Library,
    desired_world: egui::Pos2,
    viewport_world: egui::Rect,
    item_width: f32,
    item_height: f32,
    import_gap: f32,
) -> Result<egui::Pos2, String> {
    let existing_items = library.load_board_layout_items()?;
    Ok(arrange_rect_with_existing_layout(
        &existing_items,
        desired_world,
        viewport_world,
        item_width,
        item_height,
        import_gap,
    ))
}

fn resolve_import_world(
    library: &Library,
    target_board_id: i64,
    desired_world: egui::Pos2,
    reserved_world: Option<egui::Pos2>,
    viewport_world: egui::Rect,
    item_width: f32,
    item_height: f32,
    import_gap: f32,
) -> Result<egui::Pos2, String> {
    if let Some(reserved_world) = reserved_world {
        return Ok(reserved_world);
    }

    let existing_items = library.load_board_layout_items_for_board(target_board_id)?;
    Ok(arrange_rect_with_existing_layout(
        &existing_items,
        desired_world,
        viewport_world,
        item_width,
        item_height,
        import_gap,
    ))
}

pub fn arrange_rect_with_existing_layout(
    existing_items: &[BoardLayoutItem],
    desired_world: egui::Pos2,
    viewport_world: egui::Rect,
    item_width: f32,
    item_height: f32,
    import_gap: f32,
) -> egui::Pos2 {
    if existing_items.is_empty() {
        return desired_world;
    }

    best_arrangement_candidate_with_existing_layout(
        existing_items,
        desired_world,
        viewport_world,
        item_width,
        item_height,
        import_gap,
    )
    .map(|candidate| candidate.center)
    .unwrap_or(desired_world)
}

pub fn best_arrangement_candidate_with_existing_layout(
    existing_items: &[BoardLayoutItem],
    desired_world: egui::Pos2,
    viewport_world: egui::Rect,
    item_width: f32,
    item_height: f32,
    import_gap: f32,
) -> Option<ArrangementCandidate> {
    let new_rect = egui::Rect::from_center_size(
        desired_world,
        egui::vec2(item_width.max(0.0), item_height.max(0.0)),
    );

    if existing_items.is_empty() {
        return None;
    }

    let mut existing_rects = existing_items
        .iter()
        .map(displayed_layout_rect)
        .collect::<Vec<_>>();
    existing_rects.sort_by(|left, right| {
        left.rect
            .top()
            .total_cmp(&right.rect.top())
            .then_with(|| left.rect.left().total_cmp(&right.rect.left()))
            .then_with(|| left.id.cmp(&right.id))
    });

    let mut best_candidate: Option<(f32, f32, f32, usize, egui::Pos2)> = None;
    let mut generation_index = 0_usize;

    for existing in &existing_rects {
        for candidate_rect in [
            egui::Rect::from_min_size(
                egui::pos2(existing.rect.right() + import_gap, existing.rect.top()),
                new_rect.size(),
            ),
            egui::Rect::from_min_size(
                egui::pos2(existing.rect.left(), existing.rect.bottom() + import_gap),
                new_rect.size(),
            ),
        ] {
            let candidate = PlacementCandidate {
                center: candidate_rect.center(),
                generation_index,
            };
            generation_index += 1;

            if candidate_overlaps_any(candidate_rect, &existing_rects, import_gap) {
                continue;
            }

            let viewport_score = viewport_distance_score(candidate_rect.center(), viewport_world);
            let distance_score = candidate.center.distance_sq(desired_world);
            let squareness_score = occupied_bounds_squareness(&existing_rects, candidate_rect);
            let score = (
                viewport_score,
                distance_score,
                squareness_score,
                candidate.generation_index,
                candidate.center,
            );

            match best_candidate {
                Some((best_viewport, best_distance, best_squareness, best_index, _))
                    if viewport_score > best_viewport
                        || ((viewport_score - best_viewport).abs() < 0.001
                            && distance_score > best_distance)
                        || ((viewport_score - best_viewport).abs() < 0.001
                            && (distance_score - best_distance).abs() < 0.001
                            && squareness_score > best_squareness)
                        || ((viewport_score - best_viewport).abs() < 0.001
                            && (distance_score - best_distance).abs() < 0.001
                            && (squareness_score - best_squareness).abs() < 0.001
                            && candidate.generation_index >= best_index) => {}
                _ => best_candidate = Some(score),
            }
        }
    }

    best_candidate.map(|(_, _, _, _, center)| ArrangementCandidate { center })
}

fn viewport_distance_score(candidate_center: egui::Pos2, viewport_world: egui::Rect) -> f32 {
    let clamped = egui::pos2(
        candidate_center
            .x
            .clamp(viewport_world.left(), viewport_world.right()),
        candidate_center
            .y
            .clamp(viewport_world.top(), viewport_world.bottom()),
    );
    candidate_center.distance_sq(clamped)
}

fn displayed_layout_rect(record: &BoardLayoutItem) -> LayoutRect {
    let size = egui::vec2(record.width.max(0.0), record.height.max(0.0));
    LayoutRect {
        id: record.id,
        rect: egui::Rect::from_center_size(egui::pos2(record.x, record.y), size),
    }
}

fn candidate_overlaps_any(
    candidate_rect: egui::Rect,
    existing_rects: &[LayoutRect],
    gap: f32,
) -> bool {
    existing_rects
        .iter()
        .any(|existing| rects_overlap_with_gap(candidate_rect, existing.rect, gap))
}

fn rects_overlap_with_gap(left: egui::Rect, right: egui::Rect, gap: f32) -> bool {
    left.left() < right.right() + gap
        && left.right() > right.left() - gap
        && left.top() < right.bottom() + gap
        && left.bottom() > right.top() - gap
}

fn occupied_bounds_squareness(existing_rects: &[LayoutRect], candidate_rect: egui::Rect) -> f32 {
    let mut min_x = candidate_rect.left();
    let mut min_y = candidate_rect.top();
    let mut max_x = candidate_rect.right();
    let mut max_y = candidate_rect.bottom();

    for existing in existing_rects {
        min_x = min_x.min(existing.rect.left());
        min_y = min_y.min(existing.rect.top());
        max_x = max_x.max(existing.rect.right());
        max_y = max_y.max(existing.rect.bottom());
    }

    let width = (max_x - min_x).max(1.0);
    let height = (max_y - min_y).max(1.0);
    width.max(height) / width.min(height)
}

fn download_remote_image(url: &str) -> Result<DownloadedUrlImage, String> {
    let parsed = url::Url::parse(url).map_err(|error| format!("invalid image URL: {error}"))?;
    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        return Err("only http and https image URLs are supported".to_owned());
    }

    let client = reqwest::blocking::Client::builder()
        .connect_timeout(URL_IMPORT_CONNECT_TIMEOUT)
        .timeout(URL_IMPORT_REQUEST_TIMEOUT)
        .build()
        .map_err(|error| format!("failed to build HTTP client: {error}"))?;
    let mut response = client
        .get(parsed.clone())
        .send()
        .and_then(reqwest::blocking::Response::error_for_status)
        .map_err(|error| format!("failed to download image: {error}"))?;

    let content_length = response.content_length();
    if let Some(content_length) = content_length
        && content_length > URL_IMPORT_MAX_BYTES
    {
        return Err(format!(
            "remote image is too large ({content_length} bytes > {URL_IMPORT_MAX_BYTES} byte limit)"
        ));
    }

    let mime_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);

    let mut bytes = Vec::new();
    let limit = URL_IMPORT_MAX_BYTES + 1;
    std::io::Read::take(&mut response, limit)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("failed to read downloaded image: {error}"))?;
    if bytes.len() as u64 > URL_IMPORT_MAX_BYTES {
        return Err(format!(
            "remote image exceeded the {} byte download limit",
            URL_IMPORT_MAX_BYTES
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
mod tests {
    use super::{
        CanvasImageSettings, DEFAULT_IMPORT_TARGET_CANVAS_AREA, ImportResult, ImportTimings,
        ImportTrace, ImportWorker, ImportWorkerConfig, ImportedPreviewTexture, arrange_rect_world,
        build_preview_texture, crop_item, file_import_perf_report, import_clipboard,
        import_path_sync, import_url_sync_traced, initial_import_scale, load_existing,
        rects_overlap_with_gap,
    };
    use crate::data::CanvasImageLod;
    use crate::library::storage::Library;
    use image::{ColorType, ImageFormat};
    use pollster::block_on;
    use std::fs;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, mpsc};
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    fn downscaled_canvas_settings(max_side: u32) -> CanvasImageSettings {
        CanvasImageSettings { max_side }
    }

    #[test]
    fn scales_down_large_portrait_images_to_target_area() {
        let scale = initial_import_scale(1000, 1500, true, DEFAULT_IMPORT_TARGET_CANVAS_AREA);
        assert!(
            (scale - (DEFAULT_IMPORT_TARGET_CANVAS_AREA / 1_500_000.0).sqrt()).abs() < f32::EPSILON
        );
    }

    #[test]
    fn scales_down_large_landscape_images_by_area_cap() {
        let scale = initial_import_scale(1920, 1080, true, DEFAULT_IMPORT_TARGET_CANVAS_AREA);
        assert!(
            (scale - (DEFAULT_IMPORT_TARGET_CANVAS_AREA / 2_073_600.0).sqrt()).abs() < f32::EPSILON
        );
    }

    #[test]
    fn scales_down_large_images_to_configured_target_area() {
        let target_area = 40_000.0;
        let scale = initial_import_scale(1000, 1500, true, target_area);

        assert!((scale - (target_area / 1_500_000.0).sqrt()).abs() < f32::EPSILON);
    }

    #[test]
    fn keeps_smaller_images_at_full_scale() {
        assert_eq!(
            initial_import_scale(180, 240, true, DEFAULT_IMPORT_TARGET_CANVAS_AREA),
            1.0
        );
    }

    #[test]
    fn disabling_auto_scale_keeps_full_scale() {
        assert_eq!(
            initial_import_scale(1200, 2000, false, DEFAULT_IMPORT_TARGET_CANVAS_AREA),
            1.0
        );
    }

    #[test]
    fn file_imports_land_in_same_area_band() {
        let root = temp_library_root("file-import-scaling");
        let library = Library::initialize(root.clone()).unwrap();

        let portrait_path = write_png(&root, "portrait.png", 1000, 1500);
        let landscape_path = write_png(&root, "landscape.png", 1920, 1080);
        let square_path = write_png(&root, "square.png", 1000, 1000);
        let small_path = write_png(&root, "small.png", 180, 240);

        let portrait = import_path_sync(
            &library,
            portrait_path,
            eframe::egui::Pos2::ZERO,
            test_viewport_world(),
            true,
            crate::data::DEFAULT_IMPORT_GAP,
        )
        .unwrap();
        let landscape = import_path_sync(
            &library,
            landscape_path,
            eframe::egui::Pos2::ZERO,
            test_viewport_world(),
            true,
            crate::data::DEFAULT_IMPORT_GAP,
        )
        .unwrap();
        let square = import_path_sync(
            &library,
            square_path,
            eframe::egui::Pos2::ZERO,
            test_viewport_world(),
            true,
            crate::data::DEFAULT_IMPORT_GAP,
        )
        .unwrap();
        let small = import_path_sync(
            &library,
            small_path,
            eframe::egui::Pos2::ZERO,
            test_viewport_world(),
            true,
            crate::data::DEFAULT_IMPORT_GAP,
        )
        .unwrap();

        assert!((displayed_area(&portrait) - DEFAULT_IMPORT_TARGET_CANVAS_AREA).abs() < 0.1);
        assert!((displayed_area(&landscape) - DEFAULT_IMPORT_TARGET_CANVAS_AREA).abs() < 0.1);
        assert!((displayed_area(&square) - DEFAULT_IMPORT_TARGET_CANVAS_AREA).abs() < 0.1);
        assert_eq!(small.record.item.scale, 1.0);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn clipboard_import_respects_toggle_and_small_assets_stay_unchanged() {
        let root = temp_library_root("clipboard-import-scaling");
        let library = Library::initialize(root.clone()).unwrap();

        let tall_rgba = solid_rgba(390, 844);
        let small_rgba = solid_rgba(160, 200);

        let tall = match import_clipboard(
            &library,
            1,
            1,
            None,
            tall_rgba,
            390,
            844,
            eframe::egui::Pos2::ZERO,
            None,
            test_viewport_world(),
            usize::MAX,
            CanvasImageSettings::default(),
            true,
            DEFAULT_IMPORT_TARGET_CANVAS_AREA,
            crate::data::DEFAULT_IMPORT_GAP,
            None,
            None,
        ) {
            ImportResult::Loaded(loaded) => loaded,
            other => panic!("expected loaded tall clipboard image, got {other:?}"),
        };
        let small = match import_clipboard(
            &library,
            1,
            2,
            None,
            small_rgba,
            160,
            200,
            eframe::egui::Pos2::ZERO,
            None,
            test_viewport_world(),
            usize::MAX,
            CanvasImageSettings::default(),
            false,
            DEFAULT_IMPORT_TARGET_CANVAS_AREA,
            crate::data::DEFAULT_IMPORT_GAP,
            None,
            None,
        ) {
            ImportResult::Loaded(loaded) => loaded,
            other => panic!("expected loaded small clipboard image, got {other:?}"),
        };

        assert!((displayed_area(&tall) - DEFAULT_IMPORT_TARGET_CANVAS_AREA).abs() < 0.1);
        assert_eq!(small.record.item.scale, 1.0);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn clipboard_import_uses_configured_target_area() {
        let root = temp_library_root("clipboard-import-custom-area");
        let library = Library::initialize(root.clone()).unwrap();
        let target_area = 40_000.0;

        let loaded = match import_clipboard(
            &library,
            1,
            1,
            None,
            solid_rgba(1000, 1500),
            1000,
            1500,
            eframe::egui::Pos2::ZERO,
            None,
            test_viewport_world(),
            usize::MAX,
            CanvasImageSettings::default(),
            true,
            target_area,
            crate::data::DEFAULT_IMPORT_GAP,
            None,
            None,
        ) {
            ImportResult::Loaded(loaded) => loaded,
            other => panic!("expected loaded clipboard image, got {other:?}"),
        };

        assert!((displayed_area(&loaded) - target_area).abs() < 0.1);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn clipboard_import_persists_to_queued_board_even_when_another_board_is_active() {
        let root = temp_library_root("clipboard-import-target-board");
        let library = Library::initialize(root.clone()).unwrap();
        let first = library.list_boards().unwrap()[0].clone();
        seed_board_item(&library, 1, 42.0, 24.0, 160, 120, 1.0);
        let second = library.create_board("Second").unwrap();

        let loaded = match import_clipboard(
            &library,
            second.id,
            1,
            None,
            solid_rgba(160, 120),
            160,
            120,
            eframe::egui::pos2(42.0, 24.0),
            None,
            test_viewport_world(),
            usize::MAX,
            CanvasImageSettings::default(),
            false,
            DEFAULT_IMPORT_TARGET_CANVAS_AREA,
            crate::data::DEFAULT_IMPORT_GAP,
            None,
            None,
        ) {
            ImportResult::Loaded(loaded) => loaded,
            other => panic!("expected target-board clipboard import to load, got {other:?}"),
        };

        assert_eq!(loaded.target_board_id, second.id);
        assert_eq!(loaded.record.item.x, 42.0);
        assert_eq!(loaded.record.item.y, 24.0);
        assert_eq!(
            library.load_board_items_for_board(first.id).unwrap().len(),
            1
        );
        assert_eq!(
            library.load_board_items_for_board(second.id).unwrap().len(),
            1
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn empty_board_placement_uses_requested_anchor() {
        let root = temp_library_root("empty-placement");
        let library = Library::initialize(root.clone()).unwrap();
        let anchor = eframe::egui::pos2(120.0, -48.0);

        let arranged =
            arrange_rect_world(&library, anchor, test_viewport_world(), 300.0, 200.0, 12.0)
                .unwrap();

        assert_eq!(arranged, anchor);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn deterministic_candidate_order_prefers_right_before_below_on_tie() {
        let root = temp_library_root("placement-order");
        let library = Library::initialize(root.clone()).unwrap();
        seed_board_item(&library, 1, 0.0, 0.0, 100, 100, 1.0);

        let arranged = arrange_rect_world(
            &library,
            eframe::egui::pos2(106.0, 106.0),
            test_viewport_world(),
            100.0,
            100.0,
            12.0,
        )
        .unwrap();

        assert_eq!(arranged, eframe::egui::pos2(112.0, 0.0));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn blocked_right_side_candidate_falls_through_to_valid_lower_slot() {
        let root = temp_library_root("placement-blocked-right");
        let library = Library::initialize(root.clone()).unwrap();
        seed_board_item(&library, 1, 0.0, 0.0, 100, 100, 1.0);
        seed_board_item(&library, 2, 112.0, 0.0, 100, 100, 1.0);

        let arranged = arrange_rect_world(
            &library,
            eframe::egui::pos2(112.0, 50.0),
            test_viewport_world(),
            100.0,
            100.0,
            12.0,
        )
        .unwrap();

        assert_eq!(arranged, eframe::egui::pos2(112.0, 112.0));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn square_scoring_beats_anchor_distance_for_adjacent_imports() {
        let root = temp_library_root("placement-square-scoring");
        let library = Library::initialize(root.clone()).unwrap();
        seed_board_item(&library, 1, 0.0, 0.0, 240, 80, 1.0);

        let arranged = arrange_rect_world(
            &library,
            eframe::egui::pos2(172.0, 0.0),
            test_viewport_world(),
            80.0,
            80.0,
            12.0,
        )
        .unwrap();

        assert_eq!(arranged, eframe::egui::pos2(172.0, 0.0));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn viewport_bounding_prefers_on_screen_candidate() {
        let root = temp_library_root("placement-viewport-bounded");
        let library = Library::initialize(root.clone()).unwrap();
        seed_board_item(&library, 1, 0.0, 0.0, 240, 80, 1.0);
        let viewport = eframe::egui::Rect::from_min_max(
            eframe::egui::pos2(100.0, -60.0),
            eframe::egui::pos2(260.0, 60.0),
        );

        let arranged = arrange_rect_world(
            &library,
            eframe::egui::pos2(172.0, 0.0),
            viewport,
            80.0,
            80.0,
            12.0,
        )
        .unwrap();

        assert_eq!(arranged, eframe::egui::pos2(172.0, 0.0));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn sequential_imports_preserve_custom_gap_in_displayed_bounds() {
        let root = temp_library_root("placement-custom-gap");
        let library = Library::initialize(root.clone()).unwrap();
        let gap = 8.0;

        let first = match import_clipboard(
            &library,
            1,
            1,
            None,
            solid_rgba(200, 200),
            200,
            200,
            eframe::egui::Pos2::ZERO,
            None,
            test_viewport_world(),
            usize::MAX,
            CanvasImageSettings::default(),
            false,
            DEFAULT_IMPORT_TARGET_CANVAS_AREA,
            gap,
            None,
            None,
        ) {
            ImportResult::Loaded(loaded) => loaded,
            other => panic!("expected first loaded image, got {other:?}"),
        };

        let second = match import_clipboard(
            &library,
            1,
            2,
            None,
            solid_rgba(200, 200),
            200,
            200,
            eframe::egui::Pos2::ZERO,
            None,
            test_viewport_world(),
            usize::MAX,
            CanvasImageSettings::default(),
            false,
            DEFAULT_IMPORT_TARGET_CANVAS_AREA,
            gap,
            None,
            None,
        ) {
            ImportResult::Loaded(loaded) => loaded,
            other => panic!("expected second loaded image, got {other:?}"),
        };

        let first_rect = eframe::egui::Rect::from_center_size(
            eframe::egui::pos2(first.record.item.x, first.record.item.y),
            eframe::egui::vec2(200.0, 200.0),
        );
        let second_rect = eframe::egui::Rect::from_center_size(
            eframe::egui::pos2(second.record.item.x, second.record.item.y),
            eframe::egui::vec2(200.0, 200.0),
        );

        assert_eq!(second_rect.left() - first_rect.right(), gap);
        assert!(!rects_overlap_with_gap(first_rect, second_rect, gap));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn reserved_world_is_persisted_without_worker_relayout() {
        let root = temp_library_root("placement-reserved-world");
        let library = Library::initialize(root.clone()).unwrap();
        seed_board_item(&library, 1, 0.0, 0.0, 100, 100, 1.0);
        let reserved_world = eframe::egui::pos2(240.0, 36.0);

        let loaded = match import_clipboard(
            &library,
            1,
            2,
            None,
            solid_rgba(100, 100),
            100,
            100,
            eframe::egui::Pos2::ZERO,
            Some(reserved_world),
            test_viewport_world(),
            usize::MAX,
            CanvasImageSettings::default(),
            false,
            DEFAULT_IMPORT_TARGET_CANVAS_AREA,
            crate::data::DEFAULT_IMPORT_GAP,
            None,
            None,
        ) {
            ImportResult::Loaded(loaded) => loaded,
            other => panic!("expected reserved clipboard import to load, got {other:?}"),
        };

        assert_eq!(loaded.record.item.x, reserved_world.x);
        assert_eq!(loaded.record.item.y, reserved_world.y);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn overlapping_reserved_world_is_preserved_when_persisting() {
        let root = temp_library_root("placement-preserved-reserved-world");
        let library = Library::initialize(root.clone()).unwrap();
        seed_board_item(&library, 1, 0.0, 0.0, 100, 100, 1.0);
        let reserved_world = eframe::egui::Pos2::ZERO;

        let loaded = match import_clipboard(
            &library,
            1,
            2,
            None,
            solid_rgba(100, 100),
            100,
            100,
            reserved_world,
            Some(reserved_world),
            test_viewport_world(),
            usize::MAX,
            CanvasImageSettings::default(),
            false,
            DEFAULT_IMPORT_TARGET_CANVAS_AREA,
            crate::data::DEFAULT_IMPORT_GAP,
            None,
            None,
        ) {
            ImportResult::Loaded(loaded) => loaded,
            other => {
                panic!("expected overlapping reserved clipboard import to load, got {other:?}")
            }
        };

        assert_eq!(loaded.record.item.x, reserved_world.x);
        assert_eq!(loaded.record.item.y, reserved_world.y);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn url_import_downloads_and_persists_original_bytes() {
        let root = temp_library_root("url-import");
        fs::create_dir_all(&root).unwrap();
        let library = Library::initialize(root.clone()).unwrap();
        let png_bytes = solid_png_bytes(64, 48, [188, 146, 255, 255]);
        let (url, server) = serve_bytes_once("/test-image.png", "image/png", png_bytes.clone());

        let loaded = import_url_sync_traced(
            &library,
            1,
            Some(7),
            None,
            &url,
            eframe::egui::Pos2::ZERO,
            None,
            test_viewport_world(),
            usize::MAX,
            CanvasImageSettings::default(),
            false,
            DEFAULT_IMPORT_TARGET_CANVAS_AREA,
            crate::data::DEFAULT_IMPORT_GAP,
            None,
            None,
        )
        .unwrap();

        server.join().unwrap();

        assert_eq!(loaded.record.asset.mime_type, "image/png");
        assert_eq!(loaded.record.asset.width, 64);
        assert_eq!(loaded.record.asset.height, 48);
        assert_eq!(
            loaded
                .record
                .asset
                .original_path
                .extension()
                .and_then(|value| value.to_str()),
            Some("png")
        );
        assert!(
            loaded
                .record
                .asset
                .original_path
                .file_name()
                .and_then(|value| value.to_str())
                .is_some_and(|name| name.ends_with("test-image.png"))
        );
        assert_eq!(
            fs::read(&loaded.record.asset.original_path).unwrap(),
            png_bytes
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn url_import_preserves_svg_as_vector_source() {
        let root = temp_library_root("url-svg-import");
        fs::create_dir_all(&root).unwrap();
        let library = Library::initialize(root.clone()).unwrap();
        let svg_bytes = svg_bytes(128, 96);
        let (url, server) =
            serve_bytes_once("/remote-icon.svg", "image/svg+xml", svg_bytes.clone());

        let loaded = import_url_sync_traced(
            &library,
            1,
            Some(9),
            None,
            &url,
            eframe::egui::Pos2::ZERO,
            None,
            test_viewport_world(),
            usize::MAX,
            CanvasImageSettings::default(),
            false,
            DEFAULT_IMPORT_TARGET_CANVAS_AREA,
            crate::data::DEFAULT_IMPORT_GAP,
            None,
            None,
        )
        .unwrap();

        server.join().unwrap();

        assert_eq!(loaded.origin, super::ImportOrigin::Url);
        assert_eq!(loaded.record.asset.mime_type, "image/svg+xml");
        assert_eq!(loaded.record.asset.width, 128);
        assert_eq!(loaded.record.asset.height, 96);
        assert!(loaded.preview_rgba.is_empty());
        assert!(matches!(
            loaded.preview_texture,
            ImportedPreviewTexture::Svg { .. }
        ));
        assert_eq!(
            fs::read(&loaded.record.asset.original_path).unwrap(),
            svg_bytes
        );
        assert!(
            library
                .canvas_image_cache_path(loaded.record.asset.id, 2000)
                .is_none()
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn svg_import_preserves_vector_source_and_skips_canvas_cache() {
        let root = temp_library_root("svg-import");
        fs::create_dir_all(&root).unwrap();
        let library = Library::initialize(root.clone()).unwrap();
        let svg_path = write_svg(&root, "icon.svg", 320, 180);

        let loaded = import_path_sync(
            &library,
            svg_path.clone(),
            eframe::egui::Pos2::ZERO,
            test_viewport_world(),
            false,
            crate::data::DEFAULT_IMPORT_GAP,
        )
        .unwrap();

        assert_eq!(loaded.record.asset.mime_type, "image/svg+xml");
        assert_eq!(loaded.record.asset.width, 320);
        assert_eq!(loaded.record.asset.height, 180);
        assert!(loaded.preview_rgba.is_empty());
        assert!(matches!(
            loaded.preview_texture,
            ImportedPreviewTexture::Svg { .. }
        ));
        assert_eq!(
            fs::read_to_string(&loaded.record.asset.original_path).unwrap(),
            fs::read_to_string(&svg_path).unwrap()
        );
        assert!(
            library
                .canvas_image_cache_path(loaded.record.asset.id, 2000)
                .is_none()
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn xpm_import_uses_raster_pipeline_with_original_mime_type() {
        let root = temp_library_root("xpm-import");
        fs::create_dir_all(&root).unwrap();
        let library = Library::initialize(root.clone()).unwrap();
        let source = root.join("sample.xpm");
        fs::write(
            &source,
            r##"/* XPM */
static char * sample_xpm[] = {
"2 2 2 1",
"a c #ff0000",
". c None",
"a.",
".a"};"##,
        )
        .unwrap();

        let loaded = import_path_sync(
            &library,
            source.clone(),
            eframe::egui::Pos2::ZERO,
            test_viewport_world(),
            false,
            crate::data::DEFAULT_IMPORT_GAP,
        )
        .unwrap();

        assert_eq!(loaded.record.asset.mime_type, "image/x-xpixmap");
        assert_eq!(loaded.record.asset.width, 2);
        assert_eq!(loaded.record.asset.height, 2);
        assert_eq!(loaded.preview_rgba.len(), 2 * 2 * 4);
        assert_eq!(
            fs::read(&loaded.record.asset.original_path).unwrap(),
            fs::read(source).unwrap()
        );
        assert!(
            library
                .canvas_image_cache_path(loaded.record.asset.id, 2000)
                .is_some()
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn crop_item_rejects_svg_with_clear_error() {
        let root = temp_library_root("svg-crop");
        fs::create_dir_all(&root).unwrap();
        let library = Library::initialize(root.clone()).unwrap();
        let svg_path = write_svg(&root, "icon.svg", 100, 80);
        let loaded = import_path_sync(
            &library,
            svg_path,
            eframe::egui::Pos2::ZERO,
            test_viewport_world(),
            false,
            crate::data::DEFAULT_IMPORT_GAP,
        )
        .unwrap();

        let result = crop_item(
            &library,
            crate::canvas::CanvasCropRequest {
                item_id: loaded.record.item.id,
                source_asset: loaded.record.asset.clone(),
                crop_rect: crate::canvas::PixelCropRect {
                    x: 0,
                    y: 0,
                    width: 50,
                    height: 40,
                },
                crop_offset_local: eframe::egui::Vec2::ZERO,
            },
            usize::MAX,
            CanvasImageSettings::default(),
            None,
        );

        match result {
            ImportResult::Failed { item_id, error, .. } => {
                assert_eq!(item_id, Some(loaded.record.item.id));
                assert_eq!(error, "SVG cropping is not supported yet");
            }
            other => panic!("expected SVG crop failure, got {other:?}"),
        }

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn load_existing_svg_uses_original_vector_source() {
        let root = temp_library_root("svg-existing-load");
        fs::create_dir_all(&root).unwrap();
        let library = Library::initialize(root.clone()).unwrap();
        let svg_path = write_svg(&root, "icon.svg", 240, 120);
        let imported = import_path_sync(
            &library,
            svg_path,
            eframe::egui::Pos2::ZERO,
            test_viewport_world(),
            false,
            crate::data::DEFAULT_IMPORT_GAP,
        )
        .unwrap();

        let loaded = match load_existing(
            &library,
            1,
            None,
            imported.record.clone(),
            usize::MAX,
            CanvasImageSettings::default(),
            CanvasImageLod::Working,
            None,
            None,
        ) {
            ImportResult::Loaded(loaded) => loaded,
            other => panic!("expected SVG existing load, got {other:?}"),
        };

        assert_eq!(loaded.origin, super::ImportOrigin::Existing);
        assert_eq!(loaded.preview_width, 240);
        assert_eq!(loaded.preview_height, 120);
        assert!(loaded.preview_rgba.is_empty());
        assert!(matches!(
            loaded.preview_texture,
            ImportedPreviewTexture::Svg { .. }
        ));
        assert!(
            library
                .canvas_image_cache_path(loaded.record.asset.id, 2000)
                .is_none()
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn interactive_import_queue_pops_smallest_bytes_first() {
        let root = temp_library_root("interactive-queue-by-bytes");
        fs::create_dir_all(&root).unwrap();
        // Write three real PNGs of different byte sizes so metadata-based
        // priority can be derived from the filesystem at enqueue time.
        let small_path = write_png(&root, "small.png", 32, 32);
        let medium_path = write_png(&root, "medium.png", 128, 128);
        let large_path = write_png(&root, "large.png", 512, 512);
        let small_bytes = fs::metadata(&small_path).unwrap().len();
        let medium_bytes = fs::metadata(&medium_path).unwrap().len();
        let large_bytes = fs::metadata(&large_path).unwrap().len();
        assert!(
            small_bytes < medium_bytes && medium_bytes < large_bytes,
            "expected strictly ascending file sizes, got {small_bytes}/{medium_bytes}/{large_bytes}"
        );

        let queue = super::RequestQueue::default();
        // Enqueue in worst-case "large first" order so a FIFO would return the
        // large file first; the priority queue must invert this.
        for path in [&large_path, &medium_path, &small_path] {
            queue
                .push(make_import_path_request(path.clone()))
                .expect("queue push");
        }

        let first = queue
            .try_pop_one(super::WorkerKind::Interactive)
            .expect("first pop");
        let second = queue
            .try_pop_one(super::WorkerKind::Interactive)
            .expect("second pop");
        let third = queue
            .try_pop_one(super::WorkerKind::Interactive)
            .expect("third pop");

        assert_eq!(import_path_source(&first), small_path);
        assert_eq!(import_path_source(&second), medium_path);
        assert_eq!(import_path_source(&third), large_path);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn interactive_import_queue_breaks_ties_by_arrival_order() {
        let root = temp_library_root("interactive-queue-tie-break");
        fs::create_dir_all(&root).unwrap();
        // Two identical 64x64 PNGs produce identical byte counts; FIFO order
        // must be preserved when the sort key ties.
        let first_path = write_png(&root, "first.png", 64, 64);
        let second_path = write_png(&root, "second.png", 64, 64);
        let third_path = write_png(&root, "third.png", 64, 64);
        let a = fs::metadata(&first_path).unwrap().len();
        let b = fs::metadata(&second_path).unwrap().len();
        let c = fs::metadata(&third_path).unwrap().len();
        assert_eq!(a, b);
        assert_eq!(b, c);

        let queue = super::RequestQueue::default();
        for path in [&first_path, &second_path, &third_path] {
            queue
                .push(make_import_path_request(path.clone()))
                .expect("queue push");
        }

        let popped: Vec<PathBuf> = (0..3)
            .map(|_| {
                import_path_source(
                    &queue
                        .try_pop_one(super::WorkerKind::Interactive)
                        .expect("pop"),
                )
            })
            .collect();
        assert_eq!(popped, vec![first_path, second_path, third_path]);

        let _ = fs::remove_dir_all(root);
    }

    fn make_import_path_request(source_path: PathBuf) -> super::ImportRequest {
        super::ImportRequest::ImportPath {
            target_board_id: 1,
            queue_id: 0,
            placement_id: None,
            source_path,
            desired_world: eframe::egui::Pos2::ZERO,
            reserved_world: None,
            viewport_world: test_viewport_world(),
            max_texture_side: usize::MAX,
            canvas_image_settings: CanvasImageSettings::default(),
            auto_scale: true,
            target_canvas_area: DEFAULT_IMPORT_TARGET_CANVAS_AREA,
            import_gap: crate::data::DEFAULT_IMPORT_GAP,
            trace: None,
        }
    }

    fn import_path_source(request: &super::ImportRequest) -> PathBuf {
        match request {
            super::ImportRequest::ImportPath { source_path, .. } => source_path.clone(),
            other => panic!("expected ImportPath, got {other:?}"),
        }
    }

    #[test]
    fn import_worker_drains_ready_results_in_capped_batches() {
        let (result_tx, result_rx) = mpsc::channel();
        let worker = ImportWorker {
            queue: Arc::new(super::RequestQueue::default()),
            receiver: result_rx,
            workers: Vec::new(),
        };

        for index in 0..5 {
            result_tx
                .send(ImportResult::Failed {
                    target_board_id: None,
                    queue_id: None,
                    placement_id: None,
                    item_id: Some(index),
                    context: format!("item-{index}"),
                    error: "synthetic".to_owned(),
                })
                .unwrap();
        }

        let first_batch = worker.try_recv_up_to(4);
        let second_batch = worker.try_recv_up_to(4);

        assert_eq!(first_batch.len(), 4);
        assert_eq!(second_batch.len(), 1);
    }

    #[test]
    fn worker_config_derives_general_fallback_for_single_core() {
        assert_eq!(
            ImportWorkerConfig::for_parallelism(1),
            ImportWorkerConfig {
                general_workers: 1,
                interactive_workers: 0,
                background_workers: 0,
            }
        );
    }

    #[test]
    fn worker_config_reserves_background_worker_on_multi_core() {
        assert_eq!(
            ImportWorkerConfig::for_parallelism(4),
            ImportWorkerConfig {
                general_workers: 0,
                interactive_workers: 3,
                background_workers: 1,
            }
        );
    }

    #[test]
    fn traced_clipboard_import_populates_non_negative_timings() {
        let root = temp_library_root("clipboard-trace");
        let library = Library::initialize(root.clone()).unwrap();
        let trace = ImportTrace {
            queued_at: Instant::now()
                .checked_sub(Duration::from_millis(1))
                .unwrap_or_else(Instant::now),
            timings: ImportTimings::default(),
        };

        let loaded = match import_clipboard(
            &library,
            1,
            1,
            None,
            solid_rgba(64, 64),
            64,
            64,
            eframe::egui::Pos2::ZERO,
            None,
            test_viewport_world(),
            usize::MAX,
            CanvasImageSettings::default(),
            false,
            DEFAULT_IMPORT_TARGET_CANVAS_AREA,
            crate::data::DEFAULT_IMPORT_GAP,
            Some(trace),
            None,
        ) {
            ImportResult::Loaded(loaded) => loaded,
            other => panic!("expected traced clipboard import to load, got {other:?}"),
        };

        let trace = loaded
            .trace
            .expect("missing trace on loaded clipboard import");
        assert!(trace.timings.queue_wait >= Duration::ZERO);
        assert!(trace.timings.decode_or_load >= Duration::ZERO);
        assert!(trace.timings.store_and_persist >= Duration::ZERO);
        assert!(trace.timings.preview_prepare >= Duration::ZERO);
        assert!(trace.timings.worker_total >= trace.timings.store_and_persist);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn file_import_perf_report_imports_jpeg_and_reports_timings() {
        let root = temp_library_root("file-import-perf-report");
        fs::create_dir_all(&root).unwrap();
        let source_path = write_jpeg(&root, "source.jpg", 64, 48);

        let report = file_import_perf_report(source_path.clone()).unwrap();

        assert_eq!(report.source_path, source_path);
        assert_eq!(report.source_width, 64);
        assert_eq!(report.source_height, 48);
        assert_eq!(report.preview_width, 64);
        assert_eq!(report.preview_height, 48);
        assert!(report.source_file_size > 0);
        assert!(report.canvas_cache_file_size.unwrap_or_default() > 0);
        assert!(report.timings.decode_or_load >= Duration::ZERO);
        assert!(report.timings.store_and_persist >= Duration::ZERO);
        assert!(report.timings.preview_prepare >= Duration::ZERO);
        assert!(report.timings.worker_total >= report.timings.decode_or_load);
        assert!(report.total_ms >= 0.0);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn native_preview_texture_stays_registered_until_last_reference_drops() {
        let Some(render_state) = test_render_state() else {
            eprintln!("skipping native preview texture lifecycle test: no wgpu adapter");
            return;
        };
        let preview = crate::jobs::image_formats::preview_image_from_rgba(
            8,
            8,
            &solid_rgba(8, 8),
            usize::MAX,
        )
        .unwrap();

        let handle = match build_preview_texture(Some(&render_state), &preview).unwrap() {
            ImportedPreviewTexture::Native(handle) => handle,
            ImportedPreviewTexture::Cpu => panic!("expected native preview texture"),
            ImportedPreviewTexture::Svg { .. } => panic!("expected native preview texture"),
        };
        let texture_id = handle.texture_id();
        assert!(render_state.renderer.read().texture(&texture_id).is_some());

        let duplicate = handle.clone();
        drop(handle);
        assert!(render_state.renderer.read().texture(&texture_id).is_some());

        std::thread::spawn(move || drop(duplicate)).join().unwrap();
        assert!(render_state.renderer.read().texture(&texture_id).is_none());
    }

    #[test]
    fn load_existing_backfills_versioned_canvas_cache_and_prefers_it_on_second_load() {
        let root = temp_library_root("load-existing-canvas-cache");
        let library = Library::initialize(root.clone()).unwrap();
        let original_path = write_png(&root, "source.png", 4000, 3000);
        let file_size = fs::metadata(&original_path).unwrap().len();
        let record = library
            .persist_imported_image(
                &original_path,
                "image/png",
                4000,
                3000,
                file_size,
                eframe::egui::Pos2::ZERO,
                1.0,
            )
            .unwrap();

        let settings = downscaled_canvas_settings(2000);
        assert!(
            library
                .canvas_image_cache_path(record.asset.id, settings.max_side)
                .is_none()
        );

        let first = match load_existing(
            &library,
            1,
            None,
            record.clone(),
            usize::MAX,
            settings,
            CanvasImageLod::Working,
            None,
            None,
        ) {
            ImportResult::Loaded(loaded) => loaded,
            other => panic!("expected first load_existing to succeed, got {other:?}"),
        };
        let cache_path = library
            .canvas_image_cache_path(record.asset.id, settings.max_side)
            .unwrap();
        assert!(cache_path.is_file());
        assert_eq!(first.preview_width, 2000);
        assert_eq!(first.preview_height, 1500);

        fs::remove_file(&record.asset.original_path).unwrap();

        let second = match load_existing(
            &library,
            1,
            None,
            record.clone(),
            usize::MAX,
            settings,
            CanvasImageLod::Working,
            None,
            None,
        ) {
            ImportResult::Loaded(loaded) => loaded,
            other => {
                panic!("expected second load_existing to use versioned canvas cache, got {other:?}")
            }
        };
        assert_eq!(second.preview_width, 2000);
        assert_eq!(second.preview_height, 1500);

        let cached = crate::jobs::image_formats::decode_image_file(&cache_path).unwrap();
        assert_eq!(cached.width, 2000);
        assert_eq!(cached.height, 1500);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn load_existing_preserves_animated_webp_in_downscale_mode() {
        let root = temp_library_root("load-existing-animated-webp");
        fs::create_dir_all(&root).unwrap();
        let library = Library::initialize(root.clone()).unwrap();
        let original_path = root.join("source.webp");
        fs::write(&original_path, ANIMATED_WEBP_2X2_2_FRAMES).unwrap();
        let file_size = fs::metadata(&original_path).unwrap().len();
        let record = library
            .persist_imported_image(
                &original_path,
                image::ImageFormat::WebP.to_mime_type(),
                2,
                2,
                file_size,
                eframe::egui::Pos2::ZERO,
                1.0,
            )
            .unwrap();

        let loaded = match load_existing(
            &library,
            1,
            None,
            record,
            usize::MAX,
            downscaled_canvas_settings(2000),
            CanvasImageLod::Working,
            None,
            None,
        ) {
            ImportResult::Loaded(loaded) => loaded,
            other => panic!("expected animated WebP load_existing to succeed, got {other:?}"),
        };

        let animation = loaded
            .animated_preview
            .expect("expected animated WebP preview");
        assert_eq!(animation.frames.len(), 2);
        assert_eq!(animation.width, 2);
        assert_eq!(animation.height, 2);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn load_existing_backfills_new_cache_when_canvas_max_side_changes() {
        let root = temp_library_root("load-existing-canvas-cache-variants");
        let library = Library::initialize(root.clone()).unwrap();
        let original_path = write_png(&root, "source.png", 4000, 3000);
        let file_size = fs::metadata(&original_path).unwrap().len();
        let record = library
            .persist_imported_image(
                &original_path,
                "image/png",
                4000,
                3000,
                file_size,
                eframe::egui::Pos2::ZERO,
                1.0,
            )
            .unwrap();

        let first_settings = downscaled_canvas_settings(2000);
        let second_settings = downscaled_canvas_settings(1000);
        assert!(
            library
                .canvas_image_cache_path(record.asset.id, first_settings.max_side)
                .is_none()
        );
        assert!(
            library
                .canvas_image_cache_path(record.asset.id, second_settings.max_side)
                .is_none()
        );

        match load_existing(
            &library,
            1,
            None,
            record.clone(),
            usize::MAX,
            first_settings,
            CanvasImageLod::Working,
            None,
            None,
        ) {
            ImportResult::Loaded(_) => {}
            other => panic!("expected first cache backfill to succeed, got {other:?}"),
        }
        let first_cache = library
            .canvas_image_cache_path(record.asset.id, first_settings.max_side)
            .unwrap();
        assert!(first_cache.is_file());
        assert!(
            library
                .canvas_image_cache_path(record.asset.id, second_settings.max_side)
                .is_none()
        );

        let second = match load_existing(
            &library,
            1,
            None,
            record.clone(),
            usize::MAX,
            second_settings,
            CanvasImageLod::Working,
            None,
            None,
        ) {
            ImportResult::Loaded(loaded) => loaded,
            other => panic!("expected second cache backfill to succeed, got {other:?}"),
        };
        assert_eq!(second.preview_width, 1000);
        assert_eq!(second.preview_height, 750);
        let second_cache = library
            .canvas_image_cache_path(record.asset.id, second_settings.max_side)
            .unwrap();
        assert_ne!(first_cache, second_cache);
        assert!(second_cache.is_file());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn downscale_mode_ignores_legacy_thumb_cache_paths() {
        let root = temp_library_root("load-existing-legacy-thumb");
        let library = Library::initialize(root.clone()).unwrap();
        let original_path = write_png(&root, "source.png", 4000, 3000);
        let file_size = fs::metadata(&original_path).unwrap().len();
        let record = library
            .persist_imported_image(
                &original_path,
                "image/png",
                4000,
                3000,
                file_size,
                eframe::egui::Pos2::ZERO,
                1.0,
            )
            .unwrap();

        let legacy_thumb_path = library.asset_thumb_path(record.asset.id);
        image::save_buffer_with_format(
            &legacy_thumb_path,
            &solid_rgba(64, 64),
            64,
            64,
            ColorType::Rgba8,
            ImageFormat::Png,
        )
        .unwrap();
        fs::remove_file(&record.asset.original_path).unwrap();

        match load_existing(
            &library,
            1,
            None,
            record,
            usize::MAX,
            downscaled_canvas_settings(2000),
            CanvasImageLod::Working,
            None,
            None,
        ) {
            ImportResult::Failed { .. } => {}
            other => panic!("expected legacy thumb path to be ignored, got {other:?}"),
        }

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn crop_redecodes_original_path_and_preserves_exact_pixels() {
        let root = temp_library_root("crop-redecode");
        let library = Library::initialize(root.clone()).unwrap();
        let source_rgba = vec![
            1, 2, 3, 255, 4, 5, 6, 255, 7, 8, 9, 255, 10, 11, 12, 255, 13, 14, 15, 255, 16, 17, 18,
            255, 19, 20, 21, 255, 22, 23, 24, 255, 25, 26, 27, 255, 28, 29, 30, 255, 31, 32, 33,
            255, 34, 35, 36, 255, 37, 38, 39, 255, 40, 41, 42, 255, 43, 44, 45, 255, 46, 47, 48,
            255,
        ];
        let stored_path = library
            .store_rgba_image("crop-source", &source_rgba, 4, 4)
            .unwrap();
        let file_size = fs::metadata(&stored_path).unwrap().len();
        let request = crate::canvas::CanvasCropRequest {
            item_id: 1,
            source_asset: crate::data::ImageAsset {
                id: 1,
                original_path: stored_path.clone(),
                filename: "crop-source.png".to_owned(),
                mime_type: "image/png".to_owned(),
                width: 4,
                height: 4,
                file_size,
                imported_at: 0,
            },
            crop_rect: crate::canvas::PixelCropRect {
                x: 1,
                y: 1,
                width: 2,
                height: 2,
            },
            crop_offset_local: eframe::egui::Vec2::ZERO,
        };

        let cropped = match crop_item(
            &library,
            request,
            usize::MAX,
            CanvasImageSettings::default(),
            None,
        ) {
            ImportResult::Cropped(cropped) => cropped,
            other => panic!("expected cropped result, got {other:?}"),
        };
        let decoded = crate::jobs::image_formats::decode_image_file(&cropped.stored_path).unwrap();

        assert_eq!(decoded.width, 2);
        assert_eq!(decoded.height, 2);
        assert_eq!(
            decoded.rgba,
            vec![
                16, 17, 18, 255, 19, 20, 21, 255, 28, 29, 30, 255, 31, 32, 33, 255,
            ]
        );

        let _ = fs::remove_dir_all(root);
    }

    fn temp_library_root(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("snappyref-{label}-{unique}"))
    }

    fn write_png(root: &Path, filename: &str, width: u32, height: u32) -> PathBuf {
        let path = root.join(filename);
        let rgba = solid_rgba(width, height);
        image::save_buffer_with_format(
            &path,
            &rgba,
            width,
            height,
            ColorType::Rgba8,
            ImageFormat::Png,
        )
        .unwrap();
        path
    }

    fn write_jpeg(root: &Path, filename: &str, width: u32, height: u32) -> PathBuf {
        let path = root.join(filename);
        let rgb = vec![255; width as usize * height as usize * 3];
        image::save_buffer_with_format(
            &path,
            &rgb,
            width,
            height,
            ColorType::Rgb8,
            ImageFormat::Jpeg,
        )
        .unwrap();
        path
    }

    fn write_svg(root: &Path, filename: &str, width: u32, height: u32) -> PathBuf {
        let path = root.join(filename);
        fs::write(&path, svg_bytes(width, height)).unwrap();
        path
    }

    fn svg_bytes(width: u32, height: u32) -> Vec<u8> {
        format!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}"><rect width="{width}" height="{height}" fill="#f8fafc"/><circle cx="80" cy="48" r="32" fill="#2563eb"/></svg>"##
        )
        .into_bytes()
    }

    fn solid_rgba(width: u32, height: u32) -> Vec<u8> {
        vec![255; width as usize * height as usize * 4]
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

    fn solid_png_bytes(width: u32, height: u32, color: [u8; 4]) -> Vec<u8> {
        let rgba = vec![color; (width * height) as usize]
            .into_iter()
            .flat_map(|pixel| pixel)
            .collect::<Vec<_>>();
        let image = image::RgbaImage::from_raw(width, height, rgba).unwrap();
        let mut bytes = Vec::new();
        image
            .write_to(&mut std::io::Cursor::new(&mut bytes), ImageFormat::Png)
            .unwrap();
        bytes
    }

    fn displayed_area(loaded: &super::LoadedCanvasItem) -> f32 {
        loaded.record.asset.width as f32
            * loaded.record.asset.height as f32
            * loaded.record.item.scale
            * loaded.record.item.scale
    }

    fn test_viewport_world() -> eframe::egui::Rect {
        eframe::egui::Rect::from_min_max(
            eframe::egui::pos2(-10_000.0, -10_000.0),
            eframe::egui::pos2(10_000.0, 10_000.0),
        )
    }

    fn seed_board_item(
        library: &Library,
        id: i64,
        x: f32,
        y: f32,
        width: u32,
        height: u32,
        scale: f32,
    ) {
        let stored_path = library
            .store_rgba_image(
                &format!("seed-{id}"),
                &solid_rgba(width, height),
                width,
                height,
            )
            .unwrap();
        let file_size = fs::metadata(&stored_path).unwrap().len();
        let record = library
            .persist_imported_image(
                &stored_path,
                image::ImageFormat::Png.to_mime_type(),
                width,
                height,
                file_size,
                eframe::egui::pos2(x, y),
                scale,
            )
            .unwrap();
        assert_eq!(record.item.id, id);
    }

    fn test_render_state() -> Option<eframe::egui_wgpu::RenderState> {
        let instance = eframe::wgpu::Instance::new(
            eframe::wgpu::InstanceDescriptor::new_without_display_handle(),
        );
        block_on(eframe::egui_wgpu::RenderState::create(
            &eframe::egui_wgpu::WgpuConfiguration::default(),
            &instance,
            None,
            eframe::egui_wgpu::RendererOptions::default(),
        ))
        .ok()
    }

    fn serve_bytes_once(
        path: &str,
        content_type: &str,
        body: Vec<u8>,
    ) -> (String, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let address = listener.local_addr().unwrap();
        let path = path.to_owned();
        let request_path = path.clone();
        let content_type = content_type.to_owned();
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0_u8; 1024];
            let bytes_read = stream.read(&mut request).unwrap();
            let request_text = String::from_utf8_lossy(&request[..bytes_read]);
            assert!(request_text.starts_with(&format!("GET {request_path} ")));

            let headers = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: {}\r\nConnection: close\r\n\r\n",
                body.len(),
                content_type
            );
            stream.write_all(headers.as_bytes()).unwrap();
            stream.write_all(&body).unwrap();
            stream.flush().unwrap();
        });
        (format!("http://{address}{path}"), handle)
    }
}
