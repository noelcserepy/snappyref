use std::time::{Duration, Instant};

use eframe::egui;

use super::controller::{
    SearchController, SearchDropRequest, SearchPlacementPreviewConfig, result_is_on_bottom_row,
};
use super::placement::pending_placement_commit_intent;
use super::selection::{PendingResultSelection, SearchSelectionMode};
use crate::canvas::CanvasState;
use crate::search::palette::{
    PALETTE_GRID_MAX_HEIGHT, PALETTE_WIDTH, SEARCH_DEBOUNCE_MS, SEARCH_OVERLAY_TOP_MARGIN,
    SEARCH_RESULT_PAGE_SIZE, SearchOverlayStatus, paint_palette_divider, paint_palette_empty_state,
    paint_palette_footer, paint_palette_input_row, paint_palette_install_state,
    paint_palette_no_results_state, search_result_grid_content_height, search_result_grid_layout,
    search_result_tile,
};
use crate::search::semantic::LocalModelStoreStatus;

const SEARCH_RESULT_SCROLL_ANIMATION: egui::style::ScrollAnimation = egui::style::ScrollAnimation {
    points_per_second: 3200.0,
    duration: egui::Rangef {
        min: 0.04,
        max: 0.16,
    },
};
const SEARCH_RESULT_SCROLL_MARGIN: f32 = 12.0;

impl SearchController {
    pub(crate) fn show_overlay(
        &mut self,
        ctx: &egui::Context,
        screen_rect: egui::Rect,
        canvas: &mut CanvasState,
        status: &mut String,
        placement_preview_config: SearchPlacementPreviewConfig,
    ) -> Option<SearchDropRequest> {
        if !self.overlay.open {
            return None;
        }

        if ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
            if matches!(
                self.local_model_store_status,
                Some(LocalModelStoreStatus::Downloading { .. })
            ) {
                self.cancel_semantic_search_install(status);
            } else if self
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
                    .fill(crate::ui::theme::PALETTE_SURFACE)
                    .stroke(egui::Stroke::new(1.0, egui::Color32::from_white_alpha(33)))
                    .corner_radius(egui::CornerRadius::same(crate::ui::theme::RADIUS_PANEL))
                    .shadow(egui::epaint::Shadow {
                        offset: [0, 12],
                        blur: 40,
                        spread: 0,
                        color: egui::Color32::from_black_alpha(180),
                    })
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);

                        if self.install_surface_visible() {
                            let installing = matches!(
                                self.local_model_store_status,
                                Some(LocalModelStoreStatus::Downloading { .. })
                            );
                            let response = paint_palette_install_state(
                                ui,
                                &self.overlay.status,
                                &mut self.overlay.install_action_focus_requested,
                                installing,
                            );
                            if response.clicked() {
                                self.start_semantic_search_install(status);
                                ctx.request_repaint_after(Duration::from_millis(16));
                            }
                            return;
                        }

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
                                                crate::ui::theme::SPACE_2,
                                                crate::ui::theme::SPACE_2,
                                            );
                                            for (row_index, row_groups) in self
                                                .overlay
                                                .results
                                                .chunks(grid_layout.columns.max(1))
                                                .enumerate()
                                            {
                                                ui.horizontal(|ui| {
                                                    ui.spacing_mut().item_spacing.x =
                                                        crate::ui::theme::SPACE_2;
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
                                    self.start_result_drag(index);
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
                                self.toggle_scope();
                                self.submit_query(canvas);
                                ctx.request_repaint();
                            }
                        }
                    });
            });

        let overlay_rect = area_response.response.rect;
        if !drag_outside {
            self.overlay.last_expanded_overlay_rect = Some(overlay_rect);
        }
        self.prepare_canvas_drag_preview(ctx, canvas, screen_rect, placement_preview_config);
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
        } else if ctx.input(|input| {
            pending_placement_commit_intent(input.pointer.primary_clicked(), input.modifiers)
        }) && self
            .overlay
            .drag_state
            .as_ref()
            .is_some_and(|drag| drag.placement_pending)
            && let Some(drag) = self.overlay.drag_state.take()
            && let Some(screen_pos) = pointer_pos
        {
            drop_request = Some(SearchDropRequest {
                placement_ids: drag.placement_ids,
                asset_ids: drag.asset_ids,
                provisional_placement_ids: canvas
                    .provisional_records_in_selection_order()
                    .into_iter()
                    .map(|record| record.state.id)
                    .collect(),
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
                    placement_ids: drag.placement_ids,
                    asset_ids: drag.asset_ids,
                    provisional_placement_ids: canvas
                        .provisional_records_in_selection_order()
                        .into_iter()
                        .map(|record| record.state.id)
                        .collect(),
                    screen_pos,
                });
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
                        ctx,
                        canvas,
                        pointer_pos,
                        screen_rect,
                        placement_preview_config,
                    );
                } else {
                    self.start_keyboard_placement_for_selected_results(
                        ctx,
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
}
