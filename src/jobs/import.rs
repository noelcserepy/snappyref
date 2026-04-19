use std::collections::VecDeque;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use eframe::egui;
use image::RgbaImage;

use crate::data::{BoardItemWithAsset, BoardLayoutItem, CanvasImageCacheTier};
use crate::jobs::image_formats::{PreviewImage, decode_image_file, preview_image_from_rgba};
use crate::storage::Library;

pub const DEFAULT_IMPORT_TARGET_CANVAS_AREA: f32 = 90_000.0;

const PERF_STARTUP_IMAGE_SIZE: u32 = 1536;
const PERF_BACKLOG_IMAGE_SIZE: u32 = 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImportOrigin {
    Existing,
    Path,
    Clipboard,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CanvasImageSettings {
    pub mode: crate::data::CanvasImageMode,
    pub max_side: u32,
}

impl Default for CanvasImageSettings {
    fn default() -> Self {
        Self {
            mode: crate::data::CanvasImageMode::default(),
            max_side: crate::data::DEFAULT_CANVAS_IMAGE_MAX_SIDE,
        }
    }
}

const OVERVIEW_CACHE_MAX_SIDE: u32 = 256;
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
        source_path: PathBuf,
        desired_world: egui::Pos2,
        viewport_world: egui::Rect,
        max_texture_side: usize,
        canvas_image_settings: CanvasImageSettings,
        auto_scale: bool,
        import_gap: f32,
        trace: Option<ImportTrace>,
    },
    ImportClipboard {
        rgba: Vec<u8>,
        width: u32,
        height: u32,
        desired_world: egui::Pos2,
        viewport_world: egui::Rect,
        max_texture_side: usize,
        canvas_image_settings: CanvasImageSettings,
        auto_scale: bool,
        import_gap: f32,
        trace: Option<ImportTrace>,
    },
    Crop {
        request: crate::canvas::CanvasCropRequest,
        max_texture_side: usize,
        canvas_image_settings: CanvasImageSettings,
    },
    LoadExisting {
        record: BoardItemWithAsset,
        max_texture_side: usize,
        canvas_image_settings: CanvasImageSettings,
        target_tier: CanvasImageCacheTier,
        priority: ExistingLoadPriority,
        trace: Option<ImportTrace>,
    },
    BackfillTier {
        record: BoardItemWithAsset,
        canvas_image_settings: CanvasImageSettings,
        tier: CanvasImageCacheTier,
    },
}

#[derive(Debug)]
pub enum ImportResult {
    Loaded(LoadedCanvasItem),
    Cropped(CroppedCanvasItem),
    CacheTierBackfilled {
        asset_id: i64,
        tier: CanvasImageCacheTier,
    },
    CacheTierBackfillFailed {
        asset_id: i64,
        tier: CanvasImageCacheTier,
        error: String,
    },
    Failed {
        item_id: Option<i64>,
        context: String,
        error: String,
    },
}

#[derive(Clone)]
pub enum ImportedPreviewTexture {
    Cpu,
    Native(Arc<NativeTextureHandle>),
}

impl fmt::Debug for ImportedPreviewTexture {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cpu => f.write_str("Cpu"),
            Self::Native(handle) => f.debug_tuple("Native").field(handle).finish(),
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
        Some(device.create_bind_group(&eframe::wgpu::BindGroupDescriptor {
            label: Some("reference_canvas_image_bind_group"),
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
        }))
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
    pub record: BoardItemWithAsset,
    pub preview_rgba: Vec<u8>,
    pub preview_width: u32,
    pub preview_height: u32,
    pub preview_texture: ImportedPreviewTexture,
    pub loaded_tier: CanvasImageCacheTier,
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
    pub canvas_rgba: Vec<u8>,
    pub canvas_width: u32,
    pub canvas_height: u32,
    pub preview_rgba: Vec<u8>,
    pub preview_width: u32,
    pub preview_height: u32,
    pub preview_texture: ImportedPreviewTexture,
    pub loaded_tier: CanvasImageCacheTier,
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
                            source_path,
                            desired_world,
                            viewport_world,
                            max_texture_side,
                            canvas_image_settings,
                            auto_scale,
                            import_gap,
                            trace,
                        } => import_path(
                            &worker_library,
                            source_path,
                            desired_world,
                            viewport_world,
                            max_texture_side,
                            canvas_image_settings,
                            auto_scale,
                            import_gap,
                            trace,
                            worker_render_state.as_ref(),
                        ),
                        ImportRequest::ImportClipboard {
                            rgba,
                            width,
                            height,
                            desired_world,
                            viewport_world,
                            max_texture_side,
                            canvas_image_settings,
                            auto_scale,
                            import_gap,
                            trace,
                        } => import_clipboard(
                            &worker_library,
                            rgba,
                            width,
                            height,
                            desired_world,
                            viewport_world,
                            max_texture_side,
                            canvas_image_settings,
                            auto_scale,
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
                            record,
                            max_texture_side,
                            canvas_image_settings,
                            target_tier,
                            priority: _,
                            trace,
                        } => load_existing(
                            &worker_library,
                            record,
                            max_texture_side,
                            canvas_image_settings,
                            target_tier,
                            trace,
                            worker_render_state.as_ref(),
                        ),
                        ImportRequest::BackfillTier {
                            record,
                            canvas_image_settings,
                            tier,
                        } => backfill_cache_tier(
                            &worker_library,
                            record,
                            canvas_image_settings,
                            tier,
                        ),
                    };

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
            ImportPriority::Interactive => state.interactive.push_back(request),
            ImportPriority::Background => state.background.push_back(request),
        }
        self.ready.notify_all();
        Ok(())
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
                        return Some(request);
                    }
                    if let Some(request) = state.startup.pop_front() {
                        return Some(request);
                    }
                    if let Some(request) = state.background.pop_front() {
                        return Some(request);
                    }
                }
                WorkerKind::Interactive => {
                    if let Some(request) = state.interactive.pop_front() {
                        return Some(request);
                    }
                    if let Some(request) = state.startup.pop_front() {
                        return Some(request);
                    }
                }
                WorkerKind::Background => {
                    if let Some(request) = state.startup.pop_front() {
                        return Some(request);
                    }
                    if let Some(request) = state.background.pop_front() {
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
            ImportRequest::BackfillTier { .. } => ImportPriority::Background,
            ImportRequest::ImportPath { .. }
            | ImportRequest::ImportClipboard { .. }
            | ImportRequest::Crop { .. } => ImportPriority::Interactive,
        }
    }
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
    import_path_sync_traced(
        library,
        source_path,
        desired_world,
        viewport_world,
        usize::MAX,
        CanvasImageSettings::default(),
        auto_scale,
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
    import_clipboard_sync_traced(
        library,
        rgba,
        width,
        height,
        desired_world,
        viewport_world,
        usize::MAX,
        CanvasImageSettings::default(),
        auto_scale,
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
            record.clone(),
            usize::MAX,
            canvas_image_settings,
            CanvasImageCacheTier::Working,
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
            | ImportResult::CacheTierBackfilled { .. }
            | ImportResult::CacheTierBackfillFailed { .. } => {}
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

fn import_path_sync_traced(
    library: &Library,
    source_path: PathBuf,
    desired_world: egui::Pos2,
    viewport_world: egui::Rect,
    max_texture_side: usize,
    canvas_image_settings: CanvasImageSettings,
    auto_scale: bool,
    import_gap: f32,
    trace: Option<ImportTrace>,
    wgpu_render_state: Option<&eframe::egui_wgpu::RenderState>,
) -> Result<LoadedCanvasItem, String> {
    let worker_started_at = Instant::now();
    let mut trace = trace;
    if let Some(trace) = &mut trace {
        trace.timings.queue_wait = worker_started_at.saturating_duration_since(trace.queued_at);
    }

    let decode_started_at = Instant::now();
    let decoded = decode_image_file(&source_path)
        .map_err(|error| format!("{}: {error}", source_path.display()))?;
    if let Some(trace) = &mut trace {
        trace.timings.decode_or_load = decode_started_at.elapsed();
    }
    let initial_scale = initial_import_scale(decoded.width, decoded.height, auto_scale);
    let arranged_world = arrange_rect_world(
        library,
        desired_world,
        viewport_world,
        decoded.width as f32 * initial_scale.max(0.01),
        decoded.height as f32 * initial_scale.max(0.01),
        import_gap,
    )?;

    let store_started_at = Instant::now();
    let stored_path = library
        .store_original(&source_path)
        .map_err(|error| format!("{}: {error}", source_path.display()))?;

    let record = library
        .persist_imported_image(
            &stored_path,
            decoded.format.to_mime_type(),
            decoded.width,
            decoded.height,
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
        canvas_image_settings.mode,
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
        record,
        preview_rgba: preview.rgba,
        preview_width: preview.width,
        preview_height: preview.height,
        preview_texture,
        loaded_tier: CanvasImageCacheTier::Working,
        existing_load_diagnostics: None,
        origin: ImportOrigin::Path,
        trace,
    })
}

fn import_path(
    library: &Library,
    source_path: PathBuf,
    desired_world: egui::Pos2,
    viewport_world: egui::Rect,
    max_texture_side: usize,
    canvas_image_settings: CanvasImageSettings,
    auto_scale: bool,
    import_gap: f32,
    trace: Option<ImportTrace>,
    wgpu_render_state: Option<&eframe::egui_wgpu::RenderState>,
) -> ImportResult {
    match import_path_sync_traced(
        library,
        source_path.clone(),
        desired_world,
        viewport_world,
        max_texture_side,
        canvas_image_settings,
        auto_scale,
        import_gap,
        trace,
        wgpu_render_state,
    ) {
        Ok(loaded) => ImportResult::Loaded(loaded),
        Err(error) => ImportResult::Failed {
            item_id: None,
            context: source_path.display().to_string(),
            error,
        },
    }
}

fn import_clipboard(
    library: &Library,
    rgba: Vec<u8>,
    width: u32,
    height: u32,
    desired_world: egui::Pos2,
    viewport_world: egui::Rect,
    max_texture_side: usize,
    canvas_image_settings: CanvasImageSettings,
    auto_scale: bool,
    import_gap: f32,
    trace: Option<ImportTrace>,
    wgpu_render_state: Option<&eframe::egui_wgpu::RenderState>,
) -> ImportResult {
    match import_clipboard_sync_traced(
        library,
        rgba,
        width,
        height,
        desired_world,
        viewport_world,
        max_texture_side,
        canvas_image_settings,
        auto_scale,
        import_gap,
        trace,
        wgpu_render_state,
    ) {
        Ok(loaded) => ImportResult::Loaded(loaded),
        Err(error) => ImportResult::Failed {
            item_id: None,
            context: "clipboard".to_owned(),
            error,
        },
    }
}

fn import_clipboard_sync_traced(
    library: &Library,
    rgba: Vec<u8>,
    width: u32,
    height: u32,
    desired_world: egui::Pos2,
    viewport_world: egui::Rect,
    max_texture_side: usize,
    canvas_image_settings: CanvasImageSettings,
    auto_scale: bool,
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
    let initial_scale = initial_import_scale(width, height, auto_scale);
    let arranged_world = arrange_rect_world(
        library,
        desired_world,
        viewport_world,
        width as f32 * initial_scale.max(0.01),
        height as f32 * initial_scale.max(0.01),
        import_gap,
    )?;
    let record = library.persist_imported_image(
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
        canvas_image_settings.mode,
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
        record,
        preview_rgba: preview.rgba,
        preview_width: preview.width,
        preview_height: preview.height,
        preview_texture,
        loaded_tier: CanvasImageCacheTier::Working,
        existing_load_diagnostics: None,
        origin: ImportOrigin::Clipboard,
        trace,
    })
}

fn load_existing(
    library: &Library,
    record: BoardItemWithAsset,
    max_texture_side: usize,
    canvas_image_settings: CanvasImageSettings,
    target_tier: CanvasImageCacheTier,
    trace: Option<ImportTrace>,
    wgpu_render_state: Option<&eframe::egui_wgpu::RenderState>,
) -> ImportResult {
    let worker_started_at = Instant::now();
    let mut trace = trace;
    if let Some(trace) = &mut trace {
        trace.timings.queue_wait = worker_started_at.saturating_duration_since(trace.queued_at);
    }

    let decode_started_at = Instant::now();
    let preview_started_at = Instant::now();
    match load_existing_preview(
        library,
        &record,
        canvas_image_settings,
        target_tier,
        max_texture_side,
    ) {
        Ok((preview, diagnostics)) => {
            let decode_or_load = decode_started_at.elapsed();
            let preview_texture = match build_preview_texture(wgpu_render_state, &preview) {
                Ok(preview_texture) => preview_texture,
                Err(error) => {
                    return ImportResult::Failed {
                        item_id: Some(record.item.id),
                        context: record.asset.original_path.display().to_string(),
                        error,
                    };
                }
            };
            if let Some(trace) = &mut trace {
                trace.timings.decode_or_load = decode_or_load;
                trace.timings.preview_prepare = preview_started_at.elapsed();
                trace.timings.worker_total = worker_started_at.elapsed();
            }
            ImportResult::Loaded(LoadedCanvasItem {
                record,
                preview_rgba: preview.rgba,
                preview_width: preview.width,
                preview_height: preview.height,
                preview_texture,
                loaded_tier: target_tier,
                existing_load_diagnostics: Some(diagnostics),
                origin: ImportOrigin::Existing,
                trace,
            })
        }
        Err(error) => ImportResult::Failed {
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
            record.clone(),
            usize::MAX,
            CanvasImageSettings::default(),
            CanvasImageCacheTier::Working,
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
            | ImportResult::CacheTierBackfilled { .. }
            | ImportResult::CacheTierBackfillFailed { .. } => {}
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
                record.clone(),
                usize::MAX,
                CanvasImageSettings::default(),
                CanvasImageCacheTier::Working,
                None,
                None,
            ) {
                ImportResult::Loaded(_) => {}
                ImportResult::Failed { error, .. } => return Err(error),
                ImportResult::Cropped(_)
                | ImportResult::CacheTierBackfilled { .. }
                | ImportResult::CacheTierBackfillFailed { .. } => {}
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
            record: perf_record(
                index as i64 + 1,
                &path,
                egui::pos2(2500.0 + index as f32 * 40.0, 2500.0),
                PERF_BACKLOG_IMAGE_SIZE,
                PERF_BACKLOG_IMAGE_SIZE,
            ),
            max_texture_side: usize::MAX,
            canvas_image_settings: CanvasImageSettings::default(),
            target_tier: CanvasImageCacheTier::Working,
            priority: ExistingLoadPriority::Background,
            trace: None,
        })?;
    }

    let started_at = Instant::now();
    worker.enqueue(ImportRequest::ImportClipboard {
        rgba: solid_rgba(1200, 900),
        width: 1200,
        height: 900,
        desired_world: egui::Pos2::ZERO,
        viewport_world,
        max_texture_side: usize::MAX,
        canvas_image_settings: CanvasImageSettings::default(),
        auto_scale: true,
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
    BoardItemWithAsset {
        item: crate::data::BoardItem {
            id: item_id,
            asset_id: item_id,
            x: center.x,
            y: center.y,
            scale: 1.0,
            rotation: 0.0,
            z_index: item_id as i32,
            archived: false,
        },
        asset: crate::data::ImageAsset {
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
        },
    }
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
    std::env::temp_dir().join(format!("reference-{label}-{unique}"))
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
    let decoded = match decode_image_file(&request.source_asset.original_path) {
        Ok(decoded) => decoded,
        Err(error) => {
            return ImportResult::Failed {
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
    let stored_path = match library.store_rgba_image(
        &format!("{base_name}-crop"),
        &cropped_rgba,
        width,
        height,
    ) {
        Ok(path) => path,
        Err(error) => {
            return ImportResult::Failed {
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
                item_id: Some(request.item_id),
                context: stored_path.display().to_string(),
                error: format!("failed to read cropped image metadata: {error}"),
            };
        }
    };
    let canvas_source =
        match canvas_source_image_from_rgba(width, height, &cropped_rgba, canvas_image_settings) {
            Ok(preview) => preview,
            Err(error) => {
                let _ = std::fs::remove_file(&stored_path);
                return ImportResult::Failed {
                    item_id: Some(request.item_id),
                    context: source_context,
                    error,
                };
            }
        };
    let preview = match preview_image_from_rgba(
        canvas_source.width,
        canvas_source.height,
        &canvas_source.rgba,
        max_texture_side,
    ) {
        Ok(preview) => preview,
        Err(error) => {
            let _ = std::fs::remove_file(&stored_path);
            return ImportResult::Failed {
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
        canvas_rgba: canvas_source.rgba,
        canvas_width: canvas_source.width,
        canvas_height: canvas_source.height,
        preview_rgba: preview.rgba,
        preview_width: preview.width,
        preview_height: preview.height,
        preview_texture,
        loaded_tier: CanvasImageCacheTier::Working,
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
            label: Some("reference_import_preview"),
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

fn tier_max_side(
    canvas_image_settings: CanvasImageSettings,
    tier: CanvasImageCacheTier,
) -> u32 {
    match tier {
        CanvasImageCacheTier::Overview => OVERVIEW_CACHE_MAX_SIDE.min(canvas_image_settings.max_side),
        CanvasImageCacheTier::Working => canvas_image_settings.max_side,
        CanvasImageCacheTier::Detail => DETAIL_CACHE_MAX_SIDE.max(canvas_image_settings.max_side),
    }
}

fn tier_settings(
    canvas_image_settings: CanvasImageSettings,
    tier: CanvasImageCacheTier,
) -> CanvasImageSettings {
    CanvasImageSettings {
        mode: canvas_image_settings.mode,
        max_side: tier_max_side(canvas_image_settings, tier),
    }
}

fn load_existing_preview(
    library: &Library,
    record: &BoardItemWithAsset,
    canvas_image_settings: CanvasImageSettings,
    target_tier: CanvasImageCacheTier,
    max_texture_side: usize,
) -> Result<(PreviewImage, ExistingLoadDiagnostics), String> {
    let tier_settings = tier_settings(canvas_image_settings, target_tier);
    let (canvas_source, diagnostics) = match canvas_image_settings.mode {
        crate::data::CanvasImageMode::Original => {
            let decoded = decode_image_file(&record.asset.original_path)?;
            let source_path = record.asset.original_path.clone();
            let source_file_size = decoded.file_size;
            (
                PreviewImage {
                    rgba: decoded.rgba,
                    width: decoded.width,
                    height: decoded.height,
                },
                ExistingLoadDiagnostics {
                    source_kind: ExistingLoadSourceKind::Original,
                    source_path,
                    source_file_size,
                    source_width: decoded.width,
                    source_height: decoded.height,
                    backfilled_cache: false,
                },
            )
        }
        crate::data::CanvasImageMode::Downscale => {
            if let Some(cache_path) = library.canvas_image_cache_path_for_tier(
                record.asset.id,
                tier_settings.mode,
                tier_settings.max_side,
                target_tier,
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
                        backfill_canvas_image_cache(library, record, tier_settings, target_tier)?
                    }
                }
            } else {
                backfill_canvas_image_cache(library, record, tier_settings, target_tier)?
            }
        }
    };

    Ok((
        preview_image_from_rgba(
            canvas_source.width,
            canvas_source.height,
            &canvas_source.rgba,
            max_texture_side,
        )?,
        diagnostics,
    ))
}

fn canvas_source_image_from_rgba(
    width: u32,
    height: u32,
    rgba: &[u8],
    canvas_image_settings: CanvasImageSettings,
) -> Result<PreviewImage, String> {
    match canvas_image_settings.mode {
        crate::data::CanvasImageMode::Downscale => {
            preview_image_from_rgba(width, height, rgba, canvas_image_settings.max_side as usize)
        }
        crate::data::CanvasImageMode::Original => Ok(PreviewImage {
            rgba: rgba.to_vec(),
            width,
            height,
        }),
    }
}

fn backfill_canvas_image_cache(
    library: &Library,
    record: &BoardItemWithAsset,
    canvas_image_settings: CanvasImageSettings,
    tier: CanvasImageCacheTier,
) -> Result<(PreviewImage, ExistingLoadDiagnostics), String> {
    let decoded = if let Some(path) =
        preferred_backfill_source_path(library, record, canvas_image_settings, tier)
    {
        decode_image_file(&path)?
    } else {
        decode_image_file(&record.asset.original_path)?
    };
    let canvas_source = canvas_source_image_from_rgba(
        decoded.width,
        decoded.height,
        &decoded.rgba,
        canvas_image_settings,
    )?;
    let source_path = library
        .store_canvas_image_cache_for_tier(
        record.asset.id,
        canvas_image_settings.mode,
        canvas_image_settings.max_side,
        tier,
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
    tier: CanvasImageCacheTier,
) -> Option<PathBuf> {
    match tier {
        CanvasImageCacheTier::Working => library
            .legacy_canvas_image_cache_path_for_tier(
                record.asset.id,
                canvas_image_settings.mode,
                canvas_image_settings.max_side,
                tier,
            )
            .filter(|path| path.is_file()),
        CanvasImageCacheTier::Overview => library
            .canvas_image_cache_path_for_tier(
                record.asset.id,
                canvas_image_settings.mode,
                canvas_image_settings.max_side,
                CanvasImageCacheTier::Working,
            )
            .or_else(|| {
                library.legacy_canvas_image_cache_path_for_tier(
                    record.asset.id,
                    canvas_image_settings.mode,
                    canvas_image_settings.max_side,
                    CanvasImageCacheTier::Working,
                )
            })
            .filter(|path| path.is_file()),
        CanvasImageCacheTier::Detail => None,
    }
}

fn backfill_cache_tier(
    library: &Library,
    record: BoardItemWithAsset,
    canvas_image_settings: CanvasImageSettings,
    tier: CanvasImageCacheTier,
) -> ImportResult {
    let tier_settings = tier_settings(canvas_image_settings, tier);
    match backfill_canvas_image_cache(library, &record, tier_settings, tier) {
        Ok(_) => ImportResult::CacheTierBackfilled {
            asset_id: record.asset.id,
            tier,
        },
        Err(error) => ImportResult::CacheTierBackfillFailed {
            asset_id: record.asset.id,
            tier,
            error,
        },
    }
}

fn initial_import_scale(asset_width: u32, asset_height: u32, auto_scale: bool) -> f32 {
    if !auto_scale || asset_width == 0 || asset_height == 0 {
        return 1.0;
    }

    let asset_area = asset_width as f32 * asset_height as f32;
    (DEFAULT_IMPORT_TARGET_CANVAS_AREA / asset_area)
        .sqrt()
        .min(1.0)
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

pub fn arrange_rect_world(
    library: &Library,
    desired_world: egui::Pos2,
    viewport_world: egui::Rect,
    item_width: f32,
    item_height: f32,
    import_gap: f32,
) -> Result<egui::Pos2, String> {
    let new_rect = egui::Rect::from_center_size(
        desired_world,
        egui::vec2(item_width.max(0.0), item_height.max(0.0)),
    );

    let existing_items = library.load_board_layout_items()?;
    if existing_items.is_empty() {
        return Ok(desired_world);
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

    Ok(best_candidate
        .map(|(_, _, _, _, center)| center)
        .unwrap_or(desired_world))
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

#[cfg(test)]
mod tests {
    use super::{
        CanvasImageSettings, DEFAULT_IMPORT_TARGET_CANVAS_AREA, ImportResult, ImportTimings,
        ImportTrace, ImportWorker, ImportWorkerConfig, ImportedPreviewTexture, arrange_rect_world,
        build_preview_texture, crop_item, import_clipboard, import_path_sync, initial_import_scale,
        load_existing, rects_overlap_with_gap,
    };
    use crate::data::CanvasImageCacheTier;
    use crate::storage::Library;
    use image::{ColorType, ImageFormat};
    use pollster::block_on;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, mpsc};
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    fn downscaled_canvas_settings(max_side: u32) -> CanvasImageSettings {
        CanvasImageSettings {
            mode: crate::data::CanvasImageMode::Downscale,
            max_side,
        }
    }

    #[test]
    fn scales_down_large_portrait_images_to_target_area() {
        let scale = initial_import_scale(1000, 1500, true);
        assert!(
            (scale - (DEFAULT_IMPORT_TARGET_CANVAS_AREA / 1_500_000.0).sqrt()).abs() < f32::EPSILON
        );
    }

    #[test]
    fn scales_down_large_landscape_images_by_area_cap() {
        let scale = initial_import_scale(1920, 1080, true);
        assert!(
            (scale - (DEFAULT_IMPORT_TARGET_CANVAS_AREA / 2_073_600.0).sqrt()).abs() < f32::EPSILON
        );
    }

    #[test]
    fn keeps_smaller_images_at_full_scale() {
        assert_eq!(initial_import_scale(180, 240, true), 1.0);
    }

    #[test]
    fn disabling_auto_scale_keeps_full_scale() {
        assert_eq!(initial_import_scale(1200, 2000, false), 1.0);
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
            tall_rgba,
            390,
            844,
            eframe::egui::Pos2::ZERO,
            test_viewport_world(),
            usize::MAX,
            CanvasImageSettings::default(),
            true,
            crate::data::DEFAULT_IMPORT_GAP,
            None,
            None,
        ) {
            ImportResult::Loaded(loaded) => loaded,
            other => panic!("expected loaded tall clipboard image, got {other:?}"),
        };
        let small = match import_clipboard(
            &library,
            small_rgba,
            160,
            200,
            eframe::egui::Pos2::ZERO,
            test_viewport_world(),
            usize::MAX,
            CanvasImageSettings::default(),
            false,
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
            solid_rgba(200, 200),
            200,
            200,
            eframe::egui::Pos2::ZERO,
            test_viewport_world(),
            usize::MAX,
            CanvasImageSettings::default(),
            false,
            gap,
            None,
            None,
        ) {
            ImportResult::Loaded(loaded) => loaded,
            other => panic!("expected first loaded image, got {other:?}"),
        };

        let second = match import_clipboard(
            &library,
            solid_rgba(200, 200),
            200,
            200,
            eframe::egui::Pos2::ZERO,
            test_viewport_world(),
            usize::MAX,
            CanvasImageSettings::default(),
            false,
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
            solid_rgba(64, 64),
            64,
            64,
            eframe::egui::Pos2::ZERO,
            test_viewport_world(),
            usize::MAX,
            CanvasImageSettings::default(),
            false,
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
                .canvas_image_cache_path(record.asset.id, settings.mode, settings.max_side)
                .is_none()
        );

        let first = match load_existing(
            &library,
            record.clone(),
            usize::MAX,
            settings,
            CanvasImageCacheTier::Working,
            None,
            None,
        ) {
            ImportResult::Loaded(loaded) => loaded,
            other => panic!("expected first load_existing to succeed, got {other:?}"),
        };
        let cache_path = library
            .canvas_image_cache_path(record.asset.id, settings.mode, settings.max_side)
            .unwrap();
        assert!(cache_path.is_file());
        assert_eq!(first.preview_width, 2000);
        assert_eq!(first.preview_height, 1500);

        fs::remove_file(&record.asset.original_path).unwrap();

        let second = match load_existing(
            &library,
            record.clone(),
            usize::MAX,
            settings,
            CanvasImageCacheTier::Working,
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
                .canvas_image_cache_path(
                    record.asset.id,
                    first_settings.mode,
                    first_settings.max_side,
                )
                .is_none()
        );
        assert!(
            library
                .canvas_image_cache_path(
                    record.asset.id,
                    second_settings.mode,
                    second_settings.max_side,
                )
                .is_none()
        );

        match load_existing(
            &library,
            record.clone(),
            usize::MAX,
            first_settings,
            CanvasImageCacheTier::Working,
            None,
            None,
        ) {
            ImportResult::Loaded(_) => {}
            other => panic!("expected first cache backfill to succeed, got {other:?}"),
        }
        let first_cache = library
            .canvas_image_cache_path(
                record.asset.id,
                first_settings.mode,
                first_settings.max_side,
            )
            .unwrap();
        assert!(first_cache.is_file());
        assert!(
            library
                .canvas_image_cache_path(
                    record.asset.id,
                    second_settings.mode,
                    second_settings.max_side,
                )
                .is_none()
        );

        let second = match load_existing(
            &library,
            record.clone(),
            usize::MAX,
            second_settings,
            CanvasImageCacheTier::Working,
            None,
            None,
        ) {
            ImportResult::Loaded(loaded) => loaded,
            other => panic!("expected second cache backfill to succeed, got {other:?}"),
        };
        assert_eq!(second.preview_width, 1000);
        assert_eq!(second.preview_height, 750);
        let second_cache = library
            .canvas_image_cache_path(
                record.asset.id,
                second_settings.mode,
                second_settings.max_side,
            )
            .unwrap();
        assert_ne!(first_cache, second_cache);
        assert!(second_cache.is_file());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn original_mode_bypasses_canvas_cache() {
        let root = temp_library_root("load-existing-original-bypass");
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

        let downscaled_settings = downscaled_canvas_settings(2000);
        let downscaled_cache = library
            .store_canvas_image_cache(
            record.asset.id,
            downscaled_settings.mode,
            downscaled_settings.max_side,
            &solid_rgba(2000, 1500),
            2000,
            1500,
        )
        .unwrap()
        .unwrap();
        assert!(downscaled_cache.is_file());
        fs::remove_file(&record.asset.original_path).unwrap();

        match load_existing(
            &library,
            record,
            usize::MAX,
            CanvasImageSettings {
                mode: crate::data::CanvasImageMode::Original,
                max_side: 2000,
            },
            CanvasImageCacheTier::Working,
            None,
            None,
        ) {
            ImportResult::Failed { .. } => {}
            other => {
                panic!("expected original mode to fail without the original file, got {other:?}")
            }
        }

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
            record,
            usize::MAX,
            downscaled_canvas_settings(2000),
            CanvasImageCacheTier::Working,
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
        std::env::temp_dir().join(format!("reference-{label}-{unique}"))
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

    fn solid_rgba(width: u32, height: u32) -> Vec<u8> {
        vec![255; width as usize * height as usize * 4]
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
}
