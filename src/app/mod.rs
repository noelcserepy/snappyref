use eframe::egui;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use std::time::Duration;
use std::time::Instant;

use crate::active_board::commands::{
    ActiveBoardCommandContext, ClipboardBoardSelection, PasteCommandOutcome,
};
use crate::active_board::history::{
    DocumentHistory, HistoryAction, HistoryChange, HistoryDirection,
};
use crate::active_board::persistence::DocumentPersistence;
use crate::library::residency::{ImagePrepResultResidencyOutcome, ImageResidencyCoordinator};
use crate::placement::content_resolution::{
    ContentResolutionRequestConfig, ContentResolutionReservation, ContentResolutionRuntime,
    FailedContentResolutionOutcome, LoadedContentResolutionOutcome,
};
#[cfg(feature = "semantic-search")]
use crate::search::SearchController;
use crate::sources::crop::CropFlow;
use crate::sources::import::ExternalImportSource;
use crate::ui::sidebar::{
    SemanticSearchSettingsAction, SemanticSearchSettingsStatus, SidebarBoardAction, SidebarTab,
    block_sidebar_input, close_sidebar_on_outside_click, show_sidebar, show_sidebar_hint,
    sidebar_rect,
};
use crate::updater::{UpdateConfig, UpdateService, UpdateState};

const STARTUP_VIEWPORT_SIZE: egui::Vec2 = egui::vec2(960.0, 640.0);
const MAX_IMPORT_RESULTS_PER_FRAME: usize = 4;
const USER_SESSION_SAVE_DEBOUNCE: Duration = Duration::from_millis(750);
/// Minimum wall time between Settings Tab update-check clicks.
/// ADR-0015: discovery is opportunistic and the manual button is a
/// convenience, not a way to hammer the manifest endpoint.
const MANUAL_UPDATE_CHECK_THROTTLE: Duration = Duration::from_secs(60);
const BOARD_LOAD_DEDUPE_HASH_LIMIT: usize = 32;
const BOARD_LOAD_DEDUPE_GROUP_LIMIT: usize = 8;

mod shortcuts;

use shortcuts::{
    ArrangementAction, ContextMenuAction, ContextMenuItem, PendingBoardDelete,
    PendingLibraryRemove, SupportedShortcutAction, delete_shortcut_pressed,
    settle_direction_pressed, shortcut_pressed,
};
pub(crate) use shortcuts::{SUPPORTED_SHORTCUTS, SupportedShortcut};

#[derive(Clone, Debug)]
pub struct AppConfig {
    library_root: Result<std::path::PathBuf, String>,
    no_startup_focus: bool,
    perf_log: bool,
    perf_report: bool,
    perf_decode_path: Option<std::path::PathBuf>,
    perf_import_path: Option<std::path::PathBuf>,
    perf_embedding_lod: bool,
    perf_embedding_lod_limit: usize,
    perf_scenario: Option<String>,
    perf_corpus: Option<std::path::PathBuf>,
    perf_run_dir: Option<std::path::PathBuf>,
    startup_trace: bool,
    update_smoke_install: bool,
    update_smoke_status_path: Option<std::path::PathBuf>,
}

mod update_smoke;

use update_smoke::{UpdateSmokePhase, UpdateSmokeState};

#[derive(Clone, Debug)]
struct PerfLogState {
    app_started_at: Instant,
    startup_logged: bool,
}

#[derive(Clone, Copy, Debug, Default)]
struct FramePerfState {
    import_results_drained: usize,
    import_result_drain_ms: f64,
    canvas: crate::canvas::CanvasFrameMetrics,
}

#[derive(Clone, Debug)]
struct PerfCorpusAsset {
    file: String,
    format: String,
    animated: bool,
    bytes: Option<u64>,
}

#[derive(Debug)]
struct PerfScenarioState {
    name: String,
    corpus_root: PathBuf,
    run_dir: PathBuf,
    events: File,
    started_at: Instant,
    queued: bool,
    expected_imports: usize,
    loaded_imports: usize,
    visible_imports: usize,
    demand_embeddings_queued: usize,
    demand_embeddings_done: usize,
    first_loaded_logged: bool,
    all_loaded_logged: bool,
    first_visible_logged: bool,
    all_visible_logged: bool,
    first_embedding_queued_logged: bool,
    first_embedding_done_logged: bool,
    all_embeddings_done_logged: bool,
    embedding_phases: EmbeddingPhaseMarkers,
    idle_since: Option<Instant>,
    settled_idle_logged: bool,
    model_unload_logged: bool,
    /// Per-asset corpus metadata indexed by import queue id, so events
    /// emitted after `import_queued` can still report `format`.
    asset_by_queue_id: std::collections::HashMap<u64, PerfCorpusAsset>,
    /// Per-asset corpus metadata indexed by stored asset id, used to
    /// stamp `format` onto embedding events once an asset id is known.
    asset_by_asset_id: std::collections::HashMap<i64, PerfCorpusAsset>,
    /// `trickle-import` only: entries still to enqueue, popped one per
    /// `trickle_interval` to reproduce **Embedding Helper Process** churn.
    pending_trickle_imports: Vec<PerfCorpusEntry>,
    /// Spacing between successive trickle imports. The default value
    /// keeps the bug-reproduction precise: longer than the previous
    /// "drop helper immediately" behavior so the second import always
    /// reloads, but shorter than the new 3s grace so post-fix the
    /// helper stays alive.
    trickle_interval: Duration,
    /// Earliest instant at which the next trickle import should be
    /// enqueued; `None` once the trickle queue is empty.
    trickle_next_at: Option<Instant>,
}

#[derive(Clone, Copy, Debug, Default, serde::Serialize)]
struct ModelLoadTimingSummary {
    assets_resolved_ms: f64,
    tokenizer_loaded_ms: f64,
    text_session_loaded_ms: f64,
    vision_session_loaded_ms: f64,
    total_ms: f64,
}

#[derive(Clone, Copy, Debug, Default)]
struct EmbeddingPhaseMarkers {
    first_embedding_queued_ms: Option<f64>,
    semantic_model_load_started_ms: Option<f64>,
    semantic_model_ready_ms: Option<f64>,
    first_embedding_started_ms: Option<f64>,
    first_embedding_done_ms: Option<f64>,
    all_embeddings_done_ms: Option<f64>,
    semantic_model_unload_requested_ms: Option<f64>,
    semantic_model_unloaded_ms: Option<f64>,
    model_load_timing: Option<ModelLoadTimingSummary>,
}

#[derive(Clone, Copy, Debug, serde::Serialize)]
struct EmbeddingPhaseSummary {
    queue_to_model_load_started_ms: Option<f64>,
    model_cold_mount_ms: Option<f64>,
    first_embedding_start_after_model_ready_ms: Option<f64>,
    first_embedding_after_model_ready_ms: Option<f64>,
    first_embedding_run_ms: Option<f64>,
    total_import_embedding_drain_ms: Option<f64>,
    model_unload_ms: Option<f64>,
    model_load_timing: Option<ModelLoadTimingSummary>,
}

#[derive(Clone, Debug, serde::Deserialize)]
struct PerfCorpusEntry {
    file: String,
    format: String,
    role: String,
    #[serde(default)]
    animated: bool,
    #[serde(default)]
    animation_control: bool,
}

impl PerfScenarioState {
    fn from_config(config: &AppConfig) -> Result<Option<Self>, String> {
        let Some(name) = config.perf_scenario.clone() else {
            return Ok(None);
        };
        let corpus_root = config
            .perf_corpus
            .clone()
            .ok_or_else(|| "--perf-corpus is required with --perf-scenario".to_owned())?;
        let run_dir = config
            .perf_run_dir
            .clone()
            .ok_or_else(|| "--perf-run-dir is required with --perf-scenario".to_owned())?;
        std::fs::create_dir_all(&run_dir).map_err(|error| {
            format!(
                "failed to create perf run dir {}: {error}",
                run_dir.display()
            )
        })?;
        let events_path = run_dir.join("events.ndjson");
        let events = File::create(&events_path)
            .map_err(|error| format!("failed to create {}: {error}", events_path.display()))?;
        Ok(Some(Self {
            name,
            corpus_root,
            run_dir,
            events,
            started_at: Instant::now(),
            queued: false,
            expected_imports: 0,
            loaded_imports: 0,
            visible_imports: 0,
            demand_embeddings_queued: 0,
            demand_embeddings_done: 0,
            first_loaded_logged: false,
            all_loaded_logged: false,
            first_visible_logged: false,
            all_visible_logged: false,
            first_embedding_queued_logged: false,
            first_embedding_done_logged: false,
            all_embeddings_done_logged: false,
            embedding_phases: EmbeddingPhaseMarkers::default(),
            idle_since: None,
            settled_idle_logged: false,
            model_unload_logged: false,
            asset_by_queue_id: std::collections::HashMap::new(),
            asset_by_asset_id: std::collections::HashMap::new(),
            pending_trickle_imports: Vec::new(),
            trickle_interval: Duration::from_secs(2),
            trickle_next_at: None,
        }))
    }

    fn elapsed_ms(&self) -> f64 {
        self.started_at.elapsed().as_secs_f64() * 1000.0
    }

    fn emit(&mut self, event: &str, mut extra: serde_json::Value) {
        let elapsed_ms = self.elapsed_ms();
        self.embedding_phases.record_event(event, elapsed_ms);
        if let Some(object) = extra.as_object_mut() {
            object.insert("event".to_owned(), serde_json::json!(event));
            object.insert("elapsed_ms".to_owned(), serde_json::json!(elapsed_ms));
            object.insert("scenario".to_owned(), serde_json::json!(self.name));
        }
        let _ = writeln!(self.events, "{extra}");
        let _ = self.events.flush();
    }

    fn emit_model_load_timing(&mut self, timing: ModelLoadTimingSummary) {
        self.embedding_phases
            .model_load_timing
            .get_or_insert(timing);
        self.emit(
            "semantic_model_load_timing",
            serde_json::json!({
                "assets_resolved_ms": timing.assets_resolved_ms,
                "tokenizer_loaded_ms": timing.tokenizer_loaded_ms,
                "text_session_loaded_ms": timing.text_session_loaded_ms,
                "vision_session_loaded_ms": timing.vision_session_loaded_ms,
                "total_ms": timing.total_ms,
            }),
        );
    }

    fn manifest_path(&self) -> PathBuf {
        self.corpus_root
            .parent()
            .map(|root| root.join("manifest.json"))
            .unwrap_or_else(|| PathBuf::from("perf-assets/manifest.json"))
    }

    fn selected_entries(&self) -> Result<Vec<PerfCorpusEntry>, String> {
        let manifest_path = self.manifest_path();
        let manifest = std::fs::read_to_string(&manifest_path)
            .map_err(|error| format!("failed to read {}: {error}", manifest_path.display()))?;
        let entries: Vec<PerfCorpusEntry> = serde_json::from_str(&manifest)
            .map_err(|error| format!("failed to parse {}: {error}", manifest_path.display()))?;
        let mut selected = match self.name.as_str() {
            "import-1" => entries
                .into_iter()
                .filter(|entry| entry.role.contains("default-1-image"))
                .take(1)
                .collect::<Vec<_>>(),
            "trickle-import" => {
                // Three copies of the same medium-JPEG entry used by
                // `import-1`, dripped out 2s apart by `drive_perf_scenario`
                // to reproduce **Embedding Helper Process** churn.
                let entry = entries
                    .into_iter()
                    .find(|entry| entry.role.contains("default-1-image"))
                    .ok_or_else(|| {
                        "trickle-import requires a default-1-image entry in the corpus manifest"
                            .to_owned()
                    })?;
                vec![entry.clone(), entry.clone(), entry]
            }
            "import-20" | "mixed-corpus" => entries,
            "animated" => entries
                .into_iter()
                .filter(|entry| entry.animated)
                .collect::<Vec<_>>(),
            "animated-control" => entries
                .into_iter()
                .filter(|entry| entry.animation_control)
                .collect::<Vec<_>>(),
            "static" => entries
                .into_iter()
                .filter(|entry| !entry.animated)
                .collect::<Vec<_>>(),
            "static-20" => {
                let static_entries = entries
                    .into_iter()
                    .filter(|entry| !entry.animated)
                    .collect::<Vec<_>>();
                static_entries
                    .iter()
                    .cycle()
                    .take(20)
                    .cloned()
                    .collect::<Vec<_>>()
            }
            name if name.starts_with("format:") => {
                let format = name.trim_start_matches("format:");
                entries
                    .into_iter()
                    .filter(|entry| entry.format == format)
                    .collect::<Vec<_>>()
            }
            other => return Err(format!("unknown perf scenario: {other}")),
        };
        selected.sort_by(|left, right| left.file.cmp(&right.file));
        if selected.is_empty() {
            return Err(format!("perf scenario selected no files: {}", self.name));
        }
        Ok(selected)
    }

    fn write_summary(
        &mut self,
        worker_stats: crate::jobs::ImagePrepWorkerStats,
        image_pass: crate::canvas::ImagePassMetrics,
    ) {
        let summary = serde_json::json!({
            "scenario": self.name,
            "time_to_idle_ms": self.elapsed_ms(),
            "expected_imports": self.expected_imports,
            "loaded_imports": self.loaded_imports,
            "visible_imports": self.visible_imports,
            "demand_embeddings_queued": self.demand_embeddings_queued,
            "demand_embeddings_done": self.demand_embeddings_done,
            "embedding_phases": self.embedding_phases.summary(),
            "import_worker": {
                "queued_startup": worker_stats.queued_startup,
                "queued_interactive": worker_stats.queued_interactive,
                "queued_background": worker_stats.queued_background,
                "in_flight": worker_stats.in_flight,
            },
            "last_canvas_frame": {
                "visible_images": image_pass.visible_images,
                "visible_animated_images": image_pass.visible_animated_images,
                "active_animated_images": image_pass.active_animated_images,
                "fallback_images": image_pass.fallback_images,
                "draw_calls": image_pass.draw_calls,
                "texture_groups": image_pass.texture_groups,
                "build_time_ms": image_pass.build_time_ms,
            },
        });
        let path = self.run_dir.join("summary.json");
        let _ = std::fs::write(
            &path,
            serde_json::to_string_pretty(&summary).unwrap_or_default(),
        );
    }
}

impl EmbeddingPhaseMarkers {
    fn record_event(&mut self, event: &str, elapsed_ms: f64) {
        match event {
            "first_import_embedding_queued" => {
                self.first_embedding_queued_ms.get_or_insert(elapsed_ms);
            }
            "semantic_model_load_started" => {
                self.semantic_model_load_started_ms
                    .get_or_insert(elapsed_ms);
            }
            "semantic_model_ready" => {
                self.semantic_model_ready_ms.get_or_insert(elapsed_ms);
            }
            "first_import_embedding_started" => {
                self.first_embedding_started_ms.get_or_insert(elapsed_ms);
            }
            "first_import_embedding_done" => {
                self.first_embedding_done_ms.get_or_insert(elapsed_ms);
            }
            "all_import_embeddings_done" => {
                self.all_embeddings_done_ms.get_or_insert(elapsed_ms);
            }
            "semantic_model_unload_requested" => {
                self.semantic_model_unload_requested_ms
                    .get_or_insert(elapsed_ms);
            }
            "semantic_model_unloaded" => {
                self.semantic_model_unloaded_ms.get_or_insert(elapsed_ms);
            }
            _ => {}
        }
    }

    fn summary(&self) -> Option<EmbeddingPhaseSummary> {
        let has_embedding_phase = self.first_embedding_queued_ms.is_some()
            || self.semantic_model_load_started_ms.is_some()
            || self.semantic_model_ready_ms.is_some()
            || self.first_embedding_started_ms.is_some()
            || self.first_embedding_done_ms.is_some()
            || self.all_embeddings_done_ms.is_some()
            || self.model_load_timing.is_some();
        has_embedding_phase.then(|| EmbeddingPhaseSummary {
            queue_to_model_load_started_ms: duration_between(
                self.first_embedding_queued_ms,
                self.semantic_model_load_started_ms,
            ),
            model_cold_mount_ms: self
                .model_load_timing
                .map(|timing| timing.total_ms)
                .or_else(|| {
                    duration_between(
                        self.semantic_model_load_started_ms,
                        self.semantic_model_ready_ms,
                    )
                }),
            first_embedding_start_after_model_ready_ms: duration_between(
                self.semantic_model_ready_ms,
                self.first_embedding_started_ms,
            ),
            first_embedding_after_model_ready_ms: duration_between(
                self.semantic_model_ready_ms,
                self.first_embedding_done_ms,
            ),
            first_embedding_run_ms: duration_between(
                self.first_embedding_started_ms,
                self.first_embedding_done_ms,
            ),
            total_import_embedding_drain_ms: duration_between(
                self.first_embedding_queued_ms,
                self.all_embeddings_done_ms,
            ),
            model_unload_ms: duration_between(
                self.semantic_model_unload_requested_ms,
                self.semantic_model_unloaded_ms,
            ),
            model_load_timing: self.model_load_timing,
        })
    }
}

fn duration_between(start_ms: Option<f64>, end_ms: Option<f64>) -> Option<f64> {
    match (start_ms, end_ms) {
        (Some(start_ms), Some(end_ms)) => Some((end_ms - start_ms).max(0.0)),
        _ => None,
    }
}

impl AppConfig {
    pub fn from_env() -> Self {
        let mut no_startup_focus = false;
        let mut perf_log = false;
        let mut perf_report = false;
        let mut perf_decode_path = None;
        let mut perf_import_path = None;
        let mut perf_embedding_lod = false;
        let mut perf_embedding_lod_limit = 30;
        let mut perf_scenario = None;
        let mut perf_corpus = None;
        let mut perf_run_dir = None;
        let mut startup_trace = false;
        let mut update_smoke_install = false;
        let mut update_smoke_status_path =
            std::env::var_os("SNAPPYREF_UPDATE_SMOKE_STATUS").map(std::path::PathBuf::from);
        let library_root = crate::library::location::resolve_library_root();
        let mut arguments = std::env::args().skip(1);

        while let Some(argument) = arguments.next() {
            if argument == "--no-startup-focus" {
                no_startup_focus = true;
            } else if argument == "--perf-log" {
                perf_log = true;
            } else if argument == "--update-smoke-install" {
                update_smoke_install = true;
            } else if argument == "--update-smoke-status" {
                update_smoke_status_path = arguments.next().map(std::path::PathBuf::from);
            } else if let Some(path) = argument.strip_prefix("--update-smoke-status=") {
                update_smoke_status_path = Some(std::path::PathBuf::from(path));
            } else if argument == "--perf-report" {
                perf_report = true;
            } else if argument == "--perf-decode" {
                perf_decode_path = arguments.next().map(std::path::PathBuf::from);
            } else if let Some(path) = argument.strip_prefix("--perf-decode=") {
                perf_decode_path = Some(std::path::PathBuf::from(path));
            } else if argument == "--perf-import" {
                perf_import_path = arguments.next().map(std::path::PathBuf::from);
            } else if let Some(path) = argument.strip_prefix("--perf-import=") {
                perf_import_path = Some(std::path::PathBuf::from(path));
            } else if argument == "--perf-embedding-lod" {
                perf_embedding_lod = true;
            } else if argument == "--perf-embedding-limit" {
                if let Some(limit) = arguments.next().and_then(|value| value.parse().ok()) {
                    perf_embedding_lod_limit = limit;
                }
            } else if let Some(limit) = argument.strip_prefix("--perf-embedding-limit=") {
                if let Ok(limit) = limit.parse() {
                    perf_embedding_lod_limit = limit;
                }
            } else if argument == "--perf-scenario" {
                perf_scenario = arguments.next();
            } else if let Some(scenario) = argument.strip_prefix("--perf-scenario=") {
                perf_scenario = Some(scenario.to_owned());
            } else if argument == "--perf-corpus" {
                perf_corpus = arguments.next().map(std::path::PathBuf::from);
            } else if let Some(path) = argument.strip_prefix("--perf-corpus=") {
                perf_corpus = Some(std::path::PathBuf::from(path));
            } else if argument == "--perf-run-dir" {
                perf_run_dir = arguments.next().map(std::path::PathBuf::from);
            } else if let Some(path) = argument.strip_prefix("--perf-run-dir=") {
                perf_run_dir = Some(std::path::PathBuf::from(path));
            } else if argument == "--startup-trace" {
                startup_trace = true;
            }
        }

        Self {
            no_startup_focus,
            perf_log,
            perf_report,
            perf_decode_path,
            perf_import_path,
            perf_embedding_lod,
            perf_embedding_lod_limit,
            perf_scenario,
            perf_corpus,
            perf_run_dir,
            startup_trace,
            library_root,
            update_smoke_install,
            update_smoke_status_path,
        }
    }

    fn skip_existing_items_on_start(&self) -> bool {
        self.perf_scenario_requested()
    }

    fn open_sidebar_on_start(&self) -> bool {
        false
    }

    fn open_sidebar_boards_on_start(&self) -> bool {
        false
    }

    pub(crate) fn startup_viewport_size(&self) -> egui::Vec2 {
        STARTUP_VIEWPORT_SIZE
    }

    pub fn perf_log_enabled(&self) -> bool {
        self.perf_log
    }

    pub fn perf_report_requested(&self) -> bool {
        self.perf_report
    }

    pub fn perf_decode_path(&self) -> Option<&std::path::Path> {
        self.perf_decode_path.as_deref()
    }

    pub fn perf_import_path(&self) -> Option<&std::path::Path> {
        self.perf_import_path.as_deref()
    }

    pub fn perf_embedding_lod_requested(&self) -> bool {
        self.perf_embedding_lod
    }

    pub fn perf_embedding_lod_limit(&self) -> usize {
        self.perf_embedding_lod_limit
    }

    fn perf_scenario_requested(&self) -> bool {
        self.perf_scenario.is_some()
    }

    pub(crate) fn startup_focus_enabled(&self) -> bool {
        !(self.no_startup_focus || self.perf_scenario_requested() || self.update_smoke_install)
    }

    pub(crate) fn restored_window_state(&self) -> Option<crate::data::WindowState> {
        if !self.persist_window_state_enabled() {
            return None;
        }
        let library = self
            .library_root()
            .map(std::path::Path::to_path_buf)
            .and_then(crate::library::storage::Library::initialize)
            .ok()?;
        let window = library.load_user_session().ok()?.window;
        window.is_restorable().then_some(window)
    }

    pub(crate) fn restored_workspace_settings(&self) -> Option<crate::data::WorkspaceSettings> {
        if !self.persist_window_state_enabled() {
            return None;
        }
        let library = self
            .library_root()
            .map(std::path::Path::to_path_buf)
            .and_then(crate::library::storage::Library::initialize)
            .ok()?;
        Some(library.load_user_session().ok()?.settings)
    }

    fn persist_window_state_enabled(&self) -> bool {
        !self.perf_scenario_requested() && !self.update_smoke_install
    }

    pub fn startup_trace_requested(&self) -> bool {
        self.startup_trace
    }

    pub(crate) fn library_root(&self) -> Result<&std::path::Path, String> {
        self.library_root
            .as_deref()
            .map_err(std::clone::Clone::clone)
    }
}

pub struct SnappyRefApp {
    canvas: crate::canvas::CanvasState,
    wgpu_render_state: Option<eframe::egui_wgpu::RenderState>,
    library: Option<crate::library::storage::Library>,
    import_worker: Option<crate::jobs::ImagePrepWorker>,
    active_board_id: i64,
    window_state: crate::data::WindowState,
    persist_window_state: bool,
    crop_flow: CropFlow,
    document_history: DocumentHistory,
    max_texture_side: usize,
    settings: crate::data::WorkspaceSettings,
    #[cfg(feature = "semantic-search")]
    search: SearchController,
    sidebar_open: bool,
    sidebar_tab: SidebarTab,
    pending_board_delete: Option<PendingBoardDelete>,
    pending_library_remove: Option<PendingLibraryRemove>,
    #[cfg(feature = "semantic-search")]
    pending_semantic_search_remove: bool,
    context_menu_pos: Option<egui::Pos2>,
    status: String,
    perf_log: Option<PerfLogState>,
    frame_perf: FramePerfState,
    perf_scenario: Option<PerfScenarioState>,
    content_resolution: ContentResolutionRuntime,
    residency: ImageResidencyCoordinator,
    pending_import_batches: crate::placement::creation::PlacementCreationBatchTracker,
    hover_preview_dimensions: std::collections::HashMap<std::path::PathBuf, Option<(u32, u32)>>,
    last_hovered_external_drag: Option<crate::sources::external_drag::ExternalDragIntake>,
    external_hover_provisional_placement_ids: Vec<i64>,
    pending_missing_drop_fallback: Option<crate::sources::external_drag::ExternalDragIntake>,
    document_persistence: DocumentPersistence,
    pending_user_session: Option<crate::data::UserSession>,
    pending_user_session_save_at: Option<Instant>,
    board_summaries_cache: BoardSummariesCache,
    update_service: UpdateService,
    user_update_flow_active: bool,
    update_smoke: Option<UpdateSmokeState>,
    /// Monotonic instant of the last update-check click. Used to throttle
    /// the Settings Tab button to one invocation per minute
    /// independent of wall-clock noise. `None` until the first click.
    last_manual_update_check: Option<Instant>,
}

#[derive(Default)]
struct BoardSummariesCache {
    summaries: Vec<crate::data::BoardSummary>,
}

impl BoardSummariesCache {
    fn summaries(&self) -> Vec<crate::data::BoardSummary> {
        self.summaries.clone()
    }

    fn clear(&mut self) {
        self.summaries.clear();
    }

    fn refresh_from(&mut self, library: &crate::library::storage::Library) -> Result<(), String> {
        self.summaries = library.list_board_summaries()?;
        Ok(())
    }
}

impl SnappyRefApp {
    pub fn new(cc: &eframe::CreationContext<'_>, config: AppConfig) -> Self {
        crate::platform::native_drop::install_url_drop_bridge(cc);
        let max_texture_side = cc.egui_ctx.input(|input| input.max_texture_side.max(1));
        let update_smoke = UpdateSmokeState::from_config(&config);

        let library = config
            .library_root()
            .map(std::path::Path::to_path_buf)
            .and_then(crate::library::storage::Library::initialize);

        match library {
            Ok(library) => {
                let import_worker = crate::jobs::ImagePrepWorker::new(
                    library.clone(),
                    cc.egui_ctx.clone(),
                    cc.wgpu_render_state.clone(),
                );
                let status = "Library ready".to_owned();
                let mut app = Self {
                    canvas: crate::canvas::CanvasState::default(),
                    wgpu_render_state: cc.wgpu_render_state.clone(),
                    library: Some(library.clone()),
                    import_worker: Some(import_worker),
                    active_board_id: 1,
                    window_state: crate::data::WindowState::default(),
                    persist_window_state: config.persist_window_state_enabled(),
                    crop_flow: CropFlow::default(),
                    document_history: DocumentHistory::default(),
                    max_texture_side,
                    settings: crate::data::WorkspaceSettings::default(),
                    #[cfg(feature = "semantic-search")]
                    search: SearchController::default(),
                    sidebar_open: config.open_sidebar_on_start(),
                    sidebar_tab: if config.open_sidebar_boards_on_start() {
                        SidebarTab::Boards
                    } else {
                        SidebarTab::Options
                    },
                    pending_board_delete: None,
                    pending_library_remove: None,
                    #[cfg(feature = "semantic-search")]
                    pending_semantic_search_remove: false,
                    context_menu_pos: None,
                    status,
                    perf_log: config.perf_log_enabled().then(|| PerfLogState {
                        app_started_at: Instant::now(),
                        startup_logged: false,
                    }),
                    frame_perf: FramePerfState::default(),
                    perf_scenario: PerfScenarioState::from_config(&config).unwrap_or(None),
                    content_resolution: ContentResolutionRuntime::new(),
                    residency: ImageResidencyCoordinator::default(),
                    pending_import_batches:
                        crate::placement::creation::PlacementCreationBatchTracker::default(),
                    hover_preview_dimensions: std::collections::HashMap::new(),
                    last_hovered_external_drag: None,
                    external_hover_provisional_placement_ids: Vec::new(),
                    pending_missing_drop_fallback: None,
                    document_persistence: DocumentPersistence::queued(library.clone()),
                    pending_user_session: None,
                    pending_user_session_save_at: None,
                    board_summaries_cache: BoardSummariesCache::default(),
                    update_service: UpdateService::new(UpdateConfig::default()),
                    user_update_flow_active: false,
                    update_smoke: update_smoke.clone(),
                    last_manual_update_check: None,
                };
                app.load_user_session();
                app.refresh_board_summaries();
                if config.open_sidebar_on_start() {
                    app.sidebar_open = true;
                }
                if config.open_sidebar_boards_on_start() {
                    app.sidebar_tab = SidebarTab::Boards;
                }
                #[cfg(feature = "semantic-search")]
                app.sync_semantic_search_service();
                if !config.skip_existing_items_on_start() {
                    app.enqueue_existing_items();
                }
                app
            }
            Err(error) => Self {
                canvas: crate::canvas::CanvasState::default(),
                wgpu_render_state: cc.wgpu_render_state.clone(),
                library: None,
                import_worker: None,
                active_board_id: 1,
                window_state: crate::data::WindowState::default(),
                persist_window_state: config.persist_window_state_enabled(),
                crop_flow: CropFlow::default(),
                document_history: DocumentHistory::default(),
                max_texture_side,
                settings: crate::data::WorkspaceSettings::default(),
                #[cfg(feature = "semantic-search")]
                search: SearchController::default(),
                sidebar_open: config.open_sidebar_on_start(),
                sidebar_tab: if config.open_sidebar_boards_on_start() {
                    SidebarTab::Boards
                } else {
                    SidebarTab::Options
                },
                pending_board_delete: None,
                pending_library_remove: None,
                #[cfg(feature = "semantic-search")]
                pending_semantic_search_remove: false,
                context_menu_pos: None,
                status: format!("Storage unavailable: {error}"),
                perf_log: config.perf_log_enabled().then(|| PerfLogState {
                    app_started_at: Instant::now(),
                    startup_logged: false,
                }),
                frame_perf: FramePerfState::default(),
                perf_scenario: PerfScenarioState::from_config(&config).unwrap_or(None),
                content_resolution: ContentResolutionRuntime::new(),
                residency: ImageResidencyCoordinator::default(),
                pending_import_batches:
                    crate::placement::creation::PlacementCreationBatchTracker::default(),
                hover_preview_dimensions: std::collections::HashMap::new(),
                last_hovered_external_drag: None,
                external_hover_provisional_placement_ids: Vec::new(),
                pending_missing_drop_fallback: None,
                document_persistence: DocumentPersistence::default(),
                pending_user_session: None,
                pending_user_session_save_at: None,
                board_summaries_cache: BoardSummariesCache::default(),
                update_service: UpdateService::new(UpdateConfig::default()),
                user_update_flow_active: false,
                update_smoke,
                last_manual_update_check: None,
            },
        }
    }

    fn enqueue_existing_items(&mut self) {
        let Some(library) = &self.library else {
            return;
        };
        let Some(import_worker) = &self.import_worker else {
            return;
        };

        match library.load_canvas_stack_order() {
            Ok(stack_order) => self.canvas.restore_canvas_stack_order(stack_order),
            Err(error) => self.status = error,
        }

        match library.load_placements() {
            Ok(placements) => {
                let viewport_world = self.startup_viewport_world_rect();
                if let Err(error) = self.residency.queue_startup_existing_loads(
                    import_worker,
                    placements,
                    viewport_world,
                    self.active_board_id,
                    &self.settings,
                    self.max_texture_side,
                    self.perf_log.is_some(),
                ) {
                    self.status = error;
                }
            }
            Err(error) => {
                self.status = error;
            }
        }

        match library.load_canvas_annotations() {
            Ok(annotations) => self.canvas.add_annotations(annotations),
            Err(error) => self.status = error,
        }
    }

    fn drive_perf_scenario(&mut self, ctx: &egui::Context, screen_rect: egui::Rect) -> bool {
        if self.perf_scenario.is_none() {
            return false;
        }
        if self
            .perf_scenario
            .as_ref()
            .is_some_and(|scenario| !scenario.queued)
        {
            self.enqueue_perf_scenario_imports(ctx, screen_rect);
        }
        // trickle-import drips its remaining entries one per
        // `trickle_interval` so the **Embedding Helper Process** sees
        // gaps between successive imports.
        self.dispatch_perf_trickle_imports(ctx, screen_rect);
        self.update_perf_scenario_embedding_progress();
        self.finish_perf_scenario_if_idle(ctx)
    }

    fn enqueue_perf_scenario_imports(&mut self, ctx: &egui::Context, screen_rect: egui::Rect) {
        let Some(mut scenario) = self.perf_scenario.take() else {
            return;
        };
        scenario.queued = true;
        let entries = match scenario.selected_entries() {
            Ok(entries) => entries,
            Err(error) => {
                scenario.emit("error", serde_json::json!({ "message": error }));
                self.perf_scenario = Some(scenario);
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                return;
            }
        };
        scenario.expected_imports = entries.len();
        scenario.emit(
            "scenario_started",
            serde_json::json!({ "expected_imports": entries.len() }),
        );
        let is_trickle = scenario.name == "trickle-import";
        self.perf_scenario = Some(scenario);

        let desired_world = self.preferred_import_world(ctx, screen_rect);
        let viewport_world = self.import_viewport_world_rect(screen_rect);
        let mut layout = match self.import_arrangement_input_snapshot() {
            Ok(layout) => layout,
            Err(error) => {
                self.emit_perf_error_and_close(ctx, error);
                return;
            }
        };

        let entries = match self
            .perf_scenario
            .as_ref()
            .and_then(|scenario| scenario.selected_entries().ok())
        {
            Some(entries) => entries,
            None => return,
        };

        if is_trickle {
            // Enqueue the first import immediately so the
            // **Embedding Helper Process** spins up under the same
            // conditions as `import-1`; the remaining entries trickle
            // in via `dispatch_perf_trickle_imports`.
            let mut iter = entries.into_iter();
            if let Some(first) = iter.next() {
                if let Err(error) =
                    self.enqueue_perf_entry(&first, &mut layout, desired_world, viewport_world)
                {
                    self.emit_perf_error_and_close(ctx, error);
                    return;
                }
            }
            let remaining: Vec<_> = iter.collect();
            if let Some(scenario) = &mut self.perf_scenario {
                let now = Instant::now();
                scenario.trickle_next_at =
                    remaining.first().map(|_| now + scenario.trickle_interval);
                scenario.pending_trickle_imports = remaining;
            }
        } else {
            for entry in entries {
                if let Err(error) =
                    self.enqueue_perf_entry(&entry, &mut layout, desired_world, viewport_world)
                {
                    self.emit_perf_error_and_close(ctx, error);
                    return;
                }
            }
        }
        ctx.request_repaint_after(Duration::from_millis(16));
    }

    fn enqueue_perf_entry(
        &mut self,
        entry: &PerfCorpusEntry,
        layout: &mut Vec<crate::placement::arrangement::PlacementArrangementInput>,
        desired_world: egui::Pos2,
        viewport_world: egui::Rect,
    ) -> Result<(), String> {
        let path = self
            .perf_scenario
            .as_ref()
            .map(|scenario| scenario.corpus_root.join(&entry.file))
            .unwrap_or_else(|| PathBuf::from(&entry.file));
        let dimensions = image::image_dimensions(&path).ok();
        let reservation = self.reserve_import(
            layout,
            path.file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("perf-image")
                .to_owned(),
            dimensions,
            desired_world,
            viewport_world,
            crate::data::ContentResolutionSource::ExternalPath {
                path: path.to_string_lossy().into_owned(),
            },
        );
        let request = crate::placement::content_resolution::import_path_request(
            path.clone(),
            reservation,
            desired_world,
            viewport_world,
            self.import_request_config(),
            self.perf_log.as_ref().map(|_| crate::jobs::ImagePrepTrace {
                queued_at: Instant::now(),
                timings: crate::jobs::ImagePrepTimings::default(),
            }),
        );
        if let Err(error) = self
            .import_worker
            .as_ref()
            .expect("perf scenario requires import worker")
            .enqueue(request)
        {
            self.remove_import_reservation(reservation);
            return Err(error);
        }
        if let Some(scenario) = &mut self.perf_scenario {
            let bytes = std::fs::metadata(&path).ok().map(|metadata| metadata.len());
            scenario.asset_by_queue_id.insert(
                reservation.queue_id,
                PerfCorpusAsset {
                    file: entry.file.clone(),
                    format: entry.format.clone(),
                    animated: entry.animated,
                    bytes,
                },
            );
            scenario.emit(
                "import_queued",
                serde_json::json!({
                    "file": entry.file,
                    "format": entry.format,
                    "animated": entry.animated,
                    "role": entry.role,
                    "width": dimensions.map(|value| value.0),
                    "height": dimensions.map(|value| value.1),
                    "bytes": bytes,
                    "queue_id": reservation.queue_id,
                }),
            );
        }
        Ok(())
    }

    fn dispatch_perf_trickle_imports(&mut self, ctx: &egui::Context, screen_rect: egui::Rect) {
        let (next_at, interval) = match self.perf_scenario.as_ref() {
            Some(scenario) if !scenario.pending_trickle_imports.is_empty() => {
                (scenario.trickle_next_at, scenario.trickle_interval)
            }
            _ => return,
        };

        let Some(next_at) = next_at else {
            return;
        };
        if Instant::now() < next_at {
            // Make sure the egui main loop revisits us as soon as the
            // trickle window opens, even when nothing else is changing.
            ctx.request_repaint_after(next_at - Instant::now());
            return;
        }

        let desired_world = self.preferred_import_world(ctx, screen_rect);
        let viewport_world = self.import_viewport_world_rect(screen_rect);
        let mut layout = match self.import_arrangement_input_snapshot() {
            Ok(layout) => layout,
            Err(error) => {
                self.emit_perf_error_and_close(ctx, error);
                return;
            }
        };

        // Pop one entry now, schedule the next interval if more remain.
        let entry = match self
            .perf_scenario
            .as_mut()
            .and_then(|scenario| scenario.pending_trickle_imports.first().cloned())
        {
            Some(entry) => entry,
            None => return,
        };

        if let Err(error) =
            self.enqueue_perf_entry(&entry, &mut layout, desired_world, viewport_world)
        {
            self.emit_perf_error_and_close(ctx, error);
            return;
        }

        if let Some(scenario) = &mut self.perf_scenario {
            scenario.pending_trickle_imports.remove(0);
            scenario.trickle_next_at = if scenario.pending_trickle_imports.is_empty() {
                None
            } else {
                Some(Instant::now() + interval)
            };
            if let Some(next) = scenario.trickle_next_at {
                ctx.request_repaint_after(next - Instant::now());
            } else {
                ctx.request_repaint_after(Duration::from_millis(16));
            }
        }
    }

    fn emit_perf_error_and_close(&mut self, ctx: &egui::Context, error: String) {
        if let Some(scenario) = &mut self.perf_scenario {
            scenario.emit("error", serde_json::json!({ "message": error }));
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }

    fn update_perf_scenario_embedding_progress(&mut self) {
        let Some(scenario) = &mut self.perf_scenario else {
            return;
        };
        #[cfg(feature = "semantic-search")]
        {
            for event in self.search.drain_perf_events() {
                match event {
                    crate::search::SemanticPerfEvent::ModelLoadStarted => {
                        scenario.emit("semantic_model_load_started", serde_json::json!({}));
                    }
                    crate::search::SemanticPerfEvent::ModelLoadTiming {
                        assets_resolved_ms,
                        tokenizer_loaded_ms,
                        text_session_loaded_ms,
                        vision_session_loaded_ms,
                        total_ms,
                    } => scenario.emit_model_load_timing(ModelLoadTimingSummary {
                        assets_resolved_ms,
                        tokenizer_loaded_ms,
                        text_session_loaded_ms,
                        vision_session_loaded_ms,
                        total_ms,
                    }),
                    crate::search::SemanticPerfEvent::ModelReady => {
                        scenario.emit("semantic_model_ready", serde_json::json!({}));
                    }
                    crate::search::SemanticPerfEvent::ImportEmbeddingStarted { asset_id } => {
                        let metadata = scenario.asset_by_asset_id.get(&asset_id).cloned();
                        let (format_value, source_file) = match &metadata {
                            Some(meta) => (Some(meta.format.clone()), Some(meta.file.clone())),
                            None => (None, None),
                        };
                        scenario.emit(
                            "import_embedding_started",
                            serde_json::json!({
                                "asset_id": asset_id,
                                "format": format_value,
                                "file": source_file,
                            }),
                        );
                        if scenario
                            .embedding_phases
                            .first_embedding_started_ms
                            .is_none()
                        {
                            scenario.emit(
                                "first_import_embedding_started",
                                serde_json::json!({
                                    "asset_id": asset_id,
                                    "format": format_value,
                                    "file": source_file,
                                }),
                            );
                        }
                    }
                    crate::search::SemanticPerfEvent::ImportEmbeddingFinished { asset_id } => {
                        scenario.demand_embeddings_done += 1;
                        let metadata = scenario.asset_by_asset_id.get(&asset_id).cloned();
                        let (format_value, source_file) = match &metadata {
                            Some(meta) => (Some(meta.format.clone()), Some(meta.file.clone())),
                            None => (None, None),
                        };
                        if !scenario.first_embedding_done_logged {
                            scenario.first_embedding_done_logged = true;
                            scenario.emit(
                                "first_import_embedding_done",
                                serde_json::json!({
                                    "asset_id": asset_id,
                                    "format": format_value,
                                    "file": source_file,
                                }),
                            );
                        }
                        scenario.emit(
                            "import_embedding_done",
                            serde_json::json!({
                                "done": scenario.demand_embeddings_done,
                                "asset_id": asset_id,
                                "format": format_value,
                                "file": source_file,
                            }),
                        );
                    }
                }
            }
            if scenario.demand_embeddings_queued > 0
                && scenario.demand_embeddings_done >= scenario.demand_embeddings_queued
                && !scenario.all_embeddings_done_logged
            {
                scenario.all_embeddings_done_logged = true;
                scenario.emit("all_import_embeddings_done", serde_json::json!({}));
            }
        }
    }

    fn finish_perf_scenario_if_idle(&mut self, ctx: &egui::Context) -> bool {
        let Some(scenario) = &mut self.perf_scenario else {
            return false;
        };
        let idle_hold = std::env::var("SNAPPYREF_PERF_IDLE_HOLD_SECS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .map(Duration::from_secs)
            .unwrap_or_else(|| Duration::from_secs(5));
        let worker_stats = self
            .import_worker
            .as_ref()
            .map(crate::jobs::ImagePrepWorker::stats)
            .unwrap_or_default();
        let embedding_idle = {
            #[cfg(feature = "semantic-search")]
            {
                self.search.demand_embedding_pending_count() == 0
                    && self.search.embedding_backfill_is_idle()
            }
            #[cfg(not(feature = "semantic-search"))]
            {
                true
            }
        };
        let idle = scenario.queued
            && scenario.visible_imports >= scenario.expected_imports
            && !worker_stats.has_work()
            && !self.canvas.has_pending_crop_work()
            && !self.residency.has_pending_image_work()
            && embedding_idle;

        if idle {
            let idle_since = *scenario.idle_since.get_or_insert_with(Instant::now);
            if idle_since.elapsed() >= Duration::from_millis(500) && !scenario.settled_idle_logged {
                scenario.emit("settled_idle", serde_json::json!({}));
                scenario.write_summary(worker_stats, self.frame_perf.canvas.image_pass);
                scenario.settled_idle_logged = true;
            }
            if scenario.settled_idle_logged && idle_since.elapsed() >= idle_hold {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                return true;
            }
            if scenario.settled_idle_logged {
                let mut next_repaint_after = idle_hold.saturating_sub(idle_since.elapsed());
                #[cfg(feature = "semantic-search")]
                if let Some(unload_after) =
                    std::env::var("SNAPPYREF_PERF_MODEL_UNLOAD_AFTER_IDLE_MS")
                        .ok()
                        .and_then(|value| value.parse::<u64>().ok())
                        .map(Duration::from_millis)
                {
                    if !scenario.model_unload_logged {
                        if idle_since.elapsed() >= unload_after {
                            if self.search.unload_model_for_perf() {
                                scenario.model_unload_logged = true;
                                scenario
                                    .emit("semantic_model_unload_requested", serde_json::json!({}));
                                scenario.emit("semantic_model_unloaded", serde_json::json!({}));
                            }
                        } else {
                            next_repaint_after =
                                next_repaint_after.min(unload_after - idle_since.elapsed());
                        }
                    }
                }
                ctx.request_repaint_after(next_repaint_after.max(Duration::from_millis(250)));
                return false;
            }
        } else {
            scenario.idle_since = None;
        }
        ctx.request_repaint_after(Duration::from_millis(16));
        false
    }

    fn record_perf_painted_frame(&mut self) {
        let Some(scenario) = &mut self.perf_scenario else {
            return;
        };
        if scenario.expected_imports == 0 || scenario.loaded_imports == 0 {
            return;
        }

        let image_pass = self.frame_perf.canvas.image_pass;
        let visible = image_pass
            .visible_images
            .min(scenario.loaded_imports)
            .min(scenario.expected_imports);
        if visible > scenario.visible_imports {
            scenario.visible_imports = visible;
        }
        if scenario.visible_imports == 0 {
            return;
        }

        let frame = serde_json::json!({
            "visible": scenario.visible_imports,
            "expected": scenario.expected_imports,
            "canvas_frame": {
                "visible_images": image_pass.visible_images,
                "visible_animated_images": image_pass.visible_animated_images,
                "active_animated_images": image_pass.active_animated_images,
                "fallback_images": image_pass.fallback_images,
                "draw_calls": image_pass.draw_calls,
                "texture_groups": image_pass.texture_groups,
                "build_time_ms": image_pass.build_time_ms,
            },
        });
        if !scenario.first_visible_logged {
            scenario.first_visible_logged = true;
            scenario.emit("first_placement_visible", frame.clone());
        }
        if scenario.visible_imports >= scenario.expected_imports && !scenario.all_visible_logged {
            scenario.all_visible_logged = true;
            scenario.emit("all_placements_visible", frame);
        }
    }

    /// True once all visible startup work queues have drained at least
    /// once — the same work-queue notion of Settled Idle that perf
    /// scenarios use, computed independently of the perf scenario.
    /// The update-discovery scheduler latches its own
    /// `settled_idle_at` on the first true return so subsequent dips
    /// back into busy state never re-block discovery.
    fn app_settled_idle(&self) -> bool {
        if self.library.is_none() {
            // Storage failed at startup; treat as settled so the user
            // still gets an Available Update pill if one exists.
            return true;
        }
        let worker_stats = self
            .import_worker
            .as_ref()
            .map(crate::jobs::ImagePrepWorker::stats)
            .unwrap_or_default();
        let embedding_idle = {
            #[cfg(feature = "semantic-search")]
            {
                self.search.demand_embedding_pending_count() == 0
                    && self.search.embedding_backfill_is_idle()
            }
            #[cfg(not(feature = "semantic-search"))]
            {
                true
            }
        };
        !worker_stats.has_work()
            && !self.canvas.has_pending_crop_work()
            && !self.residency.has_pending_image_work()
            && embedding_idle
    }

    fn board_summaries(&self) -> Vec<crate::data::BoardSummary> {
        self.board_summaries_cache.summaries()
    }

    fn refresh_board_summaries(&mut self) {
        let Some(library) = &self.library else {
            self.board_summaries_cache.clear();
            return;
        };
        if let Err(error) = self.board_summaries_cache.refresh_from(library) {
            self.status = error;
        }
    }

    fn create_and_switch_board(&mut self, keep_sidebar_open: bool) {
        let Some(library) = &self.library else {
            self.status = "Storage unavailable".to_owned();
            return;
        };

        let boards = match library.list_boards() {
            Ok(boards) => boards,
            Err(error) => {
                self.status = error;
                return;
            }
        };
        let name = generated_board_name(&boards);
        let board = match library.create_board(&name) {
            Ok(board) => board,
            Err(error) => {
                self.status = error;
                return;
            }
        };

        self.switch_to_board(board.id);
        self.refresh_board_summaries();
        if keep_sidebar_open {
            self.sidebar_open = true;
        }
        self.status = format!("Created {}", board.name);
    }

    fn switch_to_board(&mut self, board_id: i64) {
        if let Err(error) = crate::active_board::switch::switch_to_board(self, board_id) {
            self.status = error;
        }
    }

    fn handle_board_action(&mut self, action: Option<SidebarBoardAction>) {
        match action {
            Some(SidebarBoardAction::CreateBoard) => self.create_and_switch_board(true),
            Some(SidebarBoardAction::SwitchBoard(board_id)) => self.switch_to_board(board_id),
            Some(SidebarBoardAction::RenameBoard(board_id, name)) => {
                let Some(library) = &self.library else {
                    self.status = "Storage unavailable".to_owned();
                    return;
                };
                match library.rename_board(board_id, &name) {
                    Ok(()) => {
                        self.refresh_board_summaries();
                        self.status = "Renamed board".to_owned();
                    }
                    Err(error) => self.status = error,
                }
            }
            Some(SidebarBoardAction::RequestDeleteBoard(board_id)) => {
                let board_name = self
                    .board_summaries()
                    .into_iter()
                    .find(|summary| summary.board.id == board_id)
                    .map(|summary| summary.board.name)
                    .unwrap_or_else(|| "Board".to_owned());
                self.request_board_delete(board_id, board_name);
            }
            None => {}
        }
    }

    #[cfg(feature = "semantic-search")]
    fn handle_semantic_search_settings_action(
        &mut self,
        action: Option<SemanticSearchSettingsAction>,
        ctx: &egui::Context,
    ) {
        match action {
            Some(SemanticSearchSettingsAction::Install)
            | Some(SemanticSearchSettingsAction::Retry) => {
                self.search
                    .start_semantic_search_install_from_settings(&mut self.status);
                ctx.request_repaint_after(std::time::Duration::from_millis(16));
            }
            Some(SemanticSearchSettingsAction::Cancel) => {
                self.search
                    .cancel_semantic_search_install_from_settings(&mut self.status);
                ctx.request_repaint_after(std::time::Duration::from_millis(16));
            }
            Some(SemanticSearchSettingsAction::RequestRemove) => {
                self.pending_semantic_search_remove = true;
            }
            None => {}
        }
    }

    #[cfg(not(feature = "semantic-search"))]
    fn handle_semantic_search_settings_action(
        &mut self,
        _action: Option<SemanticSearchSettingsAction>,
        _ctx: &egui::Context,
    ) {
    }

    fn request_board_delete(&mut self, board_id: i64, board_name: String) {
        self.pending_board_delete = Some(PendingBoardDelete {
            board_id,
            board_name,
        });
    }

    fn cancel_pending_board_delete(&mut self) {
        self.pending_board_delete = None;
    }

    fn confirm_pending_board_delete(&mut self) {
        let Some(pending) = self.pending_board_delete.take() else {
            return;
        };

        self.save_active_board_before_transition();

        let Some(library) = &self.library else {
            self.status = "Storage unavailable".to_owned();
            return;
        };

        let deleted_active_board = pending.board_id == self.active_board_id;
        if let Err(error) = library.delete_board(pending.board_id) {
            self.status = error;
            return;
        }

        if deleted_active_board {
            self.load_active_board();
        }
        self.refresh_board_summaries();

        self.status = format!("Deleted {}", pending.board_name);
    }

    fn save_active_board_before_transition(&mut self) {
        self.persist_canvas_changes();
        self.save_user_session();
        self.flush_persistence();
    }

    fn load_active_board(&mut self) {
        crate::active_board::load::load_active_board(self);
    }

    fn dedupe_library_exact(
        &mut self,
        hash_limit: usize,
        group_limit: usize,
    ) -> Option<crate::library::storage::ExactDedupeReport> {
        let Some(library) = &self.library else {
            self.status = "Storage unavailable".to_owned();
            return None;
        };
        match library.dedupe_exact_assets(hash_limit, group_limit) {
            Ok(report) => {
                if report.changed() {
                    #[cfg(feature = "semantic-search")]
                    self.search.invalidate_library_search_cache();
                    self.refresh_board_summaries();
                }
                Some(report)
            }
            Err(error) => {
                self.status = error;
                None
            }
        }
    }

    fn dedupe_library_for_board_load(&mut self) {
        let _ =
            self.dedupe_library_exact(BOARD_LOAD_DEDUPE_HASH_LIMIT, BOARD_LOAD_DEDUPE_GROUP_LIMIT);
    }

    fn dedupe_library_from_shortcut(&mut self) {
        self.save_active_board_before_transition();
        let Some(report) = self.dedupe_library_exact(usize::MAX, usize::MAX) else {
            return;
        };
        if report.changed() {
            self.load_active_board();
            let noun = if report.duplicate_assets_removed == 1 {
                "duplicate"
            } else {
                "duplicates"
            };
            self.status = format!("Removed {} {noun}", report.duplicate_assets_removed);
        } else if report.hashed_assets > 0 {
            self.status = format!("Checked {} assets; no duplicates", report.hashed_assets);
        } else {
            self.status = "No exact duplicates found".to_owned();
        }
    }

    fn clear_active_board_transients(&mut self) {
        #[cfg(feature = "semantic-search")]
        {
            self.search.close_overlay(Some(&mut self.canvas));
            self.search.reset_embedding_backfill();
        }
        self.crop_flow.clear_undo_stack();
    }

    fn show_board_delete_confirmation(&mut self, ctx: &egui::Context) {
        let entrance = crate::ui::motion::overlay_entrance(
            ctx,
            egui::Id::new("board_delete_dialog_entrance"),
            self.pending_board_delete.is_some(),
        );
        let Some(pending) = self.pending_board_delete.clone() else {
            return;
        };

        let settle = crate::ui::motion::settle_offset(entrance);
        egui::Window::new("Delete board?")
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, settle))
            .frame(destructive_confirmation_frame(ctx, entrance))
            .show(ctx, |ui| {
                ui.multiply_opacity(entrance);
                ui.set_min_width(destructive_dialog_width());
                destructive_dialog_header(ui, "Delete board?");
                destructive_dialog_body(ui, |ui| {
                    ui.label(
                        egui::RichText::new(format!("Delete \"{}\"?", pending.board_name))
                            .color(crate::ui::theme::TEXT),
                    );
                    ui.add_space(crate::ui::theme::SPACE_1);
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(
                                "Images stay in the shared Library. Placements, arrows, lines, and textboxes on this board will be removed.",
                            )
                            .text_style(egui::TextStyle::Small)
                            .color(crate::ui::theme::TEXT_MUTED),
                        )
                        .wrap(),
                    );
                    ui.add_space(crate::ui::theme::SPACE_3);
                    ui.horizontal(|ui| {
                        if ui.add(secondary_dialog_button("Cancel")).clicked() {
                            self.cancel_pending_board_delete();
                        }
                        if ui.add(destructive_action_button("Delete board")).clicked() {
                            self.confirm_pending_board_delete();
                        }
                    });
                });
            });
    }

    fn modal_open(&self) -> bool {
        self.pending_board_delete.is_some()
            || self.pending_library_remove.is_some()
            || {
                #[cfg(feature = "semantic-search")]
                {
                    self.pending_semantic_search_remove
                }
                #[cfg(not(feature = "semantic-search"))]
                {
                    false
                }
            }
            || self.should_show_semantic_search_first_launch_prompt()
    }

    fn should_block_sidebar_input(&self) -> bool {
        self.sidebar_open && self.modal_open()
    }

    /// Build the Settings Tab "About / Updates" view payload from the
    /// internal updater state machine.
    fn about_updates_view(&self) -> crate::ui::sidebar::AboutUpdatesView {
        let check_now_enabled = match self.last_manual_update_check {
            None => true,
            Some(instant) => instant.elapsed() >= MANUAL_UPDATE_CHECK_THROTTLE,
        };
        let (update_button_label, update_button_enabled, update_button_primary, update_status) =
            self.about_update_action_view(check_now_enabled);
        crate::ui::sidebar::AboutUpdatesView {
            current_version: self.update_service.current_version().to_owned(),
            last_checked_unix_millis: self.update_service.last_check_completed_at(),
            update_button_label,
            update_button_enabled,
            update_button_primary,
            update_status,
            semantic_search: crate::ui::sidebar::SemanticSearchSettingsView {
                #[cfg(feature = "semantic-search")]
                status: semantic_search_settings_status(self.search.local_model_store_status()),
                #[cfg(not(feature = "semantic-search"))]
                status: SemanticSearchSettingsStatus::Failed,
            },
        }
    }

    fn about_update_action_view(
        &self,
        check_now_enabled: bool,
    ) -> (String, bool, bool, Option<String>) {
        if self.update_service.check_in_flight() {
            return (
                "Checking for updates".to_owned(),
                false,
                false,
                Some("Checking for updates...".to_owned()),
            );
        }
        match self.update_service.state() {
            UpdateState::Hidden => {
                let status = self
                    .update_service
                    .latest_known()
                    .then(|| "You are on the latest version.".to_owned());
                (
                    "Check for updates".to_owned(),
                    check_now_enabled,
                    false,
                    status,
                )
            }
            UpdateState::Available { version, .. } => (
                "Update".to_owned(),
                true,
                true,
                Some(format!("Version {version} is available.")),
            ),
            UpdateState::Downloading { percent, .. } => (
                "Updating".to_owned(),
                false,
                false,
                Some(format!("Downloading {percent}%")),
            ),
            UpdateState::Ready { .. } if self.user_update_flow_active => (
                "Finishing update".to_owned(),
                false,
                false,
                Some("Restarting to finish update...".to_owned()),
            ),
            UpdateState::Ready { .. } => (
                "Restart".to_owned(),
                true,
                true,
                Some("Update ready. Restart to finish.".to_owned()),
            ),
            UpdateState::Failed => (
                "Check for updates".to_owned(),
                check_now_enabled,
                false,
                Some("Update check failed.".to_owned()),
            ),
        }
    }

    /// Mirror the update service's most recent check timestamp into the
    /// persisted Workspace Settings so it survives across launches.
    /// Returns true when the value changed and the caller should persist.
    fn sync_last_update_check_timestamp(&mut self) -> bool {
        let current = self.update_service.last_check_completed_at();
        if current != self.settings.last_update_check_at {
            self.settings.last_update_check_at = current;
            true
        } else {
            false
        }
    }

    fn handle_update_action_click(&mut self, ctx: &egui::Context) {
        match self.update_service.state().clone() {
            UpdateState::Available { .. } => {
                self.user_update_flow_active = true;
                self.update_service.start_install();
                ctx.request_repaint();
            }
            UpdateState::Ready { .. } => {
                self.user_update_flow_active = true;
                self.trigger_restart_into_update(ctx);
            }
            UpdateState::Downloading { .. } => {}
            UpdateState::Hidden | UpdateState::Failed => {
                self.request_manual_update_check(ctx);
            }
        }
    }

    /// Process a Settings Tab update check click. Throttled to one
    /// invocation per [`MANUAL_UPDATE_CHECK_THROTTLE`].
    fn request_manual_update_check(&mut self, ctx: &egui::Context) {
        if !self.update_service.check_in_flight() {
            let now = Instant::now();
            if let Some(last) = self.last_manual_update_check {
                if now.duration_since(last) < MANUAL_UPDATE_CHECK_THROTTLE {
                    return;
                }
            }
            self.last_manual_update_check = Some(now);
            self.update_service.request_check_now();
        }
        ctx.request_repaint();
    }

    fn drive_user_update_flow(&mut self, ctx: &egui::Context) {
        if !self.user_update_flow_active {
            return;
        }
        match self.update_service.state().clone() {
            UpdateState::Ready { .. } => self.trigger_restart_into_update(ctx),
            UpdateState::Failed => self.user_update_flow_active = false,
            UpdateState::Downloading { .. } => {
                ctx.request_repaint_after(Duration::from_millis(250))
            }
            _ => {}
        }
    }

    fn write_update_smoke_status(&self, status: &str) {
        let Some(path) = self
            .update_smoke
            .as_ref()
            .and_then(|smoke| smoke.status_path.as_ref())
        else {
            return;
        };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(path, status);
    }

    fn fail_update_smoke(&mut self, ctx: &egui::Context, reason: impl Into<String>) {
        let reason = reason.into();
        self.status = format!("Update smoke failed: {reason}");
        self.write_update_smoke_status(&format!("failed:{reason}"));
        self.update_smoke = None;
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }

    fn drive_update_smoke(&mut self, ctx: &egui::Context, settled_idle: bool) {
        let Some(smoke) = self.update_smoke.as_ref() else {
            return;
        };
        if smoke.started_at.elapsed() > smoke.timeout {
            let phase = smoke.phase_label();
            self.fail_update_smoke(ctx, format!("timeout in {phase}"));
            return;
        }

        match smoke.phase {
            UpdateSmokePhase::WaitingForIdle => {
                if !settled_idle {
                    ctx.request_repaint_after(Duration::from_millis(250));
                    return;
                }
                self.write_update_smoke_status("checking");
                self.update_service.request_check_now();
                if let Some(smoke) = &mut self.update_smoke {
                    smoke.phase = UpdateSmokePhase::WaitingForAvailable;
                }
                ctx.request_repaint_after(Duration::from_millis(250));
            }
            UpdateSmokePhase::WaitingForAvailable => match self.update_service.state().clone() {
                UpdateState::Available { version, .. } => {
                    self.write_update_smoke_status(&format!("available:{version}"));
                    self.update_service.start_install();
                    if let Some(smoke) = &mut self.update_smoke {
                        smoke.phase = UpdateSmokePhase::WaitingForReady;
                    }
                    ctx.request_repaint_after(Duration::from_millis(250));
                }
                UpdateState::Ready { version } => {
                    self.write_update_smoke_status(&format!("triggered:{version}"));
                    if let Some(smoke) = &mut self.update_smoke {
                        smoke.phase = UpdateSmokePhase::TriggeredRestart;
                    }
                    self.trigger_restart_into_update(ctx);
                }
                UpdateState::Failed => {
                    self.fail_update_smoke(ctx, "update service failed");
                }
                _ => {
                    ctx.request_repaint_after(Duration::from_millis(250));
                }
            },
            UpdateSmokePhase::WaitingForReady => match self.update_service.state().clone() {
                UpdateState::Ready { version } => {
                    self.write_update_smoke_status(&format!("triggered:{version}"));
                    if let Some(smoke) = &mut self.update_smoke {
                        smoke.phase = UpdateSmokePhase::TriggeredRestart;
                    }
                    self.trigger_restart_into_update(ctx);
                }
                UpdateState::Failed => {
                    self.fail_update_smoke(ctx, "update service failed");
                }
                _ => {
                    ctx.request_repaint_after(Duration::from_millis(250));
                }
            },
            UpdateSmokePhase::TriggeredRestart => {
                ctx.request_repaint_after(Duration::from_millis(250));
            }
        }
    }

    /// Drive the swap-and-relaunch flow when the user clicks the Ready
    /// pill. Sequence (ADR-0015):
    ///
    /// 1. close any open popover.
    /// 2. save the User Session — the new app's first launch reads it back.
    /// 3. drop worker child processes (embedding helper, import worker)
    ///    so the swap is not racing live filesystem writers.
    /// 4. apply the staged update using the platform's swap path:
    ///    Windows launches a temporary self-replace runner; macOS writes
    ///    a pending-update descriptor and launches `snappyref-updater`.
    fn trigger_restart_into_update(&mut self, _ctx: &egui::Context) {
        self.user_update_flow_active = false;

        // 2. Save the User Session so the new app can restore it.
        self.save_active_board_before_transition();

        // 3. Stop worker child processes. ImagePrepWorker's queue + thread
        // shutdown runs in its Drop impl; the embedding helper's stdin
        // child process is killed by EmbeddingHelperClient::drop, which
        // fires when the SearchController is dropped or rebuilt.
        self.import_worker = None;
        #[cfg(feature = "semantic-search")]
        {
            self.search.shutdown_helper_processes();
        }

        // 4. Build + write the descriptor.
        let launch_args = restart_launch_args();
        let Some(descriptor) = self.update_service.pending_restart_descriptor(launch_args) else {
            self.status = "No staged update is ready to restart into".to_owned();
            return;
        };
        #[cfg(target_os = "windows")]
        {
            let descriptor_path =
                crate::updater::descriptor::default_path(&descriptor.install_root);
            if let Err(error) = crate::updater::windows_self_replace::apply_and_relaunch(
                &descriptor,
                &descriptor_path,
            ) {
                if self.update_smoke.is_some() {
                    self.fail_update_smoke(_ctx, format!("apply failed: {error}"));
                    return;
                }
                self.status = format!("Failed to apply Windows update: {error}");
                return;
            }
            std::process::exit(0);
        }
        #[cfg(not(target_os = "windows"))]
        {
            let descriptor_path =
                crate::updater::descriptor::default_path(&descriptor.install_root);
            if let Err(error) = crate::updater::descriptor::write(&descriptor_path, &descriptor) {
                self.status = format!("Failed to write update descriptor: {error}");
                return;
            }

            // 5. Spawn the helper detached. argv[1] is the descriptor path.
            let helper_path = match snappyref_updater_path() {
                Ok(path) => path,
                Err(error) => {
                    self.status = format!("Failed to locate updater helper: {error}");
                    return;
                }
            };
            if let Err(error) = std::process::Command::new(&helper_path)
                .arg(&descriptor_path)
                .spawn()
            {
                self.status = format!("Failed to launch updater helper: {error}");
                return;
            }

            // 6. Tell egui to close — the helper polls our PID and starts the
            // swap as soon as we exit.
            _ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    fn load_user_session(&mut self) {
        let Some(library) = &self.library else {
            return;
        };

        match library.load_user_session() {
            Ok(session) => {
                self.canvas.apply_viewport_state(&session.viewport);
                self.active_board_id = session.active_board_id;
                self.window_state = session.window;
                self.settings = session.settings;
                self.settings.normalize_core_features();
                self.sidebar_open = session.sidebar_open;
                self.sidebar_tab = sidebar_tab_from_session(&session.sidebar_tab);
                #[cfg(feature = "semantic-search")]
                self.search.set_recent_searches(session.recent_searches);
                self.canvas
                    .set_rotation_snap_degrees(self.settings.rotation_snap_degrees);
                self.canvas.set_placement_gap(self.settings.placement_gap);
                self.canvas.set_pointer_mode(self.settings.pointer_mode);
                self.update_service
                    .seed_last_check_completed_at(self.settings.last_update_check_at);
                self.document_persistence
                    .record_saved_user_session(self.current_user_session());
            }
            Err(error) => self.status = error,
        }
    }

    fn load_session_viewport(&mut self) {
        let Some(library) = &self.library else {
            return;
        };

        match library.load_session_viewport() {
            Ok((active_board_id, viewport)) => {
                self.active_board_id = active_board_id;
                self.canvas.apply_viewport_state(&viewport);
                self.document_persistence
                    .record_saved_user_session(self.current_user_session());
            }
            Err(error) => self.status = error,
        }
    }

    fn startup_viewport_world_rect(&self) -> egui::Rect {
        let screen_rect = egui::Rect::from_min_size(egui::Pos2::ZERO, STARTUP_VIEWPORT_SIZE);
        egui::Rect::from_min_max(
            self.canvas.screen_to_world(screen_rect.min, screen_rect),
            self.canvas.screen_to_world(screen_rect.max, screen_rect),
        )
    }

    fn visible_canvas_screen_rect(&self, screen_rect: egui::Rect) -> egui::Rect {
        let sidebar_rect = sidebar_rect(screen_rect);
        let left = if self.sidebar_open {
            sidebar_rect.right()
        } else {
            screen_rect.left()
        };
        egui::Rect::from_min_max(egui::pos2(left, screen_rect.top()), screen_rect.max)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    fn import_size_world(&self, width: u32, height: u32) -> egui::Vec2 {
        crate::sources::import::import_preview_size_world(
            width,
            height,
            self.settings.placement_target_area,
        )
    }

    fn placement_creation_settings(&self) -> crate::placement::creation::PlacementCreationSettings {
        crate::placement::creation::PlacementCreationSettings {
            placement_target_area: self.settings.placement_target_area,
            gap: self.settings.placement_gap,
        }
    }

    fn import_request_config(&self) -> ContentResolutionRequestConfig {
        ContentResolutionRequestConfig {
            max_texture_side: self.max_texture_side,
            canvas_image_max_side: self.settings.canvas_image_max_side,
            auto_image_scaling: true,
            placement_target_area: self.settings.placement_target_area,
            placement_gap: self.settings.placement_gap,
        }
    }

    fn import_trace(&self) -> Option<crate::jobs::ImagePrepTrace> {
        self.perf_log.as_ref().map(|_| crate::jobs::ImagePrepTrace {
            queued_at: Instant::now(),
            timings: crate::jobs::ImagePrepTimings::default(),
        })
    }

    fn import_arrangement_input_snapshot(
        &self,
    ) -> Result<Vec<crate::placement::arrangement::PlacementArrangementInput>, String> {
        let layout = if let Some(library) = &self.library {
            library.load_placement_arrangement_inputs()?
        } else {
            Vec::new()
        };
        Ok(layout)
    }

    fn reserve_import(
        &mut self,
        layout: &mut Vec<crate::placement::arrangement::PlacementArrangementInput>,
        label: String,
        dimensions: Option<(u32, u32)>,
        desired_world: egui::Pos2,
        viewport_world: egui::Rect,
        source: crate::data::ContentResolutionSource,
    ) -> ContentResolutionReservation {
        let _ = viewport_world;
        self.create_committed_loading_import(
            layout,
            crate::placement::creation::SourceIntent {
                label,
                dimensions,
                approximate_dimensions: dimensions.is_none(),
                content: source,
            },
            desired_world,
        )
    }

    fn create_committed_loading_import(
        &mut self,
        layout: &mut Vec<crate::placement::arrangement::PlacementArrangementInput>,
        intent: crate::placement::creation::SourceIntent,
        center_world: egui::Pos2,
    ) -> ContentResolutionReservation {
        let Some(library) = &self.library else {
            self.status = "Storage unavailable".to_owned();
            return self
                .content_resolution
                .reserve_untracked(self.active_board_id, center_world);
        };

        let settings = self.placement_creation_settings();
        match crate::placement::creation::create_committed_loading_placement(
            library,
            &mut self.content_resolution,
            &mut self.canvas,
            layout,
            self.active_board_id,
            intent,
            center_world,
            settings,
        ) {
            Ok(created) => {
                self.persist_canvas_changes();
                created.reservation
            }
            Err(error) => {
                self.status = error;
                self.content_resolution
                    .reserve_untracked(self.active_board_id, center_world)
            }
        }
    }

    fn remove_import_reservation(&mut self, reservation: ContentResolutionReservation) {
        self.content_resolution.cancel(reservation);
    }

    fn handle_hovered_files(&mut self, ui: &egui::Ui, screen_rect: egui::Rect) {
        let hover_intake = crate::sources::external_drag::hovered(ui);
        let hovered_sources = hover_intake.sources.clone();
        if hovered_sources.is_empty() {
            crate::diagnostics::search_drag_debug(format!(
                "external_hover empty cleanup external_ids={:?} pending_fallback_before={} selected={:?}",
                self.external_hover_provisional_placement_ids,
                self.pending_missing_drop_fallback.is_some(),
                self.canvas.selected_placement_ids()
            ));
            self.pending_missing_drop_fallback = self.last_hovered_external_drag.take();
            self.canvas
                .delete_placements(&self.external_hover_provisional_placement_ids);
            self.external_hover_provisional_placement_ids.clear();
            self.hover_preview_dimensions.clear();
            return;
        }
        crate::diagnostics::search_drag_debug(format!(
            "external_hover sources count={} screen_pos={:?}",
            hovered_sources.len(),
            hover_intake.screen_pos
        ));
        if self.last_hovered_external_drag.as_ref() == Some(&hover_intake)
            && !self.external_hover_provisional_placement_ids.is_empty()
        {
            return;
        }
        self.last_hovered_external_drag = Some(hover_intake.clone());
        self.pending_missing_drop_fallback = None;

        let hovered_paths = hovered_sources
            .iter()
            .filter_map(|source| match source {
                ExternalImportSource::Path(path) => Some(path.clone()),
                ExternalImportSource::Url(_) => None,
            })
            .collect::<Vec<_>>();
        self.hover_preview_dimensions
            .retain(|path, _| hovered_paths.contains(path));

        const FALLBACK_WIDTH: u32 = 360;
        const FALLBACK_HEIGHT: u32 = 240;

        let mut intents = Vec::with_capacity(hovered_sources.len());
        for source in &hovered_sources {
            let dimensions = match source {
                ExternalImportSource::Path(path) => {
                    let dimensions = self
                        .hover_preview_dimensions
                        .entry(path.clone())
                        .or_insert_with(|| image::image_dimensions(path).ok());
                    *dimensions
                }
                ExternalImportSource::Url(_) => None,
            };
            intents.push(
                crate::placement::creation::ProvisionalSourceIntent::SourceIntent(
                    crate::placement::creation::SourceIntent {
                        label: crate::sources::import::import_source_label(source),
                        dimensions: Some(dimensions.unwrap_or((FALLBACK_WIDTH, FALLBACK_HEIGHT))),
                        approximate_dimensions: dimensions.is_none(),
                        content: Self::placement_content_intent_for_external_source(source),
                    },
                ),
            );
        }

        let anchor_world =
            self.preferred_import_world_with_pointer(screen_rect, hover_intake.screen_pos);
        let settings = self.placement_creation_settings();
        let batch = crate::placement::lifecycle::start_provisional_batch(
            &mut self.canvas,
            intents,
            anchor_world,
            screen_rect,
            settings,
        );
        self.external_hover_provisional_placement_ids = batch.placement_ids;
    }

    #[cfg_attr(not(test), allow(dead_code))]
    fn dropped_import_previews(
        &mut self,
        sources: &[ExternalImportSource],
        desired_world: egui::Pos2,
        screen_rect: egui::Rect,
    ) -> Vec<(String, Option<(u32, u32)>, egui::Pos2)> {
        let specs = sources
            .iter()
            .map(Self::source_intent_for_external_source)
            .collect::<Vec<_>>();

        crate::placement::creation::plan_source_intent_previews(
            &mut self.canvas,
            &specs,
            desired_world,
            screen_rect,
            crate::placement::creation::PlacementCreationSettings {
                placement_target_area: self.settings.placement_target_area,
                gap: self.settings.placement_gap,
            },
        )
        .into_iter()
        .map(|preview| {
            let center_world = preview.center_world();
            (preview.label, preview.dimensions, center_world)
        })
        .collect()
    }

    fn source_intent_for_external_source(
        source: &ExternalImportSource,
    ) -> crate::placement::creation::SourceIntent {
        let label = crate::sources::import::import_source_label(source);
        let dimensions = match source {
            ExternalImportSource::Path(path) => image::image_dimensions(path).ok(),
            ExternalImportSource::Url(_) => None,
        };
        crate::placement::creation::SourceIntent {
            label,
            dimensions,
            approximate_dimensions: dimensions.is_none(),
            content: Self::placement_content_intent_for_external_source(source),
        }
    }

    fn handle_dropped_files(&mut self, ui: &egui::Ui, screen_rect: egui::Rect) {
        if self.import_worker.is_none() {
            return;
        }

        let drop_intake = crate::sources::external_drag::take_drop(ui);
        let (dropped_sources, drop_screen_pos) = if drop_intake.sources.is_empty() {
            let Some(fallback) = self.take_missing_drop_event_fallback() else {
                return;
            };
            (fallback.sources, fallback.screen_pos)
        } else {
            self.last_hovered_external_drag = None;
            self.pending_missing_drop_fallback = None;
            (drop_intake.sources, drop_intake.screen_pos)
        };
        if dropped_sources.is_empty() {
            return;
        }

        let desired_world = self.preferred_import_world_with_pointer(screen_rect, drop_screen_pos);
        self.enqueue_external_import_sources(dropped_sources, screen_rect, desired_world);
        self.external_hover_provisional_placement_ids.clear();
    }

    fn take_missing_drop_event_fallback(
        &mut self,
    ) -> Option<crate::sources::external_drag::ExternalDragIntake> {
        self.pending_missing_drop_fallback
            .take()
            .filter(|fallback| !fallback.sources.is_empty())
    }

    fn register_import_batch(
        &mut self,
        target_board_id: i64,
        anchor_world: egui::Pos2,
        queue_ids: Vec<u64>,
    ) {
        self.pending_import_batches.register_multi_item_batch(
            target_board_id,
            anchor_world,
            queue_ids,
        );
    }

    #[allow(dead_code)]
    fn register_search_import_batch(
        &mut self,
        target_board_id: i64,
        anchor_world: egui::Pos2,
        queue_ids: Vec<u64>,
    ) {
        self.pending_import_batches.register_any_item_batch(
            target_board_id,
            anchor_world,
            queue_ids,
        );
    }

    fn finish_import_queue_id_in_batches(
        &mut self,
        target_board_id: Option<i64>,
        queue_id: Option<u64>,
        loaded_placement_id: Option<i64>,
    ) {
        let completed_batches = self.pending_import_batches.finish_queue_id(
            target_board_id,
            queue_id,
            loaded_placement_id,
        );
        for batch in completed_batches {
            self.finalize_completed_import_batch(
                batch.target_board_id,
                batch.anchor_world,
                batch.placement_ids,
            );
        }
    }

    fn finalize_completed_import_batch(
        &mut self,
        target_board_id: i64,
        anchor_world: egui::Pos2,
        placement_ids: Vec<i64>,
    ) {
        if placement_ids.is_empty() || target_board_id != self.active_board_id {
            return;
        }
        self.canvas.select_placements(&placement_ids);
        let arranged = self
            .canvas
            .arrange_selected_compact_preserving_sizes(self.settings.placement_gap);
        if arranged > 0 {
            self.status = format!("Compacted {arranged} imported images");
        }
        let _ = anchor_world;
        self.persist_canvas_changes();
    }

    fn handle_clipboard_paste(&mut self, ctx: &egui::Context, screen_rect: egui::Rect) {
        let paste_text = ctx.input(|input| {
            input.events.iter().find_map(|event| match event {
                egui::Event::Paste(text) => Some(text.clone()),
                _ => None,
            })
        });
        let paste_requested = paste_text.is_some()
            || ctx.input(|input| input.modifiers.command && input.key_pressed(egui::Key::V));
        if !paste_requested {
            return;
        }

        let desired_world = self.preferred_import_world(ctx, screen_rect);
        self.handle_clipboard_paste_at(screen_rect, desired_world, paste_text);
    }

    fn handle_clipboard_paste_at(
        &mut self,
        screen_rect: egui::Rect,
        desired_world: egui::Pos2,
        paste_text: Option<String>,
    ) {
        if let Some(paste_text) = paste_text {
            if self.handle_internal_clipboard_paste_at(&paste_text, desired_world) {
                return;
            }

            if self.handle_clipboard_url_import_at(screen_rect, &paste_text, desired_world) {
                return;
            }
        }

        if self.handle_clipboard_path_import_at(screen_rect, desired_world) {
            return;
        }

        if self.handle_pureref_clipboard_import_at(screen_rect, desired_world) {
            return;
        }

        self.handle_external_clipboard_image_paste_at(screen_rect, desired_world);
    }

    fn handle_clipboard_path_import_at(
        &mut self,
        screen_rect: egui::Rect,
        desired_world: egui::Pos2,
    ) -> bool {
        if self.import_worker.is_none() {
            return false;
        }
        let sources = crate::platform::native_drop::clipboard_paths()
            .into_iter()
            .map(ExternalImportSource::Path)
            .collect::<Vec<_>>();
        if sources.is_empty() {
            return false;
        }

        self.enqueue_external_import_sources(sources, screen_rect, desired_world);
        true
    }

    fn handle_pureref_clipboard_import_at(
        &mut self,
        screen_rect: egui::Rect,
        desired_world: egui::Pos2,
    ) -> bool {
        if self.import_worker.is_none() {
            return false;
        }

        let Some(bytes) = crate::platform::native_drop::clipboard_pureref_binary() else {
            return false;
        };
        let sources = crate::sources::pureref::sources_from_binary(&bytes);
        if sources.is_empty() {
            return false;
        }

        let source_count = sources.len();
        self.enqueue_external_import_sources(sources, screen_rect, desired_world);
        self.status = if source_count == 1 {
            "Queued PureRef paste: 1 image".to_owned()
        } else {
            format!("Queued PureRef paste: {source_count} images")
        };
        true
    }

    fn handle_external_clipboard_image_paste_at(
        &mut self,
        screen_rect: egui::Rect,
        desired_world: egui::Pos2,
    ) {
        if self.import_worker.is_none() {
            return;
        }

        let mut clipboard = match arboard::Clipboard::new() {
            Ok(clipboard) => clipboard,
            Err(error) => {
                self.status = format!("Clipboard unavailable: {error}");
                return;
            }
        };

        let clipboard_image = match clipboard.get_image() {
            Ok(image) => image,
            Err(error) => {
                self.status = format!("Clipboard does not contain an image: {error}");
                return;
            }
        };

        let viewport_world = self.import_viewport_world_rect(screen_rect);
        let rgba = clipboard_image.bytes.into_owned();
        let width = clipboard_image.width as u32;
        let height = clipboard_image.height as u32;
        let mut layout = match self.import_arrangement_input_snapshot() {
            Ok(layout) => layout,
            Err(error) => {
                self.status = error;
                return;
            }
        };
        let reservation = self.reserve_import(
            &mut layout,
            "Clipboard".to_owned(),
            Some((width, height)),
            desired_world,
            viewport_world,
            crate::data::ContentResolutionSource::Clipboard,
        );

        if let Err(error) = self
            .import_worker
            .as_ref()
            .expect("checked import worker")
            .enqueue(
                crate::placement::content_resolution::import_clipboard_request(
                    rgba,
                    width,
                    height,
                    reservation,
                    desired_world,
                    viewport_world,
                    self.import_request_config(),
                    self.import_trace(),
                ),
            )
        {
            self.remove_import_reservation(reservation);
            self.status = error;
            return;
        }

        self.status = "Queued clipboard image".to_owned();
    }

    fn handle_clipboard_url_import_at(
        &mut self,
        screen_rect: egui::Rect,
        paste_text: &str,
        desired_world: egui::Pos2,
    ) -> bool {
        if self.import_worker.is_none() {
            return false;
        }
        let Some(url) = crate::sources::import::normalize_import_url(paste_text) else {
            return false;
        };

        let viewport_world = self.import_viewport_world_rect(screen_rect);
        let mut layout = match self.import_arrangement_input_snapshot() {
            Ok(layout) => layout,
            Err(error) => {
                self.status = error;
                return true;
            }
        };
        let reservation = self.reserve_import(
            &mut layout,
            crate::sources::import::import_source_label(&ExternalImportSource::Url(url.clone())),
            Some((360, 240)),
            desired_world,
            viewport_world,
            crate::data::ContentResolutionSource::ExternalUrl { url: url.clone() },
        );

        if let Err(error) = self
            .import_worker
            .as_ref()
            .expect("checked import worker")
            .enqueue(crate::placement::content_resolution::import_url_request(
                url.clone(),
                reservation,
                desired_world,
                viewport_world,
                self.import_request_config(),
                self.import_trace(),
            ))
        {
            self.remove_import_reservation(reservation);
            self.status = error;
            return true;
        }

        self.status = format!("Queued URL import: {url}");
        true
    }

    fn enqueue_external_import_sources(
        &mut self,
        sources: Vec<ExternalImportSource>,
        screen_rect: egui::Rect,
        desired_world: egui::Pos2,
    ) {
        if self.import_worker.is_none() || sources.is_empty() {
            return;
        }

        let viewport_world = self.import_viewport_world_rect(screen_rect);
        let mut layout = match self.import_arrangement_input_snapshot() {
            Ok(layout) => layout,
            Err(error) => {
                self.status = error;
                return;
            }
        };

        let placement_creation_settings = self.placement_creation_settings();
        let created = match self.library.clone().map_or_else(
            || Err("Storage unavailable".to_owned()),
            |library| {
                if self.external_hover_provisional_placement_ids.len() == sources.len() {
                    let committed = crate::placement::lifecycle::commit_loading_provisional_batch(
                        &library,
                        &mut self.canvas,
                        &self.external_hover_provisional_placement_ids,
                    )?;
                    let mut created = Vec::with_capacity(committed.placements.len());
                    for placement in committed.placements {
                        let reservation = self.content_resolution.reserve_placement(
                            self.active_board_id,
                            placement.state.id,
                            placement.state.geometry.center,
                        );
                        layout.push(
                            crate::placement::arrangement::PlacementArrangementInput::new(
                                placement.state.id,
                                placement.state.geometry.center,
                                placement.state.geometry.scaled_size(),
                            ),
                        );
                        created.push(crate::placement::creation::CreatedLoadingPlacement {
                            reservation,
                            placement,
                        });
                    }
                    return Ok(created);
                }

                let source_intents = sources
                    .iter()
                    .map(Self::source_intent_for_external_source)
                    .collect::<Vec<_>>();
                crate::placement::creation::create_committed_loading_placements(
                    &library,
                    &mut self.content_resolution,
                    &mut self.canvas,
                    &mut layout,
                    self.active_board_id,
                    source_intents,
                    desired_world,
                    screen_rect,
                    placement_creation_settings,
                )
            },
        ) {
            Ok(created) => created,
            Err(error) => {
                self.status = error;
                return;
            }
        };
        self.persist_canvas_changes();

        let mut queued_batch_ids = Vec::new();
        let mut queued_placement_ids = Vec::new();

        for (source, created) in sources.into_iter().zip(created) {
            let reservation = created.reservation;
            let request = match &source {
                ExternalImportSource::Path(source_path) => {
                    crate::placement::content_resolution::import_path_request(
                        source_path.clone(),
                        reservation,
                        desired_world,
                        viewport_world,
                        self.import_request_config(),
                        self.import_trace(),
                    )
                }
                ExternalImportSource::Url(url) => {
                    crate::placement::content_resolution::import_url_request(
                        url.clone(),
                        reservation,
                        desired_world,
                        viewport_world,
                        self.import_request_config(),
                        self.import_trace(),
                    )
                }
            };

            if let Err(error) = self
                .import_worker
                .as_ref()
                .expect("checked import worker")
                .enqueue(request)
            {
                self.remove_import_reservation(reservation);
                self.status = error;
                continue;
            }

            queued_batch_ids.push(reservation.queue_id);
            queued_placement_ids.push(created.placement.state.id);
            self.status = match &source {
                ExternalImportSource::Path(path) => {
                    format!("Queued import: {}", path.display())
                }
                ExternalImportSource::Url(url) => format!("Queued URL import: {url}"),
            };
        }
        if !queued_placement_ids.is_empty() {
            self.canvas.select_placements(&queued_placement_ids);
        }
        self.register_import_batch(self.active_board_id, desired_world, queued_batch_ids);
    }

    fn placement_content_intent_for_external_source(
        source: &ExternalImportSource,
    ) -> crate::data::ContentResolutionSource {
        match source {
            ExternalImportSource::Path(path) => {
                crate::data::ContentResolutionSource::ExternalPath {
                    path: path.to_string_lossy().into_owned(),
                }
            }
            ExternalImportSource::Url(url) => {
                crate::data::ContentResolutionSource::ExternalUrl { url: url.clone() }
            }
        }
    }

    fn handle_clipboard_copy(&mut self, ctx: &egui::Context) {
        let copy_requested = ctx.input(|input| {
            input
                .events
                .iter()
                .any(|event| matches!(event, egui::Event::Copy))
        });
        if !copy_requested || !self.canvas.has_selection() {
            return;
        }

        self.copy_selection_to_system_clipboard();
    }

    fn copy_selection_to_system_clipboard(&mut self) {
        let payload = match crate::sources::clipboard::build_selection_copy_payload(
            &self.canvas,
            self.library.as_ref(),
        ) {
            Ok(Some(payload)) => payload,
            Ok(None) => return,
            Err(error) => {
                self.status = error;
                return;
            }
        };

        if let Err(error) = crate::sources::clipboard::write_payload(&payload) {
            self.status = error;
            return;
        }

        let noun = if payload.item_count == 1 {
            "item"
        } else {
            "items"
        };
        self.status = if payload.external_image.is_some() {
            format!("Copied {} {noun} + image", payload.item_count)
        } else {
            format!("Copied {} {noun}", payload.item_count)
        };
    }

    fn copy_selection_to_context_clipboard(&mut self, _ctx: &egui::Context) {
        if !self.canvas.has_selection() {
            return;
        }

        self.copy_selection_to_system_clipboard();
    }

    fn context_menu_items(&self) -> Vec<ContextMenuItem> {
        let selected_item_count = self.canvas.selected_placement_ids().len();
        let selected_annotation_count = self.canvas.selected_annotation_ids().len();
        let has_selection = selected_item_count > 0 || selected_annotation_count > 0;
        let has_selected_image = selected_item_count > 0;
        let exactly_one_image_only = selected_item_count == 1 && selected_annotation_count == 0;

        vec![
            ContextMenuItem {
                action: ContextMenuAction::Copy,
                label: "Copy",
                shortcut: Some(platform_shortcut_label("⌘ C", "Ctrl C")),
                enabled: has_selection,
            },
            ContextMenuItem {
                action: ContextMenuAction::Paste,
                label: "Paste",
                shortcut: Some(platform_shortcut_label("⌘ V", "Ctrl V")),
                enabled: true,
            },
            ContextMenuItem {
                action: ContextMenuAction::Duplicate,
                label: "Duplicate",
                shortcut: Some(platform_shortcut_label("⌘ D", "Ctrl D")),
                enabled: has_selection,
            },
            ContextMenuItem {
                action: ContextMenuAction::DeleteFromBoard,
                label: "Delete (from board)",
                shortcut: Some("del"),
                enabled: has_selection,
            },
            ContextMenuItem {
                action: ContextMenuAction::MoveToFront,
                label: "Move to Front",
                shortcut: Some(platform_shortcut_label("⌘ .", "Ctrl .")),
                enabled: has_selection,
            },
            ContextMenuItem {
                action: ContextMenuAction::MoveToBack,
                label: "Move to Back",
                shortcut: Some(platform_shortcut_label("⌘ ,", "Ctrl ,")),
                enabled: has_selection,
            },
            ContextMenuItem {
                action: ContextMenuAction::SearchUsingImage,
                label: "Search using image",
                shortcut: Some(platform_shortcut_label("⌘ F", "Ctrl F")),
                enabled: exactly_one_image_only,
            },
            ContextMenuItem {
                action: ContextMenuAction::RemoveFromLibrary,
                label: "Remove from Library...",
                shortcut: None,
                enabled: has_selected_image,
            },
        ]
    }

    fn handle_context_menu_action(
        &mut self,
        action: ContextMenuAction,
        ctx: &egui::Context,
        screen_rect: egui::Rect,
    ) {
        let Some(item) = self
            .context_menu_items()
            .into_iter()
            .find(|item| item.action == action)
        else {
            return;
        };
        if !item.enabled {
            return;
        }

        match action {
            ContextMenuAction::Copy => self.copy_selection_to_context_clipboard(ctx),
            ContextMenuAction::Paste => self.handle_clipboard_paste_from_context(ctx, screen_rect),
            ContextMenuAction::Duplicate => self.duplicate_selection_with_gap(),
            ContextMenuAction::DeleteFromBoard => {
                self.delete_selected_items_from_board(screen_rect);
            }
            ContextMenuAction::MoveToFront => self.move_selected_canvas_items_to_front(),
            ContextMenuAction::MoveToBack => self.move_selected_canvas_items_to_back(),
            ContextMenuAction::SearchUsingImage => self.search_using_selected_image(),
            ContextMenuAction::RemoveFromLibrary => self.request_selected_assets_library_remove(),
        }
    }

    fn move_selected_canvas_items_to_front(&mut self) {
        let before_stack_order = self.canvas.canvas_stack_order();
        let viewport_world = self.startup_viewport_world_rect();
        let mut context = self.active_board_command_context(viewport_world);
        match crate::active_board::commands::move_selected_to_front(&mut context) {
            Ok(true) => {
                self.record_stack_order_history("Move selection to front", before_stack_order)
            }
            Ok(false) => {}
            Err(error) => self.status = error,
        }
    }

    fn move_selected_canvas_items_to_back(&mut self) {
        let before_stack_order = self.canvas.canvas_stack_order();
        let viewport_world = self.startup_viewport_world_rect();
        let mut context = self.active_board_command_context(viewport_world);
        match crate::active_board::commands::move_selected_to_back(&mut context) {
            Ok(true) => {
                self.record_stack_order_history("Move selection to back", before_stack_order)
            }
            Ok(false) => {}
            Err(error) => self.status = error,
        }
    }

    fn handle_clipboard_paste_from_context(
        &mut self,
        ctx: &egui::Context,
        screen_rect: egui::Rect,
    ) {
        let paste_text = self.context_clipboard_text(ctx);
        let desired_world = self
            .context_menu_pos
            .map(|pos| self.canvas.screen_to_world(pos, screen_rect))
            .unwrap_or_else(|| self.preferred_import_world(ctx, screen_rect));
        self.handle_clipboard_paste_at(screen_rect, desired_world, paste_text);
    }

    fn context_clipboard_text(&mut self, ctx: &egui::Context) -> Option<String> {
        let paste_text = ctx.input(|input| {
            input.events.iter().find_map(|event| match event {
                egui::Event::Paste(text) => Some(text.clone()),
                _ => None,
            })
        });
        if paste_text.is_some() {
            return paste_text;
        }

        let mut clipboard = match arboard::Clipboard::new() {
            Ok(clipboard) => clipboard,
            Err(_) => return None,
        };
        clipboard.get_text().ok()
    }

    fn request_selected_assets_library_remove(&mut self) {
        let mut asset_ids = Vec::new();
        for placement_id in self.canvas.selected_placement_ids() {
            if let Some(placement) = self.canvas.placement_for_id(placement_id)
                && !asset_ids
                    .contains(&placement.asset.as_ref().expect("loaded placement asset").id)
            {
                asset_ids.push(placement.asset.as_ref().expect("loaded placement asset").id);
            }
        }

        if asset_ids.is_empty() {
            return;
        }

        self.pending_library_remove = Some(PendingLibraryRemove { asset_ids });
    }

    fn confirm_pending_library_remove(&mut self) {
        let Some(pending) = self.pending_library_remove.take() else {
            return;
        };

        let Some(library) = &self.library else {
            self.status = "Storage unavailable".to_owned();
            return;
        };

        if let Err(error) = library.archive_assets(&pending.asset_ids) {
            self.status = error;
            return;
        }

        self.canvas.remove_assets(&pending.asset_ids);
        self.refresh_board_summaries();
        let noun = if pending.asset_ids.len() == 1 {
            "asset"
        } else {
            "assets"
        };
        self.status = format!("Removed {} library {noun}", pending.asset_ids.len());
    }

    fn show_context_menu(
        &mut self,
        ctx: &egui::Context,
        screen_rect: egui::Rect,
        just_opened: bool,
    ) {
        let Some(pos) = self.context_menu_pos else {
            return;
        };
        let entrance =
            crate::ui::motion::entrance_on_appear(ctx, egui::Id::new("board_context_menu"));
        let settle = crate::ui::motion::settle_offset(entrance);

        let mut close_menu = false;
        let menu_response = egui::Area::new(egui::Id::new("board_context_menu"))
            .order(egui::Order::Tooltip)
            .fixed_pos(pos - egui::vec2(0.0, settle))
            .show(ctx, |ui| {
                ui.set_opacity(entrance.clamp(0.0, 1.0));
                egui::Frame::new()
                    .fill(crate::ui::theme::SURFACE)
                    .stroke(egui::Stroke::NONE)
                    .corner_radius(egui::CornerRadius::same(crate::ui::theme::RADIUS_COMPACT))
                    .shadow(ui.style().visuals.popup_shadow)
                    .inner_margin(egui::Margin::same(crate::ui::theme::SPACE_1 as i8))
                    .show(ui, |ui| {
                        ui.set_width(250.0);
                        ui.spacing_mut().item_spacing.y = 2.0;
                        for item in self.context_menu_items() {
                            let response = context_menu_row(ui, &item);
                            if response.clicked() {
                                self.handle_context_menu_action(item.action, ctx, screen_rect);
                                close_menu = true;
                            }
                        }
                    });
            });
        crate::ui::motion::settle_layer_scale(ctx, menu_response.response.layer_id, pos, entrance);

        let should_close = ctx.input(|input| {
            context_menu_should_close(
                close_menu,
                just_opened,
                menu_response.response.rect,
                input.pointer.latest_pos(),
                input.pointer.button_pressed(egui::PointerButton::Primary),
                input.pointer.button_pressed(egui::PointerButton::Secondary),
                input.key_pressed(egui::Key::Escape),
            )
        });
        if should_close {
            self.context_menu_pos = None;
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    fn blocks_canvas_input(&self, ctx: &egui::Context) -> bool {
        self.canvas_input_policy(ctx) != crate::canvas::CanvasInputPolicy::Full
    }

    fn canvas_input_policy(&self, ctx: &egui::Context) -> crate::canvas::CanvasInputPolicy {
        if self.context_menu_pos.is_some() || self.modal_open() {
            return crate::canvas::CanvasInputPolicy::Blocked;
        }

        let search_blocks_canvas = {
            #[cfg(feature = "semantic-search")]
            {
                self.search.blocks_canvas_input(ctx)
            }
            #[cfg(not(feature = "semantic-search"))]
            {
                let _ = ctx;
                false
            }
        };
        if !search_blocks_canvas {
            return crate::canvas::CanvasInputPolicy::Full;
        }

        #[cfg(feature = "semantic-search")]
        {
            if self.search.provisional_batch_active() {
                return crate::canvas::CanvasInputPolicy::ViewportOnly;
            }
            return crate::canvas::CanvasInputPolicy::Blocked;
        }

        #[cfg(not(feature = "semantic-search"))]
        {
            crate::canvas::CanvasInputPolicy::Blocked
        }
    }

    fn show_library_remove_confirmation(&mut self, ctx: &egui::Context) {
        let entrance = crate::ui::motion::overlay_entrance(
            ctx,
            egui::Id::new("library_remove_dialog_entrance"),
            self.pending_library_remove.is_some(),
        );
        let Some(pending) = self.pending_library_remove.clone() else {
            return;
        };

        let title = if pending.asset_ids.len() == 1 {
            "Remove asset from Library?"
        } else {
            "Remove assets from Library?"
        };
        let settle = crate::ui::motion::settle_offset(entrance);
        egui::Window::new(title)
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, settle))
            .frame(destructive_confirmation_frame(ctx, entrance))
            .show(ctx, |ui| {
                ui.multiply_opacity(entrance);
                ui.set_min_width(destructive_dialog_width());
                destructive_dialog_header(ui, title);
                destructive_dialog_body(ui, |ui| {
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(
                                "Placements will be removed from all boards. Original files are kept.",
                            )
                            .text_style(egui::TextStyle::Small)
                            .color(crate::ui::theme::TEXT_MUTED),
                        )
                        .wrap(),
                    );
                    ui.add_space(crate::ui::theme::SPACE_3);
                    ui.horizontal(|ui| {
                        if ui.add(secondary_dialog_button("Cancel")).clicked() {
                            self.pending_library_remove = None;
                        }
                        if ui
                            .add(destructive_action_button("Remove from Library"))
                            .clicked()
                        {
                            self.confirm_pending_library_remove();
                        }
                    });
                });
            });
    }

    #[cfg(feature = "semantic-search")]
    fn show_semantic_search_remove_confirmation(&mut self, ctx: &egui::Context) {
        let entrance = crate::ui::motion::overlay_entrance(
            ctx,
            egui::Id::new("semantic_remove_dialog_entrance"),
            self.pending_semantic_search_remove,
        );
        if !self.pending_semantic_search_remove {
            return;
        }

        let settle = crate::ui::motion::settle_offset(entrance);
        egui::Window::new("Remove Semantic Search?")
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, settle))
            .frame(destructive_confirmation_frame(ctx, entrance))
            .show(ctx, |ui| {
                ui.multiply_opacity(entrance);
                ui.set_min_width(destructive_dialog_width());
                destructive_dialog_header(ui, "Remove Semantic Search?");
                destructive_dialog_body(ui, |ui| {
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(
                                "Local search assets will be removed. Library data and saved search indexing stay intact.",
                            )
                            .text_style(egui::TextStyle::Small)
                            .color(crate::ui::theme::TEXT_MUTED),
                        )
                        .wrap(),
                    );
                    ui.add_space(crate::ui::theme::SPACE_3);
                    ui.horizontal(|ui| {
                        if ui.add(secondary_dialog_button("Cancel")).clicked() {
                            self.pending_semantic_search_remove = false;
                        }
                        if ui
                            .add(destructive_action_button("Remove Semantic Search"))
                            .clicked()
                        {
                            self.pending_semantic_search_remove = false;
                            self.search
                                .remove_semantic_search_assets_from_settings(&mut self.status);
                        }
                    });
                });
            });
    }

    #[cfg(feature = "semantic-search")]
    fn search_using_selected_image(&mut self) {
        self.search.search_using_selected_image(
            self.settings.semantic_search_enabled,
            self.library.as_ref(),
            &mut self.canvas,
            &mut self.status,
            self.settings.canvas_image_max_side,
        );
    }

    #[cfg(not(feature = "semantic-search"))]
    fn search_using_selected_image(&mut self) {
        self.status = "Semantic search unavailable".to_owned();
    }

    #[cfg_attr(not(test), allow(dead_code))]
    fn copy_selected_board_payload(&self) -> Option<ClipboardBoardSelection> {
        crate::active_board::commands::copy_selected_board_payload(&self.canvas)
    }

    fn handle_internal_clipboard_paste_at(
        &mut self,
        paste_text: &str,
        anchor_world: egui::Pos2,
    ) -> bool {
        let viewport_world = self.import_viewport_world_rect(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            STARTUP_VIEWPORT_SIZE,
        ));
        let before_stack_order = self.canvas.canvas_stack_order();
        let mut context = self.active_board_command_context(viewport_world);
        match crate::active_board::commands::paste_json_at(&mut context, paste_text, anchor_world) {
            Ok(Some(outcome)) => {
                let label = outcome.status.clone();
                self.apply_paste_command_outcome(outcome);
                self.record_created_selection_history(label, before_stack_order);
                true
            }
            Ok(None) => false,
            Err(error) => {
                self.status = error;
                true
            }
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    fn paste_board_payload_at(
        &mut self,
        payload: &ClipboardBoardSelection,
        requested_anchor_world: egui::Pos2,
    ) -> Result<(), String> {
        let viewport_world = self.import_viewport_world_rect(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            STARTUP_VIEWPORT_SIZE,
        ));
        let before_stack_order = self.canvas.canvas_stack_order();
        let mut context = self.active_board_command_context(viewport_world);
        let outcome = crate::active_board::commands::paste_board_payload_at(
            &mut context,
            payload,
            requested_anchor_world,
        )?;
        let label = outcome.status.clone();
        self.apply_paste_command_outcome(outcome);
        self.record_created_selection_history(label, before_stack_order);
        Ok(())
    }

    fn record_stack_order_history(
        &mut self,
        label: &str,
        before_stack_order: Vec<crate::data::CanvasStackItem>,
    ) {
        self.document_history.record_applied(HistoryAction {
            label: label.to_owned(),
            placements: Vec::new(),
            annotations: Vec::new(),
            stack_order: Some(HistoryChange {
                before: Some(before_stack_order),
                after: Some(self.canvas.canvas_stack_order()),
            }),
        });
    }

    fn record_created_selection_history(
        &mut self,
        label: String,
        before_stack_order: Vec<crate::data::CanvasStackItem>,
    ) {
        let placement_changes = self
            .canvas
            .selected_placement_ids()
            .into_iter()
            .filter_map(|placement_id| self.canvas.placement_for_id(placement_id))
            .map(|placement| HistoryChange {
                before: None,
                after: Some(placement),
            })
            .collect();
        let annotation_changes = self
            .canvas
            .selected_annotation_ids()
            .into_iter()
            .filter_map(|annotation_id| self.canvas.annotation_for_id(annotation_id))
            .map(|annotation| HistoryChange {
                before: None,
                after: Some(annotation),
            })
            .collect();
        self.document_history.record_applied(HistoryAction {
            label,
            placements: placement_changes,
            annotations: annotation_changes,
            stack_order: Some(HistoryChange {
                before: Some(before_stack_order),
                after: Some(self.canvas.canvas_stack_order()),
            }),
        });
    }

    fn apply_paste_command_outcome(&mut self, outcome: PasteCommandOutcome) {
        let needs_existing_reload = outcome.needs_existing_reload;
        self.status = outcome.status;
        if needs_existing_reload {
            self.enqueue_existing_items();
        }
        self.refresh_board_summaries();
    }

    fn handle_import_results(&mut self, ctx: &egui::Context) {
        let Some(import_worker) = &self.import_worker else {
            return;
        };

        let drain_started_at = Instant::now();
        let results = import_worker.try_recv_up_to(MAX_IMPORT_RESULTS_PER_FRAME);
        let drained_count = results.len();
        let hit_result_cap = results.len() == MAX_IMPORT_RESULTS_PER_FRAME;

        for result in results {
            let outcome = self.residency.process_import_result(
                ctx,
                &mut self.canvas,
                self.import_worker.as_ref(),
                self.active_board_id,
                &self.settings,
                result,
            );
            self.apply_import_result_residency_outcome(ctx, outcome);
        }
        if drained_count > 0 {
            self.refresh_board_summaries();
        }

        self.frame_perf.import_results_drained = drained_count;
        self.frame_perf.import_result_drain_ms = drain_started_at.elapsed().as_secs_f64() * 1000.0;
        if drained_count > 0 && self.perf_log.is_some() {
            eprintln!(
                "[perf] import_result_drain count={} drain_ms={:.2} last_visible={} last_draw_calls={} last_groups={} last_fallback={}",
                drained_count,
                self.frame_perf.import_result_drain_ms,
                self.frame_perf.canvas.image_pass.visible_images,
                self.frame_perf.canvas.image_pass.draw_calls,
                self.frame_perf.canvas.image_pass.texture_groups,
                self.frame_perf.canvas.image_pass.fallback_images,
            );
        }

        if hit_result_cap {
            ctx.request_repaint();
        }
    }

    fn apply_import_result_residency_outcome(
        &mut self,
        ctx: &egui::Context,
        outcome: ImagePrepResultResidencyOutcome,
    ) {
        match outcome {
            ImagePrepResultResidencyOutcome::Loaded(outcome) => {
                self.apply_loaded_import_outcome(outcome);
            }
            ImagePrepResultResidencyOutcome::Cropped(cropped) => {
                self.integrate_cropped_import_result(ctx, cropped);
            }
            ImagePrepResultResidencyOutcome::Failed(outcome) => {
                self.apply_failed_import_outcome(outcome);
            }
            ImagePrepResultResidencyOutcome::Status(Some(status)) => {
                self.status = status;
            }
            ImagePrepResultResidencyOutcome::Status(None) => {}
        }
    }

    fn integrate_cropped_import_result(
        &mut self,
        ctx: &egui::Context,
        cropped: crate::jobs::PreparedCropPreview,
    ) {
        let ui_integration_started_at = Instant::now();
        match self.crop_flow.integrate_cropped_result(
            ctx,
            &mut self.canvas,
            self.library.as_ref(),
            cropped,
            self.settings.canvas_image_max_side,
            self.settings.semantic_search_enabled,
        ) {
            Ok(integration) => {
                let ui_integration_ms = ui_integration_started_at.elapsed().as_secs_f64() * 1000.0;
                self.residency
                    .note_loaded_placement(integration.updated_record.state.id);
                self.document_history.record_applied(HistoryAction {
                    label: "Crop image".to_owned(),
                    placements: vec![HistoryChange {
                        before: Some(integration.previous_record.clone()),
                        after: Some(integration.updated_record.clone()),
                    }],
                    annotations: Vec::new(),
                    stack_order: None,
                });
                let mut status = "Cropped image".to_owned();
                #[cfg(feature = "semantic-search")]
                if let Some((asset_id, original_path)) = integration.indexed_asset {
                    if let Err(error) = self.search.queue_asset_index(asset_id, original_path) {
                        status = error;
                    }
                }
                if let Some(error) = self.residency.queue_backfill(
                    self.import_worker.as_ref(),
                    &integration.updated_record,
                    &self.settings,
                    crate::data::CanvasImageLod::Working,
                ) {
                    status = error;
                }
                self.log_cropped_timing(&integration.updated_record, ui_integration_ms);
                self.status = status;
            }
            Err(error) => self.status = error,
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    fn integrate_loaded_import(
        &mut self,
        ctx: &egui::Context,
        loaded: crate::jobs::PreparedPlacementImage,
    ) {
        let outcome = self.residency.integrate_loaded_import(
            ctx,
            &mut self.canvas,
            self.import_worker.as_ref(),
            self.active_board_id,
            &self.settings,
            loaded,
        );
        self.apply_loaded_import_outcome(outcome);
    }

    fn apply_loaded_import_outcome(&mut self, outcome: LoadedContentResolutionOutcome) {
        let finished = self.content_resolution.finish_loaded_import(outcome);
        let resolution_queue_id = finished.resolution_ticket.map(|ticket| ticket.queue_id);
        let perf_import_origin = finished.log.as_ref().map(|log| log.origin);
        let perf_counts_as_import = perf_import_origin.is_some_and(|origin| {
            matches!(
                origin,
                crate::jobs::ImportOrigin::Path
                    | crate::jobs::ImportOrigin::Clipboard
                    | crate::jobs::ImportOrigin::Url
            )
        });
        let status = finished.status;
        #[cfg(feature = "semantic-search")]
        let mut status = status;
        #[cfg(feature = "semantic-search")]
        let indexed_asset = finished.indexed_asset;
        self.finish_import_queue_id_in_batches(
            finished.batch_target_board_id,
            finished.batch_queue_id,
            finished.loaded_placement_id,
        );
        if perf_counts_as_import
            && let Some(log) = finished.log.as_ref()
            && log.placement.state.archived == false
        {
            self.document_history.record_applied(HistoryAction {
                label: "Import image".to_owned(),
                placements: vec![HistoryChange {
                    before: None,
                    after: Some(log.placement.clone()),
                }],
                annotations: Vec::new(),
                stack_order: None,
            });
        }
        if let Some(log) = finished.log {
            if perf_counts_as_import && let Some(scenario) = &mut self.perf_scenario {
                scenario.loaded_imports += 1;
                let metadata = resolution_queue_id
                    .and_then(|queue_id| scenario.asset_by_queue_id.get(&queue_id).cloned());
                if let Some(metadata) = &metadata {
                    scenario.asset_by_asset_id.insert(
                        log.placement
                            .asset
                            .as_ref()
                            .expect("loaded placement asset")
                            .id,
                        metadata.clone(),
                    );
                }
                let (format_value, source_file, animated, bytes) = match &metadata {
                    Some(meta) => (
                        Some(meta.format.clone()),
                        Some(meta.file.clone()),
                        Some(meta.animated),
                        meta.bytes,
                    ),
                    None => (None, None, None, None),
                };
                if !scenario.first_loaded_logged {
                    scenario.first_loaded_logged = true;
                    scenario.emit(
                        "first_placement_loaded",
                        serde_json::json!({
                            "format": format_value,
                            "file": source_file,
                        }),
                    );
                }
                scenario.emit(
                    "placement_loaded",
                    serde_json::json!({
                        "loaded": scenario.loaded_imports,
                        "asset_id": log.placement.asset.as_ref().expect("loaded placement asset").id,
                        "placement_id": log.placement.state.id,
                        "mime_type": log.placement.asset.as_ref().expect("loaded placement asset").mime_type,
                        "filename": log.placement.asset.as_ref().expect("loaded placement asset").filename,
                        "format": format_value,
                        "file": source_file,
                        "animated": animated,
                        "bytes": bytes,
                        "queue_id": resolution_queue_id,
                    }),
                );
                if scenario.loaded_imports >= scenario.expected_imports
                    && !scenario.all_loaded_logged
                {
                    scenario.all_loaded_logged = true;
                    scenario.emit("all_placements_loaded", serde_json::json!({}));
                }
            }
            self.log_loaded_timing(
                log.origin,
                Some(&log.placement),
                log.diagnostics.as_ref(),
                log.trace,
            );
        }
        #[cfg(feature = "semantic-search")]
        if let Some((asset_id, original_path)) = indexed_asset {
            if perf_counts_as_import && let Some(scenario) = &mut self.perf_scenario {
                scenario.demand_embeddings_queued += 1;
                let metadata = scenario.asset_by_asset_id.get(&asset_id).cloned();
                let (format_value, source_file) = match &metadata {
                    Some(meta) => (Some(meta.format.clone()), Some(meta.file.clone())),
                    None => (None, None),
                };
                if !scenario.first_embedding_queued_logged {
                    scenario.first_embedding_queued_logged = true;
                    scenario.emit(
                        "first_import_embedding_queued",
                        serde_json::json!({
                            "format": format_value,
                            "file": source_file,
                        }),
                    );
                }
                scenario.emit(
                    "import_embedding_queued",
                    serde_json::json!({
                        "asset_id": asset_id,
                        "format": format_value,
                        "file": source_file,
                    }),
                );
            }
            if let Err(error) = self.search.queue_asset_index(asset_id, original_path)
                && status.is_none()
            {
                status = Some(error);
            }
        }
        if let Some(status) = status {
            self.status = status;
        }
    }

    fn apply_failed_import_outcome(&mut self, outcome: FailedContentResolutionOutcome) {
        let finished = self.content_resolution.finish_failed_import(
            self.library.as_ref(),
            &mut self.canvas,
            self.active_board_id,
            outcome,
        );
        let _ = finished.resolution_ticket;
        self.finish_import_queue_id_in_batches(
            finished.batch_target_board_id,
            finished.batch_queue_id,
            None,
        );
        self.status = finished.status;
    }

    fn handle_history_shortcuts(&mut self, ctx: &egui::Context) {
        let undo_requested = ctx.input(|input| {
            input.modifiers.command && !input.modifiers.shift && input.key_pressed(egui::Key::Z)
        });
        let redo_requested = ctx.input(|input| {
            input.modifiers.command && input.modifiers.shift && input.key_pressed(egui::Key::Z)
        });
        if redo_requested {
            self.redo_document_history();
        } else if undo_requested {
            self.undo_document_history(ctx);
        }
    }

    fn undo_document_history(&mut self, ctx: &egui::Context) {
        let Some(library) = self.library.clone() else {
            self.status = "Storage unavailable".to_owned();
            return;
        };
        match self.document_history.undo(&library) {
            Ok(Some(applied)) => {
                self.apply_history_to_active_canvas(&applied.action, applied.direction);
                self.status = format!("Undid {}", applied.label);
            }
            Ok(None) => self.undo_crop_fallback(ctx),
            Err(error) => self.status = error,
        }
    }

    fn redo_document_history(&mut self) {
        let Some(library) = self.library.clone() else {
            self.status = "Storage unavailable".to_owned();
            return;
        };
        match self.document_history.redo(&library) {
            Ok(Some(applied)) => {
                self.apply_history_to_active_canvas(&applied.action, applied.direction);
                self.status = format!("Redid {}", applied.label);
            }
            Ok(None) => {}
            Err(error) => self.status = error,
        }
    }

    fn apply_history_to_active_canvas(
        &mut self,
        action: &HistoryAction,
        direction: HistoryDirection,
    ) {
        for change in &action.annotations {
            let target = match direction {
                HistoryDirection::Forward => change.after.clone(),
                HistoryDirection::Backward => change.before.clone(),
            }
            .or_else(|| match direction {
                HistoryDirection::Forward => change.before.clone().map(|mut annotation| {
                    annotation.archived = true;
                    annotation
                }),
                HistoryDirection::Backward => change.after.clone().map(|mut annotation| {
                    annotation.archived = true;
                    annotation
                }),
            });
            self.canvas.apply_annotation_history_state(target);
        }

        for change in &action.placements {
            let target = match direction {
                HistoryDirection::Forward => change.after.clone(),
                HistoryDirection::Backward => change.before.clone(),
            }
            .or_else(|| match direction {
                HistoryDirection::Forward => change.before.clone().map(|mut placement| {
                    placement.state.archived = true;
                    placement
                }),
                HistoryDirection::Backward => change.after.clone().map(|mut placement| {
                    placement.state.archived = true;
                    placement
                }),
            });
            self.canvas.apply_placement_history_state(target);
        }

        if let Some(change) = &action.stack_order {
            let target = match direction {
                HistoryDirection::Forward => change.after.clone(),
                HistoryDirection::Backward => change.before.clone(),
            };
            if let Some(stack_order) = target {
                self.canvas.restore_canvas_stack_order(stack_order);
            }
        }
    }

    fn undo_crop_fallback(&mut self, ctx: &egui::Context) {
        match self.crop_flow.undo_last_crop(
            ctx,
            &mut self.canvas,
            self.library.as_ref(),
            self.active_board_id,
        ) {
            Ok(true) => self.status = "Undid crop".to_owned(),
            Ok(false) => {}
            Err(error) => self.status = error,
        }
    }

    fn handle_selection_shortcuts(&mut self, ctx: &egui::Context, screen_rect: egui::Rect) {
        #[cfg(feature = "semantic-search")]
        if self.search.is_open() {
            return;
        }

        if ctx.egui_wants_keyboard_input() {
            return;
        }

        let select_all_requested =
            ctx.input(|input| input.modifiers.command && input.key_pressed(egui::Key::A));
        if select_all_requested {
            self.canvas.select_all_content();
            return;
        }

        if let Some(direction) = settle_direction_pressed(ctx)
            && self.canvas.selected_placement_ids().len() > 1
        {
            apply_settle_arrangement(
                &mut self.canvas,
                direction,
                self.settings.placement_gap,
                &mut self.status,
            );
            return;
        }

        for shortcut in SUPPORTED_SHORTCUTS {
            let Some(shortcut_action) = shortcut.shortcut_action else {
                continue;
            };
            let triggered = match shortcut_action {
                SupportedShortcutAction::DeleteSelection => delete_shortcut_pressed(ctx),
                SupportedShortcutAction::FocusSelection
                | SupportedShortcutAction::DuplicateSelection
                | SupportedShortcutAction::MoveSelectionToFront
                | SupportedShortcutAction::MoveSelectionToBack
                | SupportedShortcutAction::ToggleSidebar
                | SupportedShortcutAction::ToggleAlwaysOnTop
                | SupportedShortcutAction::ApplyDefaultAverageArea
                | SupportedShortcutAction::AverageSelectionArea
                | SupportedShortcutAction::DedupeLibrary
                | SupportedShortcutAction::Arrange(_) => shortcut
                    .binding
                    .is_some_and(|binding| shortcut_pressed(ctx, binding)),
            };
            if !triggered {
                continue;
            }

            match shortcut_action {
                SupportedShortcutAction::DeleteSelection => {
                    if !self.canvas.has_selection() {
                        continue;
                    }
                    self.delete_selected_items_from_board(screen_rect);
                }
                SupportedShortcutAction::DuplicateSelection => {
                    if !self.canvas.has_selection() {
                        continue;
                    }
                    self.duplicate_selection_with_gap();
                }
                SupportedShortcutAction::MoveSelectionToFront => {
                    if !self.canvas.has_selection() {
                        continue;
                    }
                    self.move_selected_canvas_items_to_front();
                }
                SupportedShortcutAction::MoveSelectionToBack => {
                    if !self.canvas.has_selection() {
                        continue;
                    }
                    self.move_selected_canvas_items_to_back();
                }
                SupportedShortcutAction::FocusSelection => {
                    if !self.canvas.has_selection() {
                        continue;
                    }
                    let visible_rect = self.visible_canvas_screen_rect(screen_rect);
                    if self
                        .canvas
                        .focus_selection_in_view(screen_rect, visible_rect)
                    {
                        self.status = "Focused selection".to_owned();
                    }
                }
                SupportedShortcutAction::ToggleSidebar => {
                    self.sidebar_open = !self.sidebar_open;
                }
                SupportedShortcutAction::ToggleAlwaysOnTop => {
                    self.set_always_on_top(ctx, !self.settings.always_on_top);
                }
                SupportedShortcutAction::Arrange(action) => {
                    if self.canvas.selected_placement_ids().len() > 1 {
                        apply_arrangement_action(
                            &mut self.canvas,
                            action,
                            self.settings.placement_gap,
                            &mut self.status,
                        );
                    }
                }
                SupportedShortcutAction::ApplyDefaultAverageArea => {
                    if !self.canvas.has_image_selection() {
                        continue;
                    }
                    apply_default_average_area_to_selection(&mut self.canvas, &mut self.status);
                }
                SupportedShortcutAction::AverageSelectionArea => {
                    if self.canvas.selected_placement_ids().len() < 2 {
                        self.status = "Select 2 or more images to average their area".to_owned();
                        continue;
                    }
                    apply_average_selection_area(&mut self.canvas, &mut self.status);
                }
                SupportedShortcutAction::DedupeLibrary => {
                    self.dedupe_library_from_shortcut();
                }
            }
            break;
        }
    }

    fn handle_board_shortcuts(&mut self, ctx: &egui::Context) {
        if self.pending_board_delete.is_some() || self.pending_library_remove.is_some() {
            return;
        }

        #[cfg(feature = "semantic-search")]
        if self.search.is_open() {
            return;
        }

        if ctx.egui_wants_keyboard_input() {
            return;
        }

        let create_requested =
            ctx.input(|input| input.modifiers.command && input.key_pressed(egui::Key::N));
        if create_requested {
            self.create_and_switch_board(false);
            return;
        }

        let requested_index = ctx.input(|input| {
            if !input.modifiers.command {
                return None;
            }
            [
                (egui::Key::Num1, 0_usize),
                (egui::Key::Num2, 1),
                (egui::Key::Num3, 2),
                (egui::Key::Num4, 3),
                (egui::Key::Num5, 4),
                (egui::Key::Num6, 5),
                (egui::Key::Num7, 6),
                (egui::Key::Num8, 7),
                (egui::Key::Num9, 8),
                (egui::Key::Num0, 9),
            ]
            .into_iter()
            .find_map(|(key, index)| input.key_pressed(key).then_some(index))
        });

        let Some(index) = requested_index else {
            return;
        };
        let boards = self.board_summaries();
        if let Some(summary) = boards.get(index) {
            self.switch_to_board(summary.board.id);
        }
    }

    fn duplicate_selection_with_gap(&mut self) {
        let viewport_world = self.import_viewport_world_rect(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            STARTUP_VIEWPORT_SIZE,
        ));
        let mut context = self.active_board_command_context(viewport_world);
        match crate::active_board::commands::duplicate_selection(&mut context) {
            Ok(Some(outcome)) => self.apply_paste_command_outcome(outcome),
            Ok(None) => {}
            Err(error) => self.status = error,
        }
    }

    fn handle_pending_crop_requests(&mut self, ctx: &egui::Context) {
        match self.crop_flow.enqueue_pending_requests(
            ctx,
            &mut self.canvas,
            self.library.as_ref(),
            self.import_worker.as_ref(),
            self.max_texture_side,
            self.settings.canvas_image_max_side,
        ) {
            Ok(true) => self.status = "Cropping image".to_owned(),
            Ok(false) => {}
            Err(error) => self.status = error,
        }
    }

    fn handle_pending_annotation_requests(&mut self) {
        let Some(library) = &self.library else {
            return;
        };

        while let Some(request) = self.canvas.take_pending_annotation() {
            let before_stack_order = self.canvas.canvas_stack_order();
            let background_enabled = request.kind == crate::data::CanvasAnnotationKind::Textbox
                && self.settings.default_textbox_background;
            match library.create_canvas_annotation(
                request.kind,
                request.start_world(),
                request.end_world(),
                &request.text,
                request.text_size,
                crate::data::CanvasStrokeWidth::default(),
                crate::data::CanvasAnnotationColor::default(),
                background_enabled,
            ) {
                Ok(annotation) => {
                    let should_focus_text =
                        annotation.kind == crate::data::CanvasAnnotationKind::Textbox;
                    let annotation_id = annotation.id;
                    self.canvas.add_annotation(annotation);
                    if let Some(created) = self.canvas.annotation_for_id(annotation_id) {
                        self.document_history.record_applied(HistoryAction {
                            label: match request.kind {
                                crate::data::CanvasAnnotationKind::Arrow => "Added arrow",
                                crate::data::CanvasAnnotationKind::Line => "Added line",
                                crate::data::CanvasAnnotationKind::Textbox => "Added textbox",
                            }
                            .to_owned(),
                            placements: Vec::new(),
                            annotations: vec![HistoryChange {
                                before: None,
                                after: Some(created),
                            }],
                            stack_order: Some(HistoryChange {
                                before: Some(before_stack_order),
                                after: Some(self.canvas.canvas_stack_order()),
                            }),
                        });
                    }
                    if should_focus_text {
                        self.canvas.begin_textbox_edit(annotation_id);
                    }
                    self.status = match request.kind {
                        crate::data::CanvasAnnotationKind::Arrow => "Added arrow".to_owned(),
                        crate::data::CanvasAnnotationKind::Line => "Added line".to_owned(),
                        crate::data::CanvasAnnotationKind::Textbox => "Added textbox".to_owned(),
                    };
                }
                Err(error) => {
                    self.status = error;
                }
            }
        }
    }

    fn persist_canvas_changes(&mut self) {
        self.record_pending_document_changes_history("Edit board");
        let session = self.current_user_session();
        if let Err(error) = self.document_persistence.commit_canvas_changes(
            self.library.as_ref(),
            &mut self.canvas,
            session,
        ) {
            self.status = error;
        }
        self.pending_user_session = None;
        self.pending_user_session_save_at = None;
    }

    fn persist_canvas_changes_debounced(&mut self, ctx: &egui::Context) {
        self.record_pending_document_changes_history("Edit board");
        let document_changed = match self
            .document_persistence
            .commit_canvas_document_changes(self.library.as_ref(), &mut self.canvas)
        {
            Ok(document_changed) => document_changed,
            Err(error) => {
                self.status = error;
                false
            }
        };
        let session = self.current_user_session();
        if document_changed {
            if let Err(error) = self
                .document_persistence
                .save_user_session(self.library.as_ref(), session)
            {
                self.status = error;
            }
            self.pending_user_session = None;
            self.pending_user_session_save_at = None;
            return;
        }

        if !self.document_persistence.user_session_changed(&session) {
            self.pending_user_session = None;
            self.pending_user_session_save_at = None;
            return;
        }

        if self.pending_user_session.as_ref() != Some(&session) {
            self.pending_user_session = Some(session);
            self.pending_user_session_save_at = Some(Instant::now() + USER_SESSION_SAVE_DEBOUNCE);
        }

        let Some(save_at) = self.pending_user_session_save_at else {
            return;
        };
        let now = Instant::now();
        if now >= save_at {
            if let Some(session) = self.pending_user_session.take()
                && let Err(error) = self
                    .document_persistence
                    .save_user_session(self.library.as_ref(), session)
            {
                self.status = error;
            }
            self.pending_user_session_save_at = None;
        } else {
            ctx.request_repaint_after(save_at - now);
        }
    }

    fn record_pending_document_changes_history(&mut self, label: &str) {
        let Some(library) = &self.library else {
            return;
        };
        let changes = self.canvas.pending_document_changes_preview();
        if !changes.has_changes() {
            return;
        }

        let current_items = library.load_placements().unwrap_or_default();
        let current_annotations = library.load_canvas_annotations().unwrap_or_default();
        let current_stack_order = library.load_canvas_stack_order().ok();
        let placement_changes = changes
            .updated_placements
            .into_iter()
            .filter_map(|after| {
                let before = current_items
                    .iter()
                    .find(|placement| placement.state.id == after.id)
                    .cloned();
                let asset = if let Some(before) = &before {
                    before
                        .asset
                        .as_ref()
                        .expect("loaded placement asset")
                        .clone()
                } else {
                    library.load_asset(after.asset_id?).ok()?
                };
                Some(HistoryChange {
                    before,
                    after: Some(crate::data::Placement::loaded(after, asset)),
                })
            })
            .collect::<Vec<_>>();
        let annotation_changes = changes
            .updated_annotations
            .into_iter()
            .map(|after| {
                let before = current_annotations
                    .iter()
                    .find(|annotation| annotation.id == after.id)
                    .cloned();
                HistoryChange {
                    before,
                    after: Some(after),
                }
            })
            .collect::<Vec<_>>();
        let stack_order = changes.stack_order.map(|after| HistoryChange {
            before: current_stack_order,
            after: Some(after),
        });

        self.document_history.record_applied(HistoryAction {
            label: label.to_owned(),
            placements: placement_changes,
            annotations: annotation_changes,
            stack_order,
        });
    }

    fn update_window_state(&mut self, ctx: &egui::Context) {
        if !self.persist_window_state {
            return;
        }
        let viewport = ctx.input(|input| input.viewport().clone());
        let Some(inner_rect) = viewport.inner_rect else {
            return;
        };

        self.window_state.inner_width = Some(inner_rect.width());
        self.window_state.inner_height = Some(inner_rect.height());
        if let Some(outer_rect) = viewport.outer_rect {
            self.window_state.outer_x = Some(outer_rect.min.x);
            self.window_state.outer_y = Some(outer_rect.min.y);
        } else {
            self.window_state.outer_x = Some(inner_rect.min.x);
            self.window_state.outer_y = Some(inner_rect.min.y);
        }
    }

    fn current_user_session(&self) -> crate::data::UserSession {
        crate::data::UserSession {
            active_board_id: self.active_board_id,
            viewport: self.canvas.viewport_state(),
            window: self.window_state.clone(),
            settings: self.settings.clone(),
            sidebar_open: self.sidebar_open,
            sidebar_tab: sidebar_tab_session_value(self.sidebar_tab).to_owned(),
            #[cfg(feature = "semantic-search")]
            recent_searches: self.search.recent_searches().to_vec(),
            #[cfg(not(feature = "semantic-search"))]
            recent_searches: Vec::new(),
        }
    }

    fn active_board_command_context(
        &mut self,
        viewport_world: egui::Rect,
    ) -> ActiveBoardCommandContext<'_> {
        let user_session = self.current_user_session();
        ActiveBoardCommandContext {
            canvas: &mut self.canvas,
            library: self.library.as_ref(),
            document_persistence: &mut self.document_persistence,
            user_session,
            import_worker_available: self.import_worker.is_some(),
            placement_gap: self.settings.placement_gap,
            viewport_world,
        }
    }

    fn delete_selected_items_from_board(&mut self, screen_rect: egui::Rect) {
        let before_items = self
            .canvas
            .selected_placement_ids()
            .into_iter()
            .filter_map(|placement_id| self.canvas.placement_for_id(placement_id))
            .collect::<Vec<_>>();
        let before_annotations = self
            .canvas
            .selected_annotation_ids()
            .into_iter()
            .filter_map(|annotation_id| self.canvas.annotation_for_id(annotation_id))
            .collect::<Vec<_>>();
        let before_stack_order = self.canvas.canvas_stack_order();
        let viewport_world = self.import_viewport_world_rect(screen_rect);
        let mut context = self.active_board_command_context(viewport_world);
        match crate::active_board::commands::delete_selected_from_board(&mut context) {
            Ok(Some(status)) => {
                let placement_changes = before_items
                    .into_iter()
                    .map(|placement| {
                        let mut archived = placement.clone();
                        archived.state.archived = true;
                        HistoryChange {
                            before: Some(placement),
                            after: Some(archived),
                        }
                    })
                    .collect();
                let annotation_changes = before_annotations
                    .into_iter()
                    .map(|annotation| {
                        let mut archived = annotation.clone();
                        archived.archived = true;
                        HistoryChange {
                            before: Some(annotation),
                            after: Some(archived),
                        }
                    })
                    .collect();
                self.document_history.record_applied(HistoryAction {
                    label: status.clone(),
                    placements: placement_changes,
                    annotations: annotation_changes,
                    stack_order: Some(HistoryChange {
                        before: Some(before_stack_order),
                        after: Some(self.canvas.canvas_stack_order()),
                    }),
                });
                self.flush_persistence();
                self.refresh_board_summaries();
                self.status = status;
            }
            Ok(None) => {}
            Err(error) => self.status = error,
        }
    }

    fn save_user_session(&mut self) {
        let session = self.current_user_session();
        if let Err(error) = self
            .document_persistence
            .save_user_session(self.library.as_ref(), session)
        {
            self.status = error;
        }
        self.pending_user_session = None;
        self.pending_user_session_save_at = None;
    }

    fn flush_persistence(&mut self) {
        if let Err(error) = self.document_persistence.flush() {
            self.status = error;
        }
    }

    #[cfg(feature = "semantic-search")]
    fn sync_semantic_search_service(&mut self) {
        self.search.sync_service(
            self.settings.semantic_search_enabled,
            self.library.as_ref(),
            &mut self.status,
            Some(&mut self.canvas),
            self.settings.canvas_image_max_side,
        );
    }

    #[cfg(feature = "semantic-search")]
    fn handle_search_results(&mut self) {
        self.search.handle_results(&mut self.canvas);
    }

    #[cfg(feature = "semantic-search")]
    fn drive_semantic_search_install(&mut self, ctx: &egui::Context) {
        if self.search.drive_semantic_search_install(&mut self.status) {
            if self.search.local_model_assets_available() {
                self.sync_semantic_search_service();
            }
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
        }
    }

    #[cfg(feature = "semantic-search")]
    fn should_show_semantic_search_first_launch_prompt(&self) -> bool {
        self.library.is_some()
            && !self.settings.semantic_search_first_launch_prompt_seen
            && self.search.local_model_assets_missing()
    }

    #[cfg(not(feature = "semantic-search"))]
    fn should_show_semantic_search_first_launch_prompt(&self) -> bool {
        false
    }

    #[cfg(feature = "semantic-search")]
    fn mark_semantic_search_first_launch_prompt_seen(&mut self) {
        self.settings.semantic_search_first_launch_prompt_seen = true;
        self.save_user_session();
    }

    #[cfg(feature = "semantic-search")]
    fn decline_semantic_search_first_launch_prompt(&mut self) {
        self.mark_semantic_search_first_launch_prompt_seen();
    }

    #[cfg(feature = "semantic-search")]
    fn enable_semantic_search_from_first_launch_prompt(&mut self, ctx: &egui::Context) {
        self.mark_semantic_search_first_launch_prompt_seen();
        self.search
            .start_semantic_search_install_from_first_launch_prompt(&mut self.status);
        ctx.request_repaint_after(std::time::Duration::from_millis(16));
    }

    #[cfg(feature = "semantic-search")]
    fn show_semantic_search_first_launch_prompt(&mut self, ctx: &egui::Context) {
        let entrance = crate::ui::motion::overlay_entrance(
            ctx,
            egui::Id::new("semantic_first_launch_prompt_entrance"),
            self.should_show_semantic_search_first_launch_prompt(),
        );
        if !self.should_show_semantic_search_first_launch_prompt() {
            return;
        }

        let mut decline = false;
        let mut enable = false;
        let settle = crate::ui::motion::settle_offset(entrance);
        egui::Window::new("Enable Semantic Search?")
            .id(egui::Id::new("semantic_search_first_launch_prompt"))
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, settle))
            .collapsible(false)
            .resizable(false)
            .title_bar(false)
            .frame(semantic_search_prompt_frame(ctx, entrance))
            .show(ctx, |ui| {
                ui.multiply_opacity(entrance);
                ui.set_width(semantic_search_prompt_width());
                semantic_search_prompt_header(ui, "Enable Semantic Search?");
                semantic_search_prompt_body(ui, |ui| {
                    ui.label(
                        egui::RichText::new(
                            "Semantic Search can find references by description after a local download.",
                        )
                        .text_style(egui::TextStyle::Body)
                        .color(crate::ui::theme::TEXT),
                    );
                    ui.add_space(crate::ui::theme::SPACE_2);
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(
                                "Download about 200 MB of search assets now, or install later from Settings or the search palette with Command+F / Control+F.",
                            )
                            .text_style(egui::TextStyle::Small)
                            .color(crate::ui::theme::TEXT_MUTED),
                        )
                        .wrap(),
                    );
                    ui.add_space(crate::ui::theme::SPACE_4);
                    ui.horizontal(|ui| {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.add(primary_dialog_button("Enable")).clicked() {
                                enable = true;
                            }
                            if ui.add(secondary_dialog_button("Not Now")).clicked() {
                                decline = true;
                            }
                        });
                    });
                });
            });

        if enable {
            self.enable_semantic_search_from_first_launch_prompt(ctx);
        } else if decline {
            self.decline_semantic_search_first_launch_prompt();
        }
    }

    #[cfg(feature = "semantic-search")]
    fn drive_embedding_backfill(&mut self, ctx: &egui::Context) {
        let idle = crate::search::EmbeddingBackfillIdleState {
            app_idle: self.frame_perf.import_results_drained == 0
                && !self.canvas.has_pending_crop_work()
                && !self.residency.has_pending_image_work(),
        };
        if let Err(error) = self.search.drive_embedding_backfill(Instant::now(), idle) {
            self.status = error;
        }
        if self.search.embedding_backfill_may_need_repaint() {
            ctx.request_repaint_after(std::time::Duration::from_millis(750));
        }
    }

    #[cfg(feature = "semantic-search")]
    fn handle_search_shortcut(&mut self, ctx: &egui::Context) {
        self.search.handle_shortcut(
            ctx,
            self.settings.semantic_search_enabled,
            self.library.as_ref(),
            &mut self.canvas,
            &mut self.status,
            self.settings.canvas_image_max_side,
        );
    }

    #[cfg(feature = "semantic-search")]
    fn show_search_overlay(&mut self, ctx: &egui::Context, screen_rect: egui::Rect) {
        let visible_canvas_rect = self.visible_canvas_screen_rect(screen_rect);
        if let Some(drop_request) = self.search.show_overlay(
            ctx,
            screen_rect,
            &mut self.canvas,
            &mut self.status,
            crate::search::SearchPlacementCreationConfig {
                placement_target_area: self.settings.placement_target_area,
                placement_gap: self.settings.placement_gap,
                visible_canvas_rect,
            },
        ) {
            self.handle_search_drop(ctx, drop_request, screen_rect);
        }
        if self.search.take_recent_searches_dirty() {
            self.save_user_session();
        }
    }

    #[cfg(feature = "semantic-search")]
    fn handle_search_drop(
        &mut self,
        ctx: &egui::Context,
        drop_request: crate::search::SearchDropRequest,
        _screen_rect: egui::Rect,
    ) {
        crate::diagnostics::search_drag_debug(format!(
            "handle_search_drop start asset_ids={:?} provisional_ids={:?}",
            drop_request.asset_ids, drop_request.provisional_placement_ids
        ));
        if drop_request.asset_ids.is_empty() {
            crate::diagnostics::search_drag_debug("handle_search_drop abort empty_asset_ids");
            return;
        }

        let Some(library) = self.library.clone() else {
            crate::diagnostics::search_drag_debug("handle_search_drop abort no_library");
            self.status = "Storage unavailable".to_owned();
            return;
        };

        let placement_settings = self.placement_creation_settings();
        let committed = match crate::placement::lifecycle::commit_search_provisional_batch(
            &library,
            &mut self.canvas,
            crate::placement::lifecycle::CommitSearchProvisionalRequest {
                asset_ids: drop_request.asset_ids,
                provisional_placement_ids: drop_request.provisional_placement_ids,
            },
            placement_settings,
        ) {
            Ok(committed) => committed,
            Err(error) => {
                crate::diagnostics::search_drag_debug(format!(
                    "handle_search_drop commit_error error={error}"
                ));
                self.status = error;
                return;
            }
        };
        crate::diagnostics::search_drag_debug(format!(
            "handle_search_drop committed added={:?} load_existing_count={}",
            committed.added_placement_ids,
            committed.load_existing_placements.len()
        ));

        for placement in &committed.load_existing_placements {
            if let Some(import_worker) = &self.import_worker {
                if let Err(error) =
                    import_worker.enqueue(crate::jobs::ImagePrepRequest::LoadExisting {
                        target_board_id: self.active_board_id,
                        queue_id: None,
                        placement: placement.clone(),
                        max_texture_side: self.max_texture_side,
                        canvas_image_settings: crate::jobs::CanvasImageSettings {
                            max_side: self.settings.canvas_image_max_side,
                        },
                        target_lod: crate::data::CanvasImageLod::Working,
                        priority: crate::jobs::ExistingLoadPriority::Background,
                        trace: None,
                    })
                {
                    crate::diagnostics::search_drag_debug(format!(
                        "handle_search_drop enqueue_load_existing_error placement_id={} error={error}",
                        placement.state.id
                    ));
                    self.status = error;
                    return;
                }
            } else {
                crate::diagnostics::search_drag_debug(format!(
                    "handle_search_drop load_existing_sync placement_id={}",
                    placement.state.id
                ));
                match crate::jobs::load_existing(
                    &library,
                    self.active_board_id,
                    None,
                    placement.clone(),
                    self.max_texture_side,
                    crate::jobs::CanvasImageSettings {
                        max_side: self.settings.canvas_image_max_side,
                    },
                    crate::data::CanvasImageLod::Working,
                    None,
                    self.wgpu_render_state.as_ref(),
                ) {
                    crate::jobs::ImagePrepResult::Loaded(loaded) => {
                        crate::diagnostics::search_drag_debug(format!(
                            "handle_search_drop load_existing_loaded placement_id={}",
                            loaded.placement.state.id
                        ));
                        self.integrate_loaded_import(ctx, loaded);
                    }
                    crate::jobs::ImagePrepResult::Failed { error, .. } => {
                        crate::diagnostics::search_drag_debug(format!(
                            "handle_search_drop load_existing_failed error={error}"
                        ));
                        self.status = error;
                        return;
                    }
                    _ => {}
                }
            }
        }

        if !committed.added_placement_ids.is_empty() {
            if let Some(history_action) = committed.history_action.clone() {
                self.document_history.record_applied(history_action);
            }
            self.save_user_session();
            self.refresh_board_summaries();
        }
        let placed_count = committed.placed_count();
        let noun = if placed_count == 1 { "image" } else { "images" };
        self.status = format!("Added {placed_count} search {noun}");
        crate::diagnostics::search_drag_debug(format!(
            "handle_search_drop done status={}",
            self.status
        ));
        self.search.close_overlay(Some(&mut self.canvas));
    }

    #[cfg(feature = "semantic-search")]
    #[cfg_attr(not(test), allow(dead_code))]
    fn source_center_for_placement_ids(&self, placement_ids: &[i64]) -> Option<egui::Pos2> {
        source_center_for_placements(
            &placement_ids
                .iter()
                .filter_map(|placement_id| self.canvas.placement_for_id(*placement_id))
                .collect::<Vec<_>>(),
        )
    }

    fn persist_settings_if_changed(&mut self, settings_changed: bool) {
        Self::persist_settings_if_changed_with(settings_changed, || self.save_user_session());
    }

    fn set_always_on_top(&mut self, ctx: &egui::Context, enabled: bool) {
        if self.settings.always_on_top == enabled {
            return;
        }
        self.settings.always_on_top = enabled;
        self.apply_always_on_top_window_level(ctx);
        self.save_user_session();
    }

    fn apply_always_on_top_window_level(&self, ctx: &egui::Context) {
        let level = if self.settings.always_on_top {
            egui::viewport::WindowLevel::AlwaysOnTop
        } else {
            egui::viewport::WindowLevel::Normal
        };
        ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(level));
    }

    fn persist_settings_if_changed_with<F>(settings_changed: bool, mut persist: F)
    where
        F: FnMut(),
    {
        if settings_changed {
            persist();
        }
    }

    fn preferred_import_world(&self, ctx: &egui::Context, screen_rect: egui::Rect) -> egui::Pos2 {
        let pointer_screen_pos = ctx.input(|input| input.pointer.latest_pos());
        self.preferred_import_world_with_pointer(screen_rect, pointer_screen_pos)
    }

    fn import_viewport_world_rect(&self, screen_rect: egui::Rect) -> egui::Rect {
        let visible_screen_rect = self.visible_canvas_screen_rect(screen_rect);
        egui::Rect::from_two_pos(
            self.canvas
                .screen_to_world(visible_screen_rect.min, screen_rect),
            self.canvas
                .screen_to_world(visible_screen_rect.max, screen_rect),
        )
    }

    fn preferred_import_world_with_pointer(
        &self,
        screen_rect: egui::Rect,
        pointer_screen_pos: Option<egui::Pos2>,
    ) -> egui::Pos2 {
        let visible_rect = self.visible_canvas_screen_rect(screen_rect);
        let canvas_center = visible_rect.center();
        let import_screen_pos = pointer_screen_pos
            .filter(|screen_pos| visible_rect.contains(*screen_pos))
            .unwrap_or(canvas_center);
        self.canvas.screen_to_world(import_screen_pos, screen_rect)
    }

    fn log_loaded_timing(
        &mut self,
        origin: crate::jobs::ImportOrigin,
        placement: Option<&crate::data::Placement>,
        diagnostics: Option<&crate::jobs::ExistingLoadDiagnostics>,
        trace: Option<crate::jobs::ImagePrepTrace>,
    ) {
        let Some(perf_log) = &mut self.perf_log else {
            return;
        };

        let format_trace = |trace: crate::jobs::ImagePrepTrace| {
            format!(
                " total_ms={:.2} queue_ms={:.2} decode_ms={:.2} store_ms={:.2} preview_ms={:.2} worker_ms={:.2} ui_ms={:.2}",
                trace.queued_at.elapsed().as_secs_f64() * 1000.0,
                trace.timings.queue_wait.as_secs_f64() * 1000.0,
                trace.timings.decode_or_load.as_secs_f64() * 1000.0,
                trace.timings.store_and_persist.as_secs_f64() * 1000.0,
                trace.timings.preview_prepare.as_secs_f64() * 1000.0,
                trace.timings.worker_total.as_secs_f64() * 1000.0,
                trace.timings.ui_integration.as_secs_f64() * 1000.0,
            )
        };

        match origin {
            crate::jobs::ImportOrigin::Existing if !perf_log.startup_logged => {
                perf_log.startup_logged = true;
                let startup_ms = perf_log.app_started_at.elapsed().as_secs_f64() * 1000.0;
                if let Some(trace) = trace {
                    eprintln!(
                        "[perf] startup_first_image_ms={startup_ms:.2}{}",
                        format_trace(trace)
                    );
                } else {
                    eprintln!("[perf] startup_first_image_ms={startup_ms:.2}");
                }
            }
            crate::jobs::ImportOrigin::Existing => {
                let startup_ms = perf_log.app_started_at.elapsed().as_secs_f64() * 1000.0;
                let asset = placement.map(|placement| {
                    format!(
                        " placement_id={} asset_id={} file={}",
                        placement.state.id,
                        placement.asset.as_ref().expect("loaded placement asset").id,
                        placement
                            .asset
                            .as_ref()
                            .expect("loaded placement asset")
                            .filename
                    )
                });
                let source = diagnostics.map(|diagnostics| {
                    format!(
                        " source_kind={:?} source_mb={:.2} backfilled_cache={} source={} source_dims={}x{}",
                        diagnostics.source_kind,
                        diagnostics.source_file_size as f64 / 1024.0 / 1024.0,
                        diagnostics.backfilled_cache,
                        diagnostics.source_path.display(),
                        diagnostics.source_width,
                        diagnostics.source_height
                    )
                });
                let trace_text = trace.map(format_trace).unwrap_or_default();
                eprintln!(
                    "[perf] startup_image_visible startup_ms={startup_ms:.2}{}{}{}{}",
                    asset.unwrap_or_default(),
                    source.unwrap_or_default(),
                    if trace_text.is_empty() { "" } else { " " },
                    trace_text
                );
            }
            crate::jobs::ImportOrigin::Clipboard => {
                if let Some(trace) = trace {
                    eprintln!("[perf] clipboard_import{}", format_trace(trace));
                }
            }
            crate::jobs::ImportOrigin::Path => {
                if let Some(trace) = trace {
                    eprintln!("[perf] dropped_file_import{}", format_trace(trace));
                }
            }
            crate::jobs::ImportOrigin::Url => {
                if let Some(trace) = trace {
                    eprintln!("[perf] url_import{}", format_trace(trace));
                }
            }
        }
    }

    fn log_cropped_timing(&mut self, placement: &crate::data::Placement, ui_integration_ms: f64) {
        if self.perf_log.is_none() {
            return;
        }

        eprintln!(
            "[perf] crop_integrated placement_id={} asset_id={} file={} ui_ms={:.2}",
            placement.state.id,
            placement.asset.as_ref().expect("loaded placement asset").id,
            placement
                .asset
                .as_ref()
                .expect("loaded placement asset")
                .filename,
            ui_integration_ms
        );
    }
}

mod sidebar_session;

use sidebar_session::{sidebar_tab_from_session, sidebar_tab_session_value};

fn context_menu_should_close(
    action_clicked: bool,
    just_opened: bool,
    menu_rect: egui::Rect,
    pointer_pos: Option<egui::Pos2>,
    primary_pressed: bool,
    secondary_pressed: bool,
    escape_pressed: bool,
) -> bool {
    if action_clicked || escape_pressed {
        return true;
    }
    if just_opened || (!primary_pressed && !secondary_pressed) {
        return false;
    }
    pointer_pos.is_none_or(|pos| !menu_rect.contains(pos))
}

fn context_menu_row(ui: &mut egui::Ui, item: &ContextMenuItem) -> egui::Response {
    let row_height = 28.0;
    let shortcut_width: f32 = 54.0;
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), row_height),
        if item.enabled {
            egui::Sense::click()
        } else {
            egui::Sense::hover()
        },
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, item.enabled, item.label)
    });

    let hover = crate::ui::motion::hover_amount(
        ui.ctx(),
        response.id.with("context-menu-row-hover"),
        response.hovered() && item.enabled,
    );
    if hover > 0.0 {
        ui.painter().rect_filled(
            rect,
            egui::CornerRadius::same(crate::ui::theme::RADIUS_COMPACT),
            egui::Color32::from_white_alpha((18.0 * hover) as u8),
        );
    }

    let text_color = if item.enabled && item.action.is_destructive() {
        crate::ui::theme::DESTRUCTIVE
    } else if item.enabled {
        crate::ui::theme::TEXT
    } else {
        crate::ui::theme::TEXT_MUTED
    };
    let shortcut_color = if item.enabled {
        crate::ui::theme::TEXT_SUBTLE
    } else {
        crate::ui::theme::TEXT_MUTED
    };

    let label_pos = egui::pos2(rect.left() + 8.0, rect.center().y - 7.5);
    ui.painter().text(
        label_pos,
        egui::Align2::LEFT_TOP,
        item.label,
        egui::FontId::proportional(13.0),
        text_color,
    );

    if let Some(shortcut) = item.shortcut {
        let pill_size = egui::vec2(shortcut_width.min(rect.width() * 0.35), 20.0);
        let pill_rect = egui::Rect::from_center_size(
            egui::pos2(rect.right() - 8.0 - pill_size.x * 0.5, rect.center().y),
            pill_size,
        );
        ui.painter().text(
            pill_rect.center(),
            egui::Align2::CENTER_CENTER,
            shortcut,
            egui::FontId::monospace(11.0),
            shortcut_color,
        );
    }

    response
}

#[cfg(feature = "semantic-search")]
fn semantic_search_settings_status(
    status: crate::search::semantic::LocalModelStoreStatus,
) -> SemanticSearchSettingsStatus {
    match status {
        crate::search::semantic::LocalModelStoreStatus::Installed
        | crate::search::semantic::LocalModelStoreStatus::InstalledViaMigration => {
            SemanticSearchSettingsStatus::Installed
        }
        crate::search::semantic::LocalModelStoreStatus::Missing => {
            SemanticSearchSettingsStatus::Missing
        }
        crate::search::semantic::LocalModelStoreStatus::Downloading { percent } => {
            SemanticSearchSettingsStatus::Downloading { percent }
        }
        crate::search::semantic::LocalModelStoreStatus::Failed(_) => {
            SemanticSearchSettingsStatus::Failed
        }
    }
}

#[cfg(feature = "semantic-search")]
#[cfg_attr(not(test), allow(dead_code))]
fn source_center_for_placements(placements: &[crate::data::Placement]) -> Option<egui::Pos2> {
    let mut min = egui::Pos2::new(f32::INFINITY, f32::INFINITY);
    let mut max = egui::Pos2::new(f32::NEG_INFINITY, f32::NEG_INFINITY);
    let mut found = false;

    for placement in placements {
        found = true;
        let half_size = placement.scaled_display_size() * 0.5;
        min.x = min.x.min(placement.state.geometry.center.x - half_size.x);
        min.y = min.y.min(placement.state.geometry.center.y - half_size.y);
        max.x = max.x.max(placement.state.geometry.center.x + half_size.x);
        max.y = max.y.max(placement.state.geometry.center.y + half_size.y);
    }

    found.then(|| egui::pos2((min.x + max.x) * 0.5, (min.y + max.y) * 0.5))
}

impl eframe::App for SnappyRefApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let screen_rect = ui.ctx().viewport_rect();
        self.update_window_state(ui.ctx());
        ui.set_clip_rect(screen_rect);
        ui.expand_to_include_rect(screen_rect);
        self.frame_perf.import_results_drained = 0;
        self.frame_perf.import_result_drain_ms = 0.0;

        self.handle_import_results(ui.ctx());
        #[cfg(feature = "semantic-search")]
        self.handle_search_results();
        #[cfg(feature = "semantic-search")]
        self.drive_semantic_search_install(ui.ctx());
        self.handle_history_shortcuts(ui.ctx());
        if self.drive_perf_scenario(ui.ctx(), screen_rect) {
            return;
        }

        self.handle_clipboard_copy(ui.ctx());
        self.handle_clipboard_paste(ui.ctx(), screen_rect);
        self.handle_hovered_files(ui, screen_rect);
        self.handle_dropped_files(ui, screen_rect);
        #[cfg(feature = "semantic-search")]
        self.handle_search_shortcut(ui.ctx());
        self.handle_board_shortcuts(ui.ctx());
        self.handle_selection_shortcuts(ui.ctx(), screen_rect);
        let sidebar_open = self.sidebar_open;
        if self.should_block_sidebar_input() {
            block_sidebar_input(ui.ctx(), sidebar_rect(screen_rect));
        }
        self.canvas
            .set_rotation_snap_degrees(self.settings.rotation_snap_degrees);
        self.canvas.set_placement_gap(self.settings.placement_gap);
        self.canvas.set_pointer_mode(self.settings.pointer_mode);
        let visible_canvas_rect = self.visible_canvas_screen_rect(screen_rect);
        #[cfg(feature = "semantic-search")]
        self.search.prepare_canvas_provisional_batch(
            ui.ctx(),
            &mut self.canvas,
            screen_rect,
            crate::search::SearchPlacementCreationConfig {
                placement_target_area: self.settings.placement_target_area,
                placement_gap: self.settings.placement_gap,
                visible_canvas_rect,
            },
        );
        let canvas_input_policy = self.canvas_input_policy(ui.ctx());
        self.frame_perf.canvas = crate::canvas::show(
            ui,
            screen_rect,
            visible_canvas_rect,
            &mut self.canvas,
            self.wgpu_render_state.as_ref(),
            true,
            canvas_input_policy,
            false,
        );
        self.record_perf_painted_frame();
        let context_menu_just_opened =
            if let Some(request) = self.canvas.take_context_menu_request() {
                self.context_menu_pos = Some(request.screen_pos);
                true
            } else {
                false
            };
        if self.canvas.take_focus_selection_request() {
            let visible_rect = self.visible_canvas_screen_rect(screen_rect);
            if self
                .canvas
                .focus_selection_in_view(screen_rect, visible_rect)
            {
                self.status = "Focused selection".to_owned();
            }
        }
        if let Some(error) = self.residency.dispatch_pending_lod_requests(
            self.import_worker.as_ref(),
            &mut self.canvas,
            self.active_board_id,
            &self.settings,
            self.max_texture_side,
        ) {
            self.status = error;
        }
        self.handle_pending_crop_requests(ui.ctx());
        #[cfg(feature = "semantic-search")]
        self.drive_embedding_backfill(ui.ctx());
        self.handle_pending_annotation_requests();
        self.persist_canvas_changes_debounced(ui.ctx());
        let settled_idle = self.app_settled_idle();
        self.update_service.tick(settled_idle);
        self.drive_update_smoke(ui.ctx(), settled_idle);
        self.drive_user_update_flow(ui.ctx());
        if let Some(wait) = self.update_service.time_until_next_check() {
            // Make sure we wake up at the next scheduled check; clamp to
            // a reasonable lower bound so we never hot-loop.
            ui.ctx()
                .request_repaint_after(wait.max(Duration::from_millis(250)));
        }
        let board_summaries = self.board_summaries();
        let active_board_name = board_summaries
            .iter()
            .find(|summary| summary.board.id == self.active_board_id)
            .map(|summary| summary.board.name.clone())
            .unwrap_or_else(|| "My Board".to_owned());
        let about_updates = self.about_updates_view();
        let always_on_top_before = self.settings.always_on_top;
        let sidebar_result = show_sidebar(
            ui.ctx(),
            screen_rect,
            sidebar_open,
            !self.modal_open(),
            &mut self.sidebar_tab,
            &mut self.settings,
            &mut self.canvas,
            &board_summaries,
            self.active_board_id,
            None,
            self.update_service.state(),
            about_updates,
        );
        let board_action = if self.modal_open() {
            None
        } else {
            sidebar_result.board_action
        };
        self.handle_board_action(board_action);
        self.handle_semantic_search_settings_action(
            sidebar_result.semantic_search_action,
            ui.ctx(),
        );
        let modal_open = self.modal_open();
        if sidebar_open && !modal_open {
            close_sidebar_on_outside_click(
                ui.ctx(),
                sidebar_result
                    .interactive_rect
                    .unwrap_or_else(|| sidebar_rect(screen_rect)),
                &mut self.sidebar_open,
            );
        }
        if sidebar_result.settings_changed {
            self.sidebar_open = true;
            if self.settings.always_on_top != always_on_top_before {
                self.apply_always_on_top_window_level(ui.ctx());
            }
            #[cfg(feature = "semantic-search")]
            self.sync_semantic_search_service();
        }
        if sidebar_result.update_action_clicked {
            self.handle_update_action_click(ui.ctx());
        }
        let mut settings_changed = sidebar_result.settings_changed;
        if self.sync_last_update_check_timestamp() {
            settings_changed = true;
        }
        self.persist_settings_if_changed(settings_changed);
        self.show_board_delete_confirmation(ui.ctx());
        self.show_library_remove_confirmation(ui.ctx());
        #[cfg(feature = "semantic-search")]
        self.show_semantic_search_remove_confirmation(ui.ctx());
        #[cfg(feature = "semantic-search")]
        self.show_semantic_search_first_launch_prompt(ui.ctx());
        if !modal_open {
            self.show_context_menu(ui.ctx(), screen_rect, context_menu_just_opened);
            if show_sidebar_hint(ui.ctx(), screen_rect, sidebar_open, &active_board_name) {
                self.sidebar_open = !self.sidebar_open;
            }
        }
        #[cfg(feature = "semantic-search")]
        self.show_search_overlay(ui.ctx(), screen_rect);
    }

    fn on_exit(&mut self) {
        self.save_active_board_before_transition();
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        crate::ui::theme::BACKGROUND.to_normalized_gamma_f32()
    }
}

impl crate::active_board::switch::BoardSwitchHost for SnappyRefApp {
    fn active_board_id(&self) -> i64 {
        self.active_board_id
    }

    fn persist_active_board_before_switch(&mut self) {
        self.save_active_board_before_transition();
    }

    fn set_active_board_in_library(&mut self, board_id: i64) -> Result<(), String> {
        let Some(library) = &self.library else {
            return Err("Storage unavailable".to_owned());
        };
        library.set_active_board(board_id)
    }

    fn load_active_board(&mut self) {
        SnappyRefApp::load_active_board(self);
    }
}

impl crate::active_board::load::ActiveBoardLoadHost for SnappyRefApp {
    fn clear_board_local_transients(&mut self) {
        self.clear_active_board_transients();
    }

    fn clear_canvas_for_active_board_load(&mut self) {
        self.canvas.clear_for_board_switch();
    }

    fn load_active_board_viewport(&mut self) {
        SnappyRefApp::load_session_viewport(self);
    }

    fn dedupe_library_for_active_board_load(&mut self) {
        SnappyRefApp::dedupe_library_for_board_load(self);
    }

    fn enqueue_active_board_existing_items(&mut self) {
        SnappyRefApp::enqueue_existing_items(self);
    }
}

/// Locate the macOS `snappyref-updater` helper next to the running executable.
/// The helper ships inside the same `MacOS/` directory as the main `snappyref`
/// binary, so the stable lookup is by sibling path from `current_exe`.
#[cfg(not(target_os = "windows"))]
fn snappyref_updater_path() -> Result<std::path::PathBuf, String> {
    let current_exe = std::env::current_exe().map_err(|error| format!("current_exe: {error}"))?;
    let parent = current_exe
        .parent()
        .ok_or_else(|| "current_exe has no parent directory".to_owned())?;
    Ok(parent.join("snappyref-updater"))
}

fn restart_launch_args() -> Vec<String> {
    let mut filtered = Vec::new();
    let mut arguments = std::env::args().skip(1);
    while let Some(argument) = arguments.next() {
        if argument == "--update-smoke-install" {
            continue;
        }
        if argument == "--update-smoke-status" {
            let _ = arguments.next();
            continue;
        }
        if argument.starts_with("--update-smoke-status=") {
            continue;
        }
        filtered.push(argument);
    }
    filtered
}

#[cfg(target_os = "macos")]
fn platform_shortcut_label(mac: &'static str, _windows: &'static str) -> &'static str {
    mac
}

#[cfg(not(target_os = "macos"))]
fn platform_shortcut_label(_mac: &'static str, windows: &'static str) -> &'static str {
    windows
}

mod dialogs;

use dialogs::{
    destructive_action_button, destructive_confirmation_frame, destructive_dialog_body,
    destructive_dialog_header, destructive_dialog_width, secondary_dialog_button,
};
#[cfg(feature = "semantic-search")]
use dialogs::{
    primary_dialog_button, semantic_search_prompt_body, semantic_search_prompt_frame,
    semantic_search_prompt_header, semantic_search_prompt_width,
};

fn apply_arrangement_action(
    canvas: &mut crate::canvas::CanvasState,
    action: ArrangementAction,
    gap: f32,
    status: &mut String,
) {
    let arranged = match action {
        ArrangementAction::Compact => canvas.arrange_selected_compact(gap),
        ArrangementAction::Horizontal => canvas.arrange_selected_horizontal(gap),
        ArrangementAction::Vertical => canvas.arrange_selected_vertical(gap),
    };

    let noun = if arranged == 1 { "image" } else { "images" };
    *status = match action {
        ArrangementAction::Compact => format!("Compacted {arranged} {noun}"),
        ArrangementAction::Horizontal => format!("Arranged {arranged} {noun} in a row"),
        ArrangementAction::Vertical => format!("Arranged {arranged} {noun} in a column"),
    };
}

fn apply_settle_arrangement(
    canvas: &mut crate::canvas::CanvasState,
    direction: crate::placement::arrangement::SettleDirection,
    gap: f32,
    status: &mut String,
) {
    let arranged = canvas.settle_selected(direction, gap);
    if arranged == 0 {
        return;
    }

    let noun = if arranged == 1 { "image" } else { "images" };
    let direction_label = match direction {
        crate::placement::arrangement::SettleDirection::Left => "left",
        crate::placement::arrangement::SettleDirection::Right => "right",
        crate::placement::arrangement::SettleDirection::Up => "up",
        crate::placement::arrangement::SettleDirection::Down => "down",
    };
    *status = format!("Settled {arranged} {noun} {direction_label}");
}

fn apply_default_average_area_to_selection(
    canvas: &mut crate::canvas::CanvasState,
    status: &mut String,
) {
    let changed =
        canvas.apply_target_area_to_selected_images(crate::jobs::DEFAULT_PLACEMENT_TARGET_AREA);
    let noun = if changed == 1 { "image" } else { "images" };
    *status = format!("Applied default average area to {changed} {noun}");
}

fn apply_average_selection_area(canvas: &mut crate::canvas::CanvasState, status: &mut String) {
    let Some(target_area) = canvas.average_selected_image_area() else {
        *status = "Select 2 or more images to average their area".to_owned();
        return;
    };
    let changed = canvas.apply_target_area_to_selected_images(target_area);
    let noun = if changed == 1 { "image" } else { "images" };
    *status = format!("Averaged area across {changed} {noun}");
}

fn generated_board_name(boards: &[crate::data::Board]) -> String {
    let names = boards
        .iter()
        .filter(|board| !board.archived)
        .map(|board| board.name.as_str())
        .collect::<std::collections::HashSet<_>>();
    (2..)
        .map(|number| format!("Board {number}"))
        .find(|name| !names.contains(name.as_str()))
        .unwrap_or_else(|| "Board 2".to_owned())
}

#[cfg(test)]
mod tests;
