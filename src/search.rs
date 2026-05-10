use std::collections::HashMap;
use std::collections::HashSet;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use eframe::egui;

use crate::canvas::CanvasState;
use crate::palette::{
    ImageSearchPivot, PALETTE_GRID_MAX_HEIGHT, PALETTE_WIDTH, SEARCH_DEBOUNCE_MS,
    SEARCH_OVERLAY_TOP_MARGIN, SEARCH_RESULT_PAGE_SIZE, SearchOverlayState, SearchOverlayStatus,
    paint_palette_divider, paint_palette_empty_state, paint_palette_footer,
    paint_palette_input_row, paint_palette_no_results_state, search_result_grid_content_height,
    search_result_grid_layout, search_result_tile,
};
use crate::semantic_search::{
    DEFAULT_SEMANTIC_SEARCH_MODEL, IndexResult as SearchIndexResult, SearchQueryResult,
    SearchResultGroup, SearchScope, SemanticSearchConfig, SemanticSearchService,
};
use crate::storage::Library;

const SEARCH_RESULT_SCROLL_ANIMATION: egui::style::ScrollAnimation = egui::style::ScrollAnimation {
    points_per_second: 3200.0,
    duration: egui::Rangef {
        min: 0.04,
        max: 0.16,
    },
};
const SEARCH_RESULT_SCROLL_MARGIN: f32 = 12.0;
const MAX_RECENT_SEARCHES: usize = 8;
const FALLBACK_ASSET_PREVIEW_WIDTH: u32 = 360;
const FALLBACK_ASSET_PREVIEW_HEIGHT: u32 = 240;
const EMBEDDING_BACKFILL_IDLE_QUIET_MS: u64 = 750;

#[derive(Clone, Copy)]
pub(crate) struct SearchPlacementPreviewConfig {
    pub(crate) import_target_canvas_area: f32,
    pub(crate) import_gap: f32,
}

#[derive(Default)]
pub(crate) struct SearchController {
    service: Option<SemanticSearchService>,
    pub(crate) overlay: SearchOverlayState,
    result_cache: HashMap<SearchCacheKey, SearchCacheEntry>,
    active_search: Option<ActiveSearchRequest>,
    prefetch_searches: HashMap<u64, SearchCacheKey>,
    pending_result_selection: Option<PendingResultSelection>,
    embedding_backfill: EmbeddingBackfillScheduler,
    pending_demand_embeddings: HashSet<i64>,
    completed_demand_embeddings: usize,
    next_search_request_id: u64,
    recent_searches: Vec<String>,
    recent_searches_dirty: bool,
    perf_events: Vec<SemanticPerfEvent>,
    #[cfg(all(debug_assertions, feature = "semantic-search"))]
    latency_log: SearchLatencyLog,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum SemanticPerfEvent {
    ModelLoadStarted,
    ModelLoadTiming {
        assets_resolved_ms: f64,
        tokenizer_loaded_ms: f64,
        text_session_loaded_ms: f64,
        vision_session_loaded_ms: f64,
        total_ms: f64,
    },
    ModelReady,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
struct SearchCacheKey {
    scope: SearchScope,
    query: String,
    pivot_asset_id: Option<i64>,
    result_limit: usize,
}

#[derive(Clone, Debug)]
struct SearchCacheEntry {
    results: Vec<crate::semantic_search::SearchResultGroup>,
    has_more_results: bool,
}

#[derive(Clone, Debug)]
struct ActiveSearchRequest {
    request_id: u64,
    cache_key: SearchCacheKey,
    query: String,
}

#[derive(Clone, Copy, Debug)]
struct PendingResultSelection {
    index: usize,
    mode: SearchSelectionMode,
}

#[derive(Default)]
struct EmbeddingBackfillScheduler {
    quiet_since: Option<Instant>,
    in_flight: bool,
    exhausted: bool,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct EmbeddingBackfillIdleState {
    pub(crate) app_idle: bool,
}

#[cfg(all(debug_assertions, feature = "semantic-search"))]
#[derive(Clone, Debug, Default)]
struct SearchLatencyLog {
    enabled: bool,
    command_started_at: Option<Instant>,
    tab_started_at: Option<Instant>,
    tab_target_scope: Option<SearchScope>,
    command_ready_logged: bool,
    command_painted_logged: bool,
    tab_ready_logged: bool,
    tab_painted_logged: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct SearchDropRequest {
    pub item_ids: Vec<i64>,
    pub asset_ids: Vec<i64>,
    pub screen_pos: egui::Pos2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SearchSelectionMode {
    Replace,
    Toggle,
    Range,
}

impl SearchController {
    pub(crate) fn is_open(&self) -> bool {
        self.overlay.open
    }

    pub(crate) fn set_recent_searches(&mut self, recent_searches: Vec<String>) {
        self.recent_searches = normalized_recent_searches(recent_searches);
        self.recent_searches_dirty = false;
        self.overlay.active_recent_search_index = 0;
    }

    pub(crate) fn recent_searches(&self) -> &[String] {
        &self.recent_searches
    }

    pub(crate) fn take_recent_searches_dirty(&mut self) -> bool {
        std::mem::take(&mut self.recent_searches_dirty)
    }

    pub(crate) fn drain_perf_events(&mut self) -> Vec<SemanticPerfEvent> {
        std::mem::take(&mut self.perf_events)
    }

    pub(crate) fn unload_model_for_perf(&mut self) -> bool {
        let Some(service) = &self.service else {
            return false;
        };
        service.unload_model()
    }

    #[cfg(all(debug_assertions, feature = "semantic-search"))]
    pub(crate) fn set_latency_logging_enabled(&mut self, enabled: bool) {
        self.latency_log.enabled = enabled;
        if enabled {
            eprintln!("[search-latency] passive normal-mode logging enabled");
        }
    }

    #[cfg(all(debug_assertions, feature = "semantic-search"))]
    pub(crate) fn begin_image_search_latency_trace(&mut self, source: &'static str, asset_id: i64) {
        if !self.latency_log.enabled {
            return;
        }
        self.latency_log.command_started_at = Some(Instant::now());
        self.latency_log.tab_started_at = None;
        self.latency_log.tab_target_scope = None;
        self.latency_log.command_ready_logged = false;
        self.latency_log.command_painted_logged = false;
        self.latency_log.tab_ready_logged = false;
        self.latency_log.tab_painted_logged = false;
        eprintln!("[search-latency] {source}_started asset_id={asset_id}");
    }

    #[cfg(all(debug_assertions, feature = "semantic-search"))]
    fn begin_tab_latency_trace(&mut self) {
        if !self.latency_log.enabled {
            return;
        }
        let target_scope = match self.overlay.scope {
            SearchScope::ThisBoard => SearchScope::Library,
            SearchScope::Library => SearchScope::ThisBoard,
        };
        self.latency_log.tab_started_at = Some(Instant::now());
        self.latency_log.tab_target_scope = Some(target_scope);
        self.latency_log.tab_ready_logged = false;
        self.latency_log.tab_painted_logged = false;
        eprintln!("[search-latency] tab_started target_scope={target_scope:?}");
    }

    #[cfg(all(debug_assertions, feature = "semantic-search"))]
    fn log_active_results_ready(&mut self, scope: SearchScope) {
        if !self.latency_log.enabled {
            return;
        }
        if scope == SearchScope::ThisBoard
            && !self.latency_log.command_ready_logged
            && let Some(started_at) = self.latency_log.command_started_at
        {
            let elapsed_ms = started_at.elapsed().as_secs_f64() * 1000.0;
            self.latency_log.command_ready_logged = true;
            eprintln!("[search-latency] normal_board_result_ready_ms={elapsed_ms:.2}");
        }
        if Some(scope) == self.latency_log.tab_target_scope
            && !self.latency_log.tab_ready_logged
            && let Some(started_at) = self.latency_log.tab_started_at
        {
            let elapsed_ms = started_at.elapsed().as_secs_f64() * 1000.0;
            self.latency_log.tab_ready_logged = true;
            eprintln!("[search-latency] normal_tab_result_ready_ms={elapsed_ms:.2}");
        }
    }

    #[cfg(all(debug_assertions, feature = "semantic-search"))]
    fn log_visible_results_painted(&mut self) {
        if !self.latency_log.enabled || self.overlay.results.is_empty() {
            return;
        }
        if self.overlay.scope == SearchScope::ThisBoard
            && !self.latency_log.command_painted_logged
            && let Some(started_at) = self.latency_log.command_started_at
        {
            let elapsed_ms = started_at.elapsed().as_secs_f64() * 1000.0;
            self.latency_log.command_painted_logged = true;
            eprintln!("[search-latency] normal_board_results_painted_ms={elapsed_ms:.2}");
        }
        if Some(self.overlay.scope) == self.latency_log.tab_target_scope
            && !self.latency_log.tab_painted_logged
            && let Some(started_at) = self.latency_log.tab_started_at
        {
            let elapsed_ms = started_at.elapsed().as_secs_f64() * 1000.0;
            self.latency_log.tab_painted_logged = true;
            eprintln!("[search-latency] normal_tab_results_painted_ms={elapsed_ms:.2}");
        }
    }

    pub(crate) fn sync_service(
        &mut self,
        enabled: bool,
        library: Option<&Library>,
        status: &mut String,
        canvas: Option<&mut CanvasState>,
        canvas_image_max_side: u32,
    ) {
        if !enabled {
            self.service = None;
            self.close_overlay(canvas);
            return;
        }

        if self.service.is_some() {
            return;
        }

        let Some(library) = library else {
            self.overlay.status = SearchOverlayStatus::Error("Storage unavailable".to_owned());
            return;
        };

        match SemanticSearchService::new(
            library.clone(),
            SemanticSearchConfig {
                enabled: false,
                model: DEFAULT_SEMANTIC_SEARCH_MODEL,
                canvas_image_max_side,
            },
        ) {
            Ok(service) => {
                self.overlay.status =
                    SearchOverlayStatus::Loading("Preparing semantic search…".to_owned());
                self.service = Some(service);
            }
            Err(error) => {
                self.overlay.status = SearchOverlayStatus::Error(error.clone());
                *status = format!("Semantic search unavailable: {error}");
            }
        }
    }

    pub(crate) fn handle_results(&mut self, canvas: &mut CanvasState) {
        let Some(service) = &self.service else {
            return;
        };
        let index_results = service.try_recv_all();
        let search_results = service.try_recv_search_all();

        let mut refresh_active_query = false;
        for result in index_results {
            if matches!(
                result,
                SearchIndexResult::BackfillIndexed
                    | SearchIndexResult::BackfillIdle
                    | SearchIndexResult::BackfillFailed { .. }
            ) {
                self.embedding_backfill.in_flight = false;
            }
            if matches!(
                result,
                SearchIndexResult::BackfillIdle | SearchIndexResult::BackfillFailed { .. }
            ) {
                self.embedding_backfill.exhausted = true;
            }
            refresh_active_query |= self.process_index_result(result);
        }

        for result in search_results {
            self.process_search_result(result, canvas);
        }

        if refresh_active_query {
            self.submit_query(canvas);
        }
    }

    pub(crate) fn can_start_embedding_backfill(
        &mut self,
        now: Instant,
        idle: EmbeddingBackfillIdleState,
    ) -> bool {
        if self.embedding_backfill.exhausted
            || self.embedding_backfill.in_flight
            || !idle.app_idle
            || self.search_work_in_flight(now)
        {
            self.embedding_backfill.quiet_since = None;
            return false;
        }

        let quiet_since = *self.embedding_backfill.quiet_since.get_or_insert(now);
        now.duration_since(quiet_since) >= Duration::from_millis(EMBEDDING_BACKFILL_IDLE_QUIET_MS)
    }

    pub(crate) fn drive_embedding_backfill(
        &mut self,
        now: Instant,
        idle: EmbeddingBackfillIdleState,
    ) -> Result<bool, String> {
        if !self.can_start_embedding_backfill(now, idle) {
            return Ok(false);
        }
        let Some(service) = &self.service else {
            return Ok(false);
        };
        service.enqueue_idle_backfill_step()?;
        self.embedding_backfill.in_flight = true;
        self.embedding_backfill.quiet_since = None;
        Ok(true)
    }

    pub(crate) fn embedding_backfill_may_need_repaint(&self) -> bool {
        self.service.is_some()
            && !self.embedding_backfill.exhausted
            && !self.embedding_backfill.in_flight
    }

    pub(crate) fn reset_embedding_backfill(&mut self) {
        self.embedding_backfill = EmbeddingBackfillScheduler::default();
    }

    fn search_work_in_flight(&self, now: Instant) -> bool {
        self.active_search.is_some()
            || !self.prefetch_searches.is_empty()
            || self.search_debounce_pending(now)
    }

    fn search_debounce_pending(&self, now: Instant) -> bool {
        self.overlay.last_query_edit_at.is_some_and(|changed_at| {
            self.overlay.query.trim() != self.overlay.last_submitted_query
                && now.duration_since(changed_at) < Duration::from_millis(SEARCH_DEBOUNCE_MS as u64)
        })
    }

    fn process_index_result(&mut self, result: SearchIndexResult) -> bool {
        match result {
            SearchIndexResult::ModelLoadStarted => {
                self.perf_events.push(SemanticPerfEvent::ModelLoadStarted);
                false
            }
            SearchIndexResult::ModelLoadTiming { timing } => {
                self.perf_events.push(SemanticPerfEvent::ModelLoadTiming {
                    assets_resolved_ms: timing.assets_resolved.as_secs_f64() * 1000.0,
                    tokenizer_loaded_ms: timing.tokenizer_loaded.as_secs_f64() * 1000.0,
                    text_session_loaded_ms: timing.text_session_loaded.as_secs_f64() * 1000.0,
                    vision_session_loaded_ms: timing.vision_session_loaded.as_secs_f64() * 1000.0,
                    total_ms: timing.total.as_secs_f64() * 1000.0,
                });
                false
            }
            SearchIndexResult::Progress { message } => {
                if !matches!(self.overlay.status, SearchOverlayStatus::Error(_)) {
                    self.overlay.status = SearchOverlayStatus::Loading(message);
                }
                false
            }
            SearchIndexResult::ModelReady => {
                self.perf_events.push(SemanticPerfEvent::ModelReady);
                let should_refresh = !self.overlay.query.trim().is_empty()
                    || (self.overlay.image_pivot.is_some() && self.overlay.results.is_empty());
                if should_refresh {
                    self.overlay.last_submitted_query.clear();
                }
                if self.active_search.is_none()
                    && !matches!(self.overlay.status, SearchOverlayStatus::Error(_))
                {
                    self.overlay.status = SearchOverlayStatus::Ready;
                }
                should_refresh
            }
            SearchIndexResult::Indexed { asset_id } => {
                if self.pending_demand_embeddings.remove(&asset_id) {
                    self.completed_demand_embeddings += 1;
                }
                let should_refresh = self.overlay.open
                    && (!self.overlay.query.trim().is_empty()
                        || self.overlay.image_pivot.is_some())
                    && self.overlay.results.is_empty();
                if should_refresh {
                    self.overlay.last_submitted_query.clear();
                }
                should_refresh
            }
            SearchIndexResult::BackfillIndexed => {
                let should_refresh = self.overlay.open
                    && (!self.overlay.query.trim().is_empty()
                        || self.overlay.image_pivot.is_some())
                    && self.overlay.results.is_empty();
                if should_refresh {
                    self.overlay.last_submitted_query.clear();
                }
                should_refresh
            }
            SearchIndexResult::BackfillIdle => false,
            SearchIndexResult::BackfillFailed { error } => {
                let _ = error;
                false
            }
            SearchIndexResult::Failed { asset_id, error } => {
                if let Some(asset_id) = asset_id
                    && self.pending_demand_embeddings.remove(&asset_id)
                {
                    self.completed_demand_embeddings += 1;
                }
                self.overlay.status = SearchOverlayStatus::Error(error);
                false
            }
        }
    }

    fn process_search_result(&mut self, result: SearchQueryResult, canvas: &mut CanvasState) {
        let active_request_id = self
            .active_search
            .as_ref()
            .map(|request| request.request_id);
        match result {
            SearchQueryResult::Completed {
                request_id,
                mut results,
            } if Some(request_id) == active_request_id => {
                let Some(active_search) = self.active_search.take() else {
                    return;
                };
                self.overlay.last_submitted_query = active_search.query;
                self.overlay.has_more_results = results.len() > self.overlay.result_limit;
                results.truncate(self.overlay.result_limit);
                self.overlay.results = results;
                let completed_scope = active_search.cache_key.scope;
                self.result_cache.insert(
                    active_search.cache_key,
                    SearchCacheEntry {
                        results: self.overlay.results.clone(),
                        has_more_results: self.overlay.has_more_results,
                    },
                );
                self.select_after_results_refresh();
                self.overlay.status = SearchOverlayStatus::Ready;
                self.apply_search_matches(canvas);
                #[cfg(all(debug_assertions, feature = "semantic-search"))]
                self.log_active_results_ready(completed_scope);
            }
            SearchQueryResult::Failed { request_id, error }
                if Some(request_id) == active_request_id =>
            {
                self.active_search = None;
                if error == "Loading model…" {
                    if !matches!(self.overlay.status, SearchOverlayStatus::Loading(_)) {
                        self.overlay.status =
                            SearchOverlayStatus::Loading("Preparing semantic search…".to_owned());
                    }
                    return;
                }
                self.overlay.status = SearchOverlayStatus::Error(error);
                self.overlay.results.clear();
                self.overlay.result_selection.clear();
                self.pending_result_selection = None;
                self.overlay.has_more_results = false;
                canvas.clear_search_matches();
            }
            SearchQueryResult::Completed {
                request_id,
                mut results,
            } => {
                let Some(cache_key) = self.prefetch_searches.remove(&request_id) else {
                    return;
                };
                #[cfg(all(debug_assertions, feature = "semantic-search"))]
                if self.latency_log.enabled {
                    eprintln!(
                        "[search-latency] prefetch_completed scope={:?} pivot_asset_id={:?} request_id={request_id} results={}",
                        cache_key.scope,
                        cache_key.pivot_asset_id,
                        results.len()
                    );
                }
                let has_more_results = results.len() > cache_key.result_limit;
                results.truncate(cache_key.result_limit);
                self.result_cache.insert(
                    cache_key,
                    SearchCacheEntry {
                        results,
                        has_more_results,
                    },
                );
            }
            SearchQueryResult::Failed { request_id, .. } => {
                self.prefetch_searches.remove(&request_id);
            }
        }
    }

    pub(crate) fn blocks_canvas_input(&self, ctx: &egui::Context) -> bool {
        if !self.overlay.open {
            return false;
        }

        if self.overlay.drag_state.is_some() {
            return true;
        }

        let Some(pointer_pos) = ctx.input(|input| input.pointer.latest_pos()) else {
            return false;
        };

        self.overlay
            .last_expanded_overlay_rect
            .is_some_and(|rect| rect.contains(pointer_pos))
    }

    pub(crate) fn queue_asset_index(
        &mut self,
        asset_id: i64,
        original_path: PathBuf,
    ) -> Result<(), String> {
        let Some(service) = &self.service else {
            return Err(
                "Semantic search service unavailable; cropped image was not queued for indexing"
                    .to_owned(),
            );
        };
        service.enqueue_asset_index(asset_id, original_path)?;
        self.pending_demand_embeddings.insert(asset_id);
        self.embedding_backfill.exhausted = false;
        self.overlay.status = SearchOverlayStatus::Indexing;
        Ok(())
    }

    pub(crate) fn demand_embedding_pending_count(&self) -> usize {
        self.pending_demand_embeddings.len()
    }

    pub(crate) fn completed_demand_embedding_count(&self) -> usize {
        self.completed_demand_embeddings
    }

    pub(crate) fn embedding_backfill_is_idle(&self) -> bool {
        !self.embedding_backfill.in_flight
            && self.embedding_backfill.exhausted
            && !self
                .service
                .as_ref()
                .is_some_and(|service| service.search_embedding_cache_warming())
    }

    pub(crate) fn drive_verification_search(
        &mut self,
        query: &str,
        scope: SearchScope,
        enabled: &mut bool,
        library: Option<&Library>,
        canvas: &mut CanvasState,
        status: &mut String,
        canvas_image_max_side: u32,
    ) {
        if !*enabled {
            *enabled = true;
            self.sync_service(
                *enabled,
                library,
                status,
                Some(canvas),
                canvas_image_max_side,
            );
        }

        self.overlay.open = true;
        self.overlay.focus_requested = true;
        self.overlay.suppress_next_outside_click = true;
        self.overlay.scope = scope;
        if self.overlay.query.trim() != query.trim() {
            self.overlay.query = query.to_owned();
            self.overlay.last_query_edit_at = None;
            self.overlay.last_submitted_query.clear();
            self.overlay.results.clear();
            self.overlay.result_selection.clear();
            self.pending_result_selection = None;
            self.overlay.active_recent_search_index = 0;
            self.overlay.result_limit = SEARCH_RESULT_PAGE_SIZE;
            self.overlay.has_more_results = false;
            canvas.clear_search_matches();
        }

        self.submit_query(canvas);
    }

    pub(crate) fn verification_search_settled(&self, query: &str) -> bool {
        self.overlay.open
            && self.overlay.query.trim() == query.trim()
            && self.overlay.last_submitted_query == query.trim()
            && matches!(
                self.overlay.status,
                SearchOverlayStatus::Ready | SearchOverlayStatus::Error(_)
            )
    }

    #[cfg(debug_assertions)]
    pub(crate) fn verification_result_count(&self) -> usize {
        self.overlay.results.len()
    }

    #[cfg(debug_assertions)]
    pub(crate) fn verification_scope(&self) -> SearchScope {
        self.overlay.scope
    }

    #[cfg(debug_assertions)]
    pub(crate) fn verification_toggle_scope(&mut self, canvas: &mut CanvasState) {
        self.toggle_scope();
        self.submit_query(canvas);
    }

    pub(crate) fn search_using_selected_image(
        &mut self,
        enabled: bool,
        library: Option<&Library>,
        canvas: &mut CanvasState,
        status: &mut String,
        canvas_image_max_side: u32,
    ) -> bool {
        let Some(pivot) = selected_image_pivot(canvas) else {
            return false;
        };

        self.sync_service(
            enabled,
            library,
            status,
            Some(canvas),
            canvas_image_max_side,
        );
        self.open_image_pivot_search(pivot, "selected_image_search", canvas);
        self.overlay.open = true;
        self.overlay.focus_requested = true;
        self.overlay.suppress_next_outside_click = true;
        true
    }

    pub(crate) fn handle_shortcut(
        &mut self,
        ctx: &egui::Context,
        enabled: bool,
        library: Option<&Library>,
        canvas: &mut CanvasState,
        status: &mut String,
        canvas_image_max_side: u32,
    ) {
        if !enabled {
            return;
        }

        let triggered =
            ctx.input(|input| input.modifiers.command && input.key_pressed(egui::Key::F));
        if !triggered {
            return;
        }

        self.sync_service(
            enabled,
            library,
            status,
            Some(canvas),
            canvas_image_max_side,
        );

        let pivot = selected_image_pivot(canvas);

        if pivot.is_some() && self.overlay.image_pivot != pivot {
            if let Some(pivot) = pivot {
                self.open_image_pivot_search(pivot, "cmd_f_image_search", canvas);
            }
        }

        self.open_overlay_from_command();
    }

    pub(crate) fn show_overlay(
        &mut self,
        ctx: &egui::Context,
        screen_rect: egui::Rect,
        canvas: &mut CanvasState,
        placement_preview_config: SearchPlacementPreviewConfig,
    ) -> Option<SearchDropRequest> {
        if !self.overlay.open {
            return None;
        }

        if ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
            if self
                .overlay
                .drag_state
                .as_ref()
                .is_some_and(|drag| drag.placement_pending)
            {
                self.cancel_pending_placement(canvas);
            } else {
                self.close_overlay(Some(canvas));
            }
            return None;
        }
        let tab_toggled_scope =
            ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Tab));
        if tab_toggled_scope {
            #[cfg(all(debug_assertions, feature = "semantic-search"))]
            self.begin_tab_latency_trace();
            self.toggle_scope();
            self.submit_query(canvas);
            ctx.request_repaint();
        }
        if self.active_search.is_some() {
            ctx.request_repaint_after(Duration::from_millis(16));
        }
        if matches!(self.overlay.status, SearchOverlayStatus::Loading(_)) {
            ctx.request_repaint_after(Duration::from_millis(16));
        }

        let input_id = egui::Id::new("semantic_search_input");
        let width = PALETTE_WIDTH;
        let result_count = self.overlay.results.len();
        let body_h_pad = 12.0_f32;
        let grid_inner_width = width - body_h_pad * 2.0;
        let grid_layout = search_result_grid_layout(grid_inner_width, result_count);
        let grid_content_height =
            search_result_grid_content_height(result_count, grid_layout.columns);
        let grid_viewport_height = grid_content_height.min(PALETTE_GRID_MAX_HEIGHT);
        let pointer_pos = ctx.input(|input| input.pointer.latest_pos());
        let drag_outside = self
            .overlay
            .drag_state
            .as_ref()
            .is_some_and(|drag| drag.outside_overlay);
        let scrim_visible = !drag_outside;
        let scrim_t = ctx.animate_bool(
            egui::Id::new("semantic_search_scrim_visible"),
            scrim_visible,
        );
        let scrim_alpha = (170.0 * scrim_t).round() as u8;
        if (scrim_visible && scrim_t < 1.0) || (!scrim_visible && scrim_t > 0.0) {
            ctx.request_repaint_after(Duration::from_millis(16));
        }
        egui::Area::new(egui::Id::new("semantic_search_scrim"))
            .order(egui::Order::Middle)
            .fixed_pos(screen_rect.min)
            .show(ctx, |ui| {
                let painter = ui.painter();
                painter.rect_filled(screen_rect, 0, egui::Color32::from_black_alpha(scrim_alpha));
            });
        let area_response = egui::Area::new(egui::Id::new("semantic_search_overlay"))
            .order(egui::Order::Foreground)
            .pivot(egui::Align2::CENTER_TOP)
            .fixed_pos(egui::pos2(
                screen_rect.center().x,
                screen_rect.top() + SEARCH_OVERLAY_TOP_MARGIN,
            ))
            .show(ctx, |ui| {
                ui.set_width(width);
                ui.set_min_width(width);
                ui.set_max_width(width);
                egui::Frame::new()
                    .fill(crate::theme::PALETTE_SURFACE)
                    .stroke(egui::Stroke::new(1.0, egui::Color32::from_white_alpha(33)))
                    .corner_radius(egui::CornerRadius::same(crate::theme::RADIUS_PANEL))
                    .shadow(egui::epaint::Shadow {
                        offset: [0, 12],
                        blur: 40,
                        spread: 0,
                        color: egui::Color32::from_black_alpha(180),
                    })
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);

                        let input_result = paint_palette_input_row(
                            ui,
                            &mut self.overlay.query,
                            input_id,
                            self.overlay.image_pivot.as_ref(),
                            canvas,
                        );
                        let response = input_result.response;
                        let pivot_backspace_dismissed = self.overlay.image_pivot.is_some()
                            && self.overlay.query.is_empty()
                            && response.has_focus()
                            && ctx.input(|input| input.key_pressed(egui::Key::Backspace));
                        if input_result.pivot_dismissed || pivot_backspace_dismissed {
                            self.dismiss_image_pivot(canvas);
                        }
                        if self.overlay.focus_requested {
                            response.request_focus();
                            self.overlay.focus_requested =
                                !ctx.memory(|memory| memory.has_focus(input_id));
                            if self.overlay.focus_requested {
                                ctx.request_repaint_after(Duration::from_millis(16));
                            }
                        }
                        if response.changed() {
                            self.overlay.last_query_edit_at = Some(Instant::now());
                            self.overlay
                                .result_selection
                                .select_initial(self.overlay.results.len());
                            self.overlay.active_recent_search_index = 0;
                            self.overlay.result_limit = SEARCH_RESULT_PAGE_SIZE;
                            self.overlay.has_more_results = false;
                            self.pending_result_selection = None;
                            if self.overlay.query.trim().is_empty()
                                && self.overlay.image_pivot.is_none()
                            {
                                self.overlay.results.clear();
                                self.overlay.result_selection.clear();
                                canvas.clear_search_matches();
                            }
                        }

                        if !drag_outside {
                            paint_palette_divider(ui);

                            if result_count == 0 {
                                if self.overlay.query.trim().is_empty()
                                    && self.overlay.image_pivot.is_none()
                                {
                                    if let Some(recent_search) = paint_palette_empty_state(
                                        ui,
                                        &self.recent_searches,
                                        self.overlay
                                            .active_recent_search_index
                                            .min(self.recent_searches.len().saturating_sub(1)),
                                    ) {
                                        self.submit_recent_search(&recent_search, canvas);
                                    }
                                } else {
                                    paint_palette_no_results_state(ui, &self.overlay.status);
                                }
                            } else {
                                let mut clicked_index = None;
                                let mut drag_start_index = None;
                                let scroll_lead_index = if self.overlay.scroll_lead_result_into_view
                                {
                                    self.overlay.result_selection.lead()
                                } else {
                                    None
                                };
                                let mut scrolled_lead_result = false;
                                let selected_indices =
                                    self.overlay.result_selection.selected().to_vec();
                                let lead_index = self.overlay.result_selection.lead();
                                egui::Frame::new()
                                    .inner_margin(egui::Margin {
                                        left: body_h_pad as i8,
                                        right: body_h_pad as i8,
                                        top: body_h_pad as i8,
                                        bottom: body_h_pad as i8,
                                    })
                                    .show(ui, |ui| {
                                        ui.set_min_width(grid_inner_width);
                                        ui.set_max_width(grid_inner_width);
                                        let mut render_grid = |ui: &mut egui::Ui| {
                                            ui.spacing_mut().item_spacing = egui::vec2(
                                                crate::theme::SPACE_2,
                                                crate::theme::SPACE_2,
                                            );
                                            for (row_index, row_groups) in self
                                                .overlay
                                                .results
                                                .chunks(grid_layout.columns.max(1))
                                                .enumerate()
                                            {
                                                ui.horizontal(|ui| {
                                                    ui.spacing_mut().item_spacing.x =
                                                        crate::theme::SPACE_2;
                                                    for (column_index, group) in
                                                        row_groups.iter().enumerate()
                                                    {
                                                        let index = row_index * grid_layout.columns
                                                            + column_index;
                                                        let lead = Some(index) == lead_index;
                                                        let selected =
                                                            selected_indices.contains(&index);
                                                        let response = search_result_tile(
                                                            ui,
                                                            canvas,
                                                            group,
                                                            &mut self.overlay.asset_preview_cache,
                                                            lead,
                                                            selected,
                                                            grid_layout.tile_width,
                                                        );
                                                        if scroll_lead_index == Some(index) {
                                                            ui.scroll_to_rect_animation(
                                                                response.rect.expand2(egui::vec2(
                                                                    0.0,
                                                                    SEARCH_RESULT_SCROLL_MARGIN,
                                                                )),
                                                                None,
                                                                SEARCH_RESULT_SCROLL_ANIMATION,
                                                            );
                                                            scrolled_lead_result = true;
                                                        }
                                                        if response.clicked() {
                                                            clicked_index = Some((
                                                                index,
                                                                ctx.input(|input| {
                                                                    if input.modifiers.shift {
                                                                        SearchSelectionMode::Range
                                                                    } else if input
                                                                        .modifiers
                                                                        .command
                                                                        || input.modifiers.ctrl
                                                                    {
                                                                        SearchSelectionMode::Toggle
                                                                    } else {
                                                                        SearchSelectionMode::Replace
                                                                    }
                                                                }),
                                                            ));
                                                        }
                                                        if response.drag_started() {
                                                            drag_start_index = Some(index);
                                                        }
                                                    }
                                                });
                                            }
                                        };

                                        if grid_content_height <= PALETTE_GRID_MAX_HEIGHT {
                                            render_grid(ui);
                                        } else {
                                            egui::ScrollArea::vertical()
                                                .max_height(grid_viewport_height)
                                                .auto_shrink([false, false])
                                                .show(ui, |ui| render_grid(ui));
                                        }
                                    });
                                if scrolled_lead_result {
                                    self.overlay.scroll_lead_result_into_view = false;
                                }
                                if let Some((index, mode)) = clicked_index {
                                    self.overlay.scroll_lead_result_into_view = true;
                                    self.update_result_selection(index, mode);
                                    self.apply_search_matches(canvas);
                                }
                                if let Some(index) = drag_start_index {
                                    self.start_result_drag(index, placement_preview_config);
                                }
                            }

                            paint_palette_divider(ui);
                            if paint_palette_footer(
                                ui,
                                result_count,
                                self.overlay.has_more_results,
                                &self.overlay.status,
                                self.overlay.scope,
                            ) {
                                #[cfg(all(debug_assertions, feature = "semantic-search"))]
                                self.begin_tab_latency_trace();
                                self.toggle_scope();
                                self.submit_query(canvas);
                                ctx.request_repaint();
                            }
                        }
                    });
            });

        #[cfg(all(debug_assertions, feature = "semantic-search"))]
        self.log_visible_results_painted();

        let overlay_rect = area_response.response.rect;
        if !drag_outside {
            self.overlay.last_expanded_overlay_rect = Some(overlay_rect);
        }
        if let Some(drag) = &mut self.overlay.drag_state {
            let expanded_rect = self
                .overlay
                .last_expanded_overlay_rect
                .unwrap_or(overlay_rect);
            if !drag.outside_overlay && !drag.placement_pending {
                drag.outside_overlay =
                    pointer_pos.is_some_and(|pos| !expanded_rect.expand(2.0).contains(pos));
            }
            if drag.outside_overlay {
                if let Some(pointer_pos) = pointer_pos {
                    let pointer_world = canvas.screen_to_world(pointer_pos, screen_rect);
                    update_search_placement_preview(
                        canvas,
                        drag,
                        pointer_world,
                        screen_rect,
                        placement_preview_config.import_gap,
                    );
                }
                ctx.request_repaint_after(Duration::from_millis(16));
            }
        }
        let mut drop_request = None;
        let mut cancelled_pending_placement = false;
        let cancel_pending_placement = ctx.input(|input| input.pointer.secondary_clicked());
        if cancel_pending_placement
            && self
                .overlay
                .drag_state
                .as_ref()
                .is_some_and(|drag| drag.placement_pending)
        {
            self.cancel_pending_placement(canvas);
            cancelled_pending_placement = true;
            ctx.request_repaint_after(Duration::from_millis(16));
        } else if ctx.input(|input| input.pointer.primary_clicked())
            && self
                .overlay
                .drag_state
                .as_ref()
                .is_some_and(|drag| drag.placement_pending)
            && let Some(drag) = self.overlay.drag_state.take()
            && let Some(screen_pos) = pointer_pos
        {
            drop_request = Some(SearchDropRequest {
                item_ids: drag.item_ids,
                asset_ids: drag.asset_ids,
                screen_pos,
            });
            ctx.request_repaint_after(Duration::from_millis(16));
        } else if ctx.input(|input| input.pointer.any_released())
            && let Some(drag) = self.overlay.drag_state.take()
        {
            if drag.outside_overlay
                && let Some(screen_pos) = pointer_pos
            {
                drop_request = Some(SearchDropRequest {
                    item_ids: drag.item_ids,
                    asset_ids: drag.asset_ids,
                    screen_pos,
                });
            } else {
                canvas.clear_search_drag_preview();
            }
            ctx.request_repaint_after(Duration::from_millis(16));
        }
        let clicked_outside = ctx.input(|input| {
            input.pointer.any_click()
                && input
                    .pointer
                    .interact_pos()
                    .is_some_and(|pointer_pos| !overlay_rect.contains(pointer_pos))
        });
        let suppress_outside_click = std::mem::take(&mut self.overlay.suppress_next_outside_click);
        if clicked_outside
            && !suppress_outside_click
            && self.overlay.drag_state.is_none()
            && drop_request.is_none()
            && !cancelled_pending_placement
        {
            self.close_overlay(Some(canvas));
            return drop_request;
        }

        if let Some(changed_at) = self.overlay.last_query_edit_at
            && changed_at.elapsed().as_secs_f32() * 1000.0 >= SEARCH_DEBOUNCE_MS
            && self.overlay.query.trim() != self.overlay.last_submitted_query
        {
            self.submit_query(canvas);
        } else if self.overlay.last_query_edit_at.is_some() {
            ctx.request_repaint_after(Duration::from_millis(16));
        }

        let placement_pending = self
            .overlay
            .drag_state
            .as_ref()
            .is_some_and(|drag| drag.placement_pending);
        if placement_pending {
            return drop_request;
        }

        if !self.overlay.results.is_empty() {
            let last_index = self.overlay.results.len() - 1;
            let lead_index = self.overlay.result_selection.lead().unwrap_or(0);
            let lead_is_on_bottom_row =
                result_is_on_bottom_row(lead_index, last_index, grid_layout.columns);
            let arrow_selection_mode = if ctx.input(|input| input.modifiers.shift) {
                SearchSelectionMode::Range
            } else {
                SearchSelectionMode::Replace
            };
            if ctx.input(|input| input.key_pressed(egui::Key::ArrowRight)) {
                let index = (lead_index + 1).min(last_index);
                self.update_result_selection(index, arrow_selection_mode);
                self.overlay.scroll_lead_result_into_view = true;
                self.apply_search_matches(canvas);
            } else if ctx.input(|input| input.key_pressed(egui::Key::ArrowLeft)) {
                let index = lead_index.saturating_sub(1);
                self.update_result_selection(index, arrow_selection_mode);
                self.overlay.scroll_lead_result_into_view = true;
                self.apply_search_matches(canvas);
            } else if ctx.input(|input| input.key_pressed(egui::Key::ArrowDown)) {
                if lead_is_on_bottom_row && self.overlay.has_more_results {
                    let next_row_index = lead_index + grid_layout.columns;
                    self.pending_result_selection = Some(PendingResultSelection {
                        index: next_row_index,
                        mode: arrow_selection_mode,
                    });
                    self.overlay.result_limit += SEARCH_RESULT_PAGE_SIZE;
                    self.submit_query(canvas);
                    if self.pending_result_selection.is_none() {
                        self.overlay.scroll_lead_result_into_view = true;
                    }
                } else {
                    let index = (lead_index + grid_layout.columns).min(last_index);
                    self.update_result_selection(index, arrow_selection_mode);
                    self.overlay.scroll_lead_result_into_view = true;
                    self.apply_search_matches(canvas);
                }
            } else if ctx.input(|input| input.key_pressed(egui::Key::ArrowUp)) {
                let index = lead_index.saturating_sub(grid_layout.columns);
                self.update_result_selection(index, arrow_selection_mode);
                self.overlay.scroll_lead_result_into_view = true;
                self.apply_search_matches(canvas);
            }

            if ctx.input(|input| input.key_pressed(egui::Key::Enter)) {
                if ctx.input(|input| input.modifiers.command) {
                    self.start_keyboard_placement_for_all_results(
                        canvas,
                        pointer_pos,
                        screen_rect,
                        placement_preview_config,
                    );
                } else {
                    self.start_keyboard_placement_for_selected_results(
                        canvas,
                        pointer_pos,
                        screen_rect,
                        placement_preview_config,
                    );
                }
            }
        } else if !self.recent_searches.is_empty() {
            let last_recent_index = self.recent_searches.len() - 1;
            if ctx.input(|input| input.key_pressed(egui::Key::ArrowDown)) {
                self.overlay.active_recent_search_index =
                    (self.overlay.active_recent_search_index + 1).min(last_recent_index);
            } else if ctx.input(|input| input.key_pressed(egui::Key::ArrowUp)) {
                self.overlay.active_recent_search_index =
                    self.overlay.active_recent_search_index.saturating_sub(1);
            } else if ctx.input(|input| input.key_pressed(egui::Key::Enter)) {
                let recent_search = self.recent_searches[self
                    .overlay
                    .active_recent_search_index
                    .min(last_recent_index)]
                .clone();
                self.submit_recent_search(&recent_search, canvas);
            }
        }
        drop_request
    }

    pub(crate) fn close_overlay(&mut self, canvas: Option<&mut CanvasState>) {
        let scope = self.overlay.scope;
        self.overlay = SearchOverlayState::default();
        self.overlay.scope = scope;
        self.result_cache.clear();
        self.active_search = None;
        self.prefetch_searches.clear();
        self.pending_result_selection = None;
        if let Some(canvas) = canvas {
            canvas.clear_search_matches();
            canvas.clear_search_drag_preview();
            canvas.clear_move_preview_placeholders();
        }
    }

    fn update_result_selection(&mut self, index: usize, mode: SearchSelectionMode) {
        match mode {
            SearchSelectionMode::Replace => {
                self.overlay.result_selection.replace(index);
            }
            SearchSelectionMode::Toggle => {
                self.overlay.result_selection.toggle(index);
            }
            SearchSelectionMode::Range => {
                self.overlay.result_selection.range_to(index);
            }
        }
    }

    fn select_initial_result(&mut self) {
        self.overlay
            .result_selection
            .select_initial(self.overlay.results.len());
    }

    fn toggle_scope(&mut self) {
        self.overlay.scope = match self.overlay.scope {
            SearchScope::ThisBoard => SearchScope::Library,
            SearchScope::Library => SearchScope::ThisBoard,
        };
        self.overlay.last_submitted_query.clear();
        self.active_search = None;
        self.overlay.result_limit = SEARCH_RESULT_PAGE_SIZE;
        if self.restore_cached_results() {
            return;
        }
        self.overlay.results.clear();
        self.overlay.result_selection.clear();
        self.pending_result_selection = None;
        self.overlay.has_more_results = false;
    }

    fn start_result_drag(
        &mut self,
        index: usize,
        placement_preview_config: SearchPlacementPreviewConfig,
    ) {
        if !self.overlay.result_selection.contains(index) {
            self.update_result_selection(index, SearchSelectionMode::Replace);
        }

        let item_ids = self.selected_result_item_ids();
        let asset_ids = self.selected_unplaced_result_asset_ids();
        if item_ids.is_empty() && asset_ids.is_empty() {
            return;
        }

        self.overlay.drag_state = Some(crate::palette::SearchResultDragState {
            item_ids,
            asset_placeholder_sizes: self.asset_placeholder_sizes(
                &asset_ids,
                placement_preview_config.import_target_canvas_area,
            ),
            asset_ids,
            outside_overlay: false,
            placement_pending: false,
        });
    }

    fn start_keyboard_placement_for_selected_results(
        &mut self,
        canvas: &mut CanvasState,
        pointer_pos: Option<egui::Pos2>,
        screen_rect: egui::Rect,
        placement_preview_config: SearchPlacementPreviewConfig,
    ) {
        let item_ids = self.selected_result_item_ids();
        let asset_ids = self.selected_unplaced_result_asset_ids();
        self.start_keyboard_placement(
            item_ids,
            asset_ids,
            canvas,
            pointer_pos,
            screen_rect,
            placement_preview_config,
        );
    }

    fn start_keyboard_placement_for_all_results(
        &mut self,
        canvas: &mut CanvasState,
        pointer_pos: Option<egui::Pos2>,
        screen_rect: egui::Rect,
        placement_preview_config: SearchPlacementPreviewConfig,
    ) {
        let item_ids = self.all_result_item_ids();
        let asset_ids = self.all_unplaced_result_asset_ids();
        self.start_keyboard_placement(
            item_ids,
            asset_ids,
            canvas,
            pointer_pos,
            screen_rect,
            placement_preview_config,
        );
    }

    fn start_keyboard_placement(
        &mut self,
        item_ids: Vec<i64>,
        asset_ids: Vec<i64>,
        canvas: &mut CanvasState,
        pointer_pos: Option<egui::Pos2>,
        screen_rect: egui::Rect,
        placement_preview_config: SearchPlacementPreviewConfig,
    ) {
        if item_ids.is_empty() && asset_ids.is_empty() {
            return;
        }

        self.overlay.drag_state = Some(crate::palette::SearchResultDragState {
            item_ids: item_ids.clone(),
            asset_placeholder_sizes: self.asset_placeholder_sizes(
                &asset_ids,
                placement_preview_config.import_target_canvas_area,
            ),
            asset_ids,
            outside_overlay: true,
            placement_pending: true,
        });
        let pointer_world = pointer_pos
            .map(|pos| canvas.screen_to_world(pos, screen_rect))
            .unwrap_or_else(|| canvas.screen_to_world(screen_rect.center(), screen_rect));
        if let Some(drag) = self.overlay.drag_state.as_ref() {
            update_search_placement_preview(
                canvas,
                drag,
                pointer_world,
                screen_rect,
                placement_preview_config.import_gap,
            );
        }
    }

    fn cancel_pending_placement(&mut self, canvas: &mut CanvasState) {
        self.overlay.drag_state = None;
        canvas.clear_search_drag_preview();
        canvas.clear_move_preview_placeholders();
    }

    fn selected_result_item_ids(&self) -> Vec<i64> {
        let item_ids = self
            .overlay
            .result_selection
            .selected()
            .iter()
            .filter_map(|index| self.overlay.results.get(*index))
            .filter_map(|group| group.item_ids.first().copied())
            .collect::<Vec<_>>();

        if item_ids.is_empty() {
            self.overlay
                .results
                .get(self.overlay.result_selection.lead().unwrap_or(0))
                .and_then(|group| group.item_ids.first().copied())
                .into_iter()
                .collect()
        } else {
            item_ids
        }
    }

    fn all_result_item_ids(&self) -> Vec<i64> {
        self.overlay
            .results
            .iter()
            .filter_map(|group| group.item_ids.first().copied())
            .collect()
    }

    fn selected_unplaced_result_asset_ids(&self) -> Vec<i64> {
        let asset_ids = self
            .overlay
            .result_selection
            .selected()
            .iter()
            .filter_map(|index| self.overlay.results.get(*index))
            .filter(|group| group.item_ids.is_empty())
            .map(|group| group.asset_id)
            .collect::<Vec<_>>();

        if asset_ids.is_empty() {
            self.overlay
                .results
                .get(self.overlay.result_selection.lead().unwrap_or(0))
                .filter(|group| group.item_ids.is_empty())
                .map(|group| group.asset_id)
                .into_iter()
                .collect()
        } else {
            asset_ids
        }
    }

    fn asset_placeholder_sizes(
        &self,
        asset_ids: &[i64],
        target_canvas_area: f32,
    ) -> Vec<egui::Vec2> {
        asset_ids
            .iter()
            .filter_map(|asset_id| {
                self.overlay
                    .results
                    .iter()
                    .find(|group| group.asset_id == *asset_id)
            })
            .map(|group| asset_placeholder_size(group, target_canvas_area))
            .collect()
    }

    fn all_unplaced_result_asset_ids(&self) -> Vec<i64> {
        self.overlay
            .results
            .iter()
            .filter(|group| group.item_ids.is_empty())
            .map(|group| group.asset_id)
            .collect()
    }

    fn dismiss_image_pivot(&mut self, canvas: &mut CanvasState) {
        self.overlay.image_pivot = None;
        self.overlay.last_submitted_query.clear();
        self.active_search = None;
        self.overlay.results.clear();
        self.overlay.result_selection.clear();
        self.pending_result_selection = None;
        self.overlay.active_recent_search_index = 0;
        self.overlay.result_limit = SEARCH_RESULT_PAGE_SIZE;
        self.overlay.has_more_results = false;
        canvas.clear_search_matches();
        self.overlay.last_query_edit_at = Some(Instant::now());
    }

    fn submit_recent_search(&mut self, query: &str, canvas: &mut CanvasState) {
        self.overlay.query = query.to_owned();
        self.overlay.image_pivot = None;
        self.overlay.last_query_edit_at = None;
        self.overlay.last_submitted_query.clear();
        self.active_search = None;
        self.overlay.results.clear();
        self.overlay.result_selection.clear();
        self.pending_result_selection = None;
        self.overlay.result_limit = SEARCH_RESULT_PAGE_SIZE;
        self.overlay.has_more_results = false;
        self.submit_query(canvas);
    }

    fn record_recent_search(&mut self, query: &str) {
        let query = query.trim();
        if query.is_empty() {
            return;
        }

        let mut recent_searches = self
            .recent_searches
            .iter()
            .filter(|recent| recent.as_str() != query)
            .cloned()
            .collect::<Vec<_>>();
        recent_searches.insert(0, query.to_owned());
        recent_searches.truncate(MAX_RECENT_SEARCHES);
        if recent_searches != self.recent_searches {
            self.recent_searches = recent_searches;
            self.recent_searches_dirty = true;
            self.overlay.active_recent_search_index = 0;
        }
    }

    fn open_image_pivot_search(
        &mut self,
        pivot: ImageSearchPivot,
        latency_source: &'static str,
        canvas: &mut CanvasState,
    ) {
        #[cfg(all(debug_assertions, feature = "semantic-search"))]
        self.begin_image_search_latency_trace(latency_source, pivot.asset_id);
        self.overlay.query.clear();
        self.overlay.last_submitted_query.clear();
        self.overlay.results.clear();
        self.overlay.result_selection.clear();
        self.pending_result_selection = None;
        self.overlay.active_recent_search_index = 0;
        self.overlay.result_limit = SEARCH_RESULT_PAGE_SIZE;
        self.overlay.has_more_results = false;
        self.overlay.image_pivot = Some(pivot);
        self.overlay.last_query_edit_at = None;
        canvas.clear_search_matches();
        self.submit_query(canvas);
    }

    fn open_overlay_from_command(&mut self) {
        self.overlay.open = true;
        self.overlay.focus_requested = true;
        self.overlay.suppress_next_outside_click = true;
        if matches!(self.overlay.status, SearchOverlayStatus::Error(_)) {
            return;
        }

        self.overlay.status = self
            .service
            .as_ref()
            .map(|service| {
                if service.is_ready() {
                    SearchOverlayStatus::Ready
                } else {
                    SearchOverlayStatus::Loading("Preparing semantic search…".to_owned())
                }
            })
            .unwrap_or_else(|| {
                SearchOverlayStatus::Loading("Preparing semantic search…".to_owned())
            });
    }

    pub(crate) fn submit_query(&mut self, canvas: &mut CanvasState) {
        let query = self.overlay.query.trim().to_owned();
        let pivot = self.overlay.image_pivot;
        if query.is_empty() && pivot.is_none() {
            self.active_search = None;
            self.overlay.results.clear();
            self.overlay.result_selection.clear();
            self.pending_result_selection = None;
            self.overlay.last_submitted_query.clear();
            self.overlay.has_more_results = false;
            self.overlay.status = SearchOverlayStatus::Ready;
            canvas.clear_search_matches();
            return;
        }

        if pivot.is_none() {
            self.record_recent_search(&query);
        }

        let cache_key = self.current_cache_key(&query, pivot);
        if self
            .active_search
            .as_ref()
            .is_some_and(|active_search| active_search.cache_key == cache_key)
        {
            return;
        }
        if let Some(cached) = self.result_cache.get(&cache_key).cloned() {
            #[cfg(all(debug_assertions, feature = "semantic-search"))]
            if self.latency_log.enabled {
                eprintln!(
                    "[search-latency] submit_query_cache_hit scope={:?} pivot_asset_id={:?} results={}",
                    cache_key.scope,
                    cache_key.pivot_asset_id,
                    cached.results.len()
                );
            }
            self.active_search = None;
            self.overlay.last_submitted_query = query;
            self.overlay.results = cached.results;
            self.overlay.has_more_results = cached.has_more_results;
            self.select_after_results_refresh();
            self.overlay.status = SearchOverlayStatus::Ready;
            self.apply_search_matches(canvas);
            #[cfg(all(debug_assertions, feature = "semantic-search"))]
            self.log_active_results_ready(cache_key.scope);
            return;
        }
        if let Some(request_id) =
            self.prefetch_searches
                .iter()
                .find_map(|(request_id, prefetch_key)| {
                    (*prefetch_key == cache_key).then_some(*request_id)
                })
        {
            #[cfg(all(debug_assertions, feature = "semantic-search"))]
            if self.latency_log.enabled {
                eprintln!(
                    "[search-latency] submit_query_adopt_prefetch scope={:?} pivot_asset_id={:?} request_id={request_id}",
                    cache_key.scope, cache_key.pivot_asset_id
                );
            }
            self.prefetch_searches.remove(&request_id);
            self.active_search = Some(ActiveSearchRequest {
                request_id,
                cache_key,
                query,
            });
            self.overlay.status = SearchOverlayStatus::Loading("Searching…".to_owned());
            return;
        }

        let request_id = self.next_request_id();
        let enqueue_result = {
            let Some(service) = &self.service else {
                self.overlay.status =
                    SearchOverlayStatus::Error("Semantic search unavailable".to_owned());
                return;
            };
            match pivot {
                Some(pivot) => {
                    let text = if query.is_empty() {
                        None
                    } else {
                        Some(query.as_str())
                    };
                    service.enqueue_search_with_image_pivot_with_limit_in_scope(
                        request_id,
                        self.overlay.scope,
                        pivot.asset_id,
                        text,
                        self.overlay.result_limit + 1,
                    )
                }
                None => service.enqueue_search_with_limit_in_scope(
                    request_id,
                    self.overlay.scope,
                    &query,
                    self.overlay.result_limit + 1,
                ),
            }
        };

        match enqueue_result {
            Ok(()) => {
                #[cfg(all(debug_assertions, feature = "semantic-search"))]
                if self.latency_log.enabled {
                    eprintln!(
                        "[search-latency] submit_query_enqueued scope={:?} pivot_asset_id={:?} request_id={request_id}",
                        cache_key.scope, cache_key.pivot_asset_id
                    );
                }
                self.active_search = Some(ActiveSearchRequest {
                    request_id,
                    cache_key,
                    query: query.clone(),
                });
                self.overlay.status = SearchOverlayStatus::Loading("Searching…".to_owned());
                self.prefetch_opposite_scope(&query, pivot);
            }
            Err(error) => {
                self.active_search = None;
                self.overlay.status = SearchOverlayStatus::Error(error);
                self.overlay.results.clear();
                self.overlay.result_selection.clear();
                self.pending_result_selection = None;
                self.overlay.has_more_results = false;
                canvas.clear_search_matches();
            }
        }
    }

    fn next_request_id(&mut self) -> u64 {
        let request_id = self.next_search_request_id;
        self.next_search_request_id = self.next_search_request_id.wrapping_add(1).max(1);
        request_id
    }

    fn prefetch_opposite_scope(&mut self, query: &str, pivot: Option<ImageSearchPivot>) {
        let opposite_scope = match self.overlay.scope {
            SearchScope::ThisBoard => SearchScope::Library,
            SearchScope::Library => SearchScope::ThisBoard,
        };
        let cache_key = SearchCacheKey {
            scope: opposite_scope,
            query: query.to_owned(),
            pivot_asset_id: pivot.map(|pivot| pivot.asset_id),
            result_limit: self.overlay.result_limit,
        };
        if self.result_cache.contains_key(&cache_key)
            || self
                .prefetch_searches
                .values()
                .any(|prefetch_key| *prefetch_key == cache_key)
        {
            return;
        }

        let request_id = self.next_request_id();
        let Some(service) = &self.service else {
            return;
        };
        let enqueue_result = match pivot {
            Some(pivot) => {
                let text = if query.is_empty() { None } else { Some(query) };
                service.enqueue_search_with_image_pivot_with_limit_in_scope(
                    request_id,
                    opposite_scope,
                    pivot.asset_id,
                    text,
                    self.overlay.result_limit + 1,
                )
            }
            None => service.enqueue_search_with_limit_in_scope(
                request_id,
                opposite_scope,
                query,
                self.overlay.result_limit + 1,
            ),
        };
        if enqueue_result.is_ok() {
            #[cfg(all(debug_assertions, feature = "semantic-search"))]
            if self.latency_log.enabled {
                eprintln!(
                    "[search-latency] prefetch_enqueued scope={opposite_scope:?} pivot_asset_id={:?} request_id={request_id}",
                    cache_key.pivot_asset_id
                );
            }
            self.prefetch_searches.insert(request_id, cache_key);
        }
    }

    fn current_cache_key(&self, query: &str, pivot: Option<ImageSearchPivot>) -> SearchCacheKey {
        SearchCacheKey {
            scope: self.overlay.scope,
            query: query.to_owned(),
            pivot_asset_id: pivot.map(|pivot| pivot.asset_id),
            result_limit: self.overlay.result_limit,
        }
    }

    fn restore_cached_results(&mut self) -> bool {
        let query = self.overlay.query.trim().to_owned();
        let pivot = self.overlay.image_pivot;
        if query.is_empty() && pivot.is_none() {
            return false;
        }
        let cache_key = self.current_cache_key(&query, pivot);
        let Some(cached) = self.result_cache.get(&cache_key).cloned() else {
            return false;
        };

        self.overlay.last_submitted_query = query;
        self.active_search = None;
        self.overlay.results = cached.results;
        self.overlay.has_more_results = cached.has_more_results;
        self.select_after_results_refresh();
        self.overlay.status = SearchOverlayStatus::Ready;
        #[cfg(all(debug_assertions, feature = "semantic-search"))]
        self.log_active_results_ready(cache_key.scope);
        true
    }

    fn apply_search_matches(&mut self, canvas: &mut CanvasState) {
        let item_ids = self
            .overlay
            .results
            .iter()
            .flat_map(|group| group.item_ids.iter().copied())
            .collect::<Vec<_>>();
        let lead_item_id = self
            .overlay
            .results
            .get(self.overlay.result_selection.lead().unwrap_or(0))
            .and_then(|group| group.item_ids.first().copied());
        canvas.set_search_matches(item_ids, lead_item_id);
    }

    fn select_after_results_refresh(&mut self) {
        let Some(pending) = self.pending_result_selection.take() else {
            self.select_initial_result();
            return;
        };

        if self.overlay.results.is_empty() {
            self.overlay.result_selection.clear();
            return;
        }

        let index = pending.index.min(self.overlay.results.len() - 1);
        self.update_result_selection(index, pending.mode);
        self.overlay.scroll_lead_result_into_view = true;
    }
}

fn selected_image_pivot(canvas: &CanvasState) -> Option<ImageSearchPivot> {
    let selected = canvas.selected_item_ids();
    if selected.len() != 1 {
        return None;
    }
    let record = canvas.record_for_item_id(selected[0])?;
    Some(ImageSearchPivot {
        asset_id: record.asset.id,
        item_id: record.item.id,
    })
}

fn result_is_on_bottom_row(index: usize, last_index: usize, columns: usize) -> bool {
    index / columns.max(1) == last_index / columns.max(1)
}

fn normalized_recent_searches(recent_searches: Vec<String>) -> Vec<String> {
    let mut normalized = Vec::new();
    for query in recent_searches {
        let query = query.trim();
        if query.is_empty() || normalized.iter().any(|recent| recent == query) {
            continue;
        }
        normalized.push(query.to_owned());
        if normalized.len() == MAX_RECENT_SEARCHES {
            break;
        }
    }
    normalized
}

fn asset_placeholder_size(group: &SearchResultGroup, target_canvas_area: f32) -> egui::Vec2 {
    let (width, height) = group
        .asset_original_path
        .as_ref()
        .and_then(|path| image::image_dimensions(path).ok())
        .unwrap_or((FALLBACK_ASSET_PREVIEW_WIDTH, FALLBACK_ASSET_PREVIEW_HEIGHT));
    let scale = crate::jobs::initial_import_scale(width, height, true, target_canvas_area);
    egui::vec2(width as f32 * scale, height as f32 * scale)
}

fn update_search_placement_preview(
    canvas: &mut CanvasState,
    drag: &crate::palette::SearchResultDragState,
    pointer_world: egui::Pos2,
    screen_rect: egui::Rect,
    gap: f32,
) {
    if drag.item_ids.is_empty() {
        canvas.clear_search_drag_preview();
    } else {
        canvas.update_search_drag_preview(&drag.item_ids, pointer_world, screen_rect);
    }

    if drag.asset_placeholder_sizes.is_empty() {
        canvas.clear_move_preview_placeholders();
        return;
    }

    let placeholders =
        crate::canvas::compact_layout_for_sizes(&drag.asset_placeholder_sizes, gap, pointer_world);
    canvas.set_move_preview_placeholders(placeholders);
}

#[cfg(test)]
mod tests {
    use super::{
        ActiveSearchRequest, EmbeddingBackfillIdleState, PendingResultSelection,
        SEARCH_RESULT_PAGE_SIZE, SearchCacheEntry, SearchCacheKey, SearchController,
        SearchIndexResult, SearchSelectionMode, result_is_on_bottom_row,
    };
    use crate::canvas::CanvasState;
    use crate::semantic_search::{SearchQueryResult, SearchResultGroup, SearchScope};
    use eframe::egui;
    use std::time::{Duration, Instant};

    #[test]
    fn search_scope_defaults_to_this_board_and_survives_close() {
        let mut search = SearchController::default();
        assert_eq!(search.overlay.scope, SearchScope::ThisBoard);

        search.overlay.scope = SearchScope::Library;
        search.close_overlay(None);

        assert_eq!(search.overlay.scope, SearchScope::Library);
    }

    #[test]
    fn queue_asset_index_reports_missing_service() {
        let mut search = SearchController::default();

        let error = search
            .queue_asset_index(7, std::path::PathBuf::from("/tmp/cropped.png"))
            .unwrap_err();

        assert_eq!(
            error,
            "Semantic search service unavailable; cropped image was not queued for indexing"
        );
    }

    #[test]
    fn embedding_backfill_waits_for_quiet_idle_search_window() {
        let mut search = SearchController::default();
        let now = Instant::now();
        let idle = EmbeddingBackfillIdleState { app_idle: true };

        assert!(!search.can_start_embedding_backfill(now, idle));
        assert!(!search.can_start_embedding_backfill(now + Duration::from_millis(749), idle));
        assert!(search.can_start_embedding_backfill(now + Duration::from_millis(750), idle));

        search.overlay.query = "boat".to_owned();
        search.overlay.last_submitted_query.clear();
        search.overlay.last_query_edit_at = Some(now + Duration::from_millis(760));

        assert!(!search.can_start_embedding_backfill(now + Duration::from_millis(800), idle));
        assert!(!search.can_start_embedding_backfill(now + Duration::from_millis(880), idle));
        assert!(!search.can_start_embedding_backfill(now + Duration::from_millis(1000), idle));
        assert!(search.can_start_embedding_backfill(now + Duration::from_millis(1630), idle));
    }

    #[test]
    fn search_asset_placeholder_preview_uses_compact_layout() {
        let mut canvas = CanvasState::default();
        let drag = crate::palette::SearchResultDragState {
            item_ids: Vec::new(),
            asset_ids: vec![1, 2, 3, 4, 5],
            asset_placeholder_sizes: vec![egui::vec2(100.0, 100.0); 5],
            outside_overlay: true,
            placement_pending: true,
        };

        super::update_search_placement_preview(
            &mut canvas,
            &drag,
            egui::pos2(400.0, 300.0),
            egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 800.0)),
            crate::theme::SPACE_2,
        );

        let layout = canvas.move_preview_placeholder_layout_items();
        let mut rows = layout
            .iter()
            .map(|item| item.y.round() as i32)
            .collect::<Vec<_>>();
        rows.sort_unstable();
        rows.dedup();
        assert_eq!(layout.len(), 5);
        assert!(rows.len() > 1);
    }

    #[test]
    fn toggling_search_scope_invalidates_current_results() {
        let mut search = SearchController::default();
        search.overlay.query = "boat".to_owned();
        search.overlay.last_submitted_query = "boat".to_owned();
        search.overlay.results = vec![SearchResultGroup {
            asset_id: 10,
            asset_preview_path: None,
            asset_original_path: None,
            similarity: 0.9,
            item_ids: vec![101],
            instance_count: 1,
        }];

        search.toggle_scope();

        assert_eq!(search.overlay.scope, SearchScope::Library);
        assert!(search.overlay.last_submitted_query.is_empty());
        assert!(search.overlay.results.is_empty());
    }

    #[test]
    fn toggling_search_scope_restores_cached_results() {
        let mut search = SearchController::default();
        search.overlay.query = "boat".to_owned();
        search.result_cache.insert(
            SearchCacheKey {
                scope: SearchScope::Library,
                query: "boat".to_owned(),
                pivot_asset_id: None,
                result_limit: SEARCH_RESULT_PAGE_SIZE,
            },
            SearchCacheEntry {
                results: vec![SearchResultGroup {
                    asset_id: 20,
                    asset_preview_path: None,
                    asset_original_path: None,
                    similarity: 0.8,
                    item_ids: vec![202],
                    instance_count: 1,
                }],
                has_more_results: true,
            },
        );

        search.toggle_scope();

        assert_eq!(search.overlay.scope, SearchScope::Library);
        assert_eq!(search.overlay.last_submitted_query, "boat");
        assert_eq!(search.overlay.results.len(), 1);
        assert!(search.overlay.has_more_results);
    }

    #[test]
    fn indexed_events_do_not_churn_existing_visible_results() {
        let mut search = SearchController::default();
        search.overlay.open = true;
        search.overlay.query = "boat".to_owned();
        search.overlay.last_submitted_query = "boat".to_owned();
        search.overlay.results = vec![SearchResultGroup {
            asset_id: 10,
            asset_preview_path: None,
            asset_original_path: None,
            similarity: 0.9,
            item_ids: vec![101],
            instance_count: 1,
        }];

        let should_refresh =
            search.process_index_result(SearchIndexResult::Indexed { asset_id: 10 });

        assert!(!should_refresh);
        assert_eq!(search.overlay.last_submitted_query, "boat");
        assert_eq!(search.overlay.results.len(), 1);
    }

    #[test]
    fn result_selection_toggles_additive_indices() {
        let mut search = SearchController::default();

        search.update_result_selection(1, SearchSelectionMode::Replace);
        assert_eq!(search.overlay.result_selection.selected(), &[1]);

        search.update_result_selection(3, SearchSelectionMode::Toggle);
        assert_eq!(search.overlay.result_selection.selected(), &[1, 3]);

        search.update_result_selection(1, SearchSelectionMode::Toggle);
        assert_eq!(search.overlay.result_selection.selected(), &[3]);
    }

    #[test]
    fn selection_lead_stays_selected() {
        let mut search = SearchController::default();

        search.update_result_selection(1, SearchSelectionMode::Replace);
        assert_eq!(search.overlay.result_selection.lead(), Some(1));

        search.update_result_selection(3, SearchSelectionMode::Range);
        assert_eq!(search.overlay.result_selection.lead(), Some(3));
        assert!(
            search
                .overlay
                .result_selection
                .contains(search.overlay.result_selection.lead().unwrap())
        );

        search.update_result_selection(3, SearchSelectionMode::Toggle);
        assert_eq!(search.overlay.result_selection.lead(), Some(1));
        assert!(
            search
                .overlay
                .result_selection
                .contains(search.overlay.result_selection.lead().unwrap())
        );
    }

    #[test]
    fn result_selection_shift_selects_anchor_range() {
        let mut search = SearchController::default();

        search.update_result_selection(1, SearchSelectionMode::Replace);
        search.update_result_selection(4, SearchSelectionMode::Range);

        assert_eq!(search.overlay.result_selection.selected(), &[1, 2, 3, 4]);
    }

    #[test]
    fn result_selection_range_keeps_anchor_when_contracting() {
        let mut search = SearchController::default();

        search.update_result_selection(2, SearchSelectionMode::Replace);
        search.update_result_selection(5, SearchSelectionMode::Range);
        search.update_result_selection(3, SearchSelectionMode::Range);

        assert_eq!(search.overlay.result_selection.selected(), &[2, 3]);
        assert_eq!(search.overlay.result_selection.lead(), Some(3));
    }

    #[test]
    fn bottom_row_detection_handles_partial_rows() {
        assert!(!result_is_on_bottom_row(4, 7, 5));
        assert!(result_is_on_bottom_row(5, 7, 5));
        assert!(result_is_on_bottom_row(7, 7, 5));
    }

    #[test]
    fn result_expansion_selects_pending_next_row_column_after_async_refresh() {
        let mut search = SearchController::default();
        let mut canvas = CanvasState::default();
        search.overlay.query = "mountain".to_owned();
        search.overlay.last_submitted_query = "mountain".to_owned();
        search.overlay.result_limit = SEARCH_RESULT_PAGE_SIZE * 3;
        search.overlay.results = search_result_groups(10);
        search.overlay.result_selection.replace(9);
        search.pending_result_selection = Some(PendingResultSelection {
            index: 10,
            mode: SearchSelectionMode::Replace,
        });
        let cache_key = SearchCacheKey {
            scope: SearchScope::ThisBoard,
            query: "mountain".to_owned(),
            pivot_asset_id: None,
            result_limit: search.overlay.result_limit,
        };
        search.active_search = Some(ActiveSearchRequest {
            request_id: 7,
            cache_key,
            query: "mountain".to_owned(),
        });

        search.process_search_result(
            SearchQueryResult::Completed {
                request_id: 7,
                results: search_result_groups(16),
            },
            &mut canvas,
        );

        assert_eq!(search.overlay.results.len(), SEARCH_RESULT_PAGE_SIZE * 3);
        assert_eq!(search.overlay.result_selection.lead(), Some(10));
        assert_eq!(search.overlay.result_selection.selected(), &[10]);
        assert!(search.overlay.scroll_lead_result_into_view);
    }

    #[test]
    fn initial_result_becomes_range_anchor() {
        let mut search = SearchController::default();
        search.overlay.results = (0..5)
            .map(|index| SearchResultGroup {
                asset_id: index,
                asset_preview_path: None,
                asset_original_path: None,
                similarity: 0.9,
                item_ids: vec![100 + index],
                instance_count: 1,
            })
            .collect();

        search.select_initial_result();
        search.update_result_selection(3, SearchSelectionMode::Range);

        assert_eq!(search.overlay.result_selection.selected(), &[0, 1, 2, 3]);
    }

    #[test]
    fn result_drag_uses_selected_result_first_instances() {
        let mut search = SearchController::default();
        search.overlay.results = vec![
            SearchResultGroup {
                asset_id: 10,
                asset_preview_path: None,
                asset_original_path: None,
                similarity: 0.9,
                item_ids: vec![101, 102],
                instance_count: 2,
            },
            SearchResultGroup {
                asset_id: 11,
                asset_preview_path: None,
                asset_original_path: None,
                similarity: 0.8,
                item_ids: vec![201],
                instance_count: 1,
            },
        ];
        search.overlay.result_selection.replace(0);
        search.update_result_selection(1, SearchSelectionMode::Toggle);

        search.start_result_drag(
            1,
            super::SearchPlacementPreviewConfig {
                import_target_canvas_area: crate::data::DEFAULT_IMPORT_TARGET_CANVAS_AREA,
                import_gap: crate::data::DEFAULT_IMPORT_GAP,
            },
        );

        let drag = search.overlay.drag_state.expect("expected search drag");
        assert_eq!(drag.item_ids, vec![101, 201]);
    }

    #[test]
    fn selected_result_item_ids_falls_back_to_lead_result() {
        let mut search = SearchController::default();
        search.overlay.results = vec![
            SearchResultGroup {
                asset_id: 10,
                asset_preview_path: None,
                asset_original_path: None,
                similarity: 0.9,
                item_ids: vec![101],
                instance_count: 1,
            },
            SearchResultGroup {
                asset_id: 11,
                asset_preview_path: None,
                asset_original_path: None,
                similarity: 0.8,
                item_ids: vec![201],
                instance_count: 1,
            },
        ];
        search.overlay.result_selection.replace(1);

        assert_eq!(search.selected_result_item_ids(), vec![201]);
    }

    #[test]
    fn all_result_item_ids_uses_loaded_results() {
        let mut search = SearchController::default();
        search.overlay.results = vec![
            SearchResultGroup {
                asset_id: 10,
                asset_preview_path: None,
                asset_original_path: None,
                similarity: 0.9,
                item_ids: vec![101, 102],
                instance_count: 2,
            },
            SearchResultGroup {
                asset_id: 11,
                asset_preview_path: None,
                asset_original_path: None,
                similarity: 0.8,
                item_ids: vec![201],
                instance_count: 1,
            },
        ];

        assert_eq!(search.all_result_item_ids(), vec![101, 201]);
    }

    fn search_result_groups(count: usize) -> Vec<SearchResultGroup> {
        (0..count)
            .map(|index| SearchResultGroup {
                asset_id: index as i64,
                asset_preview_path: None,
                asset_original_path: None,
                similarity: 0.9,
                item_ids: vec![100 + index as i64],
                instance_count: 1,
            })
            .collect()
    }
}
