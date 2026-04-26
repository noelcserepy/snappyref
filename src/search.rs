use std::path::PathBuf;
use std::time::{Duration, Instant};

use eframe::egui;

use crate::canvas::CanvasState;
use crate::palette::{
    ImageSearchPivot, PALETTE_GRID_MAX_HEIGHT, PALETTE_WIDTH, RECENT_SEARCHES, SEARCH_DEBOUNCE_MS,
    SEARCH_OVERLAY_TOP_MARGIN, SEARCH_RESULT_PAGE_SIZE, SearchOverlayState, SearchOverlayStatus,
    paint_palette_divider, paint_palette_empty_state, paint_palette_footer,
    paint_palette_input_row, search_result_grid_content_height, search_result_grid_layout,
    search_result_tile,
};
use crate::semantic_search::{
    DEFAULT_SEMANTIC_SEARCH_MODEL, IndexResult as SearchIndexResult, SearchScope,
    SemanticSearchConfig, SemanticSearchService,
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

#[derive(Default)]
pub(crate) struct SearchController {
    service: Option<SemanticSearchService>,
    pub(crate) overlay: SearchOverlayState,
}

#[derive(Clone, Debug)]
pub(crate) struct SearchDropRequest {
    pub item_ids: Vec<i64>,
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

    pub(crate) fn sync_service(
        &mut self,
        enabled: bool,
        library: Option<&Library>,
        status: &mut String,
        canvas: Option<&mut CanvasState>,
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
                enabled: true,
                model: DEFAULT_SEMANTIC_SEARCH_MODEL,
            },
        ) {
            Ok(service) => {
                self.overlay.status =
                    SearchOverlayStatus::Loading("Preparing semantic search…".to_owned());
                let _ = service.enqueue_startup_backfill();
                self.service = Some(service);
            }
            Err(error) => {
                self.overlay.status = SearchOverlayStatus::Error(error.clone());
                *status = format!("Semantic search unavailable: {error}");
            }
        }
    }

    pub(crate) fn handle_results(&mut self) {
        let Some(service) = &self.service else {
            return;
        };

        for result in service.try_recv_all() {
            match result {
                SearchIndexResult::Progress { message } => {
                    if !matches!(self.overlay.status, SearchOverlayStatus::Error(_)) {
                        self.overlay.status = SearchOverlayStatus::Loading(message);
                    }
                }
                SearchIndexResult::ModelReady => {
                    if !self.overlay.query.trim().is_empty() {
                        self.overlay.last_submitted_query.clear();
                    }
                    if !matches!(self.overlay.status, SearchOverlayStatus::Error(_)) {
                        self.overlay.status = SearchOverlayStatus::Ready;
                    }
                }
                SearchIndexResult::Indexed => {
                    if !self.overlay.query.trim().is_empty() {
                        self.overlay.last_submitted_query.clear();
                    }
                }
                SearchIndexResult::Failed { error } => {
                    self.overlay.status = SearchOverlayStatus::Error(error);
                }
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

    pub(crate) fn queue_asset_index(&mut self, asset_id: i64, original_path: PathBuf) {
        let Some(service) = &self.service else {
            return;
        };
        let _ = service.enqueue_asset_index(asset_id, original_path);
        self.overlay.status = SearchOverlayStatus::Indexing;
    }

    pub(crate) fn drive_verification_search(
        &mut self,
        query: &str,
        scope: SearchScope,
        enabled: &mut bool,
        library: Option<&Library>,
        canvas: &mut CanvasState,
        status: &mut String,
    ) {
        if !*enabled {
            *enabled = true;
            self.sync_service(*enabled, library, status, Some(canvas));
        }

        self.overlay.open = true;
        self.overlay.focus_requested = true;
        self.overlay.scope = scope;
        if self.overlay.query.trim() != query.trim() {
            self.overlay.query = query.to_owned();
            self.overlay.last_query_edit_at = None;
            self.overlay.last_submitted_query.clear();
            self.overlay.results.clear();
            self.overlay.result_selection.clear();
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

    pub(crate) fn handle_shortcut(
        &mut self,
        ctx: &egui::Context,
        enabled: bool,
        library: Option<&Library>,
        canvas: &mut CanvasState,
        status: &mut String,
    ) {
        if !enabled {
            return;
        }

        let triggered =
            ctx.input(|input| input.modifiers.command && input.key_pressed(egui::Key::F));
        if !triggered {
            return;
        }

        self.sync_service(enabled, library, status, Some(canvas));

        let pivot = {
            let selected = canvas.selected_item_ids();
            if selected.len() == 1
                && let Some(record) = canvas.record_for_item_id(selected[0])
            {
                Some(ImageSearchPivot {
                    asset_id: record.asset.id,
                    item_id: record.item.id,
                })
            } else {
                None
            }
        };

        if pivot.is_some() && self.overlay.image_pivot != pivot {
            self.overlay.query.clear();
            self.overlay.last_submitted_query.clear();
            self.overlay.results.clear();
            self.overlay.result_selection.clear();
            self.overlay.active_recent_search_index = 0;
            self.overlay.result_limit = SEARCH_RESULT_PAGE_SIZE;
            self.overlay.has_more_results = false;
            self.overlay.image_pivot = pivot;
            self.overlay.last_query_edit_at = None;
            canvas.clear_search_matches();
            self.submit_query(canvas);
        }

        self.overlay.open = true;
        self.overlay.focus_requested = true;
        if !matches!(self.overlay.status, SearchOverlayStatus::Error(_)) {
            let overlay_status = self
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
            self.overlay.status = overlay_status;
        }
    }

    pub(crate) fn show_overlay(
        &mut self,
        ctx: &egui::Context,
        screen_rect: egui::Rect,
        canvas: &mut CanvasState,
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
                    .corner_radius(egui::CornerRadius::same(12))
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
                            self.overlay.scope,
                        );
                        let response = input_result.response;
                        let tab_toggled_scope = response.has_focus()
                            && ctx.input_mut(|input| {
                                input.consume_key(egui::Modifiers::NONE, egui::Key::Tab)
                            });
                        if input_result.scope_toggled || tab_toggled_scope {
                            self.toggle_scope();
                            self.submit_query(canvas);
                        }
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
                                if let Some(recent_search) = paint_palette_empty_state(
                                    ui,
                                    self.overlay
                                        .active_recent_search_index
                                        .min(RECENT_SEARCHES.len().saturating_sub(1)),
                                ) {
                                    self.submit_recent_search(recent_search, canvas);
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
                                    self.start_result_drag(index);
                                }
                            }

                            paint_palette_divider(ui);
                            paint_palette_footer(
                                ui,
                                result_count,
                                self.overlay.has_more_results,
                                &self.overlay.status,
                            );
                        }
                    });
            });

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
                    canvas.update_search_drag_preview(&drag.item_ids, pointer_world, screen_rect);
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
        if clicked_outside
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
                    self.overlay.result_limit += SEARCH_RESULT_PAGE_SIZE;
                    self.submit_query(canvas);
                    if !self.overlay.results.is_empty() {
                        let index = next_row_index.min(self.overlay.results.len() - 1);
                        self.update_result_selection(index, arrow_selection_mode);
                        self.overlay.scroll_lead_result_into_view = true;
                        self.apply_search_matches(canvas);
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
                    self.start_keyboard_placement_for_all_results(canvas, pointer_pos, screen_rect);
                } else {
                    self.start_keyboard_placement_for_selected_results(
                        canvas,
                        pointer_pos,
                        screen_rect,
                    );
                }
            }
        } else if !RECENT_SEARCHES.is_empty() {
            let last_recent_index = RECENT_SEARCHES.len() - 1;
            if ctx.input(|input| input.key_pressed(egui::Key::ArrowDown)) {
                self.overlay.active_recent_search_index =
                    (self.overlay.active_recent_search_index + 1).min(last_recent_index);
            } else if ctx.input(|input| input.key_pressed(egui::Key::ArrowUp)) {
                self.overlay.active_recent_search_index =
                    self.overlay.active_recent_search_index.saturating_sub(1);
            } else if ctx.input(|input| input.key_pressed(egui::Key::Enter)) {
                let recent_search = RECENT_SEARCHES[self
                    .overlay
                    .active_recent_search_index
                    .min(last_recent_index)];
                self.submit_recent_search(recent_search, canvas);
            }
        }
        drop_request
    }

    pub(crate) fn close_overlay(&mut self, canvas: Option<&mut CanvasState>) {
        self.overlay = SearchOverlayState::default();
        if let Some(canvas) = canvas {
            canvas.clear_search_matches();
            canvas.clear_search_drag_preview();
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
        self.overlay.results.clear();
        self.overlay.result_selection.clear();
        self.overlay.result_limit = SEARCH_RESULT_PAGE_SIZE;
        self.overlay.has_more_results = false;
    }

    fn start_result_drag(&mut self, index: usize) {
        if !self.overlay.result_selection.contains(index) {
            self.update_result_selection(index, SearchSelectionMode::Replace);
        }

        let item_ids = self.selected_result_item_ids();
        if item_ids.is_empty() {
            return;
        }

        self.overlay.drag_state = Some(crate::palette::SearchResultDragState {
            item_ids,
            outside_overlay: false,
            placement_pending: false,
        });
    }

    fn start_keyboard_placement_for_selected_results(
        &mut self,
        canvas: &mut CanvasState,
        pointer_pos: Option<egui::Pos2>,
        screen_rect: egui::Rect,
    ) {
        let item_ids = self.selected_result_item_ids();
        self.start_keyboard_placement(item_ids, canvas, pointer_pos, screen_rect);
    }

    fn start_keyboard_placement_for_all_results(
        &mut self,
        canvas: &mut CanvasState,
        pointer_pos: Option<egui::Pos2>,
        screen_rect: egui::Rect,
    ) {
        let item_ids = self.all_result_item_ids();
        self.start_keyboard_placement(item_ids, canvas, pointer_pos, screen_rect);
    }

    fn start_keyboard_placement(
        &mut self,
        item_ids: Vec<i64>,
        canvas: &mut CanvasState,
        pointer_pos: Option<egui::Pos2>,
        screen_rect: egui::Rect,
    ) {
        if item_ids.is_empty() {
            return;
        }

        self.overlay.drag_state = Some(crate::palette::SearchResultDragState {
            item_ids: item_ids.clone(),
            outside_overlay: true,
            placement_pending: true,
        });
        let pointer_world = pointer_pos
            .map(|pos| canvas.screen_to_world(pos, screen_rect))
            .unwrap_or_else(|| canvas.screen_to_world(screen_rect.center(), screen_rect));
        canvas.update_search_drag_preview(&item_ids, pointer_world, screen_rect);
    }

    fn cancel_pending_placement(&mut self, canvas: &mut CanvasState) {
        self.overlay.drag_state = None;
        canvas.clear_search_drag_preview();
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

    fn dismiss_image_pivot(&mut self, canvas: &mut CanvasState) {
        self.overlay.image_pivot = None;
        self.overlay.last_submitted_query.clear();
        self.overlay.results.clear();
        self.overlay.result_selection.clear();
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
        self.overlay.results.clear();
        self.overlay.result_selection.clear();
        self.overlay.result_limit = SEARCH_RESULT_PAGE_SIZE;
        self.overlay.has_more_results = false;
        self.submit_query(canvas);
    }

    fn submit_query(&mut self, canvas: &mut CanvasState) {
        let query = self.overlay.query.trim().to_owned();
        let pivot = self.overlay.image_pivot;
        if query.is_empty() && pivot.is_none() {
            self.overlay.results.clear();
            self.overlay.result_selection.clear();
            self.overlay.last_submitted_query.clear();
            self.overlay.has_more_results = false;
            self.overlay.status = SearchOverlayStatus::Ready;
            canvas.clear_search_matches();
            return;
        }

        let Some(service) = &self.service else {
            self.overlay.status =
                SearchOverlayStatus::Error("Semantic search unavailable".to_owned());
            return;
        };

        let search_result = match pivot {
            Some(pivot) => {
                let text = if query.is_empty() {
                    None
                } else {
                    Some(query.as_str())
                };
                service.search_with_image_pivot_with_limit_in_scope(
                    self.overlay.scope,
                    pivot.asset_id,
                    text,
                    self.overlay.result_limit + 1,
                )
            }
            None => service.search_with_limit_in_scope(
                self.overlay.scope,
                &query,
                self.overlay.result_limit + 1,
            ),
        };

        match search_result {
            Ok(mut results) => {
                self.overlay.last_submitted_query = query;
                self.overlay.has_more_results = results.len() > self.overlay.result_limit;
                results.truncate(self.overlay.result_limit);
                self.overlay.results = results;
                self.select_initial_result();
                self.overlay.status = SearchOverlayStatus::Ready;
                self.apply_search_matches(canvas);
            }
            Err(error) if error == "Loading model…" => {
                if !matches!(self.overlay.status, SearchOverlayStatus::Loading(_)) {
                    self.overlay.status =
                        SearchOverlayStatus::Loading("Preparing semantic search…".to_owned());
                }
            }
            Err(error) => {
                self.overlay.status = SearchOverlayStatus::Error(error);
                self.overlay.results.clear();
                self.overlay.result_selection.clear();
                self.overlay.has_more_results = false;
                canvas.clear_search_matches();
            }
        }
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
}

fn result_is_on_bottom_row(index: usize, last_index: usize, columns: usize) -> bool {
    index / columns.max(1) == last_index / columns.max(1)
}

#[cfg(test)]
mod tests {
    use super::{SearchController, SearchSelectionMode, result_is_on_bottom_row};
    use crate::semantic_search::{SearchResultGroup, SearchScope};

    #[test]
    fn search_scope_defaults_to_this_board_and_resets_on_close() {
        let mut search = SearchController::default();
        assert_eq!(search.overlay.scope, SearchScope::ThisBoard);

        search.overlay.scope = SearchScope::Library;
        search.close_overlay(None);

        assert_eq!(search.overlay.scope, SearchScope::ThisBoard);
    }

    #[test]
    fn toggling_search_scope_invalidates_current_results() {
        let mut search = SearchController::default();
        search.overlay.query = "boat".to_owned();
        search.overlay.last_submitted_query = "boat".to_owned();
        search.overlay.results = vec![SearchResultGroup {
            asset_id: 10,
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
    fn initial_result_becomes_range_anchor() {
        let mut search = SearchController::default();
        search.overlay.results = (0..5)
            .map(|index| SearchResultGroup {
                asset_id: index,
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
                asset_original_path: None,
                similarity: 0.9,
                item_ids: vec![101, 102],
                instance_count: 2,
            },
            SearchResultGroup {
                asset_id: 11,
                asset_original_path: None,
                similarity: 0.8,
                item_ids: vec![201],
                instance_count: 1,
            },
        ];
        search.overlay.result_selection.replace(0);
        search.update_result_selection(1, SearchSelectionMode::Toggle);

        search.start_result_drag(1);

        let drag = search.overlay.drag_state.expect("expected search drag");
        assert_eq!(drag.item_ids, vec![101, 201]);
    }

    #[test]
    fn selected_result_item_ids_falls_back_to_lead_result() {
        let mut search = SearchController::default();
        search.overlay.results = vec![
            SearchResultGroup {
                asset_id: 10,
                asset_original_path: None,
                similarity: 0.9,
                item_ids: vec![101],
                instance_count: 1,
            },
            SearchResultGroup {
                asset_id: 11,
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
                asset_original_path: None,
                similarity: 0.9,
                item_ids: vec![101, 102],
                instance_count: 2,
            },
            SearchResultGroup {
                asset_id: 11,
                asset_original_path: None,
                similarity: 0.8,
                item_ids: vec![201],
                instance_count: 1,
            },
        ];

        assert_eq!(search.all_result_item_ids(), vec![101, 201]);
    }
}
