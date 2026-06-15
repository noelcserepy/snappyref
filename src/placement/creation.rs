use std::collections::HashSet;

use eframe::egui;

use crate::canvas::CanvasState;
use crate::data::{
    CanvasStackItem, ContentResolutionSource, Placement, PlacementContent, PlacementLifecycle,
    PlacementState, RectGeometry,
};
use crate::library::storage::Library;
use crate::placement::content_resolution::{
    ContentResolutionReservation, ContentResolutionRuntime,
};

// Placement Creation converts user source intent into real Placements. It owns
// preview geometry and initial loading state; source-specific adapters
// (import, clipboard, search) only describe intent and later resolve content.
const FALLBACK_IMPORT_WIDTH: u32 = 360;
const FALLBACK_IMPORT_HEIGHT: u32 = 240;

#[derive(Clone, Debug)]
pub(crate) struct SourceIntent {
    pub(crate) label: String,
    pub(crate) dimensions: Option<(u32, u32)>,
    pub(crate) approximate_dimensions: bool,
    pub(crate) content: ContentResolutionSource,
}

#[cfg_attr(not(test), allow(dead_code))]
#[derive(Clone, Debug)]
pub(crate) struct SourceIntentPreview {
    pub(crate) label: String,
    pub(crate) dimensions: Option<(u32, u32)>,
    pub(crate) geometry: RectGeometry,
}

impl SourceIntentPreview {
    pub(crate) fn center_world(&self) -> egui::Pos2 {
        self.geometry.center
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct PlacementCreationSettings {
    pub(crate) placement_target_area: f32,
    pub(crate) gap: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct CompletedPlacementCreationBatch {
    pub(crate) target_board_id: i64,
    pub(crate) anchor_world: egui::Pos2,
    pub(crate) placement_ids: Vec<i64>,
}

#[allow(dead_code)]
pub(crate) struct CreatedLoadedPlacement {
    pub(crate) reservation: Option<ContentResolutionReservation>,
    pub(crate) placement: Placement,
}

pub(crate) struct CreatedLoadingPlacement {
    pub(crate) reservation: ContentResolutionReservation,
    pub(crate) placement: Placement,
}

pub(crate) enum ProvisionalSourceIntent {
    SourceIntent(SourceIntent),
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
    loaded_placement_ids: Vec<i64>,
}

pub(crate) fn display_size_for_dimensions(
    width: u32,
    height: u32,
    placement_target_area: f32,
) -> egui::Vec2 {
    let width = width.max(1);
    let height = height.max(1);
    let scale = crate::jobs::display_scale_for_placement_target_area(
        width,
        height,
        true,
        placement_target_area,
    );
    egui::vec2(width as f32 * scale, height as f32 * scale)
}

#[allow(dead_code)]
pub(crate) fn canvas_input_policy(
    placement_creation_active: bool,
) -> crate::canvas::CanvasInputPolicy {
    if placement_creation_active {
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
            display_size_for_dimensions(width, height, settings.placement_target_area)
        })
        .collect::<Vec<_>>();
    let centers = if uses_approximate_dimensions && sizes.len() > 1 {
        canvas.balanced_guided_layout_for_sizes(&sizes, anchor_world, screen_rect, settings.gap)
    } else {
        canvas.compact_guided_layout_for_sizes(&sizes, anchor_world, screen_rect, settings.gap)
    };

    intents
        .iter()
        .zip(centers)
        .map(|(intent, (center_world, size_world))| SourceIntentPreview {
            label: intent.label.clone(),
            dimensions: Some(normalized_dimensions(intent)),
            geometry: RectGeometry::new(center_world, size_world, 1.0, 0.0, 0),
        })
        .collect()
}

pub(crate) fn create_committed_loading_placement(
    library: &Library,
    content_resolution: &mut ContentResolutionRuntime,
    canvas: &mut CanvasState,
    layout: &mut Vec<crate::placement::arrangement::PlacementArrangementInput>,
    target_board_id: i64,
    intent: SourceIntent,
    center_world: egui::Pos2,
    settings: PlacementCreationSettings,
) -> Result<CreatedLoadingPlacement, String> {
    let (width, height) = normalized_dimensions(&intent);
    let size_world = display_size_for_dimensions(width, height, settings.placement_target_area);
    let placement = library.persist_committed_loading_placement(
        center_world,
        1.0,
        size_world.x.max(1.0),
        size_world.y.max(1.0),
        intent.content,
        Some("Loading".to_owned()),
    )?;
    let reservation =
        content_resolution.reserve_placement(target_board_id, placement.state.id, center_world);

    layout.push(
        crate::placement::arrangement::PlacementArrangementInput::new(
            placement.state.id,
            placement.state.geometry.center,
            placement.state.geometry.scaled_size(),
        ),
    );
    canvas.add_unresolved_placement(placement.clone());
    canvas.select_placements(&[placement.state.id]);

    Ok(CreatedLoadingPlacement {
        reservation,
        placement,
    })
}

pub(crate) fn create_committed_loading_placements(
    library: &Library,
    content_resolution: &mut ContentResolutionRuntime,
    canvas: &mut CanvasState,
    layout: &mut Vec<crate::placement::arrangement::PlacementArrangementInput>,
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
            preview.center_world(),
            settings,
        )?);
    }
    let placement_ids = created
        .iter()
        .map(|created| created.placement.state.id)
        .collect::<Vec<_>>();
    canvas.select_placements(&placement_ids);
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
            ProvisionalSourceIntent::SourceIntent(intent) => {
                let (width, height) = normalized_dimensions(intent);
                display_size_for_dimensions(width, height, settings.placement_target_area)
            }
            ProvisionalSourceIntent::ExistingAsset { dimensions, .. } => {
                display_size_for_dimensions(
                    dimensions.0,
                    dimensions.1,
                    settings.placement_target_area,
                )
            }
        })
        .collect::<Vec<_>>();
    let centers =
        canvas.compact_guided_layout_for_sizes(&sizes, anchor_world, screen_rect, settings.gap);
    let mut next_id = canvas.next_provisional_placement_id();
    let mut next_z_index = next_front_placement_z_index(canvas);
    let mut provisional_ids = Vec::with_capacity(intents.len());

    for (intent, (center_world, _size_world)) in intents.into_iter().zip(centers) {
        match intent {
            ProvisionalSourceIntent::SourceIntent(intent) => {
                let placement = provisional_loading_placement(
                    next_id,
                    intent,
                    center_world,
                    next_z_index,
                    settings,
                );
                canvas.add_provisional_unresolved_placement(placement);
            }
            ProvisionalSourceIntent::ExistingAsset {
                asset_id,
                label,
                dimensions,
            } => {
                let placement = provisional_asset_placement(
                    next_id,
                    asset_id,
                    label,
                    dimensions,
                    center_world,
                    next_z_index,
                    settings,
                );
                canvas.add_provisional_unresolved_placement(placement);
            }
        }
        provisional_ids.push(next_id);
        next_id -= 1;
        next_z_index = next_z_index.saturating_add(1);
    }

    canvas.select_placements(&provisional_ids);
    provisional_ids
}

fn next_front_placement_z_index(canvas: &CanvasState) -> i32 {
    canvas
        .canvas_stack_order()
        .into_iter()
        .filter_map(|item| match item {
            CanvasStackItem::Placement(placement_id) => canvas
                .placement_for_id(placement_id)
                .map(|placement| placement.state.geometry.z_index),
            CanvasStackItem::Annotation(_) => None,
        })
        .max()
        .unwrap_or(0)
        .saturating_add(1)
}

fn provisional_loading_placement(
    placement_id: i64,
    intent: SourceIntent,
    center_world: egui::Pos2,
    z_index: i32,
    settings: PlacementCreationSettings,
) -> Placement {
    let (width, height) = normalized_dimensions(&intent);
    let size_world = display_size_for_dimensions(width, height, settings.placement_target_area);
    let _ = intent.label;
    let _ = intent.approximate_dimensions;
    Placement {
        state: PlacementState::with_display_size(
            placement_id,
            None,
            center_world,
            size_world,
            1.0,
            0.0,
            z_index,
            false,
        ),
        asset: None,
        content: PlacementContent::Loading {
            source: intent.content,
            status_message: Some("Loading".to_owned()),
        },
        lifecycle: PlacementLifecycle::Provisional,
    }
}

fn provisional_asset_placement(
    placement_id: i64,
    asset_id: i64,
    label: String,
    dimensions: (u32, u32),
    center_world: egui::Pos2,
    z_index: i32,
    settings: PlacementCreationSettings,
) -> Placement {
    let (width, height) = (dimensions.0.max(1), dimensions.1.max(1));
    let scale = crate::jobs::display_scale_for_placement_target_area(
        width,
        height,
        true,
        settings.placement_target_area,
    );
    let _ = label;
    Placement {
        state: PlacementState::with_display_size(
            placement_id,
            Some(asset_id),
            center_world,
            egui::vec2(width as f32, height as f32),
            scale,
            0.0,
            z_index,
            false,
        ),
        asset: None,
        content: PlacementContent::Loading {
            source: ContentResolutionSource::SearchAsset { asset_id },
            status_message: Some("Loading".to_owned()),
        },
        lifecycle: PlacementLifecycle::Provisional,
    }
}

#[allow(dead_code)]
pub(crate) fn create_existing_asset_placements(
    library: &Library,
    canvas: &mut CanvasState,
    layout: &mut Vec<crate::placement::arrangement::PlacementArrangementInput>,
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
                content: ContentResolutionSource::SearchAsset { asset_id: asset.id },
            })
            .collect::<Vec<_>>();
        plan_source_intent_previews(canvas, &source_intents, anchor_world, screen_rect, settings)
            .into_iter()
            .map(|preview| preview.center_world())
            .collect()
    };

    let mut placements = Vec::with_capacity(assets.len());
    for (asset_index, asset) in assets.into_iter().enumerate() {
        let center_world = centers.get(asset_index).copied().unwrap_or(anchor_world);
        let scale = crate::jobs::display_scale_for_placement_target_area(
            asset.width,
            asset.height,
            true,
            settings.placement_target_area,
        );
        let placement =
            match library.create_placement_from_asset(asset.id, center_world, scale, 0.0, 0) {
                Ok(placement) => placement,
                Err(error) => return Err(error),
            };
        layout.push(
            crate::placement::arrangement::PlacementArrangementInput::new(
                placement.state.id,
                placement.state.geometry.center,
                placement.state.geometry.scaled_size(),
            ),
        );
        canvas.add_unresolved_placement(placement.clone());
        placements.push(CreatedLoadedPlacement {
            reservation: None,
            placement,
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
            loaded_placement_ids: Vec::new(),
        });
    }

    pub(crate) fn finish_queue_id(
        &mut self,
        target_board_id: Option<i64>,
        queue_id: Option<u64>,
        loaded_placement_id: Option<i64>,
    ) -> Vec<CompletedPlacementCreationBatch> {
        let Some(queue_id) = queue_id else {
            return Vec::new();
        };

        let mut completed_batches = Vec::new();
        for batch in &mut self.batches {
            if !batch.pending_queue_ids.remove(&queue_id) {
                continue;
            }
            if let Some(placement_id) = loaded_placement_id
                && !batch.loaded_placement_ids.contains(&placement_id)
            {
                batch.loaded_placement_ids.push(placement_id);
            }
            if batch.pending_queue_ids.is_empty()
                && target_board_id.is_none_or(|id| id == batch.target_board_id)
            {
                completed_batches.push(CompletedPlacementCreationBatch {
                    target_board_id: batch.target_board_id,
                    anchor_world: batch.anchor_world,
                    placement_ids: std::mem::take(&mut batch.loaded_placement_ids),
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
    use super::*;

    #[test]
    fn active_placement_creation_keeps_viewport_navigation_available() {
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

    #[test]
    fn source_intent_preview_has_geometry_before_content_resolves() {
        let mut canvas = crate::canvas::CanvasState::default();
        let settings = PlacementCreationSettings {
            placement_target_area: 36_000.0,
            gap: 12.0,
        };

        let previews = plan_source_intent_previews(
            &mut canvas,
            &[SourceIntent {
                label: "clipboard".to_owned(),
                dimensions: None,
                approximate_dimensions: false,
                content: ContentResolutionSource::Clipboard,
            }],
            eframe::egui::pos2(120.0, 80.0),
            eframe::egui::Rect::from_min_size(
                eframe::egui::Pos2::ZERO,
                eframe::egui::vec2(800.0, 600.0),
            ),
            settings,
        );

        assert_eq!(previews.len(), 1);
        assert_eq!(previews[0].label, "clipboard");
        assert_eq!(
            previews[0].geometry.size,
            display_size_for_dimensions(
                FALLBACK_IMPORT_WIDTH,
                FALLBACK_IMPORT_HEIGHT,
                settings.placement_target_area,
            )
        );
    }

    #[test]
    fn provisional_source_intent_creates_real_loading_placement() {
        let mut canvas = crate::canvas::CanvasState::default();
        let ids = create_provisional_placements(
            &mut canvas,
            vec![ProvisionalSourceIntent::SourceIntent(SourceIntent {
                label: "clipboard".to_owned(),
                dimensions: Some((320, 240)),
                approximate_dimensions: false,
                content: ContentResolutionSource::Clipboard,
            })],
            eframe::egui::pos2(120.0, 80.0),
            eframe::egui::Rect::from_min_size(
                eframe::egui::Pos2::ZERO,
                eframe::egui::vec2(800.0, 600.0),
            ),
            PlacementCreationSettings {
                placement_target_area: 36_000.0,
                gap: 12.0,
            },
        );

        assert_eq!(canvas.selected_placement_ids(), ids);
        let placement = canvas.placement_for_id(ids[0]).unwrap();
        assert_eq!(placement.lifecycle, PlacementLifecycle::Provisional);
        assert!(placement.asset.is_none());
        assert!(matches!(
            placement.content,
            PlacementContent::Loading {
                source: ContentResolutionSource::Clipboard,
                ..
            }
        ));
    }

    #[test]
    fn provisional_existing_asset_intent_reuses_asset_as_loading_placement() {
        let mut canvas = crate::canvas::CanvasState::default();
        let ids = create_provisional_placements(
            &mut canvas,
            vec![ProvisionalSourceIntent::ExistingAsset {
                asset_id: 42,
                label: "existing.png".to_owned(),
                dimensions: (640, 480),
            }],
            eframe::egui::pos2(120.0, 80.0),
            eframe::egui::Rect::from_min_size(
                eframe::egui::Pos2::ZERO,
                eframe::egui::vec2(800.0, 600.0),
            ),
            PlacementCreationSettings {
                placement_target_area: 36_000.0,
                gap: 12.0,
            },
        );

        let placement = canvas.placement_for_id(ids[0]).unwrap();
        assert_eq!(placement.lifecycle, PlacementLifecycle::Provisional);
        assert_eq!(placement.state.asset_id, Some(42));
        assert!(placement.asset.is_none());
        assert!(matches!(
            placement.content,
            PlacementContent::Loading {
                source: ContentResolutionSource::SearchAsset { asset_id: 42 },
                ..
            }
        ));
    }
}
