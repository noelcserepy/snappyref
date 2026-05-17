use eframe::egui;

use super::{CanvasState, DragState, SelectedLayoutItem};

pub(super) fn begin_selected_placement_drag(
    state: &mut CanvasState,
    pointer_world: egui::Pos2,
    now_seconds: f64,
) {
    state.interaction.drag_state = Some(DragState {
        pointer_start_world: pointer_world,
        starting_items: state.selected_layout_items(),
        snap_preview_rect_world: None,
        started_at_seconds: now_seconds,
        last_movement_seconds: now_seconds,
        last_world_delta: egui::Vec2::ZERO,
        moved: false,
    });
}

pub(super) fn update_selected_placement_drag(
    state: &mut CanvasState,
    screen_rect: egui::Rect,
    pointer_world: egui::Pos2,
    guide_snap_disabled: bool,
    now_seconds: f64,
) -> bool {
    let Some(drag_state) = state.interaction.drag_state.clone() else {
        return false;
    };

    let world_delta = pointer_world - drag_state.pointer_start_world;
    let snap_preview_rect = if guide_snap_disabled {
        state.clear_guide_snap_indicators();
        None
    } else {
        compute_drag_snap_preview(state, screen_rect, &drag_state, world_delta)
    };

    apply_dragged_item_positions(
        state,
        &drag_state.starting_items,
        world_delta,
        snap_preview_rect,
    );
    record_active_drag_movement(state, snap_preview_rect, world_delta, now_seconds);
    true
}

pub(super) fn move_selected_placement_group_to_center(
    state: &mut CanvasState,
    center_world: egui::Pos2,
    screen_rect: egui::Rect,
    now_seconds: f64,
) -> usize {
    let items = state.selected_layout_items();
    let Some(bounds) = layout_bounds_with_offset(&items, egui::Vec2::ZERO) else {
        state.clear_guide_snap_indicators();
        return 0;
    };

    let mut delta = center_world - bounds.center();
    let Some(desired_bounds) = layout_bounds_with_offset(&items, delta) else {
        state.clear_guide_snap_indicators();
        return 0;
    };
    let excluded_ids = items.iter().map(|item| item.id).collect::<Vec<_>>();
    let previously_snapped = state
        .interaction
        .drag_state
        .as_ref()
        .is_some_and(|drag| drag.snap_preview_rect_world.is_some())
        || !state.interaction.guide_snap_indicators.is_empty();

    if let Some(snap_result) = compute_guide_snap_for_rect(
        state,
        screen_rect,
        desired_bounds,
        &excluded_ids,
        previously_snapped,
    ) {
        let snap_offset = snap_result.rect.center() - desired_bounds.center();
        if snap_offset == egui::Vec2::ZERO {
            state.clear_guide_snap_indicators();
        } else {
            delta += snap_offset;
            state.set_guide_snap_indicators(snap_result.indicators);
        }
    } else {
        state.clear_guide_snap_indicators();
    }

    let moved = apply_dragged_item_positions(state, &items, delta, None);
    if delta.length_sq() > 0.01 {
        record_synthetic_drag_movement(state, now_seconds, delta);
    }
    moved
}

pub(super) fn compute_drag_snap_preview(
    state: &mut CanvasState,
    rect: egui::Rect,
    drag_state: &DragState,
    world_delta: egui::Vec2,
) -> Option<egui::Rect> {
    let Some(result) = compute_placement_guide_snap(state, rect, drag_state, world_delta) else {
        state.clear_guide_snap_indicators();
        return None;
    };
    let rect = result.rect;
    state.set_guide_snap_indicators(result.indicators);
    Some(rect)
}

fn compute_placement_guide_snap(
    state: &CanvasState,
    screen_rect: egui::Rect,
    drag_state: &DragState,
    world_delta: egui::Vec2,
) -> Option<super::guide_snap::GuideSnapResult> {
    let desired_rect = layout_bounds_with_offset(&drag_state.starting_items, world_delta)?;
    let excluded_ids = drag_state
        .starting_items
        .iter()
        .map(|item| item.id)
        .collect::<Vec<_>>();
    compute_guide_snap_for_rect(
        state,
        screen_rect,
        desired_rect,
        &excluded_ids,
        drag_state.snap_preview_rect_world.is_some(),
    )
}

pub(super) fn compute_guide_snap_for_rect(
    state: &CanvasState,
    screen_rect: egui::Rect,
    desired_rect: egui::Rect,
    excluded_ids: &[i64],
    previously_snapped: bool,
) -> Option<super::guide_snap::GuideSnapResult> {
    let candidates = guide_snap_candidate_rects(state, screen_rect, excluded_ids);
    super::guide_snap::snap_rect(super::guide_snap::GuideSnapRequest {
        desired_rect,
        candidates: &candidates,
        gap: state.layout_gap,
        threshold: super::guide_snap::threshold_world(state.zoom, previously_snapped),
    })
}

fn guide_snap_candidate_rects(
    state: &CanvasState,
    screen_rect: egui::Rect,
    excluded_item_ids: &[i64],
) -> Vec<super::guide_snap::GuideSnapCandidate> {
    let viewport = viewport_near_world_rect(state, screen_rect);
    state
        .document
        .images
        .iter()
        .filter(|image| !excluded_item_ids.contains(&image.record.item.id))
        .filter_map(|image| {
            let scaled_size = image.record.scaled_visual_size();
            let visual_size = super::rotated_visual_size(
                scaled_size.x,
                scaled_size.y,
                image.record.item.rotation,
            );
            let rect = egui::Rect::from_center_size(
                egui::pos2(image.record.item.x, image.record.item.y),
                visual_size,
            );
            rect.intersects(viewport)
                .then_some(super::guide_snap::GuideSnapCandidate::new(rect))
        })
        .collect()
}

fn viewport_near_world_rect(state: &CanvasState, screen_rect: egui::Rect) -> egui::Rect {
    let expanded = screen_rect.expand(super::guide_snap::VIEWPORT_PADDING_SCREEN);
    egui::Rect::from_two_pos(
        state.screen_to_world(expanded.min, screen_rect),
        state.screen_to_world(expanded.max, screen_rect),
    )
}

pub(super) fn layout_bounds_with_offset(
    items: &[SelectedLayoutItem],
    offset: egui::Vec2,
) -> Option<egui::Rect> {
    let mut bounds = egui::Rect::NOTHING;
    let mut found = false;
    for item in items {
        let rect = egui::Rect::from_center_size(
            egui::pos2(item.x + offset.x, item.y + offset.y),
            egui::vec2(item.width, item.height),
        );
        if found {
            bounds = bounds.union(rect);
        } else {
            bounds = rect;
            found = true;
        }
    }
    found.then_some(bounds)
}

fn apply_dragged_item_positions(
    state: &mut CanvasState,
    items: &[SelectedLayoutItem],
    world_delta: egui::Vec2,
    snap_preview_rect: Option<egui::Rect>,
) -> usize {
    let snap_offset = snap_preview_rect
        .and_then(|snap_rect| {
            layout_bounds_with_offset(items, world_delta)
                .map(|bounds| snap_rect.center() - bounds.center())
        })
        .unwrap_or(egui::Vec2::ZERO);
    let mut moved = 0;

    for item in items {
        if let Some(image) = state
            .document
            .images
            .iter_mut()
            .find(|image| image.record.item.id == item.id)
        {
            image.record.item.x = item.x + world_delta.x + snap_offset.x;
            image.record.item.y = item.y + world_delta.y + snap_offset.y;
            moved += 1;
        }
    }

    moved
}

fn record_active_drag_movement(
    state: &mut CanvasState,
    snap_preview_rect: Option<egui::Rect>,
    world_delta: egui::Vec2,
    now_seconds: f64,
) {
    if let Some(active_drag_state) = &mut state.interaction.drag_state {
        active_drag_state.snap_preview_rect_world = snap_preview_rect;
        if (world_delta - active_drag_state.last_world_delta).length_sq() > 0.01 {
            active_drag_state.last_movement_seconds = now_seconds;
            active_drag_state.last_world_delta = world_delta;
        }
        active_drag_state.moved = true;
    }
}

fn record_synthetic_drag_movement(
    state: &mut CanvasState,
    now_seconds: f64,
    world_delta: egui::Vec2,
) {
    let starting_items = state.selected_layout_items();
    if starting_items.is_empty() {
        return;
    }

    match &mut state.interaction.drag_state {
        Some(drag_state) => {
            if (world_delta - drag_state.last_world_delta).length_sq() > 0.01 {
                drag_state.last_movement_seconds = now_seconds;
                drag_state.last_world_delta = world_delta;
            }
            drag_state.moved = true;
        }
        None => {
            state.interaction.drag_state = Some(DragState {
                pointer_start_world: egui::Pos2::ZERO,
                starting_items,
                snap_preview_rect_world: None,
                started_at_seconds: now_seconds,
                last_movement_seconds: now_seconds,
                last_world_delta: world_delta,
                moved: true,
            });
        }
    }
}
