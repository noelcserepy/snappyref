use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::Instant;

use eframe::egui;

use crate::canvas::CanvasState;
use crate::data::{BoardItemWithAsset, CanvasImageLod, WorkspaceSettings};
use crate::jobs::{
    CanvasImageSettings, CroppedCanvasItem, ExistingLoadDiagnostics, ExistingLoadPriority,
    ImportOrigin, ImportRequest, ImportResult, ImportTimings, ImportTrace, ImportWorker,
    LoadedCanvasItem,
};

#[derive(Default)]
pub(crate) struct ImageResidencyCoordinator {
    pending_cache_backfills: HashSet<(i64, CanvasImageLod)>,
    inflight_image_lod_loads: HashMap<i64, CanvasImageLod>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ImportPlaceholderRemoval {
    pub(crate) target_board_id: i64,
    pub(crate) queue_id: u64,
}

#[derive(Clone, Debug)]
pub(crate) struct LoadedImportLog {
    pub(crate) origin: ImportOrigin,
    pub(crate) record: BoardItemWithAsset,
    pub(crate) diagnostics: Option<ExistingLoadDiagnostics>,
    pub(crate) trace: Option<ImportTrace>,
}

#[derive(Clone, Debug)]
pub(crate) struct LoadedImportOutcome {
    pub(crate) placeholder_removal: Option<ImportPlaceholderRemoval>,
    pub(crate) status: Option<String>,
    pub(crate) indexed_asset: Option<(i64, PathBuf)>,
    pub(crate) log: Option<LoadedImportLog>,
}

#[derive(Clone, Debug)]
pub(crate) struct FailedImportOutcome {
    pub(crate) placeholder_removal: Option<ImportPlaceholderRemoval>,
    pub(crate) status: String,
}

#[derive(Debug)]
pub(crate) enum ImportResultResidencyOutcome {
    Loaded(LoadedImportOutcome),
    Cropped(CroppedCanvasItem),
    Failed(FailedImportOutcome),
    Status(Option<String>),
}

impl ImageResidencyCoordinator {
    pub(crate) fn queue_startup_existing_loads(
        &mut self,
        import_worker: &ImportWorker,
        mut records: Vec<BoardItemWithAsset>,
        viewport_world: eframe::egui::Rect,
        active_board_id: i64,
        settings: &WorkspaceSettings,
        max_texture_side: usize,
        trace_enabled: bool,
    ) -> Result<(), String> {
        crate::jobs::sort_records_by_viewport_proximity(&mut records, viewport_world);
        let (canvas_image_settings, target_lod) = startup_existing_image_load_settings(settings);
        for record in records {
            let item_id = record.item.id;
            import_worker.enqueue(ImportRequest::LoadExisting {
                target_board_id: active_board_id,
                queue_id: None,
                record,
                max_texture_side,
                canvas_image_settings,
                target_lod,
                priority: ExistingLoadPriority::Startup,
                trace: trace_enabled.then(|| ImportTrace {
                    queued_at: Instant::now(),
                    timings: ImportTimings::default(),
                }),
            })?;
            self.inflight_image_lod_loads.insert(item_id, target_lod);
        }
        Ok(())
    }

    pub(crate) fn note_loaded_item(&mut self, item_id: i64) {
        self.inflight_image_lod_loads.remove(&item_id);
    }

    pub(crate) fn has_pending_image_work(&self) -> bool {
        !self.pending_cache_backfills.is_empty() || !self.inflight_image_lod_loads.is_empty()
    }

    pub(crate) fn process_import_result(
        &mut self,
        ctx: &egui::Context,
        canvas: &mut CanvasState,
        import_worker: Option<&ImportWorker>,
        active_board_id: i64,
        settings: &WorkspaceSettings,
        result: ImportResult,
    ) -> ImportResultResidencyOutcome {
        match result {
            ImportResult::Loaded(loaded) => {
                ImportResultResidencyOutcome::Loaded(self.integrate_loaded_import(
                    ctx,
                    canvas,
                    import_worker,
                    active_board_id,
                    settings,
                    loaded,
                ))
            }
            ImportResult::Cropped(cropped) => ImportResultResidencyOutcome::Cropped(cropped),
            ImportResult::CacheLodBackfilled { asset_id, lod } => {
                let status = self.note_backfill_completed(asset_id, lod, None);
                ImportResultResidencyOutcome::Status(status)
            }
            ImportResult::CacheLodBackfillFailed {
                asset_id,
                lod,
                error,
            } => {
                let status = self.note_backfill_completed(asset_id, lod, Some(error));
                ImportResultResidencyOutcome::Status(status)
            }
            ImportResult::Failed {
                target_board_id,
                queue_id,
                item_id,
                context,
                error,
            } => ImportResultResidencyOutcome::Failed(self.integrate_failed_import(
                canvas,
                active_board_id,
                target_board_id,
                queue_id,
                item_id,
                context,
                error,
            )),
        }
    }

    pub(crate) fn integrate_loaded_import(
        &mut self,
        ctx: &egui::Context,
        canvas: &mut CanvasState,
        import_worker: Option<&ImportWorker>,
        active_board_id: i64,
        settings: &WorkspaceSettings,
        loaded: LoadedCanvasItem,
    ) -> LoadedImportOutcome {
        let placeholder_removal = loaded.queue_id.map(|queue_id| ImportPlaceholderRemoval {
            target_board_id: loaded.target_board_id,
            queue_id,
        });
        let origin = loaded.origin;
        let loaded_lod = loaded.loaded_lod;
        let loaded_record = loaded.record.clone();
        let existing_load_diagnostics = loaded.existing_load_diagnostics.clone();
        self.note_loaded_item(loaded.record.item.id);

        if loaded.target_board_id != active_board_id {
            return LoadedImportOutcome {
                placeholder_removal,
                status: Some("Imported image".to_owned()),
                indexed_asset: None,
                log: None,
            };
        }

        if canvas
            .record_for_item_id(loaded.record.item.id)
            .is_some_and(|current| current.item.asset_id != loaded.record.item.asset_id)
        {
            return LoadedImportOutcome {
                placeholder_removal,
                status: None,
                indexed_asset: None,
                log: None,
            };
        }

        let indexed_asset = (settings.semantic_search_enabled
            && asset_mime_type_supports_raster_derivatives(&loaded.record.asset.mime_type))
        .then(|| {
            (
                loaded.record.asset.id,
                loaded.record.asset.original_path.clone(),
            )
        });

        let mut trace = loaded.trace;
        let ui_integration_started_at = Instant::now();
        canvas.add_loaded_item(ctx, loaded);
        if let Some(trace) = &mut trace {
            trace.timings.ui_integration = ui_integration_started_at.elapsed();
        }
        let _ = self.queue_overview_backfill_after_working_load(
            import_worker,
            &loaded_record,
            settings,
            loaded_lod,
        );

        LoadedImportOutcome {
            placeholder_removal,
            status: Some("Imported image".to_owned()),
            indexed_asset,
            log: Some(LoadedImportLog {
                origin,
                record: loaded_record,
                diagnostics: existing_load_diagnostics,
                trace,
            }),
        }
    }

    pub(crate) fn note_failed_item(&mut self, canvas: &mut CanvasState, item_id: Option<i64>) {
        if let Some(item_id) = item_id {
            self.inflight_image_lod_loads.remove(&item_id);
            canvas.clear_pending_image_lod_load(item_id);
        }
    }

    fn integrate_failed_import(
        &mut self,
        canvas: &mut CanvasState,
        active_board_id: i64,
        target_board_id: Option<i64>,
        queue_id: Option<u64>,
        item_id: Option<i64>,
        context: String,
        error: String,
    ) -> FailedImportOutcome {
        let placeholder_removal = queue_id.map(|queue_id| ImportPlaceholderRemoval {
            target_board_id: target_board_id.unwrap_or(active_board_id),
            queue_id,
        });
        self.note_failed_item(canvas, item_id);
        if let Some(item_id) = item_id {
            canvas.clear_crop_pending(item_id);
        }
        FailedImportOutcome {
            placeholder_removal,
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
        import_worker: Option<&ImportWorker>,
        record: &BoardItemWithAsset,
        settings: &WorkspaceSettings,
        lod: CanvasImageLod,
    ) -> Option<String> {
        let Some(import_worker) = import_worker else {
            return None;
        };
        if !self.pending_cache_backfills.insert((record.asset.id, lod)) {
            return None;
        }
        if let Err(error) = import_worker.enqueue(ImportRequest::BackfillLod {
            record: record.clone(),
            canvas_image_settings: canvas_image_settings(settings),
            lod,
        }) {
            self.pending_cache_backfills.remove(&(record.asset.id, lod));
            return Some(error);
        }
        None
    }

    pub(crate) fn queue_overview_backfill_after_working_load(
        &mut self,
        import_worker: Option<&ImportWorker>,
        record: &BoardItemWithAsset,
        settings: &WorkspaceSettings,
        loaded_lod: CanvasImageLod,
    ) -> Option<String> {
        if loaded_lod != CanvasImageLod::Working
            || !asset_mime_type_supports_raster_derivatives(&record.asset.mime_type)
        {
            return None;
        }
        self.queue_backfill(import_worker, record, settings, CanvasImageLod::Overview)
    }

    pub(crate) fn dispatch_pending_lod_requests(
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
        for (item_id, target_lod) in canvas.take_pending_image_lod_loads() {
            if self
                .inflight_image_lod_loads
                .get(&item_id)
                .is_some_and(|existing| *existing >= target_lod)
            {
                continue;
            }
            let Some(record) = canvas.record_for_item_id(item_id) else {
                continue;
            };
            self.inflight_image_lod_loads.insert(item_id, target_lod);
            if let Err(error) = import_worker.enqueue(ImportRequest::LoadExisting {
                target_board_id: active_board_id,
                queue_id: None,
                record,
                max_texture_side,
                canvas_image_settings: canvas_image_settings(settings),
                target_lod,
                priority: ExistingLoadPriority::Background,
                trace: None,
            }) {
                self.inflight_image_lod_loads.remove(&item_id);
                canvas.clear_pending_image_lod_load(item_id);
                last_error = Some(error);
            }
        }

        last_error
    }

    #[cfg(test)]
    fn inflight_lod_for(&self, item_id: i64) -> Option<CanvasImageLod> {
        self.inflight_image_lod_loads.get(&item_id).copied()
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

        residency.note_failed_item(&mut canvas, Some(7));

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
