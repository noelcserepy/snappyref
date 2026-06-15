use std::collections::HashMap;
use std::collections::HashSet;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use eframe::egui;

use super::backfill::EmbeddingBackfillScheduler;
use super::placement::update_search_provisional_batch;
use super::selection::PendingResultSelection;
use crate::canvas::CanvasState;
use crate::library::storage::Library;
use crate::search::palette::{
    ImageSearchPivot, SEARCH_DEBOUNCE_MS, SEARCH_RESULT_PAGE_SIZE, SearchOverlayState,
    SearchOverlayStatus,
};
#[cfg(test)]
use crate::search::semantic::SemanticSearchInstallProgress;
use crate::search::semantic::{
    DEFAULT_SEMANTIC_SEARCH_MODEL, IndexResult as SearchIndexResult, LocalModelStoreStatus,
    SearchQueryResult, SemanticSearchConfig, SemanticSearchInstallController,
    SemanticSearchInstallDisplay, SemanticSearchInstallSnapshot, SemanticSearchService,
    default_local_model_store_status,
};
#[cfg(not(test))]
use crate::search::semantic::{
    default_semantic_search_asset_descriptor, remove_semantic_search_assets,
};

const MAX_RECENT_SEARCHES: usize = 8;

#[derive(Clone, Copy)]
pub(crate) struct SearchPlacementCreationConfig {
    pub(crate) placement_target_area: f32,
    pub(crate) placement_gap: f32,
    pub(crate) visible_canvas_rect: egui::Rect,
}

#[derive(Default)]
pub(crate) struct SearchController {
    pub(super) service: Option<SemanticSearchService>,
    pub(crate) overlay: SearchOverlayState,
    pub(super) result_cache: HashMap<SearchCacheKey, SearchCacheEntry>,
    pub(super) active_search: Option<ActiveSearchRequest>,
    pub(super) pending_result_selection: Option<PendingResultSelection>,
    pub(super) embedding_backfill: EmbeddingBackfillScheduler,
    pending_demand_embeddings: HashSet<i64>,
    completed_demand_embeddings: usize,
    next_search_request_id: u64,
    pub(super) recent_searches: Vec<String>,
    recent_searches_dirty: bool,
    pub(super) local_model_store_status: Option<LocalModelStoreStatus>,
    semantic_install: SemanticSearchInstallController,
    perf_events: Vec<SemanticPerfEvent>,
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
    ImportEmbeddingStarted {
        asset_id: i64,
    },
    ImportEmbeddingFinished {
        asset_id: i64,
    },
}

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub(super) struct SearchCacheKey {
    pub(super) query: String,
    pub(super) pivot_asset_id: Option<i64>,
    pub(super) result_limit: usize,
}

#[derive(Clone, Debug)]
pub(super) struct SearchCacheEntry {
    pub(super) results: Vec<crate::search::semantic::SearchResultAssetEntry>,
    pub(super) has_more_results: bool,
}

#[derive(Clone, Debug)]
pub(super) struct ActiveSearchRequest {
    pub(super) request_id: u64,
    pub(super) cache_key: SearchCacheKey,
    pub(super) query: String,
}

#[derive(Clone, Debug)]
pub(crate) struct SearchDropRequest {
    pub asset_ids: Vec<i64>,
    pub provisional_placement_ids: Vec<i64>,
}

impl SearchController {
    pub(crate) fn is_open(&self) -> bool {
        self.overlay.open
    }

    pub(super) fn install_surface_visible(&self) -> bool {
        matches!(
            self.local_model_store_status,
            Some(
                LocalModelStoreStatus::Missing
                    | LocalModelStoreStatus::Downloading { .. }
                    | LocalModelStoreStatus::Failed(_)
            )
        )
    }

    pub(crate) fn local_model_assets_available(&self) -> bool {
        self.local_model_store_status
            .as_ref()
            .is_some_and(LocalModelStoreStatus::assets_available)
    }

    pub(crate) fn local_model_assets_missing(&self) -> bool {
        matches!(
            self.local_model_store_status,
            Some(LocalModelStoreStatus::Missing)
        )
    }

    pub(crate) fn local_model_store_status(&self) -> LocalModelStoreStatus {
        self.local_model_store_status
            .clone()
            .unwrap_or(LocalModelStoreStatus::Missing)
    }

    #[cfg(test)]
    pub(crate) fn set_local_model_store_status_for_test(&mut self, status: LocalModelStoreStatus) {
        self.local_model_store_status = Some(status);
    }

    #[cfg(test)]
    pub(crate) fn set_semantic_install_for_test(
        &mut self,
        installer: impl FnMut(std::sync::mpsc::Sender<SemanticSearchInstallProgress>) + 'static,
    ) {
        self.semantic_install.set_test_installer(installer);
    }

    pub(crate) fn start_semantic_search_install_from_first_launch_prompt(
        &mut self,
        status: &mut String,
    ) {
        self.start_semantic_search_install(status);
    }

    pub(crate) fn start_semantic_search_install_from_settings(&mut self, status: &mut String) {
        self.start_semantic_search_install(status);
    }

    pub(crate) fn cancel_semantic_search_install_from_settings(&mut self, status: &mut String) {
        self.cancel_semantic_search_install(status);
    }

    #[cfg(feature = "semantic-search")]
    pub(crate) fn remove_semantic_search_assets_from_settings(&mut self, status: &mut String) {
        self.semantic_install.force_stop();
        #[cfg(not(test))]
        match crate::library::location::resolve_data_root() {
            Ok(data_root) => {
                let descriptor = default_semantic_search_asset_descriptor();
                match remove_semantic_search_assets(&data_root, &descriptor) {
                    Ok(()) => {
                        self.service = None;
                        self.local_model_store_status = Some(LocalModelStoreStatus::Missing);
                        self.overlay.status = SearchOverlayStatus::Ready;
                        self.overlay.install_action_focus_requested = true;
                        self.reset_embedding_backfill();
                        *status = "Semantic Search removed".to_owned();
                    }
                    Err(error) => {
                        self.local_model_store_status =
                            Some(LocalModelStoreStatus::Failed(error.clone()));
                        self.overlay.status = SearchOverlayStatus::Error(error.clone());
                        *status = format!("Semantic Search remove failed: {error}");
                    }
                }
            }
            Err(error) => {
                self.local_model_store_status = Some(LocalModelStoreStatus::Failed(error.clone()));
                self.overlay.status = SearchOverlayStatus::Error(error.clone());
                *status = format!("Semantic Search remove failed: {error}");
            }
        }
        #[cfg(test)]
        {
            self.service = None;
            self.local_model_store_status = Some(LocalModelStoreStatus::Missing);
            self.overlay.status = SearchOverlayStatus::Ready;
            self.overlay.install_action_focus_requested = true;
            self.reset_embedding_backfill();
            *status = "Semantic Search removed".to_owned();
        }
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

    /// Drop the semantic-search service so its worker thread closes the
    /// channel to the embedding helper child process. The child's
    /// `EmbeddingHelperClient::drop` impl kills the child as the worker
    /// thread unwinds. Called from the auto-update restart path before
    /// the app exits and the updater helper swaps the bundle.
    pub(crate) fn shutdown_helper_processes(&mut self) {
        self.service = None;
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

        #[cfg(test)]
        let model_store_status = self
            .local_model_store_status
            .clone()
            .unwrap_or_else(default_local_model_store_status);
        #[cfg(not(test))]
        let model_store_status = default_local_model_store_status();
        self.sync_model_store_status(model_store_status, status, canvas);
        if !self
            .local_model_store_status
            .as_ref()
            .is_some_and(LocalModelStoreStatus::assets_available)
        {
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

    fn sync_model_store_status(
        &mut self,
        model_store_status: LocalModelStoreStatus,
        status: &mut String,
        _canvas: Option<&mut CanvasState>,
    ) {
        self.local_model_store_status = Some(model_store_status.clone());
        match model_store_status {
            LocalModelStoreStatus::Installed | LocalModelStoreStatus::InstalledViaMigration => {}
            LocalModelStoreStatus::Missing => {
                self.service = None;
                if self.overlay.open {
                    self.overlay.focus_requested = false;
                    self.overlay.install_action_focus_requested = true;
                    self.overlay.results.clear();
                    self.overlay.result_selection.clear();
                    self.overlay.query.clear();
                    self.overlay.image_pivot = None;
                    self.active_search = None;
                    self.pending_result_selection = None;
                }
                self.overlay.status = SearchOverlayStatus::Ready;
            }
            LocalModelStoreStatus::Downloading { percent } => {
                self.service = None;
                self.overlay.status =
                    SearchOverlayStatus::Loading(format!("Installing Semantic Search {percent}%"));
            }
            LocalModelStoreStatus::Failed(error) => {
                self.service = None;
                self.close_overlay(None);
                self.overlay.status = SearchOverlayStatus::Error(error.clone());
                *status = format!("Semantic Search install failed: {error}");
            }
        }
    }

    pub(crate) fn drive_semantic_search_install(&mut self, status: &mut String) -> bool {
        let Some(snapshot) = self.semantic_install.drain() else {
            return false;
        };
        self.apply_semantic_install_snapshot(snapshot, status);
        true
    }

    pub(super) fn start_semantic_search_install(&mut self, status: &mut String) {
        if let Some(snapshot) = self.semantic_install.start() {
            self.apply_semantic_install_snapshot(snapshot, status);
        }
    }

    pub(super) fn cancel_semantic_search_install(&mut self, status: &mut String) {
        if let Some(snapshot) = self.semantic_install.request_cancel() {
            self.apply_semantic_install_snapshot(snapshot, status);
        }
    }

    fn apply_semantic_install_snapshot(
        &mut self,
        snapshot: SemanticSearchInstallSnapshot,
        status: &mut String,
    ) {
        self.local_model_store_status = Some(snapshot.model_store_status);
        self.overlay.status = match snapshot.display {
            SemanticSearchInstallDisplay::Ready => SearchOverlayStatus::Ready,
            SemanticSearchInstallDisplay::Loading(message) => SearchOverlayStatus::Loading(message),
            SemanticSearchInstallDisplay::Error(error) => SearchOverlayStatus::Error(error),
        };
        self.overlay.install_action_focus_requested = snapshot.install_action_focus_requested;
        *status = snapshot.status_message;
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

    pub(super) fn search_work_in_flight(&self, now: Instant) -> bool {
        self.active_search.is_some() || self.search_debounce_pending(now)
    }

    pub(super) fn search_debounce_pending(&self, now: Instant) -> bool {
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
            SearchIndexResult::ImportEmbeddingStarted { asset_id } => {
                self.perf_events
                    .push(SemanticPerfEvent::ImportEmbeddingStarted { asset_id });
                false
            }
            SearchIndexResult::Indexed { asset_id } => {
                if self.pending_demand_embeddings.remove(&asset_id) {
                    self.completed_demand_embeddings += 1;
                    self.perf_events
                        .push(SemanticPerfEvent::ImportEmbeddingFinished { asset_id });
                    if self.pending_demand_embeddings.is_empty() {
                        self.note_demand_embedding_drained_at(Instant::now());
                    }
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
                    self.perf_events
                        .push(SemanticPerfEvent::ImportEmbeddingFinished { asset_id });
                    if self.pending_demand_embeddings.is_empty() {
                        self.note_demand_embedding_drained_at(Instant::now());
                    }
                }
                self.overlay.status = SearchOverlayStatus::Error(error);
                false
            }
        }
    }

    fn process_search_result(&mut self, result: SearchQueryResult, _canvas: &mut CanvasState) {
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
                self.result_cache.insert(
                    active_search.cache_key,
                    SearchCacheEntry {
                        results: self.overlay.results.clone(),
                        has_more_results: self.overlay.has_more_results,
                    },
                );
                self.select_after_results_refresh();
                self.overlay.status = SearchOverlayStatus::Ready;
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
            }
            SearchQueryResult::Completed { .. } | SearchQueryResult::Failed { .. } => {}
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

    pub(crate) fn provisional_batch_active(&self) -> bool {
        self.overlay
            .drag_state
            .as_ref()
            .is_some_and(|drag| drag.provisional_batch_active)
    }

    pub(crate) fn prepare_canvas_provisional_batch(
        &mut self,
        ctx: &egui::Context,
        canvas: &mut CanvasState,
        screen_rect: egui::Rect,
        placement_creation_config: SearchPlacementCreationConfig,
    ) {
        if !self.overlay.open {
            crate::diagnostics::search_drag_debug(
                "prepare_canvas_provisional_batch skip overlay_closed",
            );
            return;
        }
        let egui_pointer_pos = ctx.input(|input| input.pointer.latest_pos());
        let pointer_pos = self
            .overlay
            .drag_state
            .as_ref()
            .and_then(|_| crate::platform::native_drop::cursor_screen_pos())
            .or(egui_pointer_pos);
        let Some(mut drag) = self.overlay.drag_state.take() else {
            crate::diagnostics::search_drag_debug(format!(
                "prepare_canvas_provisional_batch skip no_drag pointer_pos={pointer_pos:?}"
            ));
            return;
        };
        let Some(expanded_rect) = self.overlay.last_expanded_overlay_rect else {
            crate::diagnostics::search_drag_debug(format!(
                "prepare_canvas_provisional_batch skip no_expanded_rect pointer_pos={pointer_pos:?} asset_ids={:?}",
                drag.asset_ids
            ));
            self.overlay.drag_state = Some(drag);
            return;
        };

        if !drag.outside_overlay && !drag.provisional_batch_active {
            drag.outside_overlay =
                pointer_pos.is_some_and(|pos| !expanded_rect.expand(2.0).contains(pos));
            crate::diagnostics::search_drag_debug(format!(
                "prepare_canvas_provisional_batch outside_check pointer_pos={pointer_pos:?} expanded_rect={expanded_rect:?} outside_overlay={}",
                drag.outside_overlay
            ));
        }
        if !drag.outside_overlay {
            crate::diagnostics::search_drag_debug(format!(
                "prepare_canvas_provisional_batch skip inside_overlay asset_ids={:?}",
                drag.asset_ids
            ));
            self.overlay.drag_state = Some(drag);
            return;
        }

        if let Some(pointer_pos) = pointer_pos {
            let pointer_world = canvas.screen_to_world(pointer_pos, screen_rect);
            crate::diagnostics::search_drag_debug(format!(
                "prepare_canvas_provisional_batch update pointer_pos={pointer_pos:?} pointer_world={pointer_world:?} asset_ids={:?} active={}",
                drag.asset_ids, drag.provisional_batch_active
            ));
            update_search_provisional_batch(
                ctx,
                canvas,
                self,
                &mut drag,
                pointer_world,
                Some(pointer_pos),
                screen_rect,
                placement_creation_config,
            );
        } else {
            crate::diagnostics::search_drag_debug(format!(
                "prepare_canvas_provisional_batch skip no_pointer asset_ids={:?}",
                drag.asset_ids
            ));
        }
        self.overlay.drag_state = Some(drag);
        ctx.request_repaint_after(Duration::from_millis(16));
    }

    pub(crate) fn queue_asset_index(
        &mut self,
        asset_id: i64,
        original_path: PathBuf,
    ) -> Result<(), String> {
        let Some(service) = &self.service else {
            if self
                .local_model_store_status
                .as_ref()
                .is_some_and(|status| !status.assets_available())
            {
                return Ok(());
            }
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

    pub(crate) fn invalidate_library_search_cache(&mut self) {
        self.result_cache.clear();
        if let Some(service) = &self.service {
            service.invalidate_search_embedding_cache();
        }
        self.embedding_backfill.exhausted = false;
        if self.overlay.open {
            self.overlay.last_submitted_query.clear();
        }
    }

    #[allow(dead_code)]
    pub(crate) fn completed_demand_embedding_count(&self) -> usize {
        self.completed_demand_embeddings
    }

    pub(crate) fn embedding_backfill_is_idle(&self) -> bool {
        self.service.is_none()
            || (!self.embedding_backfill.in_flight
                && self.embedding_backfill.exhausted
                && !self
                    .service
                    .as_ref()
                    .is_some_and(|service| service.search_embedding_cache_warming()))
    }

    pub(crate) fn search_using_selected_image(
        &mut self,
        enabled: bool,
        library: Option<&Library>,
        canvas: &mut CanvasState,
        status: &mut String,
        canvas_image_max_side: u32,
    ) -> bool {
        let Some(pivot) = selected_image_pivot(library, canvas) else {
            return false;
        };

        self.sync_service(
            enabled,
            library,
            status,
            Some(canvas),
            canvas_image_max_side,
        );
        self.open_image_pivot_search(pivot, canvas);
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

        let pivot = selected_image_pivot(library, canvas);

        if pivot.is_some() && self.overlay.image_pivot != pivot {
            if let Some(pivot) = pivot {
                self.open_image_pivot_search(pivot, canvas);
            }
        }

        self.open_overlay_from_command();
    }

    pub(crate) fn close_overlay(&mut self, _canvas: Option<&mut CanvasState>) {
        self.overlay = SearchOverlayState::default();
        self.result_cache.clear();
        self.active_search = None;
        self.pending_result_selection = None;
    }

    pub(super) fn dismiss_image_pivot(&mut self, _canvas: &mut CanvasState) {
        self.overlay.image_pivot = None;
        self.overlay.last_submitted_query.clear();
        self.active_search = None;
        self.overlay.results.clear();
        self.overlay.result_selection.clear();
        self.pending_result_selection = None;
        self.overlay.active_recent_search_index = 0;
        self.overlay.result_limit = SEARCH_RESULT_PAGE_SIZE;
        self.overlay.has_more_results = false;
        self.overlay.last_query_edit_at = Some(Instant::now());
    }

    pub(super) fn submit_recent_search(&mut self, query: &str, canvas: &mut CanvasState) {
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

    fn open_image_pivot_search(&mut self, pivot: ImageSearchPivot, canvas: &mut CanvasState) {
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
        self.submit_query(canvas);
    }

    fn open_overlay_from_command(&mut self) {
        self.overlay.open = true;
        self.overlay.focus_requested = true;
        self.overlay.suppress_next_outside_click = true;
        if self.install_surface_visible() {
            self.overlay.focus_requested = false;
            self.overlay.install_action_focus_requested = true;
            if matches!(
                self.local_model_store_status,
                Some(LocalModelStoreStatus::Missing)
            ) {
                self.overlay.status = SearchOverlayStatus::Ready;
            }
            return;
        }
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

    pub(crate) fn submit_query(&mut self, _canvas: &mut CanvasState) {
        let query = self.overlay.query.trim().to_owned();
        let pivot = self.overlay.image_pivot.clone();
        if query.is_empty() && pivot.is_none() {
            self.active_search = None;
            self.overlay.results.clear();
            self.overlay.result_selection.clear();
            self.pending_result_selection = None;
            self.overlay.last_submitted_query.clear();
            self.overlay.has_more_results = false;
            self.overlay.status = SearchOverlayStatus::Ready;
            return;
        }

        if pivot.is_none() {
            self.record_recent_search(&query);
        }

        let cache_key = self.current_cache_key(&query, pivot.clone());
        if self
            .active_search
            .as_ref()
            .is_some_and(|active_search| active_search.cache_key == cache_key)
        {
            return;
        }
        if let Some(cached) = self.result_cache.get(&cache_key).cloned() {
            self.active_search = None;
            self.overlay.last_submitted_query = query;
            self.overlay.results = cached.results;
            self.overlay.has_more_results = cached.has_more_results;
            self.select_after_results_refresh();
            self.overlay.status = SearchOverlayStatus::Ready;
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
                    service.enqueue_search_with_image_pivot_with_limit(
                        request_id,
                        pivot.asset_id,
                        text,
                        self.overlay.result_limit + 1,
                    )
                }
                None => service.enqueue_search_with_limit(
                    request_id,
                    &query,
                    self.overlay.result_limit + 1,
                ),
            }
        };

        match enqueue_result {
            Ok(()) => {
                self.active_search = Some(ActiveSearchRequest {
                    request_id,
                    cache_key,
                    query: query.clone(),
                });
                self.overlay.status = SearchOverlayStatus::Loading("Searching…".to_owned());
            }
            Err(error) => {
                self.active_search = None;
                self.overlay.status = SearchOverlayStatus::Error(error);
                self.overlay.results.clear();
                self.overlay.result_selection.clear();
                self.pending_result_selection = None;
                self.overlay.has_more_results = false;
            }
        }
    }

    fn next_request_id(&mut self) -> u64 {
        let request_id = self.next_search_request_id;
        self.next_search_request_id = self.next_search_request_id.wrapping_add(1).max(1);
        request_id
    }

    fn current_cache_key(&self, query: &str, pivot: Option<ImageSearchPivot>) -> SearchCacheKey {
        SearchCacheKey {
            query: query.to_owned(),
            pivot_asset_id: pivot.map(|pivot| pivot.asset_id),
            result_limit: self.overlay.result_limit,
        }
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

fn selected_image_pivot(
    library: Option<&Library>,
    canvas: &CanvasState,
) -> Option<ImageSearchPivot> {
    let selected = canvas.selected_placement_ids();
    if selected.len() != 1 {
        return None;
    }
    let record = canvas.placement_for_id(selected[0])?;
    let asset_id = record.require_loaded_asset().id;
    let asset_preview_path = library
        .map(|library| library.asset_search_preview_paths_for_assets(&[asset_id]))
        .and_then(|mut paths| paths.remove(&asset_id));
    Some(ImageSearchPivot {
        asset_id,
        asset_preview_path,
    })
}

pub(super) fn result_is_on_bottom_row(index: usize, last_index: usize, columns: usize) -> bool {
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

#[cfg(test)]
mod tests {
    use super::super::backfill::EmbeddingBackfillIdleState;
    use super::super::placement::provisional_placement_commit_requested;
    use super::super::selection::{PendingResultSelection, SearchSelectionMode};
    use super::{
        ActiveSearchRequest, SEARCH_RESULT_PAGE_SIZE, SearchCacheKey, SearchController,
        SearchIndexResult, SearchOverlayStatus, result_is_on_bottom_row,
    };
    use crate::canvas::CanvasState;
    use crate::data::{Placement, PlacementState};
    use crate::search::semantic::{
        LocalModelStoreStatus, SearchQueryResult, SearchResultAssetEntry,
        SemanticSearchInstallProgress,
    };
    use eframe::egui;
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    #[test]
    fn provisional_placement_commit_requires_unmodified_primary_click() {
        assert!(provisional_placement_commit_requested(
            true,
            egui::Modifiers::default()
        ));
        assert!(!provisional_placement_commit_requested(
            true,
            egui::Modifiers {
                alt: true,
                ..Default::default()
            }
        ));
        assert!(!provisional_placement_commit_requested(
            true,
            egui::Modifiers {
                shift: true,
                ..Default::default()
            }
        ));
        assert!(!provisional_placement_commit_requested(
            false,
            egui::Modifiers::default()
        ));
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
    fn missing_local_model_store_makes_import_embedding_queue_a_noop() {
        let mut search = SearchController::default();
        let mut status = "Ready".to_owned();
        search.sync_model_store_status(LocalModelStoreStatus::Missing, &mut status, None);

        search
            .queue_asset_index(7, std::path::PathBuf::from("/tmp/imported.png"))
            .unwrap();

        assert_eq!(search.demand_embedding_pending_count(), 0);
        assert_eq!(status, "Ready");
        assert!(search.service.is_none());
    }

    #[test]
    fn missing_local_model_store_keeps_open_palette_on_install_call_to_action() {
        let mut search = SearchController::default();
        let mut status = "Ready".to_owned();
        search.overlay.open = true;

        search.sync_model_store_status(LocalModelStoreStatus::Missing, &mut status, None);

        assert!(search.overlay.open);
        assert!(!search.overlay.focus_requested);
        assert!(search.overlay.install_action_focus_requested);
        assert!(search.service.is_none());
    }

    #[test]
    fn failed_local_model_store_status_is_inline_install_failure() {
        let mut search = SearchController::default();
        let mut status = "Ready".to_owned();

        search.sync_model_store_status(
            LocalModelStoreStatus::Failed("data root is not writable".to_owned()),
            &mut status,
            None,
        );

        assert_eq!(
            status,
            "Semantic Search install failed: data root is not writable"
        );
    }

    #[test]
    fn downloading_local_model_store_status_keeps_service_stopped() {
        let mut search = SearchController::default();
        let mut status = "Ready".to_owned();

        search.sync_model_store_status(
            LocalModelStoreStatus::Downloading { percent: 25 },
            &mut status,
            None,
        );

        assert!(search.service.is_none());
        assert_eq!(status, "Ready");
    }

    #[test]
    fn semantic_search_install_start_is_coalesced_while_download_runs() {
        let starts = std::rc::Rc::new(std::cell::Cell::new(0));
        let observed = starts.clone();
        let mut search = SearchController::default();
        search.set_semantic_install_for_test(move |_| {
            observed.set(observed.get() + 1);
        });
        let mut status = "Ready".to_owned();

        search.start_semantic_search_install(&mut status);
        search.start_semantic_search_install(&mut status);

        assert_eq!(starts.get(), 1);
        assert_eq!(
            search.local_model_store_status,
            Some(LocalModelStoreStatus::Downloading { percent: 0 })
        );
        assert_eq!(status, "Installing Semantic Search");
    }

    #[test]
    fn first_launch_prompt_enable_uses_shared_install_flow() {
        let starts = std::rc::Rc::new(std::cell::Cell::new(0));
        let observed = starts.clone();
        let mut search = SearchController::default();
        search.set_semantic_install_for_test(move |_| {
            observed.set(observed.get() + 1);
        });
        let mut status = "Ready".to_owned();

        search.start_semantic_search_install_from_first_launch_prompt(&mut status);

        assert_eq!(starts.get(), 1);
        assert_eq!(
            search.local_model_store_status,
            Some(LocalModelStoreStatus::Downloading { percent: 0 })
        );
        assert_eq!(status, "Installing Semantic Search");
    }

    #[test]
    fn semantic_search_install_progress_and_failure_are_shared_status() {
        let mut search = SearchController::default();
        search.set_semantic_install_for_test(|tx| {
            tx.send(SemanticSearchInstallProgress::Progress { percent: 42 })
                .unwrap();
            tx.send(SemanticSearchInstallProgress::Failed {
                error: "network: timeout".to_owned(),
            })
            .unwrap();
        });
        let mut status = "Ready".to_owned();

        search.start_semantic_search_install(&mut status);
        assert!(search.drive_semantic_search_install(&mut status));

        assert_eq!(
            search.local_model_store_status,
            Some(LocalModelStoreStatus::Failed("network: timeout".to_owned()))
        );
        assert!(matches!(
            search.overlay.status,
            SearchOverlayStatus::Error(ref message) if message == "network: timeout"
        ));
        assert_eq!(status, "Semantic Search install failed: network: timeout");
    }

    #[test]
    fn semantic_search_install_cancel_returns_to_missing_state() {
        let mut search = SearchController::default();
        search.set_semantic_install_for_test(|_| {});
        let mut status = "Ready".to_owned();

        search.start_semantic_search_install(&mut status);
        search.cancel_semantic_search_install(&mut status);

        assert_eq!(
            search.local_model_store_status,
            Some(LocalModelStoreStatus::Missing)
        );
        assert!(!search.semantic_install.is_running());
        assert!(search.overlay.install_action_focus_requested);
        assert_eq!(status, "Semantic Search install canceled");
    }

    #[test]
    fn embedding_backfill_waits_for_quiet_idle_search_window() {
        let (mut search, root) =
            search_controller_with_semantic_service("snappyref-quiet-backfill-search-test");
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

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn embedding_backfill_can_check_immediately_after_demand_embeddings_drain() {
        let (mut search, root) =
            search_controller_with_semantic_service("snappyref-demand-drain-backfill-search-test");
        let now = Instant::now();

        search.note_demand_embedding_drained_at(now);

        assert!(
            search.can_start_embedding_backfill(now, EmbeddingBackfillIdleState { app_idle: true })
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn embedding_backfill_does_not_start_before_semantic_search_service_exists() {
        let mut search = SearchController::default();
        let now = Instant::now();
        let idle = EmbeddingBackfillIdleState { app_idle: true };

        assert!(!search.can_start_embedding_backfill(now, idle));
        assert!(!search.can_start_embedding_backfill(now + Duration::from_secs(5), idle));
        assert!(search.embedding_backfill_is_idle());
    }

    #[test]
    fn embedding_backfill_starts_after_semantic_search_service_exists_and_app_is_idle() {
        let (mut search, root) =
            search_controller_with_semantic_service("snappyref-installed-backfill-search-test");
        let now = Instant::now();
        let idle = EmbeddingBackfillIdleState { app_idle: true };

        assert!(!search.can_start_embedding_backfill(now, idle));
        assert!(search.can_start_embedding_backfill(now + Duration::from_millis(750), idle));

        let _ = std::fs::remove_dir_all(root);
    }

    fn search_controller_with_semantic_service(
        prefix: &str,
    ) -> (SearchController, std::path::PathBuf) {
        let root = temp_root(prefix);
        let library = crate::library::storage::Library::initialize(root.join("library")).unwrap();
        let search = SearchController {
            service: Some(
                crate::search::semantic::SemanticSearchService::new(
                    library,
                    crate::search::semantic::SemanticSearchConfig {
                        enabled: false,
                        model: crate::search::semantic::DEFAULT_SEMANTIC_SEARCH_MODEL,
                        canvas_image_max_side: crate::data::DEFAULT_CANVAS_IMAGE_MAX_SIDE,
                    },
                )
                .unwrap(),
            ),
            ..SearchController::default()
        };
        (search, root)
    }

    fn temp_root(prefix: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "{}-{}",
            prefix,
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn search_asset_drag_uses_compact_provisional_placements() {
        let mut canvas = CanvasState::default();
        let mut search = SearchController::default();
        search.overlay.results = (1..=5)
            .map(|asset_id| SearchResultAssetEntry {
                asset_id,
                asset_preview_path: None,
                asset_original_path: None,
                asset_width: 100,
                asset_height: 100,
                similarity: 0.9,
            })
            .collect();
        let mut drag = crate::search::palette::SearchResultDragState {
            asset_ids: vec![1, 2, 3, 4, 5],
            outside_overlay: true,
            provisional_batch_active: false,
            auto_fit_applied: false,
        };

        let ctx = egui::Context::default();
        super::update_search_provisional_batch(
            &ctx,
            &mut canvas,
            &search,
            &mut drag,
            egui::pos2(400.0, 300.0),
            None,
            egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 800.0)),
            super::SearchPlacementCreationConfig {
                placement_target_area: 10_000.0,
                placement_gap: crate::ui::theme::SPACE_2,
                visible_canvas_rect: egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1200.0, 800.0),
                ),
            },
        );

        let layout = canvas
            .provisional_placements_in_selection_order()
            .into_iter()
            .map(|placement| {
                (
                    placement.state.geometry.center.x,
                    placement.state.geometry.center.y,
                )
            })
            .collect::<Vec<_>>();
        let mut rows = layout
            .iter()
            .map(|(_, y)| y.round() as i32)
            .collect::<Vec<_>>();
        rows.sort_unstable();
        rows.dedup();
        assert_eq!(layout.len(), 5);
        assert!(rows.len() > 1);
    }

    #[test]
    fn search_asset_drag_recenters_group_after_auto_fit() {
        let mut canvas = CanvasState::default();
        let mut search = SearchController::default();
        search.overlay.results = (1..=4)
            .map(|asset_id| SearchResultAssetEntry {
                asset_id,
                asset_preview_path: None,
                asset_original_path: None,
                asset_width: 1600,
                asset_height: 1200,
                similarity: 0.9,
            })
            .collect();
        let mut drag = crate::search::palette::SearchResultDragState {
            asset_ids: vec![1, 2, 3, 4],
            outside_overlay: true,
            provisional_batch_active: false,
            auto_fit_applied: false,
        };
        let ctx = egui::Context::default();
        let screen_rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 800.0));
        let pointer_screen = egui::pos2(900.0, 600.0);
        let pointer_world = canvas.screen_to_world(pointer_screen, screen_rect);

        super::update_search_provisional_batch(
            &ctx,
            &mut canvas,
            &search,
            &mut drag,
            pointer_world,
            Some(pointer_screen),
            screen_rect,
            super::SearchPlacementCreationConfig {
                placement_target_area: 360_000.0,
                placement_gap: crate::ui::theme::SPACE_2,
                visible_canvas_rect: egui::Rect::from_min_size(
                    egui::pos2(260.0, 220.0),
                    egui::vec2(280.0, 180.0),
                ),
            },
        );

        let bounds = canvas
            .selected_bounds_world()
            .expect("expected provisional search placement bounds");
        let recentered_pointer_world = canvas.screen_to_world(pointer_screen, screen_rect);
        assert!(drag.auto_fit_applied);
        assert!(
            (bounds.center().x - recentered_pointer_world.x).abs() < 0.5,
            "world center {:?} should match pointer world {:?}",
            bounds.center(),
            recentered_pointer_world
        );
        assert!(
            (bounds.center().y - recentered_pointer_world.y).abs() < 0.5,
            "world center {:?} should match pointer world {:?}",
            bounds.center(),
            recentered_pointer_world
        );
    }

    #[test]
    fn search_drag_uses_asset_sizes_for_new_placements() {
        let mut canvas = CanvasState::default();
        let mut search = SearchController::default();
        let source_positions = [
            egui::pos2(-800.0, -600.0),
            egui::pos2(900.0, -420.0),
            egui::pos2(-500.0, 350.0),
            egui::pos2(700.0, 720.0),
            egui::pos2(0.0, 1300.0),
        ];
        for (index, position) in source_positions.iter().enumerate() {
            let item_id = 100 + index as i64;
            let asset_id = 10 + index as i64;
            let asset = crate::data::Asset {
                id: asset_id,
                original_path: std::path::PathBuf::from("search-result"),
                filename: "search-result".to_owned(),
                mime_type: "image/png".to_owned(),
                width: 200,
                height: 100,
                file_size: 0,
                imported_at: 0,
            };
            canvas.add_unresolved_placement(Placement::loaded(
                PlacementState {
                    id: item_id,
                    asset_id: Some(asset_id),
                    geometry: crate::data::RectGeometry::new(
                        *position,
                        eframe::egui::Vec2::ZERO,
                        0.5,
                        0.0,
                        index as i32,
                    ),
                    archived: false,
                },
                asset,
            ));
            search.overlay.results.push(SearchResultAssetEntry {
                asset_id,
                asset_preview_path: None,
                asset_original_path: None,
                asset_width: 200,
                asset_height: 100,
                similarity: 0.9,
            });
        }
        search.overlay.result_selection.range_to(4);
        let mut drag = crate::search::palette::SearchResultDragState {
            asset_ids: vec![10, 11, 12, 13, 14],
            outside_overlay: true,
            provisional_batch_active: false,
            auto_fit_applied: false,
        };

        let ctx = egui::Context::default();
        super::update_search_provisional_batch(
            &ctx,
            &mut canvas,
            &search,
            &mut drag,
            egui::pos2(400.0, 300.0),
            None,
            egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 800.0)),
            super::SearchPlacementCreationConfig {
                placement_target_area: 10_000.0,
                placement_gap: crate::ui::theme::SPACE_2,
                visible_canvas_rect: egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1200.0, 800.0),
                ),
            },
        );

        let bounds = canvas
            .selected_bounds_world()
            .expect("expected provisional search placement bounds");
        assert_eq!(canvas.provisional_placements_in_selection_order().len(), 5);
        assert!(
            bounds.width() < 360.0 && bounds.height() < 420.0,
            "expected compact search provisional batch, got bounds {bounds:?}"
        );
    }

    #[test]
    fn search_drag_places_asset_at_import_size_even_when_asset_has_board_instance() {
        let mut search = SearchController::default();
        search.overlay.results = vec![SearchResultAssetEntry {
            asset_id: 10,
            asset_preview_path: None,
            asset_original_path: None,
            asset_width: 4000,
            asset_height: 3000,
            similarity: 0.9,
        }];
        search.overlay.result_selection.replace(0);

        search.start_result_drag(0);

        let drag = search
            .overlay
            .drag_state
            .as_ref()
            .expect("expected library search result drag");
        assert_eq!(drag.asset_ids, vec![10]);
    }

    #[test]
    fn search_drag_updates_snap_guides_after_provisional_placement_exists() {
        let mut canvas = CanvasState::default();
        canvas.set_placement_gap(24.0);
        let target_asset = crate::data::Asset {
            id: 900,
            original_path: std::path::PathBuf::from("snap-target"),
            filename: "snap-target".to_owned(),
            mime_type: "image/png".to_owned(),
            width: 100,
            height: 100,
            file_size: 0,
            imported_at: 0,
        };
        canvas.add_unresolved_placement(Placement::loaded(
            PlacementState::with_display_size(
                900,
                Some(900),
                eframe::egui::pos2(0.0, 0.0),
                eframe::egui::vec2(100.0, 100.0),
                1.0,
                0.0,
                0,
                false,
            ),
            target_asset,
        ));

        let mut search = SearchController::default();
        search.overlay.results = (1..=2)
            .map(|asset_id| SearchResultAssetEntry {
                asset_id,
                asset_preview_path: None,
                asset_original_path: None,
                asset_width: 100,
                asset_height: 100,
                similarity: 0.9,
            })
            .collect();
        let mut drag = crate::search::palette::SearchResultDragState {
            asset_ids: vec![1, 2],
            outside_overlay: true,
            provisional_batch_active: false,
            auto_fit_applied: false,
        };
        let ctx = egui::Context::default();
        let screen_rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 800.0));
        let placement_creation_config = super::SearchPlacementCreationConfig {
            placement_target_area: 10_000.0,
            placement_gap: crate::ui::theme::SPACE_2,
            visible_canvas_rect: screen_rect,
        };

        super::update_search_provisional_batch(
            &ctx,
            &mut canvas,
            &search,
            &mut drag,
            egui::pos2(400.0, 0.0),
            None,
            screen_rect,
            placement_creation_config,
        );
        assert_eq!(canvas.guide_snap_indicator_count(), 0);
        let initial_bounds = canvas
            .selected_bounds_world()
            .expect("expected provisional search placement bounds");
        let snap_center_x = 50.0 + 24.0 + initial_bounds.width() * 0.5 - 2.0;
        ctx.input_mut(|input| {
            input.time = 10.0;
        });

        super::update_search_provisional_batch(
            &ctx,
            &mut canvas,
            &search,
            &mut drag,
            egui::pos2(snap_center_x, 0.0),
            None,
            screen_rect,
            placement_creation_config,
        );

        assert_eq!(canvas.guide_snap_indicator_count(), 1);
        assert!(canvas.guide_snap_mark_count() > 0);
        assert!(!canvas.selection_border_visible_for_test(10.5));
        assert!(canvas.selection_border_visible_for_test(12.1));
    }

    #[test]
    fn keyboard_search_placement_creates_selected_provisional_placements() {
        let ctx = egui::Context::default();
        let mut search = SearchController::default();
        let mut canvas = CanvasState::default();
        search.overlay.results = vec![SearchResultAssetEntry {
            asset_id: 10,
            asset_preview_path: None,
            asset_original_path: None,
            asset_width: 200,
            asset_height: 100,
            similarity: 0.9,
        }];
        search.overlay.result_selection.replace(0);

        search.start_keyboard_placement_for_selected_results(
            &ctx,
            &mut canvas,
            Some(egui::pos2(400.0, 300.0)),
            egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0)),
            super::SearchPlacementCreationConfig {
                placement_target_area: 20_000.0,
                placement_gap: 24.0,
                visible_canvas_rect: egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(800.0, 600.0),
                ),
            },
        );

        let selected = canvas.selected_placement_ids();
        assert_eq!(selected.len(), 1);
        let record = canvas.placement_for_id(selected[0]).unwrap();
        assert_eq!(
            record.lifecycle,
            crate::data::PlacementLifecycle::Provisional
        );
        assert_eq!(record.state.asset_id, Some(10));
        assert!(matches!(
            record.content,
            crate::data::PlacementContent::Loading {
                source: crate::data::ContentResolutionSource::SearchAsset { asset_id: 10 },
                ..
            }
        ));
    }

    #[test]
    fn prepare_canvas_provisional_batch_updates_arrangement_inputs_before_overlay_paint() {
        let ctx = egui::Context::default();
        let mut search = SearchController::default();
        let mut canvas = CanvasState::default();
        search.overlay.open = true;
        search.overlay.last_expanded_overlay_rect = Some(egui::Rect::from_min_size(
            egui::pos2(0.0, 0.0),
            egui::vec2(240.0, 240.0),
        ));
        search.overlay.results = vec![SearchResultAssetEntry {
            asset_id: 1,
            asset_preview_path: None,
            asset_original_path: None,
            asset_width: 100,
            asset_height: 100,
            similarity: 0.9,
        }];
        search.overlay.drag_state = Some(crate::search::palette::SearchResultDragState {
            asset_ids: vec![1],
            outside_overlay: false,
            provisional_batch_active: false,
            auto_fit_applied: false,
        });

        let _ = ctx.run_ui(
            egui::RawInput {
                events: vec![egui::Event::PointerMoved(egui::pos2(420.0, 300.0))],
                ..Default::default()
            },
            |ui| {
                search.prepare_canvas_provisional_batch(
                    ui.ctx(),
                    &mut canvas,
                    egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 800.0)),
                    super::SearchPlacementCreationConfig {
                        placement_target_area: 120_000.0,
                        placement_gap: crate::ui::theme::SPACE_2,
                        visible_canvas_rect: egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(1200.0, 800.0),
                        ),
                    },
                );
            },
        );

        assert_eq!(canvas.provisional_placements_in_selection_order().len(), 1);
        assert!(
            search
                .overlay
                .drag_state
                .as_ref()
                .is_some_and(|drag| drag.outside_overlay)
        );
    }

    #[test]
    fn show_overlay_does_not_move_active_provisional_batch_after_canvas_paint() {
        let ctx = egui::Context::default();
        let mut search = SearchController::default();
        let mut canvas = CanvasState::default();
        let screen_rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 800.0));
        let placement_creation_config = super::SearchPlacementCreationConfig {
            placement_target_area: 20_000.0,
            placement_gap: 24.0,
            visible_canvas_rect: screen_rect,
        };
        search.overlay.open = true;
        search.overlay.query = "inspect drag".to_owned();
        search.overlay.status = SearchOverlayStatus::Ready;
        search.overlay.last_expanded_overlay_rect = Some(egui::Rect::from_min_size(
            egui::pos2(380.0, 72.0),
            egui::vec2(crate::search::palette::PALETTE_WIDTH, 420.0),
        ));
        search.overlay.results = vec![SearchResultAssetEntry {
            asset_id: 10,
            asset_preview_path: None,
            asset_original_path: None,
            asset_width: 200,
            asset_height: 100,
            similarity: 0.9,
        }];

        search.start_keyboard_placement_for_selected_results(
            &ctx,
            &mut canvas,
            Some(egui::pos2(200.0, 200.0)),
            screen_rect,
            placement_creation_config,
        );
        let center_before = canvas
            .selected_bounds_world()
            .expect("expected active provisional placement")
            .center();

        let _ = ctx.run_ui(
            egui::RawInput {
                events: vec![egui::Event::PointerMoved(egui::pos2(900.0, 500.0))],
                ..Default::default()
            },
            |ui| {
                let _ = search.show_overlay(
                    ui.ctx(),
                    screen_rect,
                    &mut canvas,
                    &mut String::new(),
                    placement_creation_config,
                );
            },
        );

        let center_after = canvas
            .selected_bounds_world()
            .expect("expected active provisional placement")
            .center();
        assert_eq!(center_after, center_before);
    }

    #[test]
    fn indexed_events_do_not_churn_existing_visible_results() {
        let mut search = SearchController::default();
        search.overlay.open = true;
        search.overlay.query = "boat".to_owned();
        search.overlay.last_submitted_query = "boat".to_owned();
        search.overlay.results = vec![SearchResultAssetEntry {
            asset_id: 10,
            asset_preview_path: None,
            asset_original_path: None,
            asset_width: 1,
            asset_height: 1,
            similarity: 0.9,
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
            .map(|index| SearchResultAssetEntry {
                asset_id: index,
                asset_preview_path: None,
                asset_original_path: None,
                asset_width: 1,
                asset_height: 1,
                similarity: 0.9,
            })
            .collect();

        search.select_initial_result();
        search.update_result_selection(3, SearchSelectionMode::Range);

        assert_eq!(search.overlay.result_selection.selected(), &[0, 1, 2, 3]);
    }

    #[test]
    fn result_drag_uses_selected_result_assets() {
        let mut search = SearchController::default();
        search.overlay.results = vec![
            SearchResultAssetEntry {
                asset_id: 10,
                asset_preview_path: None,
                asset_original_path: None,
                asset_width: 1,
                asset_height: 1,
                similarity: 0.9,
            },
            SearchResultAssetEntry {
                asset_id: 11,
                asset_preview_path: None,
                asset_original_path: None,
                asset_width: 1,
                asset_height: 1,
                similarity: 0.8,
            },
        ];
        search.overlay.result_selection.replace(0);
        search.update_result_selection(1, SearchSelectionMode::Toggle);

        search.start_result_drag(1);

        let drag = search.overlay.drag_state.expect("expected search drag");
        assert_eq!(drag.asset_ids, vec![10, 11]);
    }

    #[test]
    fn selected_result_asset_ids_falls_back_to_lead_result() {
        let mut search = SearchController::default();
        search.overlay.results = vec![
            SearchResultAssetEntry {
                asset_id: 10,
                asset_preview_path: None,
                asset_original_path: None,
                asset_width: 1,
                asset_height: 1,
                similarity: 0.9,
            },
            SearchResultAssetEntry {
                asset_id: 11,
                asset_preview_path: None,
                asset_original_path: None,
                asset_width: 1,
                asset_height: 1,
                similarity: 0.8,
            },
        ];
        search.overlay.result_selection.replace(1);

        assert_eq!(search.selected_result_asset_ids(), vec![11]);
    }

    #[test]
    fn all_result_asset_ids_uses_loaded_results() {
        let mut search = SearchController::default();
        search.overlay.results = vec![
            SearchResultAssetEntry {
                asset_id: 10,
                asset_preview_path: None,
                asset_original_path: None,
                asset_width: 1,
                asset_height: 1,
                similarity: 0.9,
            },
            SearchResultAssetEntry {
                asset_id: 11,
                asset_preview_path: None,
                asset_original_path: None,
                asset_width: 1,
                asset_height: 1,
                similarity: 0.8,
            },
        ];

        assert_eq!(search.all_result_asset_ids(), vec![10, 11]);
    }

    fn search_result_groups(count: usize) -> Vec<SearchResultAssetEntry> {
        (0..count)
            .map(|index| SearchResultAssetEntry {
                asset_id: index as i64,
                asset_preview_path: None,
                asset_original_path: None,
                asset_width: 1,
                asset_height: 1,
                similarity: 0.9,
            })
            .collect()
    }
}
