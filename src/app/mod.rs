use eframe::egui;
use std::time::Duration;
use std::time::Instant;

use crate::active_board::commands::{ActiveBoardCommandContext, PasteCommandOutcome};
use crate::active_board::history::{
    BoardLifecycleHistoryAction, DocumentHistory, HistoryAction, HistoryChange,
};
use crate::active_board::persistence::DocumentPersistence;
use crate::library::residency::ImageResidencyCoordinator;
use crate::placement::content_resolution::ContentResolutionRuntime;
use crate::search::SearchController;
use crate::sources::crop;
use crate::ui::sidebar::{
    SidebarBoardAction, SidebarTab, block_sidebar_input, close_sidebar_on_outside_click,
    show_sidebar, show_sidebar_hint, show_status_message, sidebar_rect,
};
use crate::updater::{UpdateConfig, UpdateService};

const STARTUP_VIEWPORT_SIZE: egui::Vec2 = egui::vec2(960.0, 640.0);
const USER_SESSION_SAVE_DEBOUNCE: Duration = Duration::from_millis(750);
const BOARD_LOAD_DEDUPE_HASH_LIMIT: usize = 32;
const BOARD_LOAD_DEDUPE_GROUP_LIMIT: usize = 8;

mod context_menu;
mod history;
mod import_intake;
mod perf_scenario;

use perf_scenario::{FramePerfState, PerfLogState, PerfScenarioState};
mod semantic_search;
mod shortcuts;
mod updates;

use updates::UpdateSmokeState;

use shortcuts::{
    ArrangementAction, PendingBoardDelete, PendingLibraryRemove, SupportedShortcutAction,
    delete_shortcut_pressed, settle_direction_pressed, shortcut_pressed,
};
pub(crate) use shortcuts::{SUPPORTED_SHORTCUTS, SupportedShortcut, platform_shortcut_label};

#[derive(Clone, Debug)]
pub struct AppConfig {
    library_root: Result<std::path::PathBuf, String>,
    no_startup_focus: bool,
    perf_log: bool,
    perf_decode_path: Option<std::path::PathBuf>,
    perf_scenario: Option<String>,
    perf_corpus: Option<std::path::PathBuf>,
    perf_run_dir: Option<std::path::PathBuf>,
    update_smoke_install: bool,
    update_smoke_status_path: Option<std::path::PathBuf>,
}

impl AppConfig {
    pub fn from_env() -> Self {
        let mut no_startup_focus = false;
        let mut perf_log = false;
        let mut perf_decode_path = None;
        let mut perf_scenario = None;
        let mut perf_corpus = None;
        let mut perf_run_dir = None;
        let mut update_smoke_install = false;
        let update_smoke_status_path =
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
            } else if argument == "--perf-decode" {
                perf_decode_path = arguments.next().map(std::path::PathBuf::from);
            } else if let Some(path) = argument.strip_prefix("--perf-decode=") {
                perf_decode_path = Some(std::path::PathBuf::from(path));
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
            }
        }

        Self {
            no_startup_focus,
            perf_log,
            perf_decode_path,
            perf_scenario,
            perf_corpus,
            perf_run_dir,
            library_root,
            update_smoke_install,
            update_smoke_status_path,
        }
    }

    fn skip_existing_items_on_start(&self) -> bool {
        self.perf_scenario_requested() && self.perf_scenario.as_deref() != Some("startup")
    }

    pub(crate) fn startup_viewport_size(&self) -> egui::Vec2 {
        STARTUP_VIEWPORT_SIZE
    }

    pub fn perf_log_enabled(&self) -> bool {
        self.perf_log
    }

    pub fn perf_decode_path(&self) -> Option<&std::path::Path> {
        self.perf_decode_path.as_deref()
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
    document_history: DocumentHistory,
    max_texture_side: usize,
    settings: crate::data::WorkspaceSettings,
    search: SearchController,
    sidebar_open: bool,
    sidebar_tab: SidebarTab,
    pending_board_delete: Option<PendingBoardDelete>,
    pending_library_remove: Option<PendingLibraryRemove>,
    pending_semantic_search_remove: bool,
    context_menu_pos: Option<egui::Pos2>,
    /// Transient status line. Actions and storage errors write it; it shows
    /// bottom-left for [`crate::ui::theme::STATUS_MESSAGE_SECONDS`] and then clears.
    status: String,
    status_shown: String,
    status_shown_since: Instant,
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

struct HistoryCanvasReplay {
    document_changes: crate::canvas::CanvasDocumentChanges,
    placements_needing_content: Vec<crate::data::Placement>,
}

impl BoardSummariesCache {
    fn summaries(&self) -> Vec<crate::data::BoardSummary> {
        self.summaries.clone()
    }

    fn clear(&mut self) {
        self.summaries.clear();
    }

    fn refresh_from(&mut self, library: &crate::library::storage::Library) -> Result<(), String> {
        let accepted = std::mem::take(&mut self.summaries);
        self.summaries = library.list_board_summaries()?;
        for summary in accepted {
            if !self
                .summaries
                .iter()
                .any(|durable| durable.board.id == summary.board.id)
            {
                self.summaries.push(summary);
            }
        }
        self.summaries.sort_by_key(|summary| summary.board.id);
        Ok(())
    }

    fn restore_document(&mut self, document: &crate::data::BoardDocumentSnapshot) {
        self.summaries
            .retain(|summary| summary.board.id != document.board.id);
        self.summaries.push(crate::data::BoardSummary {
            board: document.board.clone(),
            placement_count: document.placements.len(),
        });
        self.summaries.sort_by_key(|summary| summary.board.id);
    }

    fn archive(&mut self, board_id: i64) {
        self.summaries
            .retain(|summary| summary.board.id != board_id);
    }

    fn rename(&mut self, board_id: i64, name: &str) {
        if let Some(summary) = self
            .summaries
            .iter_mut()
            .find(|summary| summary.board.id == board_id)
        {
            summary.board.name = name.to_owned();
        }
    }

    fn set_placement_count(&mut self, board_id: i64, placement_count: usize) {
        if let Some(summary) = self
            .summaries
            .iter_mut()
            .find(|summary| summary.board.id == board_id)
        {
            summary.placement_count = placement_count;
        }
    }
}

impl SnappyRefApp {
    pub fn new(cc: &eframe::CreationContext<'_>, config: AppConfig) -> Self {
        crate::platform::native_drop::install_url_drop_bridge(cc);
        let max_texture_side = cc.egui_ctx.input(|input| input.max_texture_side.max(1));
        let mut app = Self::detached(
            cc.wgpu_render_state.clone(),
            max_texture_side,
            UpdateConfig::default(),
        );
        app.persist_window_state = config.persist_window_state_enabled();
        app.perf_log = config.perf_log_enabled().then(|| PerfLogState {
            app_started_at: Instant::now(),
            startup_logged: false,
        });
        app.perf_scenario = PerfScenarioState::from_config(&config);
        app.update_smoke = UpdateSmokeState::from_config(&config);

        let library = config
            .library_root()
            .map(std::path::Path::to_path_buf)
            .and_then(crate::library::storage::Library::initialize);
        let library = match library {
            Ok(library) => library,
            Err(error) => {
                app.status = format!("Storage unavailable: {error}");
                return app;
            }
        };

        app.status = match library.load_document_history() {
            Ok(history) => {
                app.document_history = history;
                "Library ready".to_owned()
            }
            Err(error) => format!("Board history unavailable: {error}"),
        };
        app.import_worker = Some(crate::jobs::ImagePrepWorker::new(
            library.clone(),
            cc.egui_ctx.clone(),
            cc.wgpu_render_state.clone(),
        ));
        app.document_persistence = DocumentPersistence::queued(library.clone());
        app.library = Some(library);
        app.load_user_session();
        app.refresh_board_summaries();
        app.sync_semantic_search_service();
        if !config.skip_existing_items_on_start() {
            app.load_groups();
            app.enqueue_existing_items();
        }
        app
    }

    /// An app with no library, worker, or config-derived state. `new` and
    /// tests attach what they need on top.
    fn detached(
        wgpu_render_state: Option<eframe::egui_wgpu::RenderState>,
        max_texture_side: usize,
        update_config: UpdateConfig,
    ) -> Self {
        Self {
            canvas: crate::canvas::CanvasState::default(),
            wgpu_render_state,
            library: None,
            import_worker: None,
            active_board_id: 1,
            window_state: crate::data::WindowState::default(),
            persist_window_state: true,
            document_history: DocumentHistory::default(),
            max_texture_side,
            settings: crate::data::WorkspaceSettings::default(),
            search: SearchController::default(),
            sidebar_open: false,
            sidebar_tab: SidebarTab::default(),
            pending_board_delete: None,
            pending_library_remove: None,
            pending_semantic_search_remove: false,
            context_menu_pos: None,
            status: String::new(),
            status_shown: String::new(),
            status_shown_since: Instant::now(),
            perf_log: None,
            frame_perf: FramePerfState::default(),
            perf_scenario: None,
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
            update_service: UpdateService::new(update_config),
            user_update_flow_active: false,
            update_smoke: None,
            last_manual_update_check: None,
        }
    }

    fn load_groups(&mut self) {
        if let Some(library) = &self.library {
            match library.load_groups() {
                Ok(groups) => self.canvas.restore_groups(groups),
                Err(error) => self.status = error,
            }
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
                if let Some(scenario) = &mut self.perf_scenario
                    && scenario.name == "startup"
                {
                    scenario.expected_imports = placements.len();
                }
                let viewport_world = self.startup_viewport_world_rect();
                if let Err(error) = self.residency.queue_startup_existing_loads(
                    import_worker,
                    &mut self.canvas,
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

    fn board_summaries(&self) -> Vec<crate::data::BoardSummary> {
        self.board_summaries_cache.summaries()
    }

    fn refresh_board_summaries(&mut self) {
        let Some(library) = &self.library else {
            self.board_summaries_cache.clear();
            return;
        };
        let accepted_active_board = self
            .board_summaries_cache
            .summaries()
            .into_iter()
            .find(|summary| summary.board.id == self.active_board_id)
            .map(|summary| summary.board);
        if let Err(error) = self.board_summaries_cache.refresh_from(library) {
            self.status = error;
            return;
        }
        if !self
            .board_summaries_cache
            .summaries()
            .iter()
            .any(|summary| summary.board.id == self.active_board_id)
            && let Some(board) = accepted_active_board
        {
            self.board_summaries_cache
                .restore_document(&crate::data::BoardDocumentSnapshot {
                    board,
                    placements: self.canvas.committed_placements(),
                    annotations: Vec::new(),
                    groups: self.canvas.groups().to_vec(),
                    stack_order: Vec::new(),
                    viewport: self.canvas.viewport_state(),
                });
        }
        self.board_summaries_cache.set_placement_count(
            self.active_board_id,
            self.canvas.committed_placements().len(),
        );
    }

    fn create_and_switch_board(&mut self, keep_sidebar_open: bool) {
        let Some(library) = &self.library else {
            self.status = "Storage unavailable".to_owned();
            return;
        };

        let boards = self
            .board_summaries()
            .into_iter()
            .map(|summary| summary.board)
            .collect::<Vec<_>>();
        let active_before = self.active_board_id;
        let name = generated_board_name(&boards);
        let board = library.new_board(name);
        let document = crate::data::BoardDocumentSnapshot {
            board: board.clone(),
            placements: Vec::new(),
            annotations: Vec::new(),
            groups: Vec::new(),
            stack_order: Vec::new(),
            viewport: crate::data::ViewportState::default(),
        };
        self.document_history.record_applied(
            board.id,
            HistoryAction::Board(BoardLifecycleHistoryAction::Created {
                document: document.clone(),
                active_before,
            }),
        );
        if let Err(error) = self.document_persistence.apply_board_lifecycle_unit(
            crate::active_board::persistence::BoardLifecyclePersistence::Create {
                document: document.clone(),
                active_board_id: board.id,
            },
            &self.document_history,
        ) {
            self.status = error;
            return;
        }
        self.clear_active_board_transients();
        self.canvas.clear_for_board_switch();
        self.active_board_id = board.id;
        self.canvas.apply_viewport_state(&document.viewport);
        self.save_user_session();
        self.board_summaries_cache.restore_document(&document);
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
                let Some(before) = self
                    .board_summaries()
                    .into_iter()
                    .find(|summary| summary.board.id == board_id)
                    .map(|summary| summary.board.name)
                else {
                    self.status = "Board unavailable".to_owned();
                    return;
                };
                if self.library.is_none() {
                    self.status = "Storage unavailable".to_owned();
                    return;
                }
                let after = name.trim().to_owned();
                if after.is_empty() || after == before {
                    return;
                }
                self.document_history.record_applied(
                    self.active_board_id,
                    HistoryAction::Board(BoardLifecycleHistoryAction::Renamed {
                        board_id,
                        before_name: before,
                        after_name: after.clone(),
                    }),
                );
                match self.document_persistence.apply_board_lifecycle_unit(
                    crate::active_board::persistence::BoardLifecyclePersistence::Rename {
                        board_id,
                        name: after.clone(),
                    },
                    &self.document_history,
                ) {
                    Ok(()) => {
                        self.board_summaries_cache.rename(board_id, &after);
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

    fn request_board_delete(&mut self, board_id: i64, board_name: String) {
        self.pending_board_delete = Some(PendingBoardDelete {
            board_id,
            board_name,
        });
    }

    fn cancel_pending_board_delete(&mut self) {
        self.pending_board_delete = None;
    }

    fn confirm_pending_board_delete(&mut self, ctx: &egui::Context) {
        let Some(pending) = self.pending_board_delete.take() else {
            return;
        };

        self.persist_canvas_changes();
        self.save_user_session();

        let Some(library) = &self.library else {
            self.status = "Storage unavailable".to_owned();
            return;
        };

        let active_before = self.active_board_id;
        let mut document = match library.load_board_document(pending.board_id) {
            Ok(document) => document,
            Err(error) if pending.board_id != self.active_board_id => {
                self.status = error;
                return;
            }
            Err(_) => {
                let Some(board) = self
                    .board_summaries()
                    .into_iter()
                    .find(|summary| summary.board.id == pending.board_id)
                    .map(|summary| summary.board)
                else {
                    self.status = "Board unavailable".to_owned();
                    return;
                };
                crate::data::BoardDocumentSnapshot {
                    board,
                    placements: self.canvas.committed_placements(),
                    annotations: self.canvas.annotations(),
                    groups: self.canvas.groups().to_vec(),
                    stack_order: self.canvas.canvas_stack_order(),
                    viewport: self.canvas.viewport_state(),
                }
            }
        };
        self.document_history
            .apply_accepted_document_state(pending.board_id, &mut document);
        if pending.board_id == self.active_board_id {
            document.viewport = self.canvas.viewport_state();
        }
        if let Some(board) = self
            .board_summaries()
            .into_iter()
            .find(|summary| summary.board.id == pending.board_id)
            .map(|summary| summary.board)
        {
            document.board = board;
        }
        let board_ids = self
            .board_summaries()
            .into_iter()
            .map(|summary| summary.board.id)
            .collect::<Vec<_>>();
        let Some(deleted_index) = board_ids.iter().position(|id| *id == pending.board_id) else {
            self.status = "Board unavailable".to_owned();
            return;
        };
        if board_ids.len() == 1 {
            self.status = "cannot delete the last board".to_owned();
            return;
        }
        let active_after = if active_before != pending.board_id {
            active_before
        } else if deleted_index > 0 {
            board_ids[deleted_index - 1]
        } else {
            board_ids[deleted_index + 1]
        };
        let active_document = if active_before != active_after {
            match self.accepted_board_document(library, active_after) {
                Ok(document) => Some(document),
                Err(error) => {
                    self.status = error;
                    return;
                }
            }
        } else {
            None
        };
        self.document_history.record_applied(
            active_after,
            HistoryAction::Board(BoardLifecycleHistoryAction::Deleted {
                document: document.clone(),
                active_before,
                active_after,
            }),
        );
        if let Err(error) = self.document_persistence.apply_board_lifecycle_unit(
            crate::active_board::persistence::BoardLifecyclePersistence::Archive {
                board_id: pending.board_id,
                active_board_id: active_after,
            },
            &self.document_history,
        ) {
            self.status = error;
            return;
        }

        self.board_summaries_cache.archive(pending.board_id);
        if let Some(active_document) = active_document
            && let Err(error) = self.open_board_document_snapshot(ctx, &active_document)
        {
            self.status = error;
            return;
        }
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
        {
            self.search.close_overlay(Some(&mut self.canvas));
            self.search.reset_embedding_backfill();
        }
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
                            self.confirm_pending_board_delete(ctx);
                        }
                    });
                });
            });
    }

    fn modal_open(&self) -> bool {
        self.pending_board_delete.is_some()
            || self.pending_library_remove.is_some()
            || self.pending_semantic_search_remove
            || self.should_show_semantic_search_first_launch_prompt()
    }

    fn should_block_sidebar_input(&self) -> bool {
        self.sidebar_open && self.modal_open()
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

    fn request_selected_assets_library_remove(&mut self) {
        let mut asset_ids = Vec::new();
        for placement_id in self.canvas.selected_placement_ids() {
            if let Some(asset_id) = self
                .canvas
                .placement_for_id(placement_id)
                .and_then(|placement| placement.asset_id())
                && !asset_ids.contains(&asset_id)
            {
                asset_ids.push(asset_id);
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
        {
            self.search.invalidate_library_search_cache();
        }
        self.refresh_board_summaries();
        let noun = if pending.asset_ids.len() == 1 {
            "asset"
        } else {
            "assets"
        };
        self.status = format!("Removed {} library {noun}", pending.asset_ids.len());
    }

    fn canvas_input_policy(&self, ctx: &egui::Context) -> crate::canvas::CanvasInputPolicy {
        if self.context_menu_pos.is_some() || self.modal_open() {
            return crate::canvas::CanvasInputPolicy::Blocked;
        }

        if !self.search.blocks_canvas_input(ctx) {
            return crate::canvas::CanvasInputPolicy::Full;
        }
        if self.search.provisional_batch_active() {
            return crate::canvas::CanvasInputPolicy::ViewportOnly;
        }
        crate::canvas::CanvasInputPolicy::Blocked
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

    #[cfg(test)]
    fn copy_selected_board_payload(
        &self,
    ) -> Option<crate::active_board::commands::ClipboardBoardSelection> {
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

    #[cfg(test)]
    fn paste_board_payload_at(
        &mut self,
        payload: &crate::active_board::commands::ClipboardBoardSelection,
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

    fn apply_paste_command_outcome(&mut self, outcome: PasteCommandOutcome) {
        let needs_existing_reload = outcome.needs_existing_reload;
        self.status = outcome.status;
        if needs_existing_reload {
            self.enqueue_existing_items();
        }
        self.refresh_board_summaries();
    }

    fn handle_selection_shortcuts(&mut self, ctx: &egui::Context, screen_rect: egui::Rect) {
        if self.search.is_open() {
            return;
        }

        if ctx.egui_wants_keyboard_input() {
            return;
        }

        if ctx.input(|i| i.key_pressed(egui::Key::G)) {
            let modifiers = ctx.input(|i| i.modifiers);
            if modifiers.command && modifiers.shift {
                self.canvas.ungroup_selected();
            } else if modifiers.command
                && let Some(library) = &self.library
            {
                if let Err(error) = self.canvas.create_group(library.allocate_document_id()) {
                    self.status = error;
                }
            } else if !modifiers.alt && !modifiers.shift {
                self.canvas.open_group_picker();
            }
            return;
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.canvas.leave_group_member_editing();
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
                    if !self.canvas.has_placement_selection() {
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
        let before_stack_order = self.canvas.canvas_stack_order();
        let mut context = self.active_board_command_context(viewport_world);
        match crate::active_board::commands::duplicate_selection(&mut context) {
            Ok(Some(outcome)) => {
                let label = outcome.status.clone();
                self.apply_paste_command_outcome(outcome);
                self.record_created_selection_history(label, before_stack_order);
            }
            Ok(None) => {}
            Err(error) => self.status = error,
        }
    }

    fn handle_pending_crop_requests(&mut self, ctx: &egui::Context) {
        match crop::enqueue_pending_requests(
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
        let Some(library) = self.library.clone() else {
            return;
        };

        while let Some(request) = self.canvas.take_pending_annotation() {
            let before_stack_order = self.canvas.canvas_stack_order();
            let background_enabled = request.kind == crate::data::CanvasAnnotationKind::Textbox
                && self.settings.default_textbox_background;
            let annotation = library.new_canvas_annotation(
                request.kind,
                request.start_world(),
                request.end_world(),
                request.text,
                request.text_size,
                crate::data::CanvasStrokeWidth::default(),
                crate::data::CanvasAnnotationColor::default(),
                background_enabled,
            );
            let should_focus_text = annotation.kind == crate::data::CanvasAnnotationKind::Textbox;
            let annotation_id = annotation.id;
            self.canvas.add_annotation(annotation);
            self.canvas.mark_annotation_for_persistence(annotation_id);
            if let Some(created) = self.canvas.annotation_for_id(annotation_id) {
                self.document_history.record_applied(
                    self.active_board_id,
                    HistoryAction::document(
                        match request.kind {
                            crate::data::CanvasAnnotationKind::Arrow => "Added arrow",
                            crate::data::CanvasAnnotationKind::Line => "Added line",
                            crate::data::CanvasAnnotationKind::Textbox => "Added textbox",
                        }
                        .to_owned(),
                        Vec::new(),
                        vec![HistoryChange {
                            before: None,
                            after: Some(created),
                        }],
                        Some(HistoryChange {
                            before: Some(before_stack_order),
                            after: Some(self.canvas.canvas_stack_order()),
                        }),
                    ),
                );
                self.persist_recorded_canvas_changes();
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
    }

    fn persist_canvas_changes(&mut self) {
        self.record_pending_document_changes_history("Edit board");
        self.persist_recorded_canvas_changes();
    }

    fn persist_recorded_canvas_changes(&mut self) {
        let session = self.current_user_session();
        if let Err(error) = self.document_persistence.commit_canvas_changes_unit(
            &mut self.canvas,
            self.active_board_id,
            &self.document_history,
            session,
        ) {
            self.status = error;
        }
        self.pending_user_session = None;
        self.pending_user_session_save_at = None;
    }

    fn persist_canvas_changes_debounced(&mut self, ctx: &egui::Context) {
        self.record_pending_document_changes_history("Edit board");
        let document_changed = match self.document_persistence.commit_canvas_document_unit(
            &mut self.canvas,
            self.active_board_id,
            &self.document_history,
        ) {
            Ok(document_changed) => document_changed,
            Err(error) => {
                self.status = error;
                false
            }
        };
        let session = self.current_user_session();
        if document_changed {
            if let Err(error) = self.document_persistence.save_user_session(session) {
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
                && let Err(error) = self.document_persistence.save_user_session(session)
            {
                self.status = error;
            }
            self.pending_user_session_save_at = None;
        } else {
            ctx.request_repaint_after(save_at - now);
        }
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
            recent_searches: self.search.recent_searches().to_vec(),
        }
    }

    fn active_board_command_context(
        &mut self,
        viewport_world: egui::Rect,
    ) -> ActiveBoardCommandContext<'_> {
        ActiveBoardCommandContext {
            canvas: &mut self.canvas,
            library: self.library.as_ref(),
            import_worker_available: self.import_worker.is_some(),
            placement_gap: self.settings.placement_gap,
            viewport_world,
        }
    }

    fn delete_selected_items_from_board(&mut self, screen_rect: egui::Rect) {
        let before_groups = self.canvas.groups().to_vec();
        let before_all = self.canvas.committed_placements();
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
                let mut placement_changes: Vec<_> = before_items
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
                placement_changes.extend(
                    self.canvas
                        .pending_document_changes_preview()
                        .updated_placements
                        .iter()
                        .filter_map(|state| {
                            let after = self.canvas.placement_for_id(state.id)?;
                            let before =
                                before_all.iter().find(|p| p.state.id == state.id)?.clone();
                            Some(HistoryChange {
                                before: Some(before),
                                after: Some(after),
                            })
                        }),
                );
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
                self.document_history.record_applied(
                    self.active_board_id,
                    HistoryAction::document(
                        status.clone(),
                        placement_changes,
                        annotation_changes,
                        Some(HistoryChange {
                            before: Some(before_stack_order),
                            after: Some(self.canvas.canvas_stack_order()),
                        }),
                    )
                    .with_groups(Some(HistoryChange {
                        before: Some(before_groups),
                        after: Some(self.canvas.groups().to_vec()),
                    })),
                );
                self.persist_recorded_canvas_changes();
                self.refresh_board_summaries();
                self.status = status;
            }
            Ok(None) => {}
            Err(error) => self.status = error,
        }
    }

    fn save_user_session(&mut self) {
        let session = self.current_user_session();
        if let Err(error) = self.document_persistence.save_user_session(session) {
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
}

mod sidebar_session;

use sidebar_session::{sidebar_tab_from_session, sidebar_tab_session_value};

impl SnappyRefApp {
    /// The status message currently on screen. A changed `status` starts a
    /// new visibility window; an expired one clears `status` so the same
    /// message can show again later.
    fn visible_status(&mut self, ctx: &egui::Context) -> Option<&str> {
        if self.status.is_empty() {
            self.status_shown.clear();
            return None;
        }
        if self.status != self.status_shown {
            self.status_shown.clone_from(&self.status);
            self.status_shown_since = Instant::now();
        }
        let visible_for = Duration::from_secs_f32(crate::ui::theme::STATUS_MESSAGE_SECONDS);
        let Some(remaining) = visible_for.checked_sub(self.status_shown_since.elapsed()) else {
            self.status.clear();
            self.status_shown.clear();
            return None;
        };
        ctx.request_repaint_after(remaining);
        Some(&self.status)
    }
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
        self.handle_search_results();
        self.drive_semantic_search_install(ui.ctx());
        self.handle_history_shortcuts(ui.ctx());
        if self.drive_perf_scenario(ui.ctx(), screen_rect) {
            return;
        }

        self.handle_clipboard_copy(ui.ctx());
        self.handle_clipboard_paste(ui.ctx(), screen_rect);
        self.handle_hovered_files(ui, screen_rect);
        self.handle_dropped_files(ui, screen_rect);
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
        self.drive_embedding_backfill(ui.ctx());
        self.handle_pending_annotation_requests();
        self.persist_canvas_changes_debounced(ui.ctx());
        if let Some(error) = self.document_persistence.take_error() {
            self.status = error;
        }
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
        self.show_semantic_search_remove_confirmation(ui.ctx());
        self.show_semantic_search_first_launch_prompt(ui.ctx());
        if !modal_open {
            self.show_context_menu(ui.ctx(), screen_rect, context_menu_just_opened);
            let hint = show_sidebar_hint(ui.ctx(), screen_rect, sidebar_open, &active_board_name);
            if hint.clicked {
                self.sidebar_open = !self.sidebar_open;
            }
            let status = self.visible_status(ui.ctx());
            show_status_message(ui.ctx(), screen_rect, sidebar_open, hint.rect, status);
        }
        self.show_search_overlay(ui.ctx(), screen_rect);
    }

    fn on_exit(&mut self) {
        self.save_active_board_before_transition();
        if let Err(error) = self.document_persistence.shutdown() {
            eprintln!("persistence shutdown: {error}");
        }
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
        // Written directly so a failure stops the switch before the current
        // board is unloaded. Safe to bypass the persistence queue only because
        // `persist_active_board_before_switch` drained it first.
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
        self.load_groups();
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

mod dialogs;

use dialogs::{
    destructive_action_button, destructive_confirmation_frame, destructive_dialog_body,
    destructive_dialog_header, destructive_dialog_width, secondary_dialog_button,
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
        canvas.apply_target_area_to_selected_placements(crate::jobs::DEFAULT_PLACEMENT_TARGET_AREA);
    let noun = if changed == 1 { "image" } else { "images" };
    *status = format!("Applied default average area to {changed} {noun}");
}

fn apply_average_selection_area(canvas: &mut crate::canvas::CanvasState, status: &mut String) {
    let Some(target_area) = canvas.average_selected_placement_area() else {
        *status = "Select 2 or more images to average their area".to_owned();
        return;
    };
    let changed = canvas.apply_target_area_to_selected_placements(target_area);
    let noun = if changed == 1 { "image" } else { "images" };
    *status = format!("Averaged area across {changed} {noun}");
}

fn generated_board_name(boards: &[crate::data::Board]) -> String {
    let names = boards
        .iter()
        .map(|board| board.name.as_str())
        .collect::<std::collections::HashSet<_>>();
    (2..)
        .map(|number| format!("Board {number}"))
        .find(|name| !names.contains(name.as_str()))
        .unwrap_or_else(|| "Board 2".to_owned())
}

#[cfg(test)]
mod tests;
