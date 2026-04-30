use std::collections::{HashMap, HashSet};
use std::time::Instant;

use crate::canvas::CanvasState;
use crate::data::{BoardItemWithAsset, CanvasImageCacheTier, WorkspaceSettings};
use crate::jobs::{
    CanvasImageSettings, ExistingLoadPriority, ImportRequest, ImportTimings, ImportTrace,
    ImportWorker,
};

#[derive(Default)]
pub(crate) struct ImageResidencyCoordinator {
    pending_cache_backfills: HashSet<(i64, CanvasImageCacheTier)>,
    inflight_image_tier_loads: HashMap<i64, CanvasImageCacheTier>,
}

impl ImageResidencyCoordinator {
    pub(crate) fn queue_startup_existing_loads(
        &self,
        import_worker: &ImportWorker,
        mut records: Vec<BoardItemWithAsset>,
        viewport_world: eframe::egui::Rect,
        active_board_id: i64,
        settings: &WorkspaceSettings,
        max_texture_side: usize,
        trace_enabled: bool,
    ) -> Result<(), String> {
        crate::jobs::sort_records_by_viewport_proximity(&mut records, viewport_world);
        let (canvas_image_settings, target_tier) = startup_existing_image_load_settings(settings);
        for record in records {
            import_worker.enqueue(ImportRequest::LoadExisting {
                target_board_id: active_board_id,
                record,
                max_texture_side,
                canvas_image_settings,
                target_tier,
                priority: ExistingLoadPriority::Startup,
                trace: trace_enabled.then(|| ImportTrace {
                    queued_at: Instant::now(),
                    timings: ImportTimings::default(),
                }),
            })?;
        }
        Ok(())
    }

    pub(crate) fn note_loaded_item(&mut self, item_id: i64) {
        self.inflight_image_tier_loads.remove(&item_id);
    }

    pub(crate) fn note_failed_item(&mut self, canvas: &mut CanvasState, item_id: Option<i64>) {
        if let Some(item_id) = item_id {
            self.inflight_image_tier_loads.remove(&item_id);
            canvas.clear_pending_image_tier_load(item_id);
        }
    }

    pub(crate) fn note_backfill_completed(
        &mut self,
        asset_id: i64,
        tier: CanvasImageCacheTier,
        error: Option<String>,
    ) -> Option<String> {
        self.pending_cache_backfills.remove(&(asset_id, tier));
        error
    }

    pub(crate) fn queue_backfill(
        &mut self,
        import_worker: Option<&ImportWorker>,
        record: &BoardItemWithAsset,
        settings: &WorkspaceSettings,
        tier: CanvasImageCacheTier,
    ) -> Option<String> {
        let Some(import_worker) = import_worker else {
            return None;
        };
        if !self.pending_cache_backfills.insert((record.asset.id, tier)) {
            return None;
        }
        if let Err(error) = import_worker.enqueue(ImportRequest::BackfillTier {
            record: record.clone(),
            canvas_image_settings: canvas_image_settings(settings),
            tier,
        }) {
            self.pending_cache_backfills
                .remove(&(record.asset.id, tier));
            return Some(error);
        }
        None
    }

    pub(crate) fn queue_overview_backfill_after_working_load(
        &mut self,
        import_worker: Option<&ImportWorker>,
        record: &BoardItemWithAsset,
        settings: &WorkspaceSettings,
        loaded_tier: CanvasImageCacheTier,
    ) -> Option<String> {
        if loaded_tier != CanvasImageCacheTier::Working
            || !asset_mime_type_supports_raster_derivatives(&record.asset.mime_type)
        {
            return None;
        }
        self.queue_backfill(
            import_worker,
            record,
            settings,
            CanvasImageCacheTier::Overview,
        )
    }

    pub(crate) fn dispatch_pending_tier_requests(
        &mut self,
        import_worker: Option<&ImportWorker>,
        canvas: &mut CanvasState,
        active_board_id: i64,
        settings: &WorkspaceSettings,
        max_texture_side: usize,
    ) -> Option<String> {
        let Some(import_worker) = import_worker else {
            return None;
        };

        let mut last_error = None;
        for (item_id, target_tier) in canvas.take_pending_image_tier_loads() {
            if self
                .inflight_image_tier_loads
                .get(&item_id)
                .is_some_and(|existing| *existing >= target_tier)
            {
                continue;
            }
            let Some(record) = canvas.record_for_item_id(item_id) else {
                continue;
            };
            self.inflight_image_tier_loads.insert(item_id, target_tier);
            if let Err(error) = import_worker.enqueue(ImportRequest::LoadExisting {
                target_board_id: active_board_id,
                record,
                max_texture_side,
                canvas_image_settings: canvas_image_settings(settings),
                target_tier,
                priority: ExistingLoadPriority::Background,
                trace: None,
            }) {
                self.inflight_image_tier_loads.remove(&item_id);
                canvas.clear_pending_image_tier_load(item_id);
                last_error = Some(error);
            }
        }

        last_error
    }

    #[cfg(test)]
    fn inflight_tier_for(&self, item_id: i64) -> Option<CanvasImageCacheTier> {
        self.inflight_image_tier_loads.get(&item_id).copied()
    }

    #[cfg(test)]
    fn has_pending_backfill(&self, asset_id: i64, tier: CanvasImageCacheTier) -> bool {
        self.pending_cache_backfills.contains(&(asset_id, tier))
    }
}

fn canvas_image_settings(settings: &WorkspaceSettings) -> CanvasImageSettings {
    CanvasImageSettings {
        max_side: settings.canvas_image_max_side,
    }
}

pub(crate) fn startup_existing_image_load_settings(
    settings: &WorkspaceSettings,
) -> (CanvasImageSettings, CanvasImageCacheTier) {
    (
        canvas_image_settings(settings),
        CanvasImageCacheTier::Overview,
    )
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
    use crate::data::CanvasImageCacheTier;

    #[test]
    fn failed_item_clears_inflight_and_pending_request() {
        let mut residency = ImageResidencyCoordinator::default();
        let mut canvas = CanvasState::default();
        canvas.clear_pending_image_tier_load(7);
        residency
            .inflight_image_tier_loads
            .insert(7, CanvasImageCacheTier::Detail);

        residency.note_failed_item(&mut canvas, Some(7));

        assert_eq!(residency.inflight_tier_for(7), None);
    }

    #[test]
    fn completed_backfill_removes_pending_entry() {
        let mut residency = ImageResidencyCoordinator::default();
        residency
            .pending_cache_backfills
            .insert((42, CanvasImageCacheTier::Overview));

        let error = residency.note_backfill_completed(42, CanvasImageCacheTier::Overview, None);

        assert_eq!(error, None);
        assert!(!residency.has_pending_backfill(42, CanvasImageCacheTier::Overview));
    }

    #[test]
    fn startup_existing_image_load_uses_overview_cache_tier() {
        let settings = crate::data::WorkspaceSettings {
            canvas_image_max_side: 4096,
            ..Default::default()
        };

        let (image_settings, tier) = startup_existing_image_load_settings(&settings);

        assert_eq!(image_settings.max_side, 4096);
        assert_eq!(tier, CanvasImageCacheTier::Overview);
    }
}
