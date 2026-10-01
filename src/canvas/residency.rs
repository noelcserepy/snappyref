use eframe::egui;
use std::collections::HashSet;
use std::time::Instant;

use crate::canvas::CanvasState;
use crate::data::{CanvasImageLod, Placement, WorkspaceSettings};
use crate::image_preparation::{
    CanvasImageSettings, ExistingLoadPriority, ImagePrepRequest, ImagePrepTimings, ImagePrepTrace,
    ImagePrepWorker,
};

/// Schedules image loads and tracks cache backfills. CanvasState tracks LOD load progress.
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
        sort_placements_by_viewport_proximity(&mut placements, viewport_world);
        let canvas_image_settings = canvas_image_settings(settings);
        let target_lod = CanvasImageLod::Overview;
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

    pub(crate) fn note_backfill_completed(&mut self, asset_id: i64, lod: CanvasImageLod) {
        self.pending_cache_backfills.remove(&(asset_id, lod));
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
        let import_worker = import_worker?;

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
}

fn canvas_image_settings(settings: &WorkspaceSettings) -> CanvasImageSettings {
    CanvasImageSettings {
        max_side: settings.canvas_image_max_side,
    }
}

pub(crate) fn asset_mime_type_supports_raster_derivatives(mime_type: &str) -> bool {
    !mime_type
        .split(';')
        .next()
        .unwrap_or(mime_type)
        .trim()
        .eq_ignore_ascii_case("image/svg+xml")
}

fn sort_placements_by_viewport_proximity(placements: &mut [Placement], viewport_world: egui::Rect) {
    placements.sort_by(|left, right| {
        placement_viewport_distance_sq(left, viewport_world)
            .total_cmp(&placement_viewport_distance_sq(right, viewport_world))
            .then_with(|| {
                left.state
                    .geometry
                    .z_index
                    .cmp(&right.state.geometry.z_index)
                    .then_with(|| left.state.id.cmp(&right.state.id))
            })
    });
}

fn placement_viewport_distance_sq(placement: &Placement, viewport_world: egui::Rect) -> f32 {
    let rect = placement_world_rect(placement);
    if rect.intersects(viewport_world) {
        return 0.0;
    }
    let center = rect.center();
    let clamped = egui::pos2(
        center
            .x
            .clamp(viewport_world.left(), viewport_world.right()),
        center
            .y
            .clamp(viewport_world.top(), viewport_world.bottom()),
    );
    center.distance_sq(clamped)
}

fn placement_world_rect(placement: &Placement) -> egui::Rect {
    let size = egui::vec2(
        placement.require_loaded_asset().width as f32,
        placement.require_loaded_asset().height as f32,
    ) * placement.state.geometry.scale.max(0.01);
    egui::Rect::from_center_size(
        egui::pos2(
            placement.state.geometry.center.x,
            placement.state.geometry.center.y,
        ),
        size,
    )
}
