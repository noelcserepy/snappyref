use eframe::egui;

use crate::canvas::CanvasState;

use super::controller::{SearchController, SearchPlacementCreationConfig};
use super::selection::SearchSelectionMode;

impl SearchController {
    pub(super) fn start_result_drag(&mut self, index: usize) {
        crate::diagnostics::search_drag_debug(format!(
            "start_result_drag index={index} selected_before={:?} result_count={}",
            self.overlay.result_selection.selected(),
            self.overlay.results.len()
        ));
        if !self.overlay.result_selection.contains(index) {
            self.update_result_selection(index, SearchSelectionMode::Replace);
        }

        let asset_ids = self.selected_result_asset_ids();
        if asset_ids.is_empty() {
            crate::diagnostics::search_drag_debug("start_result_drag no_asset_ids");
            return;
        }

        crate::diagnostics::search_drag_debug(format!(
            "start_result_drag set_drag_state asset_ids={asset_ids:?} selected_after={:?}",
            self.overlay.result_selection.selected()
        ));
        self.overlay.drag_state = Some(crate::search::palette::SearchResultDragState {
            asset_ids,
            outside_overlay: false,
            provisional_batch_active: false,
            auto_fit_applied: false,
        });
    }

    pub(super) fn start_keyboard_placement_for_selected_results(
        &mut self,
        ctx: &egui::Context,
        canvas: &mut CanvasState,
        pointer_pos: Option<egui::Pos2>,
        screen_rect: egui::Rect,
        placement_creation_config: SearchPlacementCreationConfig,
    ) {
        let asset_ids = self.selected_result_asset_ids();
        self.start_keyboard_placement(
            asset_ids,
            ctx,
            canvas,
            pointer_pos,
            screen_rect,
            placement_creation_config,
        );
    }

    pub(super) fn start_keyboard_placement_for_all_results(
        &mut self,
        ctx: &egui::Context,
        canvas: &mut CanvasState,
        pointer_pos: Option<egui::Pos2>,
        screen_rect: egui::Rect,
        placement_creation_config: SearchPlacementCreationConfig,
    ) {
        let asset_ids = self.all_result_asset_ids();
        self.start_keyboard_placement(
            asset_ids,
            ctx,
            canvas,
            pointer_pos,
            screen_rect,
            placement_creation_config,
        );
    }

    fn start_keyboard_placement(
        &mut self,
        asset_ids: Vec<i64>,
        ctx: &egui::Context,
        canvas: &mut CanvasState,
        pointer_pos: Option<egui::Pos2>,
        screen_rect: egui::Rect,
        placement_creation_config: SearchPlacementCreationConfig,
    ) {
        if asset_ids.is_empty() {
            return;
        }

        let pointer_world = pointer_pos
            .map(|pos| canvas.screen_to_world(pos, screen_rect))
            .unwrap_or_else(|| canvas.screen_to_world(screen_rect.center(), screen_rect));
        crate::diagnostics::search_drag_debug(format!(
            "start_keyboard_placement asset_ids={asset_ids:?} pointer_pos={pointer_pos:?} pointer_world={pointer_world:?}"
        ));
        self.create_search_provisional_placements(
            canvas,
            &asset_ids,
            pointer_world,
            screen_rect,
            placement_creation_config,
        );
        self.overlay.drag_state = Some(crate::search::palette::SearchResultDragState {
            asset_ids,
            outside_overlay: true,
            provisional_batch_active: true,
            auto_fit_applied: false,
        });
        let _ = ctx;
    }

    pub(super) fn cancel_search_provisional_batch(&mut self, canvas: &mut CanvasState) {
        let ids = canvas
            .provisional_placements_in_selection_order()
            .into_iter()
            .map(|placement| placement.state.id)
            .collect::<Vec<_>>();
        crate::diagnostics::search_drag_debug(format!(
            "cancel_search_provisional_batch ids={ids:?}"
        ));
        self.overlay.drag_state = None;
        crate::placement::lifecycle::cancel_provisional_batch(canvas);
    }

    fn create_search_provisional_placements(
        &self,
        canvas: &mut CanvasState,
        asset_ids: &[i64],
        anchor_world: egui::Pos2,
        screen_rect: egui::Rect,
        placement_creation_config: SearchPlacementCreationConfig,
    ) {
        let source_intents = asset_ids
            .iter()
            .map(|asset_id| {
                let (width, height) = self
                    .overlay
                    .results
                    .iter()
                    .find(|group| group.asset_id == *asset_id)
                    .map(|group| (group.asset_width.max(1), group.asset_height.max(1)))
                    .unwrap_or((1, 1));
                crate::placement::creation::ProvisionalSourceIntent::ExistingAsset {
                    asset_id: *asset_id,
                    dimensions: (width, height),
                }
            })
            .collect::<Vec<_>>();

        let batch = crate::placement::lifecycle::start_provisional_batch(
            canvas,
            source_intents,
            anchor_world,
            screen_rect,
            crate::placement::creation::PlacementCreationSettings {
                placement_target_area: placement_creation_config.placement_target_area,
                gap: placement_creation_config.placement_gap,
            },
        );
        let selected = canvas.selected_placement_ids();
        crate::diagnostics::search_drag_debug(format!(
            "create_search_provisional_placements asset_ids={asset_ids:?} anchor_world={anchor_world:?} batch_ids={:?} selected={selected:?}",
            batch.placement_ids
        ));
    }

    pub(super) fn selected_result_asset_ids(&self) -> Vec<i64> {
        let asset_ids = self
            .overlay
            .result_selection
            .selected()
            .iter()
            .filter_map(|index| self.overlay.results.get(*index))
            .map(|group| group.asset_id)
            .collect::<Vec<_>>();

        if asset_ids.is_empty() {
            self.overlay
                .results
                .get(self.overlay.result_selection.lead().unwrap_or(0))
                .map(|group| group.asset_id)
                .into_iter()
                .collect()
        } else {
            asset_ids
        }
    }

    pub(super) fn all_result_asset_ids(&self) -> Vec<i64> {
        self.overlay
            .results
            .iter()
            .map(|group| group.asset_id)
            .collect()
    }
}

pub(super) fn update_search_provisional_batch(
    ctx: &egui::Context,
    canvas: &mut CanvasState,
    search: &SearchController,
    drag: &mut crate::search::palette::SearchResultDragState,
    pointer_world: egui::Pos2,
    pointer_screen: Option<egui::Pos2>,
    screen_rect: egui::Rect,
    placement_creation_config: SearchPlacementCreationConfig,
) {
    if !drag.provisional_batch_active {
        crate::diagnostics::search_drag_debug(format!(
            "update_search_provisional_batch create asset_ids={:?} pointer_world={pointer_world:?}",
            drag.asset_ids
        ));
        search.create_search_provisional_placements(
            canvas,
            &drag.asset_ids,
            pointer_world,
            screen_rect,
            placement_creation_config,
        );
        drag.provisional_batch_active = true;
    } else {
        let now_seconds = ctx.input(|input| input.time);
        canvas.move_selected_placement_group_to_center(pointer_world, screen_rect, now_seconds);
        crate::diagnostics::search_drag_debug(format!(
            "update_search_provisional_batch move pointer_world={pointer_world:?} selected={:?}",
            canvas.selected_placement_ids()
        ));
    }

    let provisional_bounds = canvas.selected_bounds_world();
    let center_delta_screen = provisional_bounds.map(|bounds| {
        let delta_world = bounds.center() - pointer_world;
        egui::vec2(delta_world.x * canvas.zoom, delta_world.y * canvas.zoom)
    });
    crate::diagnostics::search_drag_debug(format!(
        "update_search_provisional_batch bounds={provisional_bounds:?} center_delta_screen={center_delta_screen:?} zoom={} auto_fit_applied={}",
        canvas.zoom, drag.auto_fit_applied
    ));
    if !drag.auto_fit_applied
        && let Some(bounds) = provisional_bounds
        && crate::placement::creation::fit_selection_bounds_if_needed(
            canvas,
            bounds,
            screen_rect,
            placement_creation_config.visible_canvas_rect,
        )
    {
        drag.auto_fit_applied = true;
        if let Some(pointer_screen) = pointer_screen {
            let now_seconds = ctx.input(|input| input.time);
            let recentered_world = canvas.screen_to_world(pointer_screen, screen_rect);
            canvas.move_selected_placement_group_to_center(
                recentered_world,
                screen_rect,
                now_seconds,
            );
            crate::diagnostics::search_drag_debug(format!(
                "update_search_provisional_batch recenter_after_autofit pointer_screen={pointer_screen:?} pointer_world={recentered_world:?} selected_bounds={:?}",
                canvas.selected_bounds_world()
            ));
        }
    }
}

pub(super) fn provisional_placement_commit_requested(
    primary_clicked: bool,
    modifiers: egui::Modifiers,
) -> bool {
    primary_clicked && !modifiers.alt && !modifiers.command && !modifiers.ctrl && !modifiers.shift
}
