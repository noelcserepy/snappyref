//! Every way images enter the active board: file drops, clipboard, URLs,
//! search results, plus the worker results that resolve them.

use eframe::egui;
use std::time::Instant;

use crate::active_board::history::{HistoryAction, HistoryChange};
use crate::jobs::ImportSource;
use crate::library::residency::ImagePrepResultResidencyOutcome;
use crate::placement::content_resolution::{
    ContentResolutionRequestConfig, ContentResolutionReservation, FailedContentResolutionOutcome,
    LoadedContentResolutionOutcome,
};
use crate::sources::crop;

use super::SnappyRefApp;

pub(super) const MAX_IMPORT_RESULTS_PER_FRAME: usize = 4;

impl SnappyRefApp {
    pub(super) fn handle_hovered_files(&mut self, ui: &egui::Ui, screen_rect: egui::Rect) {
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

    #[cfg(test)]
    pub(super) fn dropped_import_previews(
        &mut self,
        sources: &[ImportSource],
        desired_world: egui::Pos2,
        screen_rect: egui::Rect,
    ) -> Vec<egui::Pos2> {
        let specs = sources
            .iter()
            .map(Self::source_intent_for_external_source)
            .collect::<Vec<_>>();

        crate::placement::creation::plan_source_intent_geometries(
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
        .map(|geometry| geometry.center)
        .collect()
    }

    pub(super) fn source_intent_for_external_source(
        source: &ImportSource,
    ) -> crate::placement::creation::SourceIntent {
        let dimensions = match source {
            ImportSource::Path(path) => image::image_dimensions(path).ok(),
            ImportSource::Url(_) => None,
            ImportSource::Clipboard { width, height, .. } => Some((*width, *height)),
        };
        crate::placement::creation::SourceIntent {
            dimensions,
            approximate_dimensions: dimensions.is_none(),
            content: Self::placement_content_intent_for_external_source(source),
        }
    }

    pub(super) fn handle_dropped_files(&mut self, ui: &egui::Ui, screen_rect: egui::Rect) {
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
        self.enqueue_import_sources(dropped_sources, screen_rect, desired_world);
        self.external_hover_provisional_placement_ids.clear();
    }

    pub(super) fn take_missing_drop_event_fallback(
        &mut self,
    ) -> Option<crate::sources::external_drag::ExternalDragIntake> {
        self.pending_missing_drop_fallback
            .take()
            .filter(|fallback| !fallback.sources.is_empty())
    }

    pub(super) fn register_import_batch(
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

    pub(super) fn finish_import_queue_id_in_batches(
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

    pub(super) fn finalize_completed_import_batch(
        &mut self,
        target_board_id: i64,
        anchor_world: egui::Pos2,
        placement_ids: Vec<i64>,
    ) {
        if target_board_id != self.active_board_id {
            return;
        }
        let placement_ids = placement_ids
            .into_iter()
            .filter(|placement_id| self.canvas.placement_for_id(*placement_id).is_some())
            .collect::<Vec<_>>();
        if placement_ids.is_empty() {
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

    pub(super) fn handle_clipboard_path_import_at(
        &mut self,
        screen_rect: egui::Rect,
        desired_world: egui::Pos2,
    ) -> bool {
        if self.import_worker.is_none() {
            return false;
        }
        let sources = crate::platform::native_drop::clipboard_paths()
            .into_iter()
            .map(ImportSource::Path)
            .collect::<Vec<_>>();
        if sources.is_empty() {
            return false;
        }

        self.enqueue_import_sources(sources, screen_rect, desired_world);
        true
    }

    pub(super) fn handle_pureref_clipboard_import_at(
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
        self.enqueue_import_sources(sources, screen_rect, desired_world);
        self.status = if source_count == 1 {
            "Queued PureRef paste: 1 image".to_owned()
        } else {
            format!("Queued PureRef paste: {source_count} images")
        };
        true
    }

    pub(super) fn handle_external_clipboard_image_paste_at(
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

        let source = ImportSource::Clipboard {
            rgba: clipboard_image.bytes.into_owned(),
            width: clipboard_image.width as u32,
            height: clipboard_image.height as u32,
        };
        self.enqueue_import_sources(vec![source], screen_rect, desired_world);
    }

    pub(super) fn handle_clipboard_url_import_at(
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
        self.enqueue_import_sources(vec![ImportSource::Url(url)], screen_rect, desired_world);
        true
    }

    pub(super) fn enqueue_import_sources(
        &mut self,
        sources: Vec<ImportSource>,
        screen_rect: egui::Rect,
        desired_world: egui::Pos2,
    ) {
        if self.import_worker.is_none() || sources.is_empty() {
            return;
        }

        let viewport_world = self.import_viewport_world_rect(screen_rect);
        let placement_creation_settings = self.placement_creation_settings();
        let (created, history_action) = match self.library.clone().map_or_else(
            || Err("Storage unavailable".to_owned()),
            |library| {
                if self.external_hover_provisional_placement_ids.len() == sources.len() {
                    let committed = crate::placement::lifecycle::commit_loading_provisional_batch(
                        &library,
                        &mut self.canvas,
                        &self.external_hover_provisional_placement_ids,
                    );
                    let mut created = Vec::with_capacity(committed.placements.len());
                    for placement in committed.placements {
                        let reservation = self.content_resolution.reserve_placement(
                            self.active_board_id,
                            placement.state.id,
                            placement.state.geometry.center,
                        );
                        created.push(crate::placement::creation::CreatedLoadingPlacement {
                            reservation,
                            placement,
                        });
                    }
                    return Ok((created, committed.history_action));
                }

                let source_intents = sources
                    .iter()
                    .map(Self::source_intent_for_external_source)
                    .collect::<Vec<_>>();
                let batch = crate::placement::creation::create_committed_loading_placements(
                    &library,
                    &mut self.content_resolution,
                    &mut self.canvas,
                    self.active_board_id,
                    source_intents,
                    desired_world,
                    screen_rect,
                    placement_creation_settings,
                )?;
                Ok((batch.created, batch.history_action))
            },
        ) {
            Ok(batch) => batch,
            Err(error) => {
                self.status = error;
                return;
            }
        };
        if let Some(history_action) = history_action {
            self.document_history
                .record_applied(self.active_board_id, history_action);
        }
        self.persist_recorded_canvas_changes();

        let mut queued_batch_ids = Vec::new();
        let mut queued_placement_ids = Vec::new();

        for (source, created) in sources.into_iter().zip(created) {
            let reservation = created.reservation;
            let status = match &source {
                ImportSource::Path(path) => format!("Queued import: {}", path.display()),
                ImportSource::Url(url) => format!("Queued URL import: {url}"),
                ImportSource::Clipboard { .. } => "Queued clipboard image".to_owned(),
            };
            let request = crate::placement::content_resolution::import_request(
                source,
                reservation,
                desired_world,
                viewport_world,
                self.import_request_config(),
                self.import_trace(),
            );

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
            self.status = status;
        }
        if !queued_placement_ids.is_empty() {
            self.canvas.select_placements(&queued_placement_ids);
        }
        self.register_import_batch(self.active_board_id, desired_world, queued_batch_ids);
    }

    pub(super) fn placement_content_intent_for_external_source(
        source: &ImportSource,
    ) -> crate::data::ContentResolutionSource {
        match source {
            ImportSource::Path(path) => crate::data::ContentResolutionSource::ExternalPath {
                path: path.to_string_lossy().into_owned(),
            },
            ImportSource::Url(url) => {
                crate::data::ContentResolutionSource::ExternalUrl { url: url.clone() }
            }
            ImportSource::Clipboard { .. } => crate::data::ContentResolutionSource::Clipboard,
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
        let (created, history_action) =
            crate::placement::creation::create_committed_loading_placement(
                library,
                &mut self.content_resolution,
                &mut self.canvas,
                self.active_board_id,
                crate::placement::creation::SourceIntent {
                    dimensions,
                    approximate_dimensions: dimensions.is_none(),
                    content: source,
                },
                center_world,
                settings,
            )?;
        if let Some(history_action) = history_action {
            self.document_history
                .record_applied(self.active_board_id, history_action);
        }
        self.persist_recorded_canvas_changes();
        Ok(created.reservation)
    }

    pub(super) fn remove_import_reservation(&mut self, reservation: ContentResolutionReservation) {
        self.content_resolution.cancel(reservation);
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
            if self.discard_stale_content_result(&result) {
                continue;
            }
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

    pub(super) fn discard_stale_content_result(
        &mut self,
        result: &crate::jobs::ImagePrepResult,
    ) -> bool {
        let (queue_id, target_board_id, placement_id, loaded_result) = match result {
            crate::jobs::ImagePrepResult::Loaded(loaded) => {
                let Some(queue_id) = loaded.queue_id else {
                    return false;
                };
                (
                    queue_id,
                    loaded.target_board_id,
                    loaded.placement.state.id,
                    true,
                )
            }
            crate::jobs::ImagePrepResult::Failed {
                queue_id: Some(queue_id),
                target_board_id: Some(target_board_id),
                placement_id: Some(placement_id),
                ..
            } => (*queue_id, *target_board_id, *placement_id, false),
            _ => return false,
        };
        let Some(stale) = self
            .content_resolution
            .placement_is_stale(queue_id, placement_id)
        else {
            return false;
        };
        if !stale
            && (target_board_id != self.active_board_id
                || self.canvas.placement_for_id(placement_id).is_some())
        {
            return false;
        }
        let completion = self.content_resolution.resolve_completion(
            Some(queue_id),
            Some(target_board_id),
            Some(placement_id),
        );
        self.finish_import_queue_id_in_batches(
            completion.target_board_id,
            completion.queue_id,
            None,
        );
        if !loaded_result {
            self.residency
                .note_failed_placement(&mut self.canvas, Some(placement_id));
        }
        true
    }

    pub(super) fn apply_import_result_residency_outcome(
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

    pub(super) fn integrate_cropped_import_result(
        &mut self,
        ctx: &egui::Context,
        cropped: crate::jobs::PreparedCropPreview,
    ) {
        let ui_integration_started_at = Instant::now();
        match crop::integrate_cropped_result(
            ctx,
            &mut self.canvas,
            self.library.as_ref(),
            cropped,
            self.settings.semantic_search_enabled,
        ) {
            Ok(integration) => {
                let ui_integration_ms = ui_integration_started_at.elapsed().as_secs_f64() * 1000.0;
                self.document_history.record_applied(
                    self.active_board_id,
                    HistoryAction::document(
                        "Crop image".to_owned(),
                        vec![HistoryChange {
                            before: Some(integration.previous_record.clone()),
                            after: Some(integration.updated_record.clone()),
                        }],
                        Vec::new(),
                        None,
                    ),
                );
                self.persist_recorded_canvas_changes();
                let mut status = "Cropped image".to_owned();
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

    pub(super) fn integrate_loaded_import(
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

    pub(super) fn apply_loaded_import_outcome(&mut self, outcome: LoadedContentResolutionOutcome) {
        if let Some(scenario) = &mut self.perf_scenario
            && scenario.name == "startup"
            && let Some(log) = &outcome.log
            && log.origin == crate::jobs::ImportOrigin::Existing
            && scenario
                .startup_loaded_placement_ids
                .insert(log.placement.id())
        {
            // Count Placements once, including when several LOD results land.
            scenario.loaded_imports = scenario.startup_loaded_placement_ids.len();
            if !scenario.first_loaded_logged {
                scenario.first_loaded_logged = true;
                scenario.emit("first_placement_loaded", serde_json::json!({}));
            }
            if scenario.loaded_imports == scenario.expected_imports {
                scenario.all_loaded_logged = true;
                scenario.emit("all_placements_loaded", serde_json::json!({}));
            }
        }
        let (completion, outcome) = self.content_resolution.finish_loaded_import(outcome);
        if let (Some(ticket), Some(log)) = (outcome.resolution_ticket, outcome.log.as_ref()) {
            let mut placement = log.placement.clone();
            // An inactive Board has no canvas; its accepted history holds the
            // newest geometry.
            if ticket.target_board_id != self.active_board_id
                && let Some(accepted) = self
                    .document_history
                    .accepted_placement(ticket.target_board_id, placement.state.id)
            {
                placement.adopt_geometry_of(&accepted);
            }
            if self
                .document_history
                .update_accepted_placement(ticket.target_board_id, placement.clone())
            {
                let changes = crate::canvas::CanvasDocumentChanges {
                    updated_placements: vec![placement.state],
                    ..Default::default()
                };
                if let Err(error) = self.document_persistence.enqueue_document_unit(
                    ticket.target_board_id,
                    changes,
                    &self.document_history,
                ) {
                    self.status = error;
                }
            }
        }
        let resolution_queue_id = outcome.resolution_ticket.map(|ticket| ticket.queue_id);
        let perf_import_origin = outcome.log.as_ref().map(|log| log.origin);
        let perf_counts_as_import = perf_import_origin.is_some_and(|origin| {
            matches!(
                origin,
                crate::jobs::ImportOrigin::Path
                    | crate::jobs::ImportOrigin::Clipboard
                    | crate::jobs::ImportOrigin::Url
            )
        });
        let status = outcome.status;
        let mut status = status;
        let indexed_asset = outcome.indexed_asset;
        self.finish_import_queue_id_in_batches(
            completion.target_board_id,
            completion.queue_id,
            completion.placement_id,
        );
        if let Some(log) = outcome.log {
            if perf_counts_as_import && let Some(scenario) = &mut self.perf_scenario {
                scenario.loaded_imports += 1;
                let metadata = resolution_queue_id
                    .and_then(|queue_id| scenario.asset_by_queue_id.get(&queue_id).cloned());
                if let Some(metadata) = &metadata {
                    scenario
                        .asset_by_asset_id
                        .insert(log.placement.require_loaded_asset().id, metadata.clone());
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
                        "asset_id": log.placement.require_loaded_asset().id,
                        "placement_id": log.placement.state.id,
                        "mime_type": log.placement.require_loaded_asset().mime_type,
                        "filename": log.placement.require_loaded_asset().filename,
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

    pub(super) fn apply_failed_import_outcome(&mut self, outcome: FailedContentResolutionOutcome) {
        let completion = self.content_resolution.finish_failed_import(&outcome);
        if let (Some(board_id), Some(placement_id)) =
            (completion.target_board_id, completion.placement_id)
            && let Some(placement) = self.document_history.fail_accepted_placement(
                board_id,
                placement_id,
                outcome.status.clone(),
            )
        {
            if board_id == self.active_board_id
                && self.canvas.placement_for_id(placement_id).is_some()
            {
                self.canvas.add_unresolved_placement(placement.clone());
                self.canvas.select_placements(&[placement_id]);
            }
            let changes = crate::canvas::CanvasDocumentChanges {
                updated_placements: vec![placement.state],
                ..Default::default()
            };
            if let Err(error) = self.document_persistence.enqueue_document_unit(
                board_id,
                changes,
                &self.document_history,
            ) {
                self.status = error;
            }
        }
        self.finish_import_queue_id_in_batches(
            completion.target_board_id,
            completion.queue_id,
            None,
        );
        self.status = outcome.status;
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
            auto_image_scaling: true,
            placement_target_area: self.settings.placement_target_area,
            placement_gap: self.settings.placement_gap,
        }
    }

    pub(super) fn import_trace(&self) -> Option<crate::jobs::ImagePrepTrace> {
        self.perf_log.as_ref().map(|_| crate::jobs::ImagePrepTrace {
            queued_at: Instant::now(),
            timings: crate::jobs::ImagePrepTimings::default(),
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
            "handle_search_drop committed count={}",
            committed.placements.len()
        ));

        for placement in &committed.placements {
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

        if !committed.placements.is_empty() {
            if let Some(history_action) = committed.history_action {
                self.document_history
                    .record_applied(self.active_board_id, history_action);
                self.persist_recorded_canvas_changes();
            }
            self.save_user_session();
            self.refresh_board_summaries();
        }
        let placed_count = committed.placements.len();
        let noun = if placed_count == 1 { "image" } else { "images" };
        self.status = format!("Added {placed_count} search {noun}");
        crate::diagnostics::search_drag_debug(format!(
            "handle_search_drop done status={}",
            self.status
        ));
        self.search.close_overlay(Some(&mut self.canvas));
    }
}
