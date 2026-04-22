use std::path::PathBuf;
use std::time::{Duration, Instant};

use eframe::egui;

use crate::canvas::CanvasState;
use crate::palette::{
    ImageSearchPivot, PALETTE_GRID_MAX_HEIGHT, PALETTE_WIDTH, SEARCH_DEBOUNCE_MS,
    SEARCH_OVERLAY_TOP_MARGIN, SearchOverlayState, SearchOverlayStatus, paint_palette_divider,
    paint_palette_empty_state, paint_palette_facet_row, paint_palette_footer,
    paint_palette_input_row, search_result_grid_content_height, search_result_grid_layout,
    search_result_tile,
};
use crate::semantic_search::{
    DEFAULT_SEMANTIC_SEARCH_MODEL, IndexResult as SearchIndexResult, SemanticSearchConfig,
    SemanticSearchService,
};
use crate::storage::Library;

#[derive(Default)]
pub(crate) struct SearchController {
    service: Option<SemanticSearchService>,
    pub(crate) overlay: SearchOverlayState,
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
                max_results: 5,
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
        if self.overlay.query.trim() != query.trim() {
            self.overlay.query = query.to_owned();
            self.overlay.last_query_edit_at = None;
            self.overlay.last_submitted_query.clear();
            self.overlay.results.clear();
            self.overlay.active_result_index = 0;
            self.overlay.active_instance_index = 0;
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
            self.overlay.active_result_index = 0;
            self.overlay.active_instance_index = 0;
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
    ) {
        if !self.overlay.open {
            return;
        }

        if ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
            self.close_overlay(Some(canvas));
            return;
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
        egui::Area::new(egui::Id::new("semantic_search_scrim"))
            .order(egui::Order::Middle)
            .fixed_pos(screen_rect.min)
            .show(ctx, |ui| {
                let painter = ui.painter();
                painter.rect_filled(screen_rect, 0, egui::Color32::from_black_alpha(170));
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
                        );
                        let response = input_result.response;
                        if input_result.pivot_dismissed {
                            self.overlay.image_pivot = None;
                            self.overlay.last_submitted_query.clear();
                            self.overlay.results.clear();
                            self.overlay.active_result_index = 0;
                            self.overlay.active_instance_index = 0;
                            canvas.clear_search_matches();
                            self.overlay.last_query_edit_at = Some(Instant::now());
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
                            self.overlay.active_result_index = 0;
                            self.overlay.active_instance_index = 0;
                            if self.overlay.query.trim().is_empty()
                                && self.overlay.image_pivot.is_none()
                            {
                                self.overlay.results.clear();
                                canvas.clear_search_matches();
                            }
                        }

                        paint_palette_facet_row(ui, result_count);
                        paint_palette_divider(ui);

                        if result_count == 0 {
                            paint_palette_empty_state(ui);
                        } else {
                            let mut clicked_index = None;
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
                                                    let active =
                                                        index == self.overlay.active_result_index;
                                                    let response = search_result_tile(
                                                        ui,
                                                        canvas,
                                                        group,
                                                        active,
                                                        grid_layout.tile_width,
                                                    );
                                                    if response.clicked() {
                                                        clicked_index = Some(index);
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
                            if let Some(index) = clicked_index {
                                self.overlay.active_result_index = index;
                                self.overlay.active_instance_index = 0;
                                self.apply_search_matches(canvas);
                            }
                        }

                        paint_palette_divider(ui);
                        paint_palette_footer(ui, result_count, &self.overlay.status);
                    });
            });

        let overlay_rect = area_response.response.rect;
        let clicked_outside = ctx.input(|input| {
            input.pointer.any_click()
                && input
                    .pointer
                    .interact_pos()
                    .is_some_and(|pointer_pos| !overlay_rect.contains(pointer_pos))
        });
        if clicked_outside {
            self.close_overlay(Some(canvas));
            return;
        }

        if let Some(changed_at) = self.overlay.last_query_edit_at
            && changed_at.elapsed().as_secs_f32() * 1000.0 >= SEARCH_DEBOUNCE_MS
            && self.overlay.query.trim() != self.overlay.last_submitted_query
        {
            self.submit_query(canvas);
        } else if self.overlay.last_query_edit_at.is_some() {
            ctx.request_repaint_after(Duration::from_millis(16));
        }

        if !self.overlay.results.is_empty() {
            let last_index = self.overlay.results.len() - 1;
            if ctx.input(|input| input.key_pressed(egui::Key::ArrowRight)) {
                self.overlay.active_result_index =
                    (self.overlay.active_result_index + 1).min(last_index);
                self.overlay.active_instance_index = 0;
                self.apply_search_matches(canvas);
            } else if ctx.input(|input| input.key_pressed(egui::Key::ArrowLeft)) {
                self.overlay.active_result_index =
                    self.overlay.active_result_index.saturating_sub(1);
                self.overlay.active_instance_index = 0;
                self.apply_search_matches(canvas);
            } else if ctx.input(|input| input.key_pressed(egui::Key::ArrowDown)) {
                self.overlay.active_result_index =
                    (self.overlay.active_result_index + grid_layout.columns).min(last_index);
                self.overlay.active_instance_index = 0;
                self.apply_search_matches(canvas);
            } else if ctx.input(|input| input.key_pressed(egui::Key::ArrowUp)) {
                self.overlay.active_result_index = self
                    .overlay
                    .active_result_index
                    .saturating_sub(grid_layout.columns);
                self.overlay.active_instance_index = 0;
                self.apply_search_matches(canvas);
            }

            if ctx.input(|input| input.key_pressed(egui::Key::Enter)) {
                self.focus_active_search_result(canvas);
            }
        }
    }

    fn close_overlay(&mut self, canvas: Option<&mut CanvasState>) {
        self.overlay = SearchOverlayState::default();
        if let Some(canvas) = canvas {
            canvas.clear_search_matches();
        }
    }

    fn submit_query(&mut self, canvas: &mut CanvasState) {
        let query = self.overlay.query.trim().to_owned();
        let pivot = self.overlay.image_pivot;
        if query.is_empty() && pivot.is_none() {
            self.overlay.results.clear();
            self.overlay.last_submitted_query.clear();
            self.overlay.active_result_index = 0;
            self.overlay.active_instance_index = 0;
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
                service.search_with_image_pivot(pivot.asset_id, text)
            }
            None => service.search(&query),
        };

        match search_result {
            Ok(results) => {
                self.overlay.last_submitted_query = query;
                let last_index = results.len().saturating_sub(1);
                self.overlay.results = results;
                self.overlay.active_result_index = self.overlay.active_result_index.min(last_index);
                let instance_last = self
                    .overlay
                    .results
                    .get(self.overlay.active_result_index)
                    .map(|group| group.item_ids.len().saturating_sub(1))
                    .unwrap_or(0);
                self.overlay.active_instance_index =
                    self.overlay.active_instance_index.min(instance_last);
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
        let active_item_id = self
            .overlay
            .results
            .get(self.overlay.active_result_index)
            .and_then(|group| {
                if group.item_ids.is_empty() {
                    None
                } else {
                    Some(group.item_ids[self.overlay.active_instance_index % group.item_ids.len()])
                }
            });
        canvas.set_search_matches(item_ids, active_item_id);
    }

    fn focus_active_search_result(&mut self, canvas: &mut CanvasState) {
        let Some(group) = self.overlay.results.get(self.overlay.active_result_index) else {
            return;
        };
        if group.item_ids.is_empty() {
            return;
        }

        let item_id = group.item_ids[self.overlay.active_instance_index % group.item_ids.len()];
        canvas.focus_item(item_id);
        self.overlay.active_instance_index =
            (self.overlay.active_instance_index + 1) % group.item_ids.len();
        self.apply_search_matches(canvas);
    }
}
