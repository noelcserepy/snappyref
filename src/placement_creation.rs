use std::collections::HashSet;

use eframe::egui;

use crate::canvas::CanvasState;
use crate::content_resolution::{ContentResolutionReservation, ContentResolutionRuntime};
use crate::data::{
    BoardItem, BoardItemWithAsset, PlacementContent, PlacementContentIntent, PlacementLifecycle,
};
use crate::storage::Library;

const FALLBACK_IMPORT_WIDTH: u32 = 360;
const FALLBACK_IMPORT_HEIGHT: u32 = 240;

#[derive(Clone, Debug)]
pub(crate) struct SourceIntent {
    pub(crate) label: String,
    pub(crate) dimensions: Option<(u32, u32)>,
    pub(crate) approximate_dimensions: bool,
    pub(crate) content: PlacementContentIntent,
}

#[cfg_attr(not(test), allow(dead_code))]
#[derive(Clone, Debug)]
pub(crate) struct SourceIntentPreview {
    pub(crate) label: String,
    pub(crate) dimensions: Option<(u32, u32)>,
    pub(crate) center_world: egui::Pos2,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct PlacementCreationSettings {
    pub(crate) target_canvas_area: f32,
    pub(crate) gap: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct CompletedPlacementCreationBatch {
    pub(crate) target_board_id: i64,
    pub(crate) anchor_world: egui::Pos2,
    pub(crate) item_ids: Vec<i64>,
}

#[allow(dead_code)]
pub(crate) struct CreatedLoadedPlacement {
    pub(crate) reservation: Option<ContentResolutionReservation>,
    pub(crate) record: BoardItemWithAsset,
}

pub(crate) struct CreatedLoadingPlacement {
    pub(crate) reservation: ContentResolutionReservation,
    pub(crate) record: BoardItemWithAsset,
}

pub(crate) enum ProvisionalSourceIntent {
    ExistingPlacement(BoardItemWithAsset),
    ExistingAsset {
        asset_id: i64,
        label: String,
        dimensions: (u32, u32),
    },
}

#[derive(Default)]
pub(crate) struct PlacementCreationBatchTracker {
    batches: Vec<PendingPlacementCreationBatch>,
}

#[derive(Default)]
struct PendingPlacementCreationBatch {
    target_board_id: i64,
    anchor_world: egui::Pos2,
    pending_queue_ids: HashSet<u64>,
    loaded_item_ids: Vec<i64>,
}

pub(crate) fn visual_size_for_dimensions(
    width: u32,
    height: u32,
    target_canvas_area: f32,
) -> egui::Vec2 {
    let width = width.max(1);
    let height = height.max(1);
    let scale = crate::jobs::initial_import_scale(width, height, true, target_canvas_area);
    egui::vec2(width as f32 * scale, height as f32 * scale)
}

#[allow(dead_code)]
pub(crate) fn canvas_input_policy(placement_pending: bool) -> crate::canvas::CanvasInputPolicy {
    if placement_pending {
        crate::canvas::CanvasInputPolicy::ViewportOnly
    } else {
        crate::canvas::CanvasInputPolicy::Full
    }
}

pub(crate) fn fit_selection_bounds_if_needed(
    canvas: &mut CanvasState,
    bounds: egui::Rect,
    canvas_rect: egui::Rect,
    visible_canvas_rect: egui::Rect,
) -> bool {
    canvas.zoom_out_to_fit_world_rect(
        canvas_rect,
        visible_canvas_rect,
        bounds,
        crate::canvas::FOCUS_SELECTION_PADDING_SCREEN,
    )
}

#[cfg(test)]
pub(crate) fn placement_bounds(placements: &[(egui::Pos2, egui::Vec2)]) -> Option<egui::Rect> {
    let mut min = egui::pos2(f32::INFINITY, f32::INFINITY);
    let mut max = egui::pos2(f32::NEG_INFINITY, f32::NEG_INFINITY);
    let mut found = false;
    for (center, size) in placements {
        let half_size = *size * 0.5;
        min.x = min.x.min(center.x - half_size.x);
        min.y = min.y.min(center.y - half_size.y);
        max.x = max.x.max(center.x + half_size.x);
        max.y = max.y.max(center.y + half_size.y);
        found = true;
    }
    found.then(|| egui::Rect::from_min_max(min, max))
}

pub(crate) fn plan_source_intent_previews(
    canvas: &mut CanvasState,
    intents: &[SourceIntent],
    anchor_world: egui::Pos2,
    screen_rect: egui::Rect,
    settings: PlacementCreationSettings,
) -> Vec<SourceIntentPreview> {
    let uses_approximate_dimensions = intents.iter().any(|intent| intent.approximate_dimensions);
    let sizes = intents
        .iter()
        .map(|intent| {
            let (width, height) = normalized_dimensions(intent);
            visual_size_for_dimensions(width, height, settings.target_canvas_area)
        })
        .collect::<Vec<_>>();
    let centers = if uses_approximate_dimensions && sizes.len() > 1 {
        canvas.balanced_guided_preview_placeholders(&sizes, anchor_world, screen_rect, settings.gap)
    } else {
        canvas.compact_guided_preview_placeholders(&sizes, anchor_world, screen_rect, settings.gap)
    };

    intents
        .iter()
        .zip(centers)
        .map(|(intent, (center_world, _))| SourceIntentPreview {
            label: intent.label.clone(),
            dimensions: Some(normalized_dimensions(intent)),
            center_world,
        })
        .collect()
}

pub(crate) fn create_committed_loading_placement(
    library: &Library,
    content_resolution: &mut ContentResolutionRuntime,
    canvas: &mut CanvasState,
    layout: &mut Vec<crate::data::BoardLayoutItem>,
    target_board_id: i64,
    intent: SourceIntent,
    center_world: egui::Pos2,
    settings: PlacementCreationSettings,
) -> Result<CreatedLoadingPlacement, String> {
    let (width, height) = normalized_dimensions(&intent);
    let size_world = visual_size_for_dimensions(width, height, settings.target_canvas_area);
    let record = library.persist_committed_loading_placement(
        center_world,
        1.0,
        size_world.x.max(1.0),
        size_world.y.max(1.0),
        intent.content,
        Some("Loading".to_owned()),
    )?;
    let reservation =
        content_resolution.reserve_placement(target_board_id, record.item.id, center_world);

    layout.push(crate::data::BoardLayoutItem {
        id: record.item.id,
        x: record.item.x,
        y: record.item.y,
        width: record.scaled_visual_size().x,
        height: record.scaled_visual_size().y,
    });
    canvas.add_unloaded_item(record.clone());
    canvas.select_items(&[record.item.id]);

    Ok(CreatedLoadingPlacement {
        reservation,
        record,
    })
}

pub(crate) fn create_committed_loading_placements(
    library: &Library,
    content_resolution: &mut ContentResolutionRuntime,
    canvas: &mut CanvasState,
    layout: &mut Vec<crate::data::BoardLayoutItem>,
    target_board_id: i64,
    intents: Vec<SourceIntent>,
    anchor_world: egui::Pos2,
    screen_rect: egui::Rect,
    settings: PlacementCreationSettings,
) -> Result<Vec<CreatedLoadingPlacement>, String> {
    let previews =
        plan_source_intent_previews(canvas, &intents, anchor_world, screen_rect, settings);
    let mut created = Vec::with_capacity(intents.len());
    for (intent, preview) in intents.into_iter().zip(previews) {
        created.push(create_committed_loading_placement(
            library,
            content_resolution,
            canvas,
            layout,
            target_board_id,
            intent,
            preview.center_world,
            settings,
        )?);
    }
    let item_ids = created
        .iter()
        .map(|created| created.record.item.id)
        .collect::<Vec<_>>();
    canvas.select_items(&item_ids);
    Ok(created)
}

fn normalized_dimensions(intent: &SourceIntent) -> (u32, u32) {
    intent
        .dimensions
        .unwrap_or((FALLBACK_IMPORT_WIDTH, FALLBACK_IMPORT_HEIGHT))
}

pub(crate) fn create_provisional_placements(
    canvas: &mut CanvasState,
    intents: Vec<ProvisionalSourceIntent>,
    anchor_world: egui::Pos2,
    screen_rect: egui::Rect,
    settings: PlacementCreationSettings,
) -> Vec<i64> {
    let sizes = intents
        .iter()
        .map(|intent| match intent {
            ProvisionalSourceIntent::ExistingPlacement(record) => record.scaled_visual_size(),
            ProvisionalSourceIntent::ExistingAsset { dimensions, .. } => {
                visual_size_for_dimensions(dimensions.0, dimensions.1, settings.target_canvas_area)
            }
        })
        .collect::<Vec<_>>();
    let centers =
        canvas.compact_guided_preview_placeholders(&sizes, anchor_world, screen_rect, settings.gap);
    let mut next_id = canvas.next_provisional_item_id();
    let mut provisional_ids = Vec::with_capacity(intents.len());

    for (intent, (center_world, _size_world)) in intents.into_iter().zip(centers) {
        match intent {
            ProvisionalSourceIntent::ExistingPlacement(mut record) => {
                record.item.id = next_id;
                record.item.x = center_world.x;
                record.item.y = center_world.y;
                record.item.z_index = next_id as i32;
                record.lifecycle = PlacementLifecycle::Provisional;
                canvas.add_provisional_reference_item(record);
            }
            ProvisionalSourceIntent::ExistingAsset {
                asset_id,
                label,
                dimensions,
            } => {
                let record = provisional_asset_record(
                    next_id,
                    asset_id,
                    label,
                    dimensions,
                    center_world,
                    settings,
                );
                canvas.add_provisional_unloaded_item(record);
            }
        }
        provisional_ids.push(next_id);
        next_id -= 1;
    }

    canvas.select_items(&provisional_ids);
    provisional_ids
}

fn provisional_asset_record(
    item_id: i64,
    asset_id: i64,
    label: String,
    dimensions: (u32, u32),
    center_world: egui::Pos2,
    settings: PlacementCreationSettings,
) -> BoardItemWithAsset {
    let (width, height) = (dimensions.0.max(1), dimensions.1.max(1));
    let scale = crate::jobs::initial_import_scale(width, height, true, settings.target_canvas_area);
    let asset = BoardItemWithAsset::placeholder_asset(item_id, label, width, height);
    BoardItemWithAsset {
        item: BoardItem {
            id: item_id,
            asset_id,
            x: center_world.x,
            y: center_world.y,
            scale,
            rotation: 0.0,
            z_index: item_id as i32,
            archived: false,
        },
        asset,
        content: PlacementContent::Loading {
            intent: PlacementContentIntent::SearchAsset { asset_id },
            status_message: Some("Loading".to_owned()),
        },
        lifecycle: PlacementLifecycle::Provisional,
        visual_width: width as f32,
        visual_height: height as f32,
    }
}

#[allow(dead_code)]
pub(crate) fn create_existing_asset_placements(
    library: &Library,
    canvas: &mut CanvasState,
    layout: &mut Vec<crate::data::BoardLayoutItem>,
    _target_board_id: i64,
    asset_ids: Vec<i64>,
    preferred_centers: Vec<egui::Pos2>,
    anchor_world: egui::Pos2,
    screen_rect: egui::Rect,
    settings: PlacementCreationSettings,
) -> Result<Vec<CreatedLoadedPlacement>, String> {
    let assets = asset_ids
        .into_iter()
        .map(|asset_id| library.load_asset(asset_id))
        .collect::<Result<Vec<_>, _>>()?;

    let centers = if preferred_centers.len() == assets.len() {
        preferred_centers
    } else {
        let source_intents = assets
            .iter()
            .map(|asset| SourceIntent {
                label: asset.filename.clone(),
                dimensions: Some((asset.width, asset.height)),
                approximate_dimensions: false,
                content: PlacementContentIntent::SearchAsset { asset_id: asset.id },
            })
            .collect::<Vec<_>>();
        plan_source_intent_previews(canvas, &source_intents, anchor_world, screen_rect, settings)
            .into_iter()
            .map(|preview| preview.center_world)
            .collect()
    };

    let mut placements = Vec::with_capacity(assets.len());
    for (asset_index, asset) in assets.into_iter().enumerate() {
        let center_world = centers.get(asset_index).copied().unwrap_or(anchor_world);
        let scale = crate::jobs::initial_import_scale(
            asset.width,
            asset.height,
            true,
            settings.target_canvas_area,
        );
        let record =
            match library.create_board_item_from_asset(asset.id, center_world, scale, 0.0, 0) {
                Ok(record) => record,
                Err(error) => return Err(error),
            };
        layout.push(crate::data::BoardLayoutItem {
            id: record.item.id,
            x: record.item.x,
            y: record.item.y,
            width: record.scaled_visual_size().x,
            height: record.scaled_visual_size().y,
        });
        canvas.add_unloaded_item(record.clone());
        placements.push(CreatedLoadedPlacement {
            reservation: None,
            record,
        });
    }

    Ok(placements)
}

impl PlacementCreationBatchTracker {
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
        self.batches.push(PendingPlacementCreationBatch {
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
    ) -> Vec<CompletedPlacementCreationBatch> {
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
                completed_batches.push(CompletedPlacementCreationBatch {
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

#[cfg(test)]
mod tests {
    #[test]
    fn pending_placement_creation_keeps_viewport_navigation_available() {
        assert_eq!(
            super::canvas_input_policy(true),
            crate::canvas::CanvasInputPolicy::ViewportOnly
        );
        assert_eq!(
            super::canvas_input_policy(false),
            crate::canvas::CanvasInputPolicy::Full
        );
    }

    #[test]
    fn oversized_placement_creation_selection_zooms_out_to_fit_visible_canvas() {
        let mut canvas = crate::canvas::CanvasState::default();
        let canvas_rect = eframe::egui::Rect::from_min_size(
            eframe::egui::Pos2::ZERO,
            eframe::egui::vec2(800.0, 600.0),
        );
        let visible_rect = canvas_rect;
        let placements = vec![(
            eframe::egui::pos2(0.0, 0.0),
            eframe::egui::vec2(1600.0, 1200.0),
        )];

        let bounds = super::placement_bounds(&placements).unwrap();

        assert!(super::fit_selection_bounds_if_needed(
            &mut canvas,
            bounds,
            canvas_rect,
            visible_rect,
        ));
        assert!(canvas.viewport_state().camera_zoom < 1.0);
    }
}
