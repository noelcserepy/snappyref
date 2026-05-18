use eframe::egui;

use crate::canvas::CanvasState;

use super::controller::{SearchController, SearchPlacementPreviewConfig};
use super::selection::SearchSelectionMode;

impl SearchController {
    pub(super) fn start_result_drag(&mut self, index: usize) {
        if !self.overlay.result_selection.contains(index) {
            self.update_result_selection(index, SearchSelectionMode::Replace);
        }

        let placement_ids = self.selected_result_placement_ids();
        let asset_ids = self.selected_unplaced_result_asset_ids();
        if placement_ids.is_empty() && asset_ids.is_empty() {
            return;
        }

        self.overlay.drag_state = Some(crate::search::palette::SearchResultDragState {
            placement_ids,
            asset_ids,
            outside_overlay: false,
            placement_pending: false,
            auto_fit_applied: false,
        });
    }

    pub(super) fn start_keyboard_placement_for_selected_results(
        &mut self,
        ctx: &egui::Context,
        canvas: &mut CanvasState,
        pointer_pos: Option<egui::Pos2>,
        screen_rect: egui::Rect,
        placement_preview_config: SearchPlacementPreviewConfig,
    ) {
        let placement_ids = self.selected_result_placement_ids();
        let asset_ids = self.selected_unplaced_result_asset_ids();
        self.start_keyboard_placement(
            placement_ids,
            asset_ids,
            ctx,
            canvas,
            pointer_pos,
            screen_rect,
            placement_preview_config,
        );
    }

    pub(super) fn start_keyboard_placement_for_all_results(
        &mut self,
        ctx: &egui::Context,
        canvas: &mut CanvasState,
        pointer_pos: Option<egui::Pos2>,
        screen_rect: egui::Rect,
        placement_preview_config: SearchPlacementPreviewConfig,
    ) {
        let placement_ids = self.all_result_placement_ids();
        let asset_ids = self.all_unplaced_result_asset_ids();
        self.start_keyboard_placement(
            placement_ids,
            asset_ids,
            ctx,
            canvas,
            pointer_pos,
            screen_rect,
            placement_preview_config,
        );
    }

    fn start_keyboard_placement(
        &mut self,
        placement_ids: Vec<i64>,
        asset_ids: Vec<i64>,
        ctx: &egui::Context,
        canvas: &mut CanvasState,
        pointer_pos: Option<egui::Pos2>,
        screen_rect: egui::Rect,
        placement_preview_config: SearchPlacementPreviewConfig,
    ) {
        if placement_ids.is_empty() && asset_ids.is_empty() {
            return;
        }

        let pointer_world = pointer_pos
            .map(|pos| canvas.screen_to_world(pos, screen_rect))
            .unwrap_or_else(|| canvas.screen_to_world(screen_rect.center(), screen_rect));
        self.create_search_provisional_placements(
            canvas,
            &placement_ids,
            &asset_ids,
            pointer_world,
            screen_rect,
            placement_preview_config,
        );
        self.overlay.drag_state = Some(crate::search::palette::SearchResultDragState {
            placement_ids: placement_ids.clone(),
            asset_ids,
            outside_overlay: true,
            placement_pending: true,
            auto_fit_applied: false,
        });
        let _ = ctx;
    }

    pub(super) fn cancel_pending_placement(&mut self, canvas: &mut CanvasState) {
        self.overlay.drag_state = None;
        crate::placement::lifecycle::cancel_provisional_batch(canvas);
    }

    fn create_search_provisional_placements(
        &self,
        canvas: &mut CanvasState,
        placement_ids: &[i64],
        asset_ids: &[i64],
        anchor_world: egui::Pos2,
        screen_rect: egui::Rect,
        placement_preview_config: SearchPlacementPreviewConfig,
    ) {
        let mut source_intents = placement_ids
            .iter()
            .filter_map(|item_id| canvas.placement_for_id(*item_id))
            .map(crate::placement::creation::ProvisionalSourceIntent::ExistingPlacement)
            .collect::<Vec<_>>();

        for asset_id in asset_ids {
            let (width, height) = self
                .overlay
                .results
                .iter()
                .find(|group| group.asset_id == *asset_id)
                .map(|group| (group.asset_width.max(1), group.asset_height.max(1)))
                .unwrap_or((1, 1));
            source_intents.push(
                crate::placement::creation::ProvisionalSourceIntent::ExistingAsset {
                    asset_id: *asset_id,
                    label: format!("search-asset-{asset_id}"),
                    dimensions: (width, height),
                },
            );
        }

        crate::placement::lifecycle::start_provisional_batch(
            canvas,
            source_intents,
            anchor_world,
            screen_rect,
            crate::placement::creation::PlacementCreationSettings {
                target_canvas_area: placement_preview_config.import_target_canvas_area,
                gap: placement_preview_config.import_gap,
            },
        );
    }

    pub(super) fn selected_result_placement_ids(&self) -> Vec<i64> {
        if self.overlay.scope == crate::search::semantic::SearchScope::Library {
            return Vec::new();
        }

        let placement_ids = self
            .overlay
            .result_selection
            .selected()
            .iter()
            .filter_map(|index| self.overlay.results.get(*index))
            .filter_map(|group| group.placement_ids.first().copied())
            .collect::<Vec<_>>();

        if placement_ids.is_empty() {
            self.overlay
                .results
                .get(self.overlay.result_selection.lead().unwrap_or(0))
                .and_then(|group| group.placement_ids.first().copied())
                .into_iter()
                .collect()
        } else {
            placement_ids
        }
    }

    pub(super) fn all_result_placement_ids(&self) -> Vec<i64> {
        if self.overlay.scope == crate::search::semantic::SearchScope::Library {
            return Vec::new();
        }

        self.overlay
            .results
            .iter()
            .filter_map(|group| group.placement_ids.first().copied())
            .collect()
    }

    fn selected_unplaced_result_asset_ids(&self) -> Vec<i64> {
        if self.overlay.scope == crate::search::semantic::SearchScope::Library {
            let asset_ids = self
                .overlay
                .result_selection
                .selected()
                .iter()
                .filter_map(|index| self.overlay.results.get(*index))
                .map(|group| group.asset_id)
                .collect::<Vec<_>>();

            return if asset_ids.is_empty() {
                self.overlay
                    .results
                    .get(self.overlay.result_selection.lead().unwrap_or(0))
                    .map(|group| group.asset_id)
                    .into_iter()
                    .collect()
            } else {
                asset_ids
            };
        }

        let asset_ids = self
            .overlay
            .result_selection
            .selected()
            .iter()
            .filter_map(|index| self.overlay.results.get(*index))
            .filter(|group| group.placement_ids.is_empty())
            .map(|group| group.asset_id)
            .collect::<Vec<_>>();

        if asset_ids.is_empty() {
            self.overlay
                .results
                .get(self.overlay.result_selection.lead().unwrap_or(0))
                .filter(|group| group.placement_ids.is_empty())
                .map(|group| group.asset_id)
                .into_iter()
                .collect()
        } else {
            asset_ids
        }
    }

    fn all_unplaced_result_asset_ids(&self) -> Vec<i64> {
        if self.overlay.scope == crate::search::semantic::SearchScope::Library {
            return self
                .overlay
                .results
                .iter()
                .map(|group| group.asset_id)
                .collect();
        }

        self.overlay
            .results
            .iter()
            .filter(|group| group.placement_ids.is_empty())
            .map(|group| group.asset_id)
            .collect()
    }
}

pub(super) fn update_search_placement_preview(
    ctx: &egui::Context,
    canvas: &mut CanvasState,
    search: &SearchController,
    drag: &mut crate::search::palette::SearchResultDragState,
    pointer_world: egui::Pos2,
    screen_rect: egui::Rect,
    placement_preview_config: SearchPlacementPreviewConfig,
) {
    if !drag.placement_pending {
        search.create_search_provisional_placements(
            canvas,
            &drag.placement_ids,
            &drag.asset_ids,
            pointer_world,
            screen_rect,
            placement_preview_config,
        );
        drag.placement_pending = true;
    } else {
        let now_seconds = ctx.input(|input| input.time);
        canvas.move_selected_placement_group_to_center(pointer_world, screen_rect, now_seconds);
    }

    let preview_bounds = canvas.selected_bounds_world();
    if !drag.auto_fit_applied
        && let Some(bounds) = preview_bounds
        && crate::placement::creation::fit_selection_bounds_if_needed(
            canvas,
            bounds,
            screen_rect,
            placement_preview_config.visible_canvas_rect,
        )
    {
        drag.auto_fit_applied = true;
    }
    let _ = ctx;
}

pub(super) fn pending_placement_commit_intent(
    primary_clicked: bool,
    modifiers: egui::Modifiers,
) -> bool {
    primary_clicked && !modifiers.alt && !modifiers.command && !modifiers.ctrl && !modifiers.shift
}
