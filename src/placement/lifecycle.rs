use eframe::egui;

use crate::canvas::CanvasState;
use crate::data::Placement;
use crate::library::storage::Library;
use crate::placement::creation::{
    PlacementCreationSettings, ProvisionalSourceIntent, create_provisional_placements,
};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct ProvisionalPlacementBatch {
    pub(crate) item_ids: Vec<i64>,
}

#[derive(Clone, Debug)]
pub(crate) struct CommitSearchProvisionalRequest {
    pub(crate) item_ids: Vec<i64>,
    pub(crate) asset_ids: Vec<i64>,
    pub(crate) provisional_item_ids: Vec<i64>,
    pub(crate) fallback_world: egui::Pos2,
}

#[derive(Clone, Debug)]
pub(crate) struct CommittedSearchProvisionalBatch {
    pub(crate) added_item_ids: Vec<i64>,
    pub(crate) load_existing_records: Vec<Placement>,
}

impl CommittedSearchProvisionalBatch {
    pub(crate) fn placed_count(&self) -> usize {
        self.added_item_ids.len()
    }
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
        item_ids: create_provisional_placements(
            canvas,
            intents,
            anchor_world,
            screen_rect,
            settings,
        ),
    }
}

pub(crate) fn cancel_provisional_batch(canvas: &mut CanvasState) -> Vec<i64> {
    canvas.remove_provisional_items()
}

pub(crate) fn commit_search_provisional_batch(
    library: &Library,
    canvas: &mut CanvasState,
    request: CommitSearchProvisionalRequest,
    settings: PlacementCreationSettings,
) -> Result<CommittedSearchProvisionalBatch, String> {
    if request.item_ids.is_empty() && request.asset_ids.is_empty() {
        return Ok(CommittedSearchProvisionalBatch {
            added_item_ids: Vec::new(),
            load_existing_records: Vec::new(),
        });
    }

    let provisional_records = request
        .provisional_item_ids
        .iter()
        .filter_map(|item_id| canvas.record_for_item_id(*item_id))
        .collect::<Vec<_>>();
    let drop_world =
        source_center_for_records(&provisional_records).unwrap_or(request.fallback_world);
    let source_center = source_center_for_records(
        &request
            .item_ids
            .iter()
            .filter_map(|item_id| canvas.record_for_item_id(*item_id))
            .collect::<Vec<_>>(),
    )
    .unwrap_or(drop_world);

    let duplicated = library.duplicate_placements(&request.item_ids, drop_world, source_center)?;
    let item_provisionals = provisional_records
        .iter()
        .take(request.item_ids.len())
        .collect::<Vec<_>>();

    let mut added_item_ids = Vec::new();
    for (index, mut record) in duplicated.into_iter().enumerate() {
        if let Some(provisional) = item_provisionals.get(index) {
            record.state.center.x = provisional.state.center.x;
            record.state.center.y = provisional.state.center.y;
            library.update_placement_state(&record.state)?;
        }
        added_item_ids.push(record.state.id);
        canvas.add_reference_item(record)?;
    }

    let asset_provisionals = provisional_records
        .iter()
        .filter(|record| {
            record
                .state
                .asset_id
                .is_some_and(|asset_id| request.asset_ids.contains(&asset_id))
        })
        .cloned()
        .collect::<Vec<_>>();
    let mut load_existing_records = Vec::with_capacity(asset_provisionals.len());
    for provisional in asset_provisionals {
        let asset_id = provisional.state.asset_id.ok_or_else(|| {
            format!(
                "provisional placement {} has no asset",
                provisional.state.id
            )
        })?;
        let asset = library.load_asset(asset_id)?;
        let scale = crate::jobs::initial_import_scale(
            asset.width,
            asset.height,
            true,
            settings.target_canvas_area,
        );
        let record = library.create_placement_from_asset(
            asset_id,
            provisional.state.center,
            scale,
            provisional.state.rotation,
            provisional.state.z_index,
        )?;
        added_item_ids.push(record.state.id);
        canvas.add_unloaded_item(record.clone());
        load_existing_records.push(record);
    }

    if !added_item_ids.is_empty() {
        cancel_provisional_batch(canvas);
        canvas.select_items(&added_item_ids);
    }

    Ok(CommittedSearchProvisionalBatch {
        added_item_ids,
        load_existing_records,
    })
}

fn source_center_for_records(records: &[Placement]) -> Option<egui::Pos2> {
    let mut min = egui::Pos2::new(f32::INFINITY, f32::INFINITY);
    let mut max = egui::Pos2::new(f32::NEG_INFINITY, f32::NEG_INFINITY);
    let mut found = false;

    for record in records {
        found = true;
        let half_size = record.scaled_visual_size() * 0.5;
        min.x = min.x.min(record.state.center.x - half_size.x);
        min.y = min.y.min(record.state.center.y - half_size.y);
        max.x = max.x.max(record.state.center.x + half_size.x);
        max.y = max.y.max(record.state.center.y + half_size.y);
    }

    found.then(|| egui::pos2((min.x + max.x) * 0.5, (min.y + max.y) * 0.5))
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

        assert_eq!(batch.item_ids.len(), 1);
        assert_eq!(canvas.selected_item_ids(), batch.item_ids);
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

        assert_eq!(removed, batch.item_ids);
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
                item_ids: Vec::new(),
                asset_ids: vec![source.asset.as_ref().expect("loaded placement asset").id],
                provisional_item_ids: provisional.item_ids,
                fallback_world: egui::Pos2::ZERO,
            },
            PlacementCreationSettings {
                target_canvas_area: 10_000.0,
                gap: 16.0,
            },
        )
        .unwrap();

        assert_eq!(committed.added_item_ids.len(), 1);
        assert_eq!(committed.load_existing_records.len(), 1);
        assert!(canvas.provisional_records_in_selection_order().is_empty());
        let record = canvas
            .record_for_item_id(committed.added_item_ids[0])
            .unwrap();
        assert_eq!(record.lifecycle, crate::data::PlacementLifecycle::Committed);
        assert_eq!(canvas.selected_item_ids(), committed.added_item_ids);

        let _ = std::fs::remove_dir_all(root);
    }
}
