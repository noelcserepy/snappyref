use eframe::egui;

use crate::canvas::CanvasState;
use crate::data::Placement;
use crate::library::storage::Library;
use crate::placement::creation::{
    PlacementCreationSettings, ProvisionalSourceIntent, create_provisional_placements,
};

// Provisional Placement batches are real canvas Placements with negative
// temporary ids. Commit swaps them in place for persisted ids; cancel removes
// them without touching history.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct ProvisionalPlacementBatch {
    pub(crate) placement_ids: Vec<i64>,
}

#[derive(Clone, Debug)]
pub(crate) struct CommitSearchProvisionalRequest {
    pub(crate) asset_ids: Vec<i64>,
    pub(crate) provisional_placement_ids: Vec<i64>,
}

/// Placements swapped from provisional to committed ids, with the history
/// action that undoes the whole batch. Empty when nothing was provisional.
#[derive(Clone, Debug)]
pub(crate) struct CommittedProvisionalBatch {
    pub(crate) placements: Vec<Placement>,
    pub(crate) history_action: Option<crate::active_board::history::HistoryAction>,
}

pub(crate) fn start_provisional_batch(
    canvas: &mut CanvasState,
    intents: Vec<ProvisionalSourceIntent>,
    anchor_world: egui::Pos2,
    screen_rect: egui::Rect,
    settings: PlacementCreationSettings,
) -> ProvisionalPlacementBatch {
    cancel_provisional_batch(canvas);
    ProvisionalPlacementBatch {
        placement_ids: create_provisional_placements(
            canvas,
            intents,
            anchor_world,
            screen_rect,
            settings,
        ),
    }
}

pub(crate) fn cancel_provisional_batch(canvas: &mut CanvasState) -> Vec<i64> {
    canvas.remove_provisional_placements()
}

pub(crate) fn commit_loading_provisional_batch(
    library: &Library,
    canvas: &mut CanvasState,
    placement_ids: &[i64],
) -> CommittedProvisionalBatch {
    let provisional_placements = placement_ids
        .iter()
        .filter_map(|placement_id| canvas.placement_for_id(*placement_id))
        .filter(|placement| placement.lifecycle == crate::data::PlacementLifecycle::Provisional)
        .collect::<Vec<_>>();
    let committed_pairs = library.prepare_committed_loading_placements(&provisional_placements);
    commit_provisional_pairs(canvas, committed_pairs, None)
}

pub(crate) fn commit_search_provisional_batch(
    library: &Library,
    canvas: &mut CanvasState,
    request: CommitSearchProvisionalRequest,
    settings: PlacementCreationSettings,
) -> Result<CommittedProvisionalBatch, String> {
    let mut provisional_placements = if request.asset_ids.is_empty() {
        Vec::new()
    } else {
        request
            .provisional_placement_ids
            .iter()
            .filter_map(|placement_id| canvas.placement_for_id(*placement_id))
            .collect::<Vec<_>>()
    };

    for placement in &mut provisional_placements {
        let Some(asset_id) = placement.asset_id() else {
            continue;
        };
        if !request.asset_ids.contains(&asset_id) {
            continue;
        }
        let asset = library.load_asset(asset_id)?;
        placement.state.geometry.size = egui::vec2(asset.width as f32, asset.height as f32);
        placement.state.geometry.scale = crate::jobs::display_scale_for_placement_target_area(
            asset.width,
            asset.height,
            true,
            settings.placement_target_area,
        );
    }

    let committed_pairs = library.prepare_committed_asset_placements(&provisional_placements)?;
    Ok(commit_provisional_pairs(
        canvas,
        committed_pairs,
        Some("Add search images"),
    ))
}

/// Swaps provisional placements for their committed records, marks them and
/// the stack order for persistence, selects them, and builds the undo action.
/// `label` overrides the default import label.
fn commit_provisional_pairs(
    canvas: &mut CanvasState,
    committed_pairs: Vec<(i64, Placement)>,
    label: Option<&str>,
) -> CommittedProvisionalBatch {
    if committed_pairs.is_empty() {
        return CommittedProvisionalBatch {
            placements: Vec::new(),
            history_action: None,
        };
    }
    let stack_before = canvas.canvas_stack_order();
    let placements = canvas.commit_provisional_placements_in_place(committed_pairs);
    for placement in &placements {
        canvas.mark_placement_for_persistence(placement.state.id);
    }
    canvas.mark_stack_order_for_persistence();
    let stack_after = canvas.canvas_stack_order();
    let placement_ids = placements
        .iter()
        .map(|placement| placement.state.id)
        .collect::<Vec<_>>();
    canvas.select_placements(&placement_ids);

    let history_action = crate::placement::creation::committed_batch_history_action(
        &placements,
        label,
        stack_before,
        stack_after,
    );
    CommittedProvisionalBatch {
        placements,
        history_action,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::CanvasStackItem;
    use crate::test_support::TempRoot;

    #[test]
    fn provisional_batch_start_selects_real_provisional_placements() {
        let mut canvas = CanvasState::default();
        let batch = start_provisional_batch(
            &mut canvas,
            vec![ProvisionalSourceIntent::ExistingAsset {
                asset_id: 7,
                dimensions: (100, 50),
            }],
            egui::Pos2::ZERO,
            egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0)),
            PlacementCreationSettings {
                placement_target_area: 10_000.0,
                gap: 16.0,
            },
        );

        assert_eq!(batch.placement_ids.len(), 1);
        assert_eq!(canvas.selected_placement_ids(), batch.placement_ids);
        assert_eq!(canvas.provisional_placements_in_selection_order().len(), 1);
    }

    #[test]
    fn provisional_batch_places_previews_above_existing_placements() {
        let mut canvas = CanvasState::default();
        canvas.add_unresolved_placement(Placement {
            state: crate::data::PlacementState::with_display_size(
                42,
                egui::Pos2::ZERO,
                egui::vec2(100.0, 100.0),
                1.0,
                0.0,
                12,
                false,
            ),
            content: crate::data::PlacementContent::Loading {
                source: crate::data::ContentResolutionSource::SearchAsset { asset_id: 42 },
                status_message: Some("Loading".to_owned()),
            },
            lifecycle: crate::data::PlacementLifecycle::Committed,
        });

        let batch = start_provisional_batch(
            &mut canvas,
            vec![
                ProvisionalSourceIntent::ExistingAsset {
                    asset_id: 7,
                    dimensions: (100, 50),
                },
                ProvisionalSourceIntent::ExistingAsset {
                    asset_id: 8,
                    dimensions: (100, 50),
                },
            ],
            egui::Pos2::ZERO,
            egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0)),
            PlacementCreationSettings {
                placement_target_area: 10_000.0,
                gap: 16.0,
            },
        );

        let z_indexes = batch
            .placement_ids
            .iter()
            .map(|placement_id| {
                canvas
                    .placement_for_id(*placement_id)
                    .unwrap()
                    .state
                    .geometry
                    .z_index
            })
            .collect::<Vec<_>>();

        assert_eq!(z_indexes, vec![13, 14]);
        assert_eq!(
            canvas.canvas_stack_order(),
            vec![
                CanvasStackItem::Placement(42),
                CanvasStackItem::Placement(batch.placement_ids[0]),
                CanvasStackItem::Placement(batch.placement_ids[1]),
            ]
        );
    }

    #[test]
    fn cancel_provisional_batch_removes_history_neutral_items() {
        let mut canvas = CanvasState::default();
        let batch = start_provisional_batch(
            &mut canvas,
            vec![ProvisionalSourceIntent::ExistingAsset {
                asset_id: 7,
                dimensions: (100, 50),
            }],
            egui::Pos2::ZERO,
            egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0)),
            PlacementCreationSettings {
                placement_target_area: 10_000.0,
                gap: 16.0,
            },
        );

        let removed = cancel_provisional_batch(&mut canvas);

        assert_eq!(removed, batch.placement_ids);
        assert!(
            canvas
                .provisional_placements_in_selection_order()
                .is_empty()
        );
        assert!(!canvas.take_document_changes().has_changes());
    }

    #[test]
    fn commit_search_asset_provisional_creates_committed_placement_and_clears_provisional() {
        let root = TempRoot::new("snappyref-placement-lifecycle-commit");
        let library = Library::initialize(root.to_path_buf()).unwrap();
        let source_path = library
            .store_rgba_image("source", &[255, 0, 0, 255], 1, 1)
            .unwrap();
        let source = library
            .persist_imported_image(&source_path, "image/png", 1, 1, 4, egui::Pos2::ZERO, 1.0)
            .unwrap();
        let mut canvas = CanvasState::default();
        let provisional = start_provisional_batch(
            &mut canvas,
            vec![ProvisionalSourceIntent::ExistingAsset {
                asset_id: source.require_loaded_asset().id,
                dimensions: (1, 1),
            }],
            egui::pos2(42.0, 24.0),
            egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0)),
            PlacementCreationSettings {
                placement_target_area: 10_000.0,
                gap: 16.0,
            },
        );

        let committed = commit_search_provisional_batch(
            &library,
            &mut canvas,
            CommitSearchProvisionalRequest {
                asset_ids: vec![source.require_loaded_asset().id],
                provisional_placement_ids: provisional.placement_ids,
            },
            PlacementCreationSettings {
                placement_target_area: 10_000.0,
                gap: 16.0,
            },
        )
        .unwrap();

        assert_eq!(committed.placements.len(), 1);
        assert!(
            canvas
                .provisional_placements_in_selection_order()
                .is_empty()
        );
        let placement = canvas
            .placement_for_id(committed.placements[0].state.id)
            .unwrap();
        assert_eq!(
            placement.lifecycle,
            crate::data::PlacementLifecycle::Committed
        );
        assert_eq!(
            canvas.selected_placement_ids(),
            vec![committed.placements[0].state.id]
        );
    }

    #[test]
    fn loading_provisional_history_uses_current_canvas_stack() {
        let root = TempRoot::new("snappyref-loading-provisional-history-stack");
        let library = Library::initialize(root.to_path_buf()).unwrap();
        let mut canvas = CanvasState::default();
        let existing = (1..=2)
            .map(|index| {
                library
                    .persist_committed_loading_placement(
                        egui::pos2(index as f32 * 10.0, 0.0),
                        1.0,
                        100.0,
                        50.0,
                        crate::data::ContentResolutionSource::Unknown {
                            description: format!("existing placement {index}"),
                        },
                        None,
                    )
                    .unwrap()
            })
            .collect::<Vec<_>>();
        for placement in &existing {
            canvas.add_unresolved_placement(placement.clone());
        }
        let in_memory_stack = vec![
            CanvasStackItem::Placement(existing[1].state.id),
            CanvasStackItem::Placement(existing[0].state.id),
        ];
        canvas.set_canvas_stack_order(in_memory_stack.clone());
        let provisional = start_provisional_batch(
            &mut canvas,
            vec![ProvisionalSourceIntent::SourceIntent(
                crate::placement::creation::SourceIntent {
                    dimensions: Some((80, 60)),
                    approximate_dimensions: false,
                    content: crate::data::ContentResolutionSource::Unknown {
                        description: "dropped image".to_owned(),
                    },
                },
            )],
            egui::pos2(42.0, 24.0),
            egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0)),
            PlacementCreationSettings {
                placement_target_area: 10_000.0,
                gap: 16.0,
            },
        );

        let committed =
            commit_loading_provisional_batch(&library, &mut canvas, &provisional.placement_ids);
        let crate::active_board::history::HistoryAction::Document(history_action) =
            committed.history_action.unwrap()
        else {
            panic!("expected document history action");
        };
        let stack_change = history_action.stack_order.as_ref().unwrap();

        assert_eq!(stack_change.before, Some(in_memory_stack));
    }
}
