//! Every way images enter the active board: file drops, clipboard, URLs,
//! search results, plus the worker results that resolve them.

use eframe::egui;
use std::time::Instant;

use crate::image_preparation::ImportSource;
use crate::placement::completion::{CompletionContext, PreparationOutcome};
use crate::placement::content_resolution::{
    ContentResolutionRequestConfig, ContentResolutionReservation, LoadedContentResolutionOutcome,
};

use super::SnappyRefApp;

pub(super) const MAX_IMPORT_RESULTS_PER_FRAME: usize = 4;

impl SnappyRefApp {
    pub(super) fn handle_hovered_files(&mut self, ui: &egui::Ui, screen_rect: egui::Rect) {
        let hover_intake = crate::sources::external_drag::hovered(ui);
        let hovered_sources = hover_intake.sources.clone();
        if hovered_sources.is_empty() {
            self.last_hovered_external_drag = None;
            self.canvas
                .delete_placements(&self.external_hover_provisional_placement_ids);
            self.external_hover_provisional_placement_ids.clear();
            self.hover_preview_dimensions.clear();
            return;
        }
        if self.last_hovered_external_drag.as_ref() == Some(&hover_intake)
            && !self.external_hover_provisional_placement_ids.is_empty()
        {
            return;
        }
        self.last_hovered_external_drag = Some(hover_intake.clone());

        let hovered_paths = hovered_sources
            .iter()
            .filter_map(|source| match source {
                ImportSource::Path(path) => Some(path.clone()),
                ImportSource::Url(_) | ImportSource::Clipboard { .. } => None,
            })
            .collect::<Vec<_>>();
        self.hover_preview_dimensions
            .retain(|path, _| hovered_paths.contains(path));

        const FALLBACK_WIDTH: u32 = 360;
        const FALLBACK_HEIGHT: u32 = 240;

        let mut intents = Vec::with_capacity(hovered_sources.len());
        for source in &hovered_sources {
            let dimensions = match source {
                ImportSource::Path(path) => {
                    let dimensions = self
                        .hover_preview_dimensions
                        .entry(path.clone())
                        .or_insert_with(|| image::image_dimensions(path).ok());
                    *dimensions
                }
                ImportSource::Url(_) | ImportSource::Clipboard { .. } => None,
            };
            intents.push(
                crate::placement::creation::ProvisionalSourceIntent::SourceIntent(
                    crate::placement::creation::SourceIntent {
                        dimensions: Some(dimensions.unwrap_or((FALLBACK_WIDTH, FALLBACK_HEIGHT))),
                        approximate_dimensions: dimensions.is_none(),
                        content:
                            crate::placement::content_resolution::PlacementCreation::source_content(
                                source,
                            ),
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

    pub(super) fn handle_dropped_files(&mut self, ui: &egui::Ui, screen_rect: egui::Rect) {
        if self.import_worker.is_none() {
            return;
        }

        let drop_intake = crate::sources::external_drag::take_drop(ui);
        let dropped_sources = drop_intake.sources;
        let drop_screen_pos = drop_intake.screen_pos;
        if dropped_sources.is_empty() {
            return;
        }

        self.last_hovered_external_drag = None;
        let desired_world = self.preferred_import_world_with_pointer(screen_rect, drop_screen_pos);
        self.enqueue_import_sources(dropped_sources, screen_rect, desired_world);
        self.external_hover_provisional_placement_ids.clear();
    }

    pub(super) fn handle_clipboard_paste(&mut self, ctx: &egui::Context, screen_rect: egui::Rect) {
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

    pub(super) fn handle_clipboard_paste_at(
        &mut self,
        screen_rect: egui::Rect,
        desired_world: egui::Pos2,
        paste_text: Option<String>,
    ) {
        use crate::sources::clipboard::{ClipboardContent, read};

        match read(paste_text.as_deref()) {
            Ok(Some(ClipboardContent::BoardSelection(selection))) => {
                if let Err(error) = self.paste_board_payload_at(&selection, desired_world) {
                    self.status = error;
                }
            }
            Ok(Some(ClipboardContent::Sources { sources, pureref })) => {
                if self.import_worker.is_none() {
                    return;
                }
                let source_count = sources.len();
                self.enqueue_import_sources(sources, screen_rect, desired_world);
                if pureref {
                    self.status = if source_count == 1 {
                        "Queued PureRef paste: 1 image".to_owned()
                    } else {
                        format!("Queued PureRef paste: {source_count} images")
                    };
                }
            }
            Ok(None) => {}
            Err(error) => self.status = error,
        }
    }

    pub(super) fn enqueue_import_sources(
        &mut self,
        sources: Vec<ImportSource>,
        screen_rect: egui::Rect,
        desired_world: egui::Pos2,
    ) {
        let config = self.import_request_config();
        let trace = self.import_trace();
        let result = self.placement_creation.accept_sources(
            &mut CompletionContext {
                canvas: &mut self.canvas,
                library: self.library.as_ref(),
                board_id: self.active_board_id,
                worker: self.import_worker.as_ref(),
                settings: &self.settings,
                residency: &mut self.residency,
                editing: &mut self.board_editing,
            },
            sources,
            &self.external_hover_provisional_placement_ids,
            screen_rect,
            desired_world,
            config,
            trace,
        );
        match result {
            Ok(Some(status)) | Err(status) => self.status = status,
            Ok(None) => {}
        }
    }

    /// Creates one loading placement at an exact center and reserves its
    /// content resolution. Used where the caller already knows where the
    /// placement goes (perf scenarios, tests); interactive imports go through
    /// `enqueue_import_sources`, which plans layout for the batch.
    pub(super) fn reserve_import(
        &mut self,
        dimensions: Option<(u32, u32)>,
        center_world: egui::Pos2,
        source: crate::data::ContentResolutionSource,
    ) -> Result<ContentResolutionReservation, String> {
        let Some(library) = &self.library else {
            return Err("Storage unavailable".to_owned());
        };
        let settings = self.placement_creation_settings();
        self.placement_creation.reserve_committed(
            &mut self.canvas,
            library,
            &mut self.board_editing,
            self.active_board_id,
            crate::placement::creation::SourceIntent {
                dimensions,
                approximate_dimensions: dimensions.is_none(),
                content: source,
            },
            center_world,
            settings,
        )
    }

    pub(super) fn remove_import_reservation(&mut self, reservation: ContentResolutionReservation) {
        self.placement_creation.cancel(reservation);
    }

    pub(super) fn handle_import_results(&mut self, ctx: &egui::Context) {
        let Some(import_worker) = &self.import_worker else {
            return;
        };

        let drain_started_at = Instant::now();
        let results = import_worker.try_recv_up_to(MAX_IMPORT_RESULTS_PER_FRAME);
        let drained_count = results.len();
        let hit_result_cap = results.len() == MAX_IMPORT_RESULTS_PER_FRAME;

        for result in results {
            self.accept_preparation_result(ctx, result);
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

    pub(super) fn accept_preparation_result(
        &mut self,
        ctx: &egui::Context,
        result: crate::image_preparation::ImagePrepResult,
    ) {
        let outcome = self.placement_creation.accept_result(
            ctx,
            &mut CompletionContext {
                canvas: &mut self.canvas,
                library: self.library.as_ref(),
                board_id: self.active_board_id,
                worker: self.import_worker.as_ref(),
                settings: &self.settings,
                residency: &mut self.residency,
                editing: &mut self.board_editing,
            },
            result,
        );
        match outcome {
            PreparationOutcome::Loaded(outcome) => self.report_loaded_import(outcome),
            PreparationOutcome::Cropped(cropped) => {
                self.integrate_cropped_import_result(ctx, cropped)
            }
            PreparationOutcome::Status(Some(status)) => self.status = status,
            PreparationOutcome::Status(None) => {}
        }
    }

    pub(super) fn integrate_cropped_import_result(
        &mut self,
        ctx: &egui::Context,
        cropped: crate::image_preparation::PreparedCropPreview,
    ) {
        let ui_integration_started_at = Instant::now();
        match self.board_editing.accept_crop(
            ctx,
            &mut self.canvas,
            self.library.as_ref(),
            self.active_board_id,
            cropped,
        ) {
            Ok(integration) => {
                let ui_integration_ms = ui_integration_started_at.elapsed().as_secs_f64() * 1000.0;
                let mut status = "Cropped image".to_owned();
                if let Some((asset_id, original_path)) = integration.indexed_asset
                    && let Err(error) = self.search.queue_asset_index(asset_id, original_path)
                {
                    status = error;
                }
                if let Some(error) = self.residency.queue_backfill(
                    self.import_worker.as_ref(),
                    &integration.updated_record,
                    &self.settings,
                    crate::data::CanvasImageLod::Working,
                ) {
                    status = error;
                }
                crate::diagnostics::import_completion::log_cropped_timing(
                    self.perf_log.is_some(),
                    &integration.updated_record,
                    ui_integration_ms,
                );
                self.status = status;
            }
            Err(error) => self.status = error,
        }
    }

    pub(super) fn report_loaded_import(&mut self, outcome: LoadedContentResolutionOutcome) {
        crate::diagnostics::import_completion::record_loaded_import(
            self.perf_scenario.as_mut(),
            self.perf_log.as_mut(),
            &outcome,
        );
        let mut status = outcome.status;
        if let Some((asset_id, path)) = outcome.indexed_asset
            && let Err(error) = self.search.queue_asset_index(asset_id, path)
            && status.is_none()
        {
            status = Some(error);
        }
        if let Some(status) = status {
            self.status = status;
        }
    }

    pub(super) fn preferred_import_world(
        &self,
        ctx: &egui::Context,
        screen_rect: egui::Rect,
    ) -> egui::Pos2 {
        let pointer_screen_pos = ctx.input(|input| input.pointer.latest_pos());
        self.preferred_import_world_with_pointer(screen_rect, pointer_screen_pos)
    }

    pub(super) fn import_viewport_world_rect(&self, screen_rect: egui::Rect) -> egui::Rect {
        let visible_screen_rect = self.visible_canvas_screen_rect(screen_rect);
        egui::Rect::from_two_pos(
            self.canvas
                .screen_to_world(visible_screen_rect.min, screen_rect),
            self.canvas
                .screen_to_world(visible_screen_rect.max, screen_rect),
        )
    }

    pub(super) fn preferred_import_world_with_pointer(
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

    pub(super) fn import_request_config(&self) -> ContentResolutionRequestConfig {
        ContentResolutionRequestConfig {
            max_texture_side: self.max_texture_side,
            canvas_image_max_side: self.settings.canvas_image_max_side,
            placement_target_area: self.settings.placement_target_area,
        }
    }

    pub(super) fn import_trace(&self) -> Option<crate::image_preparation::ImagePrepTrace> {
        self.perf_log
            .as_ref()
            .map(|_| crate::image_preparation::ImagePrepTrace {
                queued_at: Instant::now(),
                timings: crate::image_preparation::ImagePrepTimings::default(),
            })
    }

    pub(super) fn placement_creation_settings(
        &self,
    ) -> crate::placement::creation::PlacementCreationSettings {
        crate::placement::creation::PlacementCreationSettings {
            placement_target_area: self.settings.placement_target_area,
            gap: self.settings.placement_gap,
        }
    }

    pub(super) fn handle_search_drop(
        &mut self,
        ctx: &egui::Context,
        request: crate::search::SearchDropRequest,
    ) {
        if request.asset_ids.is_empty() {
            return;
        }
        let result = self.placement_creation.accept_search(
            ctx,
            &mut CompletionContext {
                canvas: &mut self.canvas,
                library: self.library.as_ref(),
                board_id: self.active_board_id,
                worker: self.import_worker.as_ref(),
                settings: &self.settings,
                residency: &mut self.residency,
                editing: &mut self.board_editing,
            },
            crate::placement::lifecycle::CommitSearchProvisionalRequest {
                asset_ids: request.asset_ids,
                provisional_placement_ids: request.provisional_placement_ids,
            },
            self.max_texture_side,
            self.wgpu_render_state.as_ref(),
        );
        match result {
            Ok(count) => {
                if count > 0 {
                    self.save_user_session();
                    self.refresh_board_summaries();
                }
                let noun = if count == 1 { "image" } else { "images" };
                self.status = format!("Added {count} search {noun}");
                self.search.close_overlay();
            }
            Err(error) => self.status = error,
        }
    }
}
