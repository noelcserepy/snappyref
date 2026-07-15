use eframe::egui;

use crate::library::storage::Library;
use crate::placement::arrangement::PlacementArrangementInput;

#[derive(Clone, Copy, Debug)]
struct LayoutRect {
    id: i64,
    rect: egui::Rect,
}

#[derive(Clone, Copy, Debug)]
struct PlacementCandidate {
    center: egui::Pos2,
    generation_index: usize,
}

pub fn arrange_rect_world(
    library: &Library,
    desired_world: egui::Pos2,
    viewport_world: egui::Rect,
    item_width: f32,
    item_height: f32,
    placement_gap: f32,
) -> Result<egui::Pos2, String> {
    let existing_items = library.load_placement_arrangement_inputs()?;
    Ok(arrange_rect_with_existing_layout(
        &existing_items,
        desired_world,
        viewport_world,
        item_width,
        item_height,
        placement_gap,
    ))
}

pub(super) fn resolve_import_world(
    library: &Library,
    target_board_id: i64,
    desired_world: egui::Pos2,
    reserved_world: Option<egui::Pos2>,
    viewport_world: egui::Rect,
    item_width: f32,
    item_height: f32,
    placement_gap: f32,
) -> Result<egui::Pos2, String> {
    if let Some(reserved_world) = reserved_world {
        return Ok(reserved_world);
    }

    let existing_items = library.load_placement_arrangement_inputs_for_board(target_board_id)?;
    Ok(arrange_rect_with_existing_layout(
        &existing_items,
        desired_world,
        viewport_world,
        item_width,
        item_height,
        placement_gap,
    ))
}

pub fn arrange_rect_with_existing_layout(
    existing_items: &[PlacementArrangementInput],
    desired_world: egui::Pos2,
    viewport_world: egui::Rect,
    item_width: f32,
    item_height: f32,
    placement_gap: f32,
) -> egui::Pos2 {
    if existing_items.is_empty() {
        return desired_world;
    }

    best_arrangement_candidate_with_existing_layout(
        existing_items,
        desired_world,
        viewport_world,
        item_width,
        item_height,
        placement_gap,
    )
    .unwrap_or(desired_world)
}

fn best_arrangement_candidate_with_existing_layout(
    existing_items: &[PlacementArrangementInput],
    desired_world: egui::Pos2,
    viewport_world: egui::Rect,
    item_width: f32,
    item_height: f32,
    placement_gap: f32,
) -> Option<egui::Pos2> {
    let new_rect = egui::Rect::from_center_size(
        desired_world,
        egui::vec2(item_width.max(0.0), item_height.max(0.0)),
    );

    if existing_items.is_empty() {
        return None;
    }

    let mut existing_rects = existing_items
        .iter()
        .map(displayed_layout_rect)
        .collect::<Vec<_>>();
    existing_rects.sort_by(|left, right| {
        left.rect
            .top()
            .total_cmp(&right.rect.top())
            .then_with(|| left.rect.left().total_cmp(&right.rect.left()))
            .then_with(|| left.id.cmp(&right.id))
    });

    let mut best_candidate: Option<(f32, f32, f32, usize, egui::Pos2)> = None;
    let mut generation_index = 0_usize;

    for existing in &existing_rects {
        for candidate_rect in [
            egui::Rect::from_min_size(
                egui::pos2(existing.rect.right() + placement_gap, existing.rect.top()),
                new_rect.size(),
            ),
            egui::Rect::from_min_size(
                egui::pos2(existing.rect.left(), existing.rect.bottom() + placement_gap),
                new_rect.size(),
            ),
        ] {
            let candidate = PlacementCandidate {
                center: candidate_rect.center(),
                generation_index,
            };
            generation_index += 1;

            if candidate_overlaps_any(candidate_rect, &existing_rects, placement_gap) {
                continue;
            }

            let viewport_score = viewport_distance_score(candidate_rect.center(), viewport_world);
            let distance_score = candidate.center.distance_sq(desired_world);
            let squareness_score = occupied_bounds_squareness(&existing_rects, candidate_rect);
            let score = (
                viewport_score,
                distance_score,
                squareness_score,
                candidate.generation_index,
                candidate.center,
            );

            match best_candidate {
                Some((best_viewport, best_distance, best_squareness, best_index, _))
                    if viewport_score > best_viewport
                        || ((viewport_score - best_viewport).abs() < 0.001
                            && distance_score > best_distance)
                        || ((viewport_score - best_viewport).abs() < 0.001
                            && (distance_score - best_distance).abs() < 0.001
                            && squareness_score > best_squareness)
                        || ((viewport_score - best_viewport).abs() < 0.001
                            && (distance_score - best_distance).abs() < 0.001
                            && (squareness_score - best_squareness).abs() < 0.001
                            && candidate.generation_index >= best_index) => {}
                _ => best_candidate = Some(score),
            }
        }
    }

    best_candidate.map(|(_, _, _, _, center)| center)
}

fn viewport_distance_score(candidate_center: egui::Pos2, viewport_world: egui::Rect) -> f32 {
    let clamped = egui::pos2(
        candidate_center
            .x
            .clamp(viewport_world.left(), viewport_world.right()),
        candidate_center
            .y
            .clamp(viewport_world.top(), viewport_world.bottom()),
    );
    candidate_center.distance_sq(clamped)
}

fn displayed_layout_rect(placement: &PlacementArrangementInput) -> LayoutRect {
    let size = egui::vec2(placement.width().max(0.0), placement.height().max(0.0));
    LayoutRect {
        id: placement.id,
        rect: egui::Rect::from_center_size(placement.center(), size),
    }
}

fn candidate_overlaps_any(
    candidate_rect: egui::Rect,
    existing_rects: &[LayoutRect],
    gap: f32,
) -> bool {
    existing_rects
        .iter()
        .any(|existing| rects_overlap_with_gap(candidate_rect, existing.rect, gap))
}

pub(crate) fn rects_overlap_with_gap(left: egui::Rect, right: egui::Rect, gap: f32) -> bool {
    left.left() < right.right() + gap
        && left.right() > right.left() - gap
        && left.top() < right.bottom() + gap
        && left.bottom() > right.top() - gap
}

fn occupied_bounds_squareness(existing_rects: &[LayoutRect], candidate_rect: egui::Rect) -> f32 {
    let mut min_x = candidate_rect.left();
    let mut min_y = candidate_rect.top();
    let mut max_x = candidate_rect.right();
    let mut max_y = candidate_rect.bottom();

    for existing in existing_rects {
        min_x = min_x.min(existing.rect.left());
        min_y = min_y.min(existing.rect.top());
        max_x = max_x.max(existing.rect.right());
        max_y = max_y.max(existing.rect.bottom());
    }

    let width = (max_x - min_x).max(1.0);
    let height = (max_y - min_y).max(1.0);
    width.max(height) / width.min(height)
}
