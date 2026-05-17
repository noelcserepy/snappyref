use std::collections::HashSet;

use eframe::egui;

use crate::canvas::CanvasState;
use crate::data::BoardItemWithAsset;
use crate::import_intake::{ImportIntake, ImportPlacementSettings, ImportReservation};
use crate::storage::Library;

#[derive(Clone, Debug)]
pub(crate) struct AddPreviewItem {
    pub(crate) label: String,
    pub(crate) dimensions: (u32, u32),
    pub(crate) approximate_dimensions: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct PlannedAddPreview {
    pub(crate) label: String,
    pub(crate) dimensions: Option<(u32, u32)>,
    pub(crate) center_world: egui::Pos2,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct AddPreviewSettings {
    pub(crate) target_canvas_area: f32,
    pub(crate) gap: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct CompletedAddBatch {
    pub(crate) target_board_id: i64,
    pub(crate) anchor_world: egui::Pos2,
    pub(crate) item_ids: Vec<i64>,
}

pub(crate) struct ExistingAssetPlacement {
    pub(crate) reservation: ImportReservation,
    pub(crate) record: BoardItemWithAsset,
}

#[derive(Default)]
pub(crate) struct AddBatchTracker {
    batches: Vec<PendingAddBatch>,
}

#[derive(Default)]
struct PendingAddBatch {
    target_board_id: i64,
    anchor_world: egui::Pos2,
    pending_queue_ids: HashSet<u64>,
    loaded_item_ids: Vec<i64>,
}

pub(crate) fn preview_size_for_dimensions(
    width: u32,
    height: u32,
    target_canvas_area: f32,
) -> egui::Vec2 {
    let width = width.max(1);
    let height = height.max(1);
    let scale = crate::jobs::initial_import_scale(width, height, true, target_canvas_area);
    egui::vec2(width as f32 * scale, height as f32 * scale)
}

pub(crate) fn plan_preview_placeholders(
    canvas: &mut CanvasState,
    items: &[AddPreviewItem],
    anchor_world: egui::Pos2,
    screen_rect: egui::Rect,
    settings: AddPreviewSettings,
) -> Vec<PlannedAddPreview> {
    let uses_approximate_dimensions = items.iter().any(|item| item.approximate_dimensions);
    let sizes = items
        .iter()
        .map(|item| {
            preview_size_for_dimensions(
                item.dimensions.0,
                item.dimensions.1,
                settings.target_canvas_area,
            )
        })
        .collect::<Vec<_>>();
    let centers = if uses_approximate_dimensions && sizes.len() > 1 {
        canvas.balanced_guided_preview_placeholders(&sizes, anchor_world, screen_rect, settings.gap)
    } else {
        canvas.compact_guided_preview_placeholders(&sizes, anchor_world, screen_rect, settings.gap)
    };

    items
        .iter()
        .zip(centers)
        .map(|(item, (center_world, _))| PlannedAddPreview {
            label: item.label.clone(),
            dimensions: Some(item.dimensions),
            center_world,
        })
        .collect()
}

pub(crate) fn reserve_existing_asset_placements(
    library: &Library,
    import_intake: &mut ImportIntake,
    canvas: &mut CanvasState,
    layout: &mut Vec<crate::data::BoardLayoutItem>,
    target_board_id: i64,
    asset_ids: Vec<i64>,
    preferred_centers: Vec<egui::Pos2>,
    anchor_world: egui::Pos2,
    screen_rect: egui::Rect,
    settings: AddPreviewSettings,
) -> Result<Vec<ExistingAssetPlacement>, String> {
    let assets = asset_ids
        .into_iter()
        .map(|asset_id| library.load_asset(asset_id))
        .collect::<Result<Vec<_>, _>>()?;

    let centers = if preferred_centers.len() == assets.len() {
        preferred_centers
    } else {
        let preview_items = assets
            .iter()
            .map(|asset| AddPreviewItem {
                label: asset.filename.clone(),
                dimensions: (asset.width, asset.height),
                approximate_dimensions: false,
            })
            .collect::<Vec<_>>();
        plan_preview_placeholders(canvas, &preview_items, anchor_world, screen_rect, settings)
            .into_iter()
            .map(|preview| preview.center_world)
            .collect()
    };

    let mut placements = Vec::with_capacity(assets.len());
    for (asset_index, asset) in assets.into_iter().enumerate() {
        let center_world = centers.get(asset_index).copied().unwrap_or(anchor_world);
        let (reservation, placeholder) = import_intake.reserve_import_at_world(
            layout,
            target_board_id,
            asset.filename.clone(),
            Some((asset.width, asset.height)),
            center_world,
            ImportPlacementSettings {
                import_target_canvas_area: settings.target_canvas_area,
            },
        );
        if let Some(placeholder) = placeholder {
            canvas.add_import_placeholder(
                placeholder.queue_id,
                placeholder.center_world,
                placeholder.size_world,
                placeholder.label,
            );
        }
        let scale = crate::jobs::initial_import_scale(
            asset.width,
            asset.height,
            true,
            settings.target_canvas_area,
        );
        let record =
            match library.create_board_item_from_asset(asset.id, center_world, scale, 0.0, 0) {
                Ok(record) => record,
                Err(error) => {
                    import_intake.remove_import_reservation(reservation);
                    canvas.remove_import_placeholder(reservation.queue_id);
                    return Err(error);
                }
            };
        placements.push(ExistingAssetPlacement {
            reservation,
            record,
        });
    }

    Ok(placements)
}

impl AddBatchTracker {
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn len(&self) -> usize {
        self.batches.len()
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn is_empty(&self) -> bool {
        self.batches.is_empty()
    }

    pub(crate) fn register_multi_item_batch(
        &mut self,
        target_board_id: i64,
        anchor_world: egui::Pos2,
        queue_ids: Vec<u64>,
    ) {
        if queue_ids.len() <= 1 {
            return;
        }
        self.register_any_item_batch(target_board_id, anchor_world, queue_ids);
    }

    pub(crate) fn register_any_item_batch(
        &mut self,
        target_board_id: i64,
        anchor_world: egui::Pos2,
        queue_ids: Vec<u64>,
    ) {
        if queue_ids.is_empty() {
            return;
        }
        self.batches.push(PendingAddBatch {
            target_board_id,
            anchor_world,
            pending_queue_ids: queue_ids.into_iter().collect(),
            loaded_item_ids: Vec::new(),
        });
    }

    pub(crate) fn finish_queue_id(
        &mut self,
        target_board_id: Option<i64>,
        queue_id: Option<u64>,
        loaded_item_id: Option<i64>,
    ) -> Vec<CompletedAddBatch> {
        let Some(queue_id) = queue_id else {
            return Vec::new();
        };

        let mut completed_batches = Vec::new();
        for batch in &mut self.batches {
            if !batch.pending_queue_ids.remove(&queue_id) {
                continue;
            }
            if let Some(item_id) = loaded_item_id
                && !batch.loaded_item_ids.contains(&item_id)
            {
                batch.loaded_item_ids.push(item_id);
            }
            if batch.pending_queue_ids.is_empty()
                && target_board_id.is_none_or(|id| id == batch.target_board_id)
            {
                completed_batches.push(CompletedAddBatch {
                    target_board_id: batch.target_board_id,
                    anchor_world: batch.anchor_world,
                    item_ids: std::mem::take(&mut batch.loaded_item_ids),
                });
            }
        }
        self.batches
            .retain(|batch| !batch.pending_queue_ids.is_empty());

        completed_batches
    }
}
