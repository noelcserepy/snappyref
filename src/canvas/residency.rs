use std::collections::HashSet;
use std::time::Instant;

use crate::canvas::CanvasState;
use crate::data::{CanvasImageLod, Placement, WorkspaceSettings};
use crate::image_preparation::{
    CanvasImageSettings, ExistingLoadPriority, ImagePrepRequest, ImagePrepTimings, ImagePrepTrace,
    ImagePrepWorker,
};

/// Image LOD loads themselves are tracked on the canvas
/// (`CanvasState::image_lod_loads`); this only owns cache backfills.
#[derive(Default)]
pub(crate) struct ImageResidencyCoordinator {
    pending_cache_backfills: HashSet<(i64, CanvasImageLod)>,
}

impl ImageResidencyCoordinator {
    pub(crate) fn queue_startup_existing_loads(
        &mut self,
        import_worker: &ImagePrepWorker,
        canvas: &mut CanvasState,
        mut placements: Vec<Placement>,
        viewport_world: eframe::egui::Rect,
        active_board_id: i64,
        settings: &WorkspaceSettings,
        max_texture_side: usize,
        trace_enabled: bool,
    ) -> Result<(), String> {
        crate::image_preparation::sort_records_by_viewport_proximity(
            &mut placements,
            viewport_world,
        );
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
            canvas.note_image_lod_load_dispatched(placement_id, target_lod);
        }
        Ok(())
    }

    pub(crate) fn has_pending_backfills(&self) -> bool {
        !self.pending_cache_backfills.is_empty()
    }

    pub(crate) fn note_failed_placement(
        &mut self,
        canvas: &mut CanvasState,
        placement_id: Option<i64>,
    ) {
        if let Some(placement_id) = placement_id {
            canvas.clear_image_lod_load(placement_id);
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
        for (placement_id, target_lod) in canvas.undispatched_image_lod_loads() {
            let Some(placement) = canvas.placement_for_id(placement_id) else {
                canvas.clear_image_lod_load(placement_id);
                continue;
            };
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
                canvas.clear_image_lod_load(placement_id);
                last_error = Some(error);
            }
        }

        last_error
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
    fn failed_item_clears_its_image_lod_load() {
        let mut residency = ImageResidencyCoordinator::default();
        let mut canvas = CanvasState::default();
        canvas.note_image_lod_load_dispatched(7, CanvasImageLod::Detail);

        residency.note_failed_placement(&mut canvas, Some(7));

        assert!(!canvas.has_pending_image_loads());
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
