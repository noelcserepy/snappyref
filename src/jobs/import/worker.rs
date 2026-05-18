use std::collections::VecDeque;
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::thread::JoinHandle;

use eframe::egui;

use crate::library::storage::Library;

use super::{
    ExistingLoadPriority, ImagePreparationRequest, ImagePreparationResult, ImportPriority,
    ImportWorkerStats, backfill_cache_lod, crop_item, import_clipboard, import_path, import_url,
    load_existing,
};

pub struct ImportWorker {
    pub(super) queue: Arc<RequestQueue>,
    pub(super) receiver: Receiver<ImagePreparationResult>,
    pub(super) workers: Vec<JoinHandle<()>>,
}

pub(crate) struct RequestQueue {
    state: Mutex<RequestQueueState>,
    ready: Condvar,
}

#[derive(Default)]
struct RequestQueueState {
    startup: VecDeque<ImagePreparationRequest>,
    interactive: VecDeque<ImagePreparationRequest>,
    background: VecDeque<ImagePreparationRequest>,
    in_flight: usize,
    shutting_down: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WorkerKind {
    General,
    Interactive,
    Background,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ImportWorkerConfig {
    pub(super) general_workers: usize,
    pub(super) interactive_workers: usize,
    pub(super) background_workers: usize,
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

    pub(super) fn new_with_config(
        library: Library,
        ctx: egui::Context,
        wgpu_render_state: Option<eframe::egui_wgpu::RenderState>,
        config: ImportWorkerConfig,
    ) -> Self {
        let (result_tx, result_rx) = mpsc::channel::<ImagePreparationResult>();
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
                        ImagePreparationRequest::ImportPath {
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
                        ImagePreparationRequest::ImportClipboard {
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
                        ImagePreparationRequest::ImportUrl {
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
                        ImagePreparationRequest::Crop {
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
                        ImagePreparationRequest::LoadExisting {
                            target_board_id,
                            queue_id,
                            placement,
                            max_texture_side,
                            canvas_image_settings,
                            target_lod,
                            priority: _,
                            trace,
                        } => load_existing(
                            &worker_library,
                            target_board_id,
                            queue_id,
                            placement,
                            max_texture_side,
                            canvas_image_settings,
                            target_lod,
                            trace,
                            worker_render_state.as_ref(),
                        ),
                        ImagePreparationRequest::BackfillLod {
                            placement,
                            canvas_image_settings,
                            lod,
                        } => backfill_cache_lod(
                            &worker_library,
                            placement,
                            canvas_image_settings,
                            lod,
                        ),
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

    pub fn enqueue(&self, request: ImagePreparationRequest) -> Result<(), String> {
        self.queue.push(request)
    }

    pub fn try_recv_up_to(&self, limit: usize) -> Vec<ImagePreparationResult> {
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
    pub(super) fn push(&self, request: ImagePreparationRequest) -> Result<(), String> {
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
    pub(super) fn try_pop_one(&self, kind: WorkerKind) -> Option<ImagePreparationRequest> {
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

    fn pop(&self, kind: WorkerKind) -> Option<ImagePreparationRequest> {
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
    pub(super) fn for_parallelism(parallelism: usize) -> Self {
        if parallelism <= 1 {
            return Self::legacy_single_worker();
        }

        Self {
            general_workers: 0,
            interactive_workers: parallelism.saturating_sub(1).clamp(1, 4),
            background_workers: 1,
        }
    }

    pub(super) fn legacy_single_worker() -> Self {
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

impl ImagePreparationRequest {
    fn priority(&self) -> ImportPriority {
        match self {
            ImagePreparationRequest::LoadExisting { priority, .. } => match priority {
                ExistingLoadPriority::Startup => ImportPriority::Startup,
                ExistingLoadPriority::Background => ImportPriority::Background,
            },
            ImagePreparationRequest::BackfillLod { .. } => ImportPriority::Background,
            ImagePreparationRequest::ImportPath { .. }
            | ImagePreparationRequest::ImportClipboard { .. }
            | ImagePreparationRequest::ImportUrl { .. }
            | ImagePreparationRequest::Crop { .. } => ImportPriority::Interactive,
        }
    }

    /// File-size hint used to prioritize **smaller imports first** in a mixed
    /// batch so a single large asset cannot block many small assets from
    /// reaching **Placement Visible**. Returns `None` for variants whose size
    /// is unknown at enqueue time (URL downloads) or which do not participate
    /// in mixed-batch import ordering (`Crop`, `LoadExisting`, `BackfillLod`).
    fn priority_bytes(&self) -> Option<u64> {
        match self {
            ImagePreparationRequest::ImportPath { source_path, .. } => {
                std::fs::metadata(source_path)
                    .ok()
                    .map(|metadata| metadata.len())
            }
            ImagePreparationRequest::ImportClipboard { rgba, .. } => Some(rgba.len() as u64),
            ImagePreparationRequest::ImportUrl { .. }
            | ImagePreparationRequest::Crop { .. }
            | ImagePreparationRequest::LoadExisting { .. }
            | ImagePreparationRequest::BackfillLod { .. } => None,
        }
    }
}

/// Insert `request` into the **Interactive** queue so requests with smaller
/// `priority_bytes` pop first, falling back to FIFO arrival order when sizes
/// tie or are unknown. Stable across equal keys: ties keep their relative
/// arrival order, so canvas-placement reservations made at enqueue time stay
/// laid out as the user dropped them.
fn insert_interactive_sorted_by_bytes(
    interactive: &mut VecDeque<ImagePreparationRequest>,
    request: ImagePreparationRequest,
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
