use std::collections::{HashMap, HashSet};
use std::time::Instant;

use eframe::egui;

use crate::canvas::CanvasState;
use crate::data::{CanvasImageLod, Placement, WorkspaceSettings};
use crate::jobs::{
    CanvasImageSettings, ExistingLoadPriority, ImagePrepRequest, ImagePrepResult, ImagePrepTimings,
    ImagePrepTrace, ImagePrepWorker, PreparedCropPreview, PreparedPlacementImage,
};
use crate::placement::content_resolution::{
    ContentResolutionTicket, FailedContentResolutionOutcome, LoadedContentResolutionLog,
    LoadedContentResolutionOutcome,
};

#[derive(Default)]
pub(crate) struct ImageResidencyCoordinator {
    pending_cache_backfills: HashSet<(i64, CanvasImageLod)>,
    inflight_image_lod_loads: HashMap<i64, CanvasImageLod>,
}

#[derive(Debug)]
pub(crate) enum ImagePrepResultResidencyOutcome {
    Loaded(LoadedContentResolutionOutcome),
    Cropped(PreparedCropPreview),
    Failed(FailedContentResolutionOutcome),
    Status(Option<String>),
}

impl ImageResidencyCoordinator {
    pub(crate) fn queue_startup_existing_loads(
        &mut self,
        import_worker: &ImagePrepWorker,
        mut placements: Vec<Placement>,
        viewport_world: eframe::egui::Rect,
        active_board_id: i64,
        settings: &WorkspaceSettings,
        max_texture_side: usize,
        trace_enabled: bool,
    ) -> Result<(), String> {
        crate::jobs::sort_records_by_viewport_proximity(&mut placements, viewport_world);
        let (canvas_image_settings, target_lod) = startup_existing_image_load_settings(settings);
        for placement in placements {
            let placement_id = placement.state.id;
            import_worker.enqueue(ImagePrepRequest::LoadExisting {
                target_board_id: active_board_id,
                queue_id: None,
                placement,
                max_texture_side,
                canvas_image_settings,
                target_lod,
                priority: ExistingLoadPriority::Startup,
                trace: trace_enabled.then(|| ImagePrepTrace {
                    queued_at: Instant::now(),
                    timings: ImagePrepTimings::default(),
                }),
            })?;
            self.inflight_image_lod_loads
                .insert(placement_id, target_lod);
        }
        Ok(())
    }

    pub(crate) fn note_loaded_placement(&mut self, placement_id: i64) {
        self.inflight_image_lod_loads.remove(&placement_id);
    }

    pub(crate) fn has_pending_image_work(&self) -> bool {
        !self.pending_cache_backfills.is_empty() || !self.inflight_image_lod_loads.is_empty()
    }

    pub(crate) fn process_import_result(
        &mut self,
        ctx: &egui::Context,
        canvas: &mut CanvasState,
        import_worker: Option<&ImagePrepWorker>,
        active_board_id: i64,
        settings: &WorkspaceSettings,
        result: ImagePrepResult,
    ) -> ImagePrepResultResidencyOutcome {
        match result {
            ImagePrepResult::Loaded(loaded) => {
                ImagePrepResultResidencyOutcome::Loaded(self.integrate_loaded_import(
                    ctx,
                    canvas,
                    import_worker,
                    active_board_id,
                    settings,
                    loaded,
                ))
            }
            ImagePrepResult::Cropped(cropped) => ImagePrepResultResidencyOutcome::Cropped(cropped),
            ImagePrepResult::CacheLodBackfilled { asset_id, lod } => {
                let status = self.note_backfill_completed(asset_id, lod, None);
                ImagePrepResultResidencyOutcome::Status(status)
            }
            ImagePrepResult::CacheLodBackfillFailed {
                asset_id,
                lod,
                error,
            } => {
                let status = self.note_backfill_completed(asset_id, lod, Some(error));
                ImagePrepResultResidencyOutcome::Status(status)
            }
            ImagePrepResult::Failed {
                target_board_id,
                queue_id,
                placement_id,
                context,
                error,
            } => ImagePrepResultResidencyOutcome::Failed(self.integrate_failed_import(
                canvas,
                active_board_id,
                target_board_id,
                queue_id,
                placement_id,
                context,
                error,
            )),
        }
    }

    pub(crate) fn integrate_loaded_import(
        &mut self,
        ctx: &egui::Context,
        canvas: &mut CanvasState,
        import_worker: Option<&ImagePrepWorker>,
        active_board_id: i64,
        settings: &WorkspaceSettings,
        loaded: PreparedPlacementImage,
    ) -> LoadedContentResolutionOutcome {
        let resolution_ticket = loaded.queue_id.map(|queue_id| ContentResolutionTicket {
            target_board_id: loaded.target_board_id,
            queue_id,
        });
        let origin = loaded.origin;
        let loaded_lod = loaded.loaded_lod;
        let loaded_record = loaded.placement.clone();
        let existing_load_diagnostics = loaded.existing_load_diagnostics.clone();
        self.note_loaded_placement(loaded.placement.state.id);

        if loaded.target_board_id != active_board_id {
            return LoadedContentResolutionOutcome {
                resolution_ticket,
                status: Some("Imported image".to_owned()),
                indexed_asset: None,
                log: None,
            };
        }

        if canvas
            .placement_for_id(loaded.placement.state.id)
            .is_some_and(|current| {
                current.is_loaded() && current.state.asset_id != loaded.placement.state.asset_id
            })
        {
            return LoadedContentResolutionOutcome {
                resolution_ticket,
                status: None,
                indexed_asset: None,
                log: None,
            };
        }

        let indexed_asset = (settings.semantic_search_enabled
            && asset_mime_type_supports_raster_derivatives(
                &loaded.placement.require_loaded_asset().mime_type,
            ))
        .then(|| {
            (
                loaded.placement.require_loaded_asset().id,
                loaded
                    .placement
                    .require_loaded_asset()
                    .original_path
                    .clone(),
            )
        });

        let mut trace = loaded.trace;
        let ui_integration_started_at = Instant::now();
        canvas.add_prepared_placement_image(ctx, loaded);
        if let Some(trace) = &mut trace {
            trace.timings.ui_integration = ui_integration_started_at.elapsed();
        }
        let _ = self.queue_overview_backfill_after_working_load(
            import_worker,
            &loaded_record,
            settings,
            loaded_lod,
        );

        LoadedContentResolutionOutcome {
            resolution_ticket,
            status: Some("Imported image".to_owned()),
            indexed_asset,
            log: Some(LoadedContentResolutionLog {
                origin,
                placement: loaded_record,
                diagnostics: existing_load_diagnostics,
                trace,
            }),
        }
    }

    pub(crate) fn note_failed_placement(
        &mut self,
        canvas: &mut CanvasState,
        placement_id: Option<i64>,
    ) {
        if let Some(placement_id) = placement_id {
            self.inflight_image_lod_loads.remove(&placement_id);
            canvas.clear_pending_image_lod_load(placement_id);
        }
    }

    fn integrate_failed_import(
        &mut self,
        canvas: &mut CanvasState,
        active_board_id: i64,
        target_board_id: Option<i64>,
        queue_id: Option<u64>,
        placement_id: Option<i64>,
        context: String,
        error: String,
    ) -> FailedContentResolutionOutcome {
        let resolution_ticket = queue_id.map(|queue_id| ContentResolutionTicket {
            target_board_id: target_board_id.unwrap_or(active_board_id),
            queue_id,
        });
        self.note_failed_placement(canvas, placement_id);
        if let Some(placement_id) = placement_id {
            canvas.clear_crop_pending(placement_id);
        }
        FailedContentResolutionOutcome {
            resolution_ticket,
            placement_id,
            target_board_id,
            status: format!("{context}: {error}"),
        }
    }

    pub(crate) fn note_backfill_completed(
        &mut self,
        asset_id: i64,
        lod: CanvasImageLod,
        error: Option<String>,
    ) -> Option<String> {
        self.pending_cache_backfills.remove(&(asset_id, lod));
        error
    }

    pub(crate) fn queue_backfill(
        &mut self,
        import_worker: Option<&ImagePrepWorker>,
        placement: &Placement,
        settings: &WorkspaceSettings,
        lod: CanvasImageLod,
    ) -> Option<String> {
        let import_worker = import_worker?;
        let backfill_key = (placement.require_loaded_asset().id, lod);
        if !self.pending_cache_backfills.insert(backfill_key) {
            return None;
        }
        if let Err(error) = import_worker.enqueue(ImagePrepRequest::BackfillLod {
            placement: placement.clone(),
            canvas_image_settings: canvas_image_settings(settings),
            lod,
        }) {
            self.pending_cache_backfills.remove(&backfill_key);
            return Some(error);
        }
        None
    }

    pub(crate) fn queue_overview_backfill_after_working_load(
        &mut self,
        import_worker: Option<&ImagePrepWorker>,
        placement: &Placement,
        settings: &WorkspaceSettings,
        loaded_lod: CanvasImageLod,
    ) -> Option<String> {
        if loaded_lod != CanvasImageLod::Working
            || !asset_mime_type_supports_raster_derivatives(
                &placement.require_loaded_asset().mime_type,
            )
        {
            return None;
        }
        self.queue_backfill(import_worker, placement, settings, CanvasImageLod::Overview)
    }

    pub(crate) fn dispatch_pending_lod_requests(
        &mut self,
        import_worker: Option<&ImagePrepWorker>,
        canvas: &mut CanvasState,
        active_board_id: i64,
        settings: &WorkspaceSettings,
        max_texture_side: usize,
    ) -> Option<String> {
        let Some(import_worker) = import_worker else {
            return None;
        };

        let mut last_error = None;
        for (placement_id, target_lod) in canvas.take_pending_image_lod_loads() {
            if self
                .inflight_image_lod_loads
                .get(&placement_id)
                .is_some_and(|existing| *existing >= target_lod)
            {
                continue;
            }
            let Some(placement) = canvas.placement_for_id(placement_id) else {
                continue;
            };
            self.inflight_image_lod_loads
                .insert(placement_id, target_lod);
            if let Err(error) = import_worker.enqueue(ImagePrepRequest::LoadExisting {
                target_board_id: active_board_id,
                queue_id: None,
                placement,
                max_texture_side,
                canvas_image_settings: canvas_image_settings(settings),
                target_lod,
                priority: ExistingLoadPriority::Background,
                trace: None,
            }) {
                self.inflight_image_lod_loads.remove(&placement_id);
                canvas.clear_pending_image_lod_load(placement_id);
                last_error = Some(error);
            }
        }

        last_error
    }

    #[cfg(test)]
    fn inflight_lod_for(&self, placement_id: i64) -> Option<CanvasImageLod> {
        self.inflight_image_lod_loads.get(&placement_id).copied()
    }

    #[cfg(test)]
    fn has_pending_backfill(&self, asset_id: i64, lod: CanvasImageLod) -> bool {
        self.pending_cache_backfills.contains(&(asset_id, lod))
    }
}

fn canvas_image_settings(settings: &WorkspaceSettings) -> CanvasImageSettings {
    CanvasImageSettings {
        max_side: settings.canvas_image_max_side,
    }
}

pub(crate) fn startup_existing_image_load_settings(
    settings: &WorkspaceSettings,
) -> (CanvasImageSettings, CanvasImageLod) {
    (canvas_image_settings(settings), CanvasImageLod::Overview)
}

pub(crate) fn asset_mime_type_supports_raster_derivatives(mime_type: &str) -> bool {
    !mime_type
        .split(';')
        .next()
        .unwrap_or(mime_type)
        .trim()
        .eq_ignore_ascii_case("image/svg+xml")
}

#[cfg(test)]
mod tests {
    use super::{ImageResidencyCoordinator, startup_existing_image_load_settings};
    use crate::canvas::CanvasState;
    use crate::data::CanvasImageLod;

    #[test]
    fn failed_item_clears_inflight_and_pending_request() {
        let mut residency = ImageResidencyCoordinator::default();
        let mut canvas = CanvasState::default();
        canvas.clear_pending_image_lod_load(7);
        residency
            .inflight_image_lod_loads
            .insert(7, CanvasImageLod::Detail);

        residency.note_failed_placement(&mut canvas, Some(7));

        assert_eq!(residency.inflight_lod_for(7), None);
    }

    #[test]
    fn completed_backfill_removes_pending_entry() {
        let mut residency = ImageResidencyCoordinator::default();
        residency
            .pending_cache_backfills
            .insert((42, CanvasImageLod::Overview));

        let error = residency.note_backfill_completed(42, CanvasImageLod::Overview, None);

        assert_eq!(error, None);
        assert!(!residency.has_pending_backfill(42, CanvasImageLod::Overview));
    }

    #[test]
    fn startup_existing_image_load_uses_overview_cache_lod() {
        let settings = crate::data::WorkspaceSettings {
            canvas_image_max_side: 4096,
            ..Default::default()
        };

        let (image_settings, lod) = startup_existing_image_load_settings(&settings);

        assert_eq!(image_settings.max_side, 4096);
        assert_eq!(lod, CanvasImageLod::Overview);
    }
}
