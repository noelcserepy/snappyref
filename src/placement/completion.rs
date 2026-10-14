//! Preparation completion owns correlation, accepted content, and final batch layout.
use super::content_resolution::{
    ContentResolutionTicket, LoadedContentResolutionOutcome, PlacementCreation, ResolvedPlacement,
};
use crate::canvas::{CanvasState, residency::ImageResidencyCoordinator};
use crate::data::WorkspaceSettings;
use crate::image_preparation::{
    ImagePrepResult, ImagePrepWorker, PreparedCropPreview, PreparedPlacementImage,
};
use eframe::egui;
use std::time::Instant;

pub(crate) struct CompletionContext<'a> {
    pub(crate) canvas: &'a mut CanvasState,
    pub(crate) library: Option<&'a crate::library::storage::Library>,
    pub(crate) board_id: i64,
    pub(crate) worker: Option<&'a ImagePrepWorker>,
    pub(crate) settings: &'a WorkspaceSettings,
    pub(crate) residency: &'a mut ImageResidencyCoordinator,
    pub(crate) editing: &'a mut crate::active_board::editing::BoardEditing,
}
pub(crate) enum PreparationOutcome {
    Loaded(LoadedContentResolutionOutcome),
    Cropped(PreparedCropPreview),
    Status(Option<String>),
}
impl PlacementCreation {
    pub(crate) fn accept_result(
        &mut self,
        ctx: &egui::Context,
        context: &mut CompletionContext<'_>,
        result: ImagePrepResult,
    ) -> PreparationOutcome {
        if self.reject_stale(context, &result) {
            return PreparationOutcome::Status(None);
        }
        match result {
            ImagePrepResult::Loaded(loaded) => match self.accept_loaded(ctx, context, *loaded) {
                Ok(outcome) => PreparationOutcome::Loaded(outcome),
                Err(error) => PreparationOutcome::Status(Some(error)),
            },
            ImagePrepResult::Cropped(cropped) => PreparationOutcome::Cropped(cropped),
            ImagePrepResult::CacheLodBackfilled { asset_id, lod } => {
                context.residency.note_backfill_completed(asset_id, lod);
                PreparationOutcome::Status(None)
            }
            ImagePrepResult::CacheLodBackfillFailed {
                asset_id,
                lod,
                error,
            } => {
                context.residency.note_backfill_completed(asset_id, lod);
                PreparationOutcome::Status(Some(error))
            }
            ImagePrepResult::ExistingLoadFailed {
                asset_id,
                target_board_id,
                queue_id,
                placement_id,
                lod,
                context: label,
                error,
            } => {
                let status = format!("{label}: {error}");
                let completion =
                    self.resolve_completion(queue_id, Some(target_board_id), Some(placement_id));
                if target_board_id == context.board_id
                    && context
                        .canvas
                        .placement_for_id(placement_id)
                        .is_some_and(|p| p.asset_id() == asset_id)
                {
                    context.canvas.note_image_lod_load_failed(placement_id, lod);
                }
                let result = self.finish_batch(
                    context,
                    completion.target_board_id,
                    completion.queue_id,
                    None,
                );
                PreparationOutcome::Status(Some(result.err().unwrap_or(status)))
            }
            ImagePrepResult::Failed {
                target_board_id,
                queue_id,
                placement_id,
                context: label,
                error,
            } => {
                let status = format!("{label}: {error}");
                let completion = self.resolve_completion(queue_id, target_board_id, placement_id);
                if completion
                    .target_board_id
                    .is_none_or(|id| id == context.board_id)
                    && let Some(id) = placement_id
                {
                    context.canvas.clear_image_lod_load(id);
                    context.canvas.clear_crop_pending(id);
                }
                let result = (|| {
                    if let (Some(board_id), Some(id)) =
                        (completion.target_board_id, completion.placement_id)
                        // Pixel preparation cannot replace an already accepted image.
                        && context
                            .editing
                            .accepted_placement(board_id, id)
                            .is_some_and(|placement| !placement.is_loaded())
                        && let Some(placement) =
                            context
                                .editing
                                .fail_placement(board_id, id, status.clone())?
                        && board_id == context.board_id
                        && context.canvas.placement_for_id(id).is_some()
                    {
                        context.canvas.add_unresolved_placement(placement);
                        context.canvas.select_placements(&[id]);
                    }
                    self.finish_batch(
                        context,
                        completion.target_board_id,
                        completion.queue_id,
                        None,
                    )
                })();
                PreparationOutcome::Status(Some(result.err().unwrap_or(status)))
            }
        }
    }
    fn reject_stale(
        &mut self,
        context: &mut CompletionContext<'_>,
        result: &ImagePrepResult,
    ) -> bool {
        let (queue_id, board_id, id) = match result {
            ImagePrepResult::Loaded(loaded) => {
                let Some(queue_id) = loaded.queue_id else {
                    return false;
                };
                (queue_id, loaded.target_board_id, loaded.placement.id())
            }
            ImagePrepResult::ExistingLoadFailed {
                queue_id: Some(q),
                target_board_id: b,
                placement_id: id,
                ..
            } => (*q, *b, *id),
            ImagePrepResult::Failed {
                queue_id: Some(q),
                target_board_id: Some(b),
                placement_id: Some(id),
                ..
            } => (*q, *b, *id),
            _ => return false,
        };
        let Some(stale) = self.placement_is_stale(queue_id, id) else {
            return self.reservation_finished(queue_id);
        };
        if !stale && (board_id != context.board_id || context.canvas.placement_for_id(id).is_some())
        {
            return false;
        }
        let completion = self.resolve_completion(Some(queue_id), Some(board_id), Some(id));
        let _ = self.finish_batch(
            context,
            completion.target_board_id,
            completion.queue_id,
            None,
        );
        // Obsolete existing loads do not own the current canvas LOD request.
        if board_id == context.board_id
            && !matches!(result, ImagePrepResult::ExistingLoadFailed { .. })
        {
            context.canvas.clear_image_lod_load(id);
        }
        true
    }
    fn accept_loaded(
        &mut self,
        ctx: &egui::Context,
        context: &mut CompletionContext<'_>,
        mut loaded: PreparedPlacementImage,
    ) -> Result<LoadedContentResolutionOutcome, String> {
        let ticket = loaded.queue_id.map(|queue_id| ContentResolutionTicket {
            target_board_id: loaded.target_board_id,
            queue_id,
        });
        let board_id = loaded.target_board_id;
        let current = if board_id == context.board_id {
            context.canvas.placement_for_id(loaded.placement.id())
        } else {
            context
                .editing
                .accepted_placement(board_id, loaded.placement.id())
        };
        if current
            .as_ref()
            .is_some_and(|p| p.is_loaded() && p.asset_id() != loaded.placement.asset_id())
        {
            let completion = self.resolve_completion(
                loaded.queue_id,
                Some(board_id),
                Some(loaded.placement.id()),
            );
            self.finish_batch(
                context,
                completion.target_board_id,
                completion.queue_id,
                None,
            )?;
            return Ok(LoadedContentResolutionOutcome {
                resolution_ticket: ticket,
                status: None,
                indexed_asset: None,
                resolved: None,
            });
        }
        if let Some(current) = current {
            loaded.placement.adopt_geometry_of(&current);
        }
        let placement = loaded.placement.clone();
        let indexed_asset = (ticket.is_some()
            && crate::canvas::residency::asset_mime_type_supports_raster_derivatives(
                &placement.require_loaded_asset().mime_type,
            ))
        .then(|| {
            (
                placement.require_loaded_asset().id,
                placement.require_loaded_asset().original_path.clone(),
            )
        });
        let origin = loaded.origin;
        let diagnostics = loaded.existing_load_diagnostics.clone();
        let mut trace = loaded.trace;
        if board_id == context.board_id {
            let lod = loaded.loaded_lod;
            let started = Instant::now();
            context.canvas.add_prepared_placement_image(ctx, loaded);
            if let Some(trace) = &mut trace {
                trace.timings.ui_integration = started.elapsed();
            }
            let _ = context
                .residency
                .queue_overview_backfill_after_working_load(
                    context.worker,
                    &placement,
                    context.settings,
                    lod,
                );
        }
        let (completion, outcome) = self.finish_loaded_import(LoadedContentResolutionOutcome {
            resolution_ticket: ticket,
            status: ticket.map(|_| "Imported image".to_owned()),
            indexed_asset,
            resolved: Some(ResolvedPlacement {
                origin,
                placement: placement.clone(),
                diagnostics,
                trace,
            }),
        });
        if ticket.is_some()
            && context
                .editing
                .accepted_placement(board_id, placement.id())
                .is_some_and(|accepted| !accepted.state.archived)
        {
            context
                .editing
                .resolve_placements(board_id, vec![placement])?;
        }
        self.finish_batch(
            context,
            completion.target_board_id,
            completion.queue_id,
            completion.placement_id,
        )?;
        Ok(outcome)
    }
    pub(crate) fn finish_batch(
        &mut self,
        context: &mut CompletionContext<'_>,
        board_id: Option<i64>,
        queue_id: Option<u64>,
        id: Option<i64>,
    ) -> Result<(), String> {
        if queue_id.is_some()
            && let (Some(board_id), Some(id)) = (board_id, id)
        {
            let mut inactive = CanvasState::default();
            let canvas = if board_id == context.board_id {
                &mut *context.canvas
            } else {
                if let Some(library) = context.library {
                    let document = context.editing.accepted_board_document(library, board_id)?;
                    for placement in document.placements {
                        inactive.add_unresolved_placement(placement);
                    }
                    for annotation in document.annotations {
                        inactive.add_annotation(annotation);
                    }
                    inactive.restore_groups(document.groups);
                    inactive.restore_canvas_stack_order(document.stack_order);
                }
                &mut inactive
            };
            if canvas.group_for_member(id).is_some()
                && canvas.placement_for_id(id).is_some_and(|p| p.is_loaded())
            {
                canvas.finish_group_arrival(id)?;
                context.editing.resolve_group_arrival(
                    board_id,
                    id,
                    canvas.take_document_changes(),
                )?;
            }
        }
        for batch in self.batches.finish_queue_id(board_id, queue_id, id) {
            let mut inactive_canvas = CanvasState::default();
            let canvas = if batch.target_board_id == context.board_id {
                &mut *context.canvas
            } else {
                let Some(library) = context.library else {
                    continue;
                };
                let document = context
                    .editing
                    .accepted_board_document(library, batch.target_board_id)?;
                for placement in document.placements {
                    inactive_canvas.add_unresolved_placement(placement);
                }
                for annotation in document.annotations {
                    inactive_canvas.add_annotation(annotation);
                }
                inactive_canvas.restore_groups(document.groups);
                inactive_canvas.restore_canvas_stack_order(document.stack_order);
                &mut inactive_canvas
            };
            let ids = batch
                .placement_ids
                .into_iter()
                .filter(|id| {
                    canvas.placement_for_id(*id).is_some() && canvas.group_for_member(*id).is_none()
                })
                .collect::<Vec<_>>();
            if ids.is_empty() {
                continue;
            }
            canvas.select_placements(&ids);
            canvas.arrange_selected_compact_preserving_sizes(context.settings.placement_gap);
            context
                .editing
                .resolve_document_changes(batch.target_board_id, canvas.take_document_changes())?;
        }
        Ok(())
    }
}

impl PlacementCreation {
    pub(crate) fn source_intent(
        source: &crate::image_preparation::ImportSource,
    ) -> super::creation::SourceIntent {
        use crate::image_preparation::ImportSource;
        let dimensions = match source {
            ImportSource::Path(path) => image::image_dimensions(path).ok(),
            ImportSource::Url(_) => None,
            ImportSource::Clipboard { width, height, .. } => Some((*width, *height)),
        };
        super::creation::SourceIntent {
            dimensions,
            approximate_dimensions: dimensions.is_none(),
            content: Self::source_content(source),
        }
    }
    pub(crate) fn source_content(
        source: &crate::image_preparation::ImportSource,
    ) -> crate::data::ContentResolutionSource {
        use crate::image_preparation::ImportSource;
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
    pub(crate) fn accept_sources(
        &mut self,
        context: &mut CompletionContext<'_>,
        sources: Vec<crate::image_preparation::ImportSource>,
        provisional_ids: &[i64],
        screen_rect: egui::Rect,
        anchor: egui::Pos2,
        config: super::content_resolution::ContentResolutionRequestConfig,
        trace: Option<crate::image_preparation::ImagePrepTrace>,
        destination: Option<(i64, i64)>,
    ) -> Result<Option<String>, String> {
        let Some(worker) = context.worker else {
            return Ok(None);
        };
        if sources.is_empty() {
            return Ok(None);
        }
        let library = context
            .library
            .ok_or_else(|| "Storage unavailable".to_owned())?;
        let settings = super::creation::PlacementCreationSettings {
            placement_target_area: context.settings.placement_target_area,
            gap: context.settings.placement_gap,
        };
        let before_groups = context.canvas.groups().to_vec();
        let before_members = if destination.is_some() {
            context.canvas.committed_placements()
        } else {
            Vec::new()
        };
        let before_annotations = if destination.is_some() {
            context
                .canvas
                .all_annotation_ids()
                .into_iter()
                .filter_map(|id| context.canvas.annotation_for_id(id))
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        let (created, mut action) = if provisional_ids.len() == sources.len() {
            let committed = super::lifecycle::commit_loading_provisional_batch(
                library,
                context.canvas,
                provisional_ids,
            );
            let created = committed
                .placements
                .into_iter()
                .map(|placement| {
                    let reservation = self.reserve_placement(
                        context.board_id,
                        placement.id(),
                        placement.state.geometry.center,
                    );
                    super::creation::CreatedLoadingPlacement {
                        reservation,
                        placement,
                    }
                })
                .collect::<Vec<_>>();
            (created, committed.history_action)
        } else {
            let batch = super::creation::create_committed_loading_placements(
                library,
                self,
                context.canvas,
                context.board_id,
                sources.iter().map(Self::source_intent).collect(),
                anchor,
                screen_rect,
                settings,
            );
            (batch.created, batch.history_action)
        };
        if destination.is_some() {
            let members = created
                .iter()
                .map(|item| (item.placement.id(), None))
                .collect::<Vec<_>>();
            context.canvas.paste_group_members(destination, &members)?;
            if let Some(crate::active_board::history::HistoryAction::Document(action)) = &mut action
            {
                use crate::active_board::history::HistoryChange;
                action.groups = Some(HistoryChange {
                    before: Some(before_groups),
                    after: Some(context.canvas.groups().to_vec()),
                });
                for change in &mut action.placements {
                    if let Some(after) = &mut change.after
                        && let Some(current) = context.canvas.placement_for_id(after.id())
                    {
                        *after = current;
                    }
                }
                for before in before_members {
                    if let Some(after) = context.canvas.placement_for_id(before.id())
                        && before.state.geometry != after.state.geometry
                    {
                        action.placements.push(HistoryChange {
                            before: Some(before),
                            after: Some(after),
                        });
                    }
                }
                for before in before_annotations {
                    if let Some(after) = context.canvas.annotation_for_id(before.id)
                        && before != after
                    {
                        action.annotations.push(HistoryChange {
                            before: Some(before),
                            after: Some(after),
                        });
                    }
                }
            }
        }
        context
            .editing
            .accept_created_batch(context.canvas, context.board_id, action)?;
        let mut queue_ids = Vec::new();
        let mut placement_ids = Vec::new();
        let mut status = None;
        for (source, created) in sources.into_iter().zip(created) {
            use crate::image_preparation::ImportSource;
            let message = match &source {
                ImportSource::Path(path) => format!("Queued import: {}", path.display()),
                ImportSource::Url(url) => format!("Queued URL import: {url}"),
                ImportSource::Clipboard { .. } => "Queued clipboard image".to_owned(),
            };
            let reservation = created.reservation;
            let request =
                super::content_resolution::import_request(source, reservation, config, trace);
            if let Err(error) = worker.enqueue(request) {
                self.cancel(reservation);
                status = Some(error);
                continue;
            }
            queue_ids.push(reservation.queue_id);
            placement_ids.push(created.placement.id());
            status = Some(message);
        }
        if !placement_ids.is_empty() {
            context.canvas.select_placements(&placement_ids);
        }
        self.batches
            .register_multi_item_batch(context.board_id, queue_ids);
        Ok(status)
    }
    pub(crate) fn reserve_committed(
        &mut self,
        canvas: &mut CanvasState,
        library: &crate::library::storage::Library,
        editing: &mut crate::active_board::editing::BoardEditing,
        board_id: i64,
        intent: super::creation::SourceIntent,
        center: egui::Pos2,
        settings: super::creation::PlacementCreationSettings,
    ) -> Result<super::content_resolution::ContentResolutionReservation, String> {
        let (created, action) = super::creation::create_committed_loading_placement(
            library, self, canvas, board_id, intent, center, settings,
        );
        editing.accept_created_batch(canvas, board_id, action)?;
        Ok(created.reservation)
    }
}

impl PlacementCreation {
    pub(crate) fn accept_search(
        &mut self,
        ctx: &egui::Context,
        context: &mut CompletionContext<'_>,
        request: super::lifecycle::CommitSearchProvisionalRequest,
        max_texture_side: usize,
        render_state: Option<&eframe::egui_wgpu::RenderState>,
    ) -> Result<usize, String> {
        let library = context
            .library
            .ok_or_else(|| "Storage unavailable".to_owned())?;
        let committed = super::lifecycle::commit_search_provisional_batch(
            library,
            context.canvas,
            request,
            super::creation::PlacementCreationSettings {
                placement_target_area: context.settings.placement_target_area,
                gap: context.settings.placement_gap,
            },
        )?;
        let count = committed.placements.len();
        context.editing.accept_created_batch(
            context.canvas,
            context.board_id,
            committed.history_action,
        )?;
        for placement in committed.placements {
            let image_settings = crate::image_preparation::CanvasImageSettings {
                max_side: context.settings.canvas_image_max_side,
            };
            if let Some(worker) = context.worker {
                worker.enqueue(crate::image_preparation::ImagePrepRequest::LoadExisting {
                    target_board_id: context.board_id,
                    queue_id: None,
                    placement,
                    max_texture_side,
                    canvas_image_settings: image_settings,
                    target_lod: crate::data::CanvasImageLod::Working,
                    priority: crate::image_preparation::ExistingLoadPriority::Background,
                    trace: None,
                })?;
            } else {
                let result = crate::image_preparation::load_existing(
                    library,
                    context.board_id,
                    None,
                    placement,
                    max_texture_side,
                    image_settings,
                    crate::data::CanvasImageLod::Working,
                    None,
                    render_state,
                );
                if let PreparationOutcome::Status(Some(error)) =
                    self.accept_result(ctx, context, result)
                {
                    return Err(error);
                }
            }
        }
        Ok(count)
    }
}
