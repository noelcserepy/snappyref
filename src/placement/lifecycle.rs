use eframe::egui;

use crate::canvas::CanvasState;
use crate::data::{CanvasStackItem, Placement};
use crate::library::storage::Library;
use crate::placement::creation::{
    PlacementCreationSettings, ProvisionalSourceIntent, create_provisional_placements,
};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct ProvisionalPlacementBatch {
    pub(crate) placement_ids: Vec<i64>,
}

#[derive(Clone, Debug)]
pub(crate) struct CommitSearchProvisionalRequest {
    pub(crate) asset_ids: Vec<i64>,
    pub(crate) provisional_placement_ids: Vec<i64>,
}

#[derive(Clone, Debug)]
pub(crate) struct CommittedSearchProvisionalBatch {
    pub(crate) added_placement_ids: Vec<i64>,
    pub(crate) load_existing_records: Vec<Placement>,
    pub(crate) history_action: Option<crate::active_board::history::HistoryAction>,
}

impl CommittedSearchProvisionalBatch {
    pub(crate) fn placed_count(&self) -> usize {
        self.added_placement_ids.len()
    }
}

#[derive(Clone, Debug)]
pub(crate) struct CommittedLoadingProvisionalBatch {
    pub(crate) placements: Vec<Placement>,
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
) -> Result<CommittedLoadingProvisionalBatch, String> {
    let provisional_records = placement_ids
        .iter()
        .filter_map(|placement_id| canvas.placement_for_id(*placement_id))
        .filter(|placement| placement.lifecycle == crate::data::PlacementLifecycle::Provisional)
        .collect::<Vec<_>>();
    if provisional_records.is_empty() {
        return Ok(CommittedLoadingProvisionalBatch {
            placements: Vec::new(),
        });
    }

    let committed_pairs = library.commit_provisional_loading_placements(&provisional_records)?;
    let placements = canvas.commit_provisional_placements_in_place(committed_pairs);
    let stack_after = canvas.canvas_stack_order();
    library.save_canvas_stack_order(&stack_after)?;
    let placement_ids = placements
        .iter()
        .map(|placement| placement.state.id)
        .collect::<Vec<_>>();
    canvas.select_placements(&placement_ids);

    Ok(CommittedLoadingProvisionalBatch { placements })
}

pub(crate) fn commit_search_provisional_batch(
    library: &Library,
    canvas: &mut CanvasState,
    request: CommitSearchProvisionalRequest,
    _settings: PlacementCreationSettings,
) -> Result<CommittedSearchProvisionalBatch, String> {
    if request.asset_ids.is_empty() {
        return Ok(CommittedSearchProvisionalBatch {
            added_placement_ids: Vec::new(),
            load_existing_records: Vec::new(),
            history_action: None,
        });
    }

    let mut provisional_records = request
        .provisional_placement_ids
        .iter()
        .filter_map(|placement_id| canvas.placement_for_id(*placement_id))
        .collect::<Vec<_>>();
    if provisional_records.is_empty() {
        return Ok(CommittedSearchProvisionalBatch {
            added_placement_ids: Vec::new(),
            load_existing_records: Vec::new(),
            history_action: None,
        });
    }

    for placement in &mut provisional_records {
        let Some(asset_id) = placement.state.asset_id else {
            continue;
        };
        if !request.asset_ids.contains(&asset_id) {
            continue;
        }
        let asset = library.load_asset(asset_id)?;
        placement.state.geometry.size = egui::vec2(asset.width as f32, asset.height as f32);
        placement.state.geometry.scale = crate::jobs::initial_import_scale(
            asset.width,
            asset.height,
            true,
            _settings.target_canvas_area,
        );
    }

    let stack_before = library.load_canvas_stack_order().ok();
    let committed_pairs = library.commit_provisional_placements(&provisional_records)?;
    let committed_records = canvas.commit_provisional_placements_in_place(committed_pairs);
    let stack_after = canvas.canvas_stack_order();
    library.save_canvas_stack_order(&stack_after)?;

    let added_placement_ids = committed_records
        .iter()
        .map(|placement| placement.state.id)
        .collect::<Vec<_>>();
    canvas.select_placements(&added_placement_ids);
    let load_existing_records = committed_records.clone();
    let history_action = Some(crate::active_board::history::HistoryAction {
        label: "Add search images".to_owned(),
        placements: committed_records
            .iter()
            .cloned()
            .map(|placement| crate::active_board::history::RowChange {
                before: None,
                after: Some(placement),
            })
            .collect(),
        annotations: Vec::new(),
        stack_order: stack_before.map(|before| crate::active_board::history::RowChange {
            before: Some(before),
            after: Some(
                stack_after
                    .into_iter()
                    .filter(|item| {
                        matches!(
                            item,
                            CanvasStackItem::Placement(_) | CanvasStackItem::Annotation(_)
                        )
                    })
                    .collect(),
            ),
        }),
    });

    Ok(CommittedSearchProvisionalBatch {
        added_placement_ids,
        load_existing_records,
        history_action,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(prefix: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "{prefix}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn provisional_batch_start_selects_real_provisional_placements() {
        let mut canvas = CanvasState::default();
        let batch = start_provisional_batch(
            &mut canvas,
            vec![ProvisionalSourceIntent::ExistingAsset {
                asset_id: 7,
                label: "asset-7".to_owned(),
                dimensions: (100, 50),
            }],
            egui::Pos2::ZERO,
            egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0)),
            PlacementCreationSettings {
                target_canvas_area: 10_000.0,
                gap: 16.0,
            },
        );

        assert_eq!(batch.placement_ids.len(), 1);
        assert_eq!(canvas.selected_placement_ids(), batch.placement_ids);
        assert_eq!(canvas.provisional_records_in_selection_order().len(), 1);
    }

    #[test]
    fn cancel_provisional_batch_removes_history_neutral_items() {
        let mut canvas = CanvasState::default();
        let batch = start_provisional_batch(
            &mut canvas,
            vec![ProvisionalSourceIntent::ExistingAsset {
                asset_id: 7,
                label: "asset-7".to_owned(),
                dimensions: (100, 50),
            }],
            egui::Pos2::ZERO,
            egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0)),
            PlacementCreationSettings {
                target_canvas_area: 10_000.0,
                gap: 16.0,
            },
        );

        let removed = cancel_provisional_batch(&mut canvas);

        assert_eq!(removed, batch.placement_ids);
        assert!(canvas.provisional_records_in_selection_order().is_empty());
        assert!(!canvas.take_document_changes().has_changes());
    }

    #[test]
    fn commit_search_asset_provisional_creates_committed_placement_and_clears_provisional() {
        let root = temp_root("snappyref-placement-lifecycle-commit");
        let library = Library::initialize(root.clone()).unwrap();
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
                asset_id: source.asset.as_ref().expect("loaded placement asset").id,
                label: "source".to_owned(),
                dimensions: (1, 1),
            }],
            egui::pos2(42.0, 24.0),
            egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0)),
            PlacementCreationSettings {
                target_canvas_area: 10_000.0,
                gap: 16.0,
            },
        );

        let committed = commit_search_provisional_batch(
            &library,
            &mut canvas,
            CommitSearchProvisionalRequest {
                asset_ids: vec![source.asset.as_ref().expect("loaded placement asset").id],
                provisional_placement_ids: provisional.placement_ids,
            },
            PlacementCreationSettings {
                target_canvas_area: 10_000.0,
                gap: 16.0,
            },
        )
        .unwrap();

        assert_eq!(committed.added_placement_ids.len(), 1);
        assert_eq!(committed.load_existing_records.len(), 1);
        assert!(canvas.provisional_records_in_selection_order().is_empty());
        let placement = canvas
            .placement_for_id(committed.added_placement_ids[0])
            .unwrap();
        assert_eq!(
            placement.lifecycle,
            crate::data::PlacementLifecycle::Committed
        );
        assert_eq!(
            canvas.selected_placement_ids(),
            committed.added_placement_ids
        );

        let _ = std::fs::remove_dir_all(root);
    }
}
