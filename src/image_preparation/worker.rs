use std::collections::VecDeque;
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::thread::JoinHandle;

use eframe::egui;

use crate::library::storage::Library;

use super::{
    ExistingLoadPriority, ImagePrepPriority, ImagePrepRequest, ImagePrepResult,
    ImagePrepWorkerStats, ImportSource, backfill_cache_lod, crop_item, import, load_existing,
};

pub struct ImagePrepWorker {
    pub(super) queue: Arc<RequestQueue>,
    receiver: Receiver<ImagePrepResult>,
    pub(super) workers: Vec<JoinHandle<()>>,
}

pub(crate) struct RequestQueue {
    state: Mutex<RequestQueueState>,
    ready: Condvar,
}

#[derive(Default)]
struct RequestQueueState {
    startup: VecDeque<ImagePrepRequest>,
    interactive: VecDeque<ImagePrepRequest>,
    background: VecDeque<ImagePrepRequest>,
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
pub(crate) struct ImagePrepWorkerConfig {
    pub(super) general_workers: usize,
    pub(super) interactive_workers: usize,
    pub(super) background_workers: usize,
}

impl ImagePrepWorker {
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
            ImagePrepWorkerConfig::for_parallelism(parallelism),
        )
    }

    #[cfg(test)]
    pub(crate) fn controlled() -> (Self, mpsc::Sender<ImagePrepResult>) {
        let (result_tx, receiver) = mpsc::channel();
        (
            Self {
                queue: Arc::new(RequestQueue::default()),
                receiver,
                workers: Vec::new(),
            },
            result_tx,
        )
    }

    pub(super) fn new_with_config(
        library: Library,
        ctx: egui::Context,
        wgpu_render_state: Option<eframe::egui_wgpu::RenderState>,
        config: ImagePrepWorkerConfig,
    ) -> Self {
        let helper = super::decode::ImagePrepHelper::default();
        Self::new_with_processor(ctx, config, move |request| match request {
            ImagePrepRequest::Import(request) => {
                import(&library, request, wgpu_render_state.as_ref(), &helper)
            }
            ImagePrepRequest::Crop {
                request,
                max_texture_side,
                canvas_image_settings,
            } => crop_item(
                &library,
                request,
                max_texture_side,
                canvas_image_settings,
                wgpu_render_state.as_ref(),
            ),
            ImagePrepRequest::LoadExisting {
                target_board_id,
                queue_id,
                placement,
                max_texture_side,
                canvas_image_settings,
                target_lod,
                priority: _,
                trace,
            } => load_existing(
                &library,
                target_board_id,
                queue_id,
                placement,
                max_texture_side,
                canvas_image_settings,
                target_lod,
                trace,
                wgpu_render_state.as_ref(),
            ),
            ImagePrepRequest::BackfillLod {
                placement,
                canvas_image_settings,
                lod,
            } => backfill_cache_lod(&library, placement, canvas_image_settings, lod),
        })
    }

    pub(super) fn new_with_processor(
        ctx: egui::Context,
        config: ImagePrepWorkerConfig,
        process: impl Fn(ImagePrepRequest) -> ImagePrepResult + Send + Sync + 'static,
    ) -> Self {
        let process = Arc::new(process);
        let (result_tx, result_rx) = mpsc::channel::<ImagePrepResult>();
        let queue = Arc::new(RequestQueue::default());
        let mut workers = Vec::with_capacity(config.total_workers());

        for kind in config.worker_kinds() {
            let worker_queue = queue.clone();
            let worker_ctx = ctx.clone();
            let worker_result_tx = result_tx.clone();
            let process = process.clone();
            workers.push(thread::spawn(move || {
                while let Some(request) = worker_queue.pop(kind) {
                    let failure = request.panic_failure();
                    let result =
                        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| process(request)))
                            .unwrap_or(failure);

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

    pub fn enqueue(&self, request: ImagePrepRequest) -> Result<(), String> {
        self.queue.push(request)
    }

    pub fn try_recv_up_to(&self, limit: usize) -> Vec<ImagePrepResult> {
        self.receiver.try_iter().take(limit).collect()
    }

    pub fn stats(&self) -> ImagePrepWorkerStats {
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
    pub(super) fn push(&self, request: ImagePrepRequest) -> Result<(), String> {
        let mut state = self.state.lock().expect("request queue poisoned");
        if state.shutting_down {
            return Err("import worker is shutting down".to_owned());
        }
        match request.priority() {
            ImagePrepPriority::Startup => state.startup.push_back(request),
            ImagePrepPriority::Interactive => {
                insert_interactive_sorted_by_bytes(&mut state.interactive, request);
            }
            ImagePrepPriority::Background => state.background.push_back(request),
        }
        self.ready.notify_all();
        Ok(())
    }

    #[cfg(test)]
    pub(super) fn try_pop_one(&self, kind: WorkerKind) -> Option<ImagePrepRequest> {
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

    fn pop(&self, kind: WorkerKind) -> Option<ImagePrepRequest> {
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

    fn stats(&self) -> ImagePrepWorkerStats {
        let state = self.state.lock().expect("request queue poisoned");
        ImagePrepWorkerStats {
            queued_startup: state.startup.len(),
            queued_interactive: state.interactive.len(),
            queued_background: state.background.len(),
            in_flight: state.in_flight,
        }
    }
}

impl ImagePrepWorkerConfig {
    pub(super) fn for_parallelism(parallelism: usize) -> Self {
        if parallelism <= 1 {
            return Self::single_worker();
        }

        Self {
            general_workers: 0,
            interactive_workers: parallelism.saturating_sub(1).clamp(1, 4),
            background_workers: 1,
        }
    }

    pub(super) fn single_worker() -> Self {
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

impl Drop for ImagePrepWorker {
    fn drop(&mut self) {
        self.queue.shutdown();
        while let Some(worker) = self.workers.pop() {
            let _ = worker.join();
        }
    }
}

impl ImagePrepRequest {
    fn panic_failure(&self) -> ImagePrepResult {
        let error = "image preparation panicked".to_owned();
        let (target_board_id, queue_id, placement_id, context) = match self {
            Self::Import(request) => (
                Some(request.target_board_id),
                Some(request.queue_id),
                Some(request.placement_id),
                request.source.label(),
            ),
            Self::Crop { request, .. } => {
                (None, None, Some(request.placement_id), "crop".to_owned())
            }
            Self::LoadExisting {
                target_board_id,
                queue_id,
                placement,
                target_lod,
                ..
            } => {
                return ImagePrepResult::ExistingLoadFailed {
                    asset_id: placement.asset_id(),
                    target_board_id: *target_board_id,
                    queue_id: *queue_id,
                    placement_id: placement.id(),
                    lod: *target_lod,
                    context: "load existing image".to_owned(),
                    error,
                };
            }
            Self::BackfillLod { placement, lod, .. } => {
                if let Some(asset) = placement.loaded_asset() {
                    return ImagePrepResult::CacheLodBackfillFailed {
                        asset_id: asset.id,
                        lod: *lod,
                        error,
                    };
                }
                (
                    None,
                    None,
                    Some(placement.id()),
                    "backfill cache".to_owned(),
                )
            }
        };
        ImagePrepResult::Failed {
            target_board_id,
            queue_id,
            placement_id,
            context,
            error,
        }
    }

    fn priority(&self) -> ImagePrepPriority {
        match self {
            ImagePrepRequest::LoadExisting { priority, .. } => match priority {
                ExistingLoadPriority::Startup => ImagePrepPriority::Startup,
                ExistingLoadPriority::Background => ImagePrepPriority::Background,
            },
            ImagePrepRequest::BackfillLod { .. } => ImagePrepPriority::Background,
            ImagePrepRequest::Import(_) | ImagePrepRequest::Crop { .. } => {
                ImagePrepPriority::Interactive
            }
        }
    }

    /// File-size hint used to prioritize **smaller imports first** in a mixed
    /// batch so a single large asset cannot block many small assets from
    /// reaching **Placement Visible**. Returns `None` for variants whose size
    /// is unknown at enqueue time (URL downloads) or which do not participate
    /// in mixed-batch import ordering (`Crop`, `LoadExisting`, `BackfillLod`).
    fn priority_bytes(&self) -> Option<u64> {
        match self {
            ImagePrepRequest::Import(request) => match &request.source {
                ImportSource::Path(source_path) => std::fs::metadata(source_path)
                    .ok()
                    .map(|metadata| metadata.len()),
                ImportSource::Clipboard { rgba, .. } => Some(rgba.len() as u64),
                ImportSource::Url(_) => None,
            },
            ImagePrepRequest::Crop { .. }
            | ImagePrepRequest::LoadExisting { .. }
            | ImagePrepRequest::BackfillLod { .. } => None,
        }
    }
}

/// Insert `request` into the **Interactive** queue so requests with smaller
/// `priority_bytes` pop first, falling back to FIFO arrival order when sizes
/// tie or are unknown. Stable across equal keys: ties keep their relative
/// arrival order, so canvas-placement reservations made at enqueue time stay
/// laid out as the user dropped them.
fn insert_interactive_sorted_by_bytes(
    interactive: &mut VecDeque<ImagePrepRequest>,
    request: ImagePrepRequest,
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
