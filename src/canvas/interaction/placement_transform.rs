use eframe::egui;

use super::super::geometry::{
    min_loaded_placement_scale, placement_rotation_handle_screen_pos,
    placement_screen_rect_for_record,
};
use super::super::{
    CanvasState, PLACEMENT_RESIZE_HIT_RADIUS, PLACEMENT_ROTATE_HANDLE_HIT_RADIUS_SCREEN,
    PLACEMENT_ROTATE_HANDLE_OFFSET_SCREEN, PlacementResizeHandle, PlacementScaleStart,
    PlacementTransformState, RenderedPlacement,
};

const PLACEMENT_DRAG_SCALE_DOUBLING_DISTANCE_SCREEN: f32 = 240.0;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PlacementScaleTransform {
    pub(crate) placement_id: i64,
    pub(crate) center_world: egui::Pos2,
    pub(crate) scale: f32,
}

pub(crate) fn begin_placement_resize(
    state: &mut CanvasState,
    rect: egui::Rect,
    placement_id: i64,
    handle: PlacementResizeHandle,
) {
    let Some(image) = state.rendered_placement_by_id(placement_id) else {
        return;
    };
    let corners = super::super::geometry::placement_screen_corners_for_record(
        &image.placement,
        state.pan,
        state.zoom,
        rect.center(),
    );
    let start_handle_screen = placement_resize_handle_screen_pos(corners, handle);
    let anchor_screen = placement_resize_anchor_screen_pos(corners, handle);
    let start_scale = image.placement.state.geometry.scale;

    state.finish_textbox_edit();
    state.document.selected_placement_ids.clear();
    state.document.selected_placement_ids.push(placement_id);
    state.document.selected_annotation_ids.clear();
    state.interaction.drag_state = None;
    state.interaction.annotation_drag_state = None;
    state.interaction.marquee_state = None;
    state.interaction.crop_state = None;
    state.interaction.annotation_draft_state = None;
    state.interaction.textbox_transform_state = None;
    state.interaction.placement_transform_state = Some(PlacementTransformState::Resizing {
        placement_id,
        anchor_screen,
        start_handle_screen,
        start_scale,
    });
}

pub(crate) fn begin_placement_drag_scale(
    state: &mut CanvasState,
    rect: egui::Rect,
    hit_item_id: i64,
    pointer_pos: egui::Pos2,
) {
    if !state.is_selected(hit_item_id) {
        state.set_single_selection(hit_item_id);
    }
    let placement_ids = selected_loaded_placement_ids(state);
    if placement_ids.is_empty() {
        return;
    }
    let Some(selection_rect) =
        selected_placement_screen_rect_for_placements(state, rect, &placement_ids)
    else {
        return;
    };
    let group_center_world = state.screen_to_world(selection_rect.center(), rect);
    let starting_items = placement_ids
        .iter()
        .filter_map(|placement_id| {
            state.rendered_placement_by_id(*placement_id).map(|image| {
                PlacementScaleStart::new(
                    *placement_id,
                    egui::pos2(
                        image.placement.state.geometry.center.x,
                        image.placement.state.geometry.center.y,
                    ),
                    {
                        let base_size = image.placement.base_display_size();
                        super::super::rotated_display_size(
                            base_size.x,
                            base_size.y,
                            image.placement.state.geometry.rotation,
                        )
                    },
                    image.placement.state.geometry.scale,
                    min_loaded_placement_scale(image.placement.require_loaded_asset()),
                )
            })
        })
        .collect::<Vec<_>>();

    state.finish_textbox_edit();
    state.document.selected_annotation_ids.clear();
    state.interaction.drag_state = None;
    state.interaction.annotation_drag_state = None;
    state.interaction.marquee_state = None;
    state.interaction.crop_state = None;
    state.interaction.annotation_draft_state = None;
    state.interaction.textbox_transform_state = None;
    state.interaction.placement_transform_state = Some(PlacementTransformState::Scaling {
        placement_ids,
        pointer_start_screen: pointer_pos,
        group_center_world,
        starting_items,
    });
}

pub(crate) fn begin_placement_rotation(
    state: &mut CanvasState,
    rect: egui::Rect,
    pointer_pos: egui::Pos2,
) {
    let placement_ids = selected_loaded_placement_ids(state);
    if placement_ids.is_empty() {
        return;
    }
    let Some(selection_rect) = selected_placement_rotation_screen_rect(state, rect) else {
        return;
    };
    let center_screen = selection_rect.center();
    let center_world = state.screen_to_world(center_screen, rect);
    let start_pointer_delta = pointer_pos - center_screen;
    let start_pointer_angle = if start_pointer_delta.length_sq() <= f32::EPSILON {
        0.0
    } else {
        start_pointer_delta.angle()
    };
    let starting_items = placement_ids
        .iter()
        .filter_map(|placement_id| {
            state.rendered_placement_by_id(*placement_id).map(|image| {
                (
                    *placement_id,
                    egui::pos2(
                        image.placement.state.geometry.center.x,
                        image.placement.state.geometry.center.y,
                    ),
                    image.placement.state.geometry.rotation,
                )
            })
        })
        .collect::<Vec<_>>();

    state.finish_textbox_edit();
    state.interaction.drag_state = None;
    state.interaction.annotation_drag_state = None;
    state.interaction.marquee_state = None;
    state.interaction.crop_state = None;
    state.interaction.annotation_draft_state = None;
    state.interaction.textbox_transform_state = None;
    state.interaction.placement_transform_state = Some(PlacementTransformState::Rotating {
        placement_ids,
        center_screen,
        center_world,
        start_pointer_angle,
        starting_items,
    });
}

fn selected_placement_screen_rect_for_placements(
    state: &CanvasState,
    rect: egui::Rect,
    placement_ids: &[i64],
) -> Option<egui::Rect> {
    placement_ids
        .iter()
        .filter_map(|placement_id| state.rendered_placement_by_id(*placement_id))
        .map(|image| placement_screen_rect(state, rect, image))
        .reduce(|bounds, image_rect| bounds.union(image_rect))
}

pub(crate) fn placement_screen_rect(
    state: &CanvasState,
    rect: egui::Rect,
    image: &RenderedPlacement,
) -> egui::Rect {
    placement_screen_rect_for_record(&image.placement, rect, state.pan, state.zoom, rect.center())
}

fn placement_resize_handle_screen_pos(
    corners: [egui::Pos2; 4],
    handle: PlacementResizeHandle,
) -> egui::Pos2 {
    match handle {
        PlacementResizeHandle::TopLeft => corners[0],
        PlacementResizeHandle::TopRight => corners[1],
        PlacementResizeHandle::BottomRight => corners[2],
        PlacementResizeHandle::BottomLeft => corners[3],
    }
}

fn placement_resize_anchor_screen_pos(
    corners: [egui::Pos2; 4],
    handle: PlacementResizeHandle,
) -> egui::Pos2 {
    match handle {
        PlacementResizeHandle::TopLeft => corners[2],
        PlacementResizeHandle::TopRight => corners[3],
        PlacementResizeHandle::BottomRight => corners[0],
        PlacementResizeHandle::BottomLeft => corners[1],
    }
}

pub(crate) fn hit_test_selected_placement_rotation_handle(
    state: &CanvasState,
    rect: egui::Rect,
    pointer_pos: egui::Pos2,
) -> bool {
    let Some((_, handle_pos)) = selected_placement_rotation_handle_screen_pos(state, rect) else {
        return false;
    };
    handle_pos.distance(pointer_pos) <= PLACEMENT_ROTATE_HANDLE_HIT_RADIUS_SCREEN
}

pub(crate) fn selected_placement_rotation_handle_screen_pos(
    state: &CanvasState,
    rect: egui::Rect,
) -> Option<(egui::Pos2, egui::Pos2)> {
    if !state.document.selected_annotation_ids.is_empty() {
        return None;
    }

    if let Some(image) = state.selected_single_rendered_placement() {
        return Some(placement_rotation_handle_screen_pos(
            &image.placement,
            state.pan,
            state.zoom,
            rect.center(),
        ));
    }

    let rotation = selected_group_rotation_angle(state)?;
    selected_placement_rotation_screen_rect(state, rect)
        .map(|selection_rect| group_rotation_handle_screen_pos(selection_rect, rotation))
}

fn selected_placement_rotation_screen_rect(
    state: &CanvasState,
    rect: egui::Rect,
) -> Option<egui::Rect> {
    if selected_loaded_placement_ids(state).is_empty() {
        return None;
    }
    state.selection_screen_rect(rect)
}

fn selected_loaded_placement_ids(state: &CanvasState) -> Vec<i64> {
    state
        .document
        .selected_placement_ids
        .iter()
        .copied()
        .filter(|placement_id| state.rendered_placement_by_id(*placement_id).is_some())
        .collect()
}

fn selected_group_rotation_angle(state: &CanvasState) -> Option<f32> {
    let mut sin_sum = 0.0_f32;
    let mut cos_sum = 0.0_f32;
    let mut count = 0_usize;
    for placement_id in &state.document.selected_placement_ids {
        let Some(image) = state.rendered_placement_by_id(*placement_id) else {
            continue;
        };
        sin_sum += image.placement.state.geometry.rotation.sin();
        cos_sum += image.placement.state.geometry.rotation.cos();
        count += 1;
    }
    (count > 0).then(|| sin_sum.atan2(cos_sum))
}

pub(crate) fn group_rotation_handle_screen_pos(
    selection_rect: egui::Rect,
    rotation: f32,
) -> (egui::Pos2, egui::Pos2) {
    let center = selection_rect.center();
    let outward = egui::vec2(rotation.sin(), -rotation.cos());
    let mut distance = f32::INFINITY;
    if outward.x > f32::EPSILON {
        distance = distance.min((selection_rect.right() - center.x) / outward.x);
    } else if outward.x < -f32::EPSILON {
        distance = distance.min((selection_rect.left() - center.x) / outward.x);
    }
    if outward.y > f32::EPSILON {
        distance = distance.min((selection_rect.bottom() - center.y) / outward.y);
    } else if outward.y < -f32::EPSILON {
        distance = distance.min((selection_rect.top() - center.y) / outward.y);
    }

    let line_start = center + outward * distance.max(0.0);
    let handle_pos = line_start + outward * PLACEMENT_ROTATE_HANDLE_OFFSET_SCREEN;
    (line_start, handle_pos)
}

pub(crate) fn placement_resize_cursor_icon(handle: PlacementResizeHandle) -> egui::CursorIcon {
    match handle {
        PlacementResizeHandle::TopLeft | PlacementResizeHandle::BottomRight => {
            egui::CursorIcon::ResizeNwSe
        }
        PlacementResizeHandle::TopRight | PlacementResizeHandle::BottomLeft => {
            egui::CursorIcon::ResizeNeSw
        }
    }
}

pub(crate) fn hit_test_placement_resize_handle(
    state: &CanvasState,
    rect: egui::Rect,
    image: &RenderedPlacement,
    pointer_pos: egui::Pos2,
) -> Option<PlacementResizeHandle> {
    let corners = super::super::geometry::placement_screen_corners_for_record(
        &image.placement,
        state.pan,
        state.zoom,
        rect.center(),
    );
    [
        PlacementResizeHandle::TopLeft,
        PlacementResizeHandle::TopRight,
        PlacementResizeHandle::BottomLeft,
        PlacementResizeHandle::BottomRight,
    ]
    .into_iter()
    .find(|handle| {
        placement_resize_handle_screen_pos(corners, *handle).distance(pointer_pos)
            <= PLACEMENT_RESIZE_HIT_RADIUS
    })
}

pub(crate) fn resized_placement_transform(
    asset: &crate::data::Asset,
    start_scale: f32,
    anchor_screen: egui::Pos2,
    start_handle_screen: egui::Pos2,
    pointer_pos: egui::Pos2,
) -> (egui::Pos2, f32) {
    let start_delta = start_handle_screen - anchor_screen;
    let pointer_delta = pointer_pos - anchor_screen;
    let start_len_sq = start_delta.length_sq();
    let ratio = if start_len_sq <= f32::EPSILON {
        1.0
    } else {
        pointer_delta.dot(start_delta) / start_len_sq
    };
    let scale = (start_scale * ratio).max(min_loaded_placement_scale(asset));
    let normalized_ratio = scale / start_scale.max(f32::EPSILON);
    let handle_screen = anchor_screen + start_delta * normalized_ratio;
    (anchor_screen.lerp(handle_screen, 0.5), scale)
}

pub(crate) fn placement_drag_scale_factor(
    start_screen: egui::Pos2,
    pointer_pos: egui::Pos2,
) -> f32 {
    let dx = pointer_pos.x - start_screen.x;
    2.0_f32.powf(dx / PLACEMENT_DRAG_SCALE_DOUBLING_DISTANCE_SCREEN)
}

pub(crate) fn scaled_placement_transforms(
    starting_items: &[PlacementScaleStart],
    group_center_world: egui::Pos2,
    factor: f32,
) -> Vec<PlacementScaleTransform> {
    starting_items
        .iter()
        .map(|item| {
            let offset = item.center_world() - group_center_world;
            PlacementScaleTransform {
                placement_id: item.placement_id,
                center_world: group_center_world + offset * factor,
                scale: (item.start_scale() * factor).max(item.min_scale),
            }
        })
        .collect()
}

fn scaled_placement_bounds(
    starting_items: &[PlacementScaleStart],
    group_center_world: egui::Pos2,
    factor: f32,
) -> Option<egui::Rect> {
    let mut bounds = egui::Rect::NOTHING;
    let mut found = false;
    for item in starting_items {
        let offset = item.center_world() - group_center_world;
        let center = group_center_world + offset * factor;
        let scale = (item.start_scale() * factor).max(item.min_scale);
        let rect = egui::Rect::from_center_size(center, item.base_display_size() * scale);
        if found {
            bounds = bounds.union(rect);
        } else {
            bounds = rect;
            found = true;
        }
    }
    found.then_some(bounds)
}

pub(crate) fn rotated_placement_delta(
    center_screen: egui::Pos2,
    pointer_pos: egui::Pos2,
    start_pointer_angle: f32,
    snap_degrees: f32,
    unsnapped: bool,
) -> f32 {
    let pointer_delta = pointer_pos - center_screen;
    if pointer_delta.length_sq() <= f32::EPSILON {
        return 0.0;
    }

    let rotation = pointer_delta.angle() - start_pointer_angle;
    if unsnapped || snap_degrees <= f32::EPSILON {
        rotation
    } else {
        let snap_radians = snap_degrees.to_radians();
        (rotation / snap_radians).round() * snap_radians
    }
}

pub(crate) fn rotate_vec2(offset: egui::Vec2, angle: f32) -> egui::Vec2 {
    let sin = angle.sin();
    let cos = angle.cos();
    egui::vec2(
        offset.x * cos - offset.y * sin,
        offset.x * sin + offset.y * cos,
    )
}

pub(crate) fn compute_resize_snap_preview(
    state: &mut CanvasState,
    screen_rect: egui::Rect,
    placement_id: i64,
    desired_rect: egui::Rect,
    anchor_world: egui::Pos2,
    base_display_size: egui::Vec2,
    min_scale: f32,
) -> Option<egui::Rect> {
    let candidates = guide_snap_candidate_rects(state, screen_rect, &[placement_id]);
    let Some(result) =
        super::super::guide_snap::snap_resize(super::super::guide_snap::GuideResizeSnapRequest {
            desired_rect,
            anchor: anchor_world,
            base_display_size,
            min_scale,
            candidates: &candidates,
            gap: state.placement_gap,
            threshold: super::super::guide_snap::threshold_world(state.zoom, false),
        })
    else {
        state.clear_guide_snap_indicators();
        return None;
    };
    state.set_guide_snap_indicators(result.indicators);
    Some(result.rect)
}

pub(crate) fn compute_uniform_scale_snap_preview(
    state: &mut CanvasState,
    screen_rect: egui::Rect,
    placement_ids: &[i64],
    group_center_world: egui::Pos2,
    starting_items: &[PlacementScaleStart],
    desired_factor: f32,
) -> Option<f32> {
    let start_rect = scaled_placement_bounds(starting_items, group_center_world, 1.0)?;
    let min_factor = starting_items
        .iter()
        .map(|item| item.min_scale / item.start_scale().max(f32::EPSILON))
        .fold(0.0, f32::max);
    let candidates = guide_snap_candidate_rects(state, screen_rect, placement_ids);
    let Some(result) = super::super::guide_snap::snap_uniform_scale(
        super::super::guide_snap::GuideUniformScaleSnapRequest {
            origin: group_center_world,
            start_rect,
            desired_factor,
            min_factor,
            candidates: &candidates,
            gap: state.placement_gap,
            threshold: super::super::guide_snap::threshold_world(state.zoom, false),
        },
    ) else {
        state.clear_guide_snap_indicators();
        return None;
    };

    state.set_guide_snap_indicators(result.indicators);
    if start_rect.width().abs() > f32::EPSILON {
        Some(result.rect.width() / start_rect.width())
    } else if start_rect.height().abs() > f32::EPSILON {
        Some(result.rect.height() / start_rect.height())
    } else {
        None
    }
}

fn guide_snap_candidate_rects(
    state: &CanvasState,
    screen_rect: egui::Rect,
    excluded_placement_ids: &[i64],
) -> Vec<super::super::guide_snap::GuideSnapCandidate> {
    let viewport = viewport_near_world_rect(state, screen_rect);
    state
        .document
        .rendered_placements
        .iter()
        .filter(|image| !excluded_placement_ids.contains(&image.placement.state.id))
        .filter_map(|image| {
            let scaled_size = image.placement.scaled_display_size();
            let display_size = super::super::rotated_display_size(
                scaled_size.x,
                scaled_size.y,
                image.placement.state.geometry.rotation,
            );
            let rect = egui::Rect::from_center_size(
                egui::pos2(
                    image.placement.state.geometry.center.x,
                    image.placement.state.geometry.center.y,
                ),
                display_size,
            );
            rect.intersects(viewport)
                .then_some(super::super::guide_snap::GuideSnapCandidate::new(rect))
        })
        .collect()
}

fn viewport_near_world_rect(state: &CanvasState, screen_rect: egui::Rect) -> egui::Rect {
    let expanded = screen_rect.expand(super::super::guide_snap::VIEWPORT_PADDING_SCREEN);
    egui::Rect::from_two_pos(
        state.screen_to_world(expanded.min, screen_rect),
        state.screen_to_world(expanded.max, screen_rect),
    )
}
