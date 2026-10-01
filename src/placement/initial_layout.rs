use eframe::egui;

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

fn rects_overlap_with_gap(left: egui::Rect, right: egui::Rect, gap: f32) -> bool {
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

#[cfg(test)]
mod tests {
    use super::*;
    fn test_viewport_world() -> egui::Rect {
        egui::Rect::from_min_max(
            egui::pos2(-10_000.0, -10_000.0),
            egui::pos2(10_000.0, 10_000.0),
        )
    }
    #[test]
    fn empty_board_placement_uses_requested_anchor() {
        let existing = Vec::new();
        let anchor = eframe::egui::pos2(120.0, -48.0);

        let arranged = arrange_rect_with_existing_layout(
            &existing,
            anchor,
            test_viewport_world(),
            300.0,
            200.0,
            12.0,
        );

        assert_eq!(arranged, anchor);
    }

    #[test]
    fn deterministic_candidate_order_prefers_right_before_below_on_tie() {
        let existing = vec![
            crate::placement::arrangement::PlacementArrangementInput::new(
                0,
                eframe::egui::pos2(0.0, 0.0),
                eframe::egui::vec2(100_f32, 100_f32),
            ),
        ];

        let arranged = arrange_rect_with_existing_layout(
            &existing,
            eframe::egui::pos2(106.0, 106.0),
            test_viewport_world(),
            100.0,
            100.0,
            12.0,
        );

        assert_eq!(arranged, eframe::egui::pos2(112.0, 0.0));
    }

    #[test]
    fn blocked_right_side_candidate_falls_through_to_valid_lower_slot() {
        let mut existing = vec![
            crate::placement::arrangement::PlacementArrangementInput::new(
                0,
                eframe::egui::pos2(0.0, 0.0),
                eframe::egui::vec2(100_f32, 100_f32),
            ),
        ];
        existing.push(
            crate::placement::arrangement::PlacementArrangementInput::new(
                existing.len() as i64,
                eframe::egui::pos2(112.0, 0.0),
                eframe::egui::vec2(100_f32, 100_f32),
            ),
        );

        let arranged = arrange_rect_with_existing_layout(
            &existing,
            eframe::egui::pos2(112.0, 50.0),
            test_viewport_world(),
            100.0,
            100.0,
            12.0,
        );

        assert_eq!(arranged, eframe::egui::pos2(112.0, 112.0));
    }

    #[test]
    fn squarer_occupied_bounds_break_anchor_distance_ties() {
        let existing = vec![
            crate::placement::arrangement::PlacementArrangementInput::new(
                0,
                eframe::egui::pos2(0.0, 0.0),
                eframe::egui::vec2(240_f32, 80_f32),
            ),
        ];

        // Equidistant from the right slot (172, 0) and the lower slot (-80, 92).
        let arranged = arrange_rect_with_existing_layout(
            &existing,
            eframe::egui::pos2(46.0, 46.0),
            test_viewport_world(),
            80.0,
            80.0,
            12.0,
        );

        assert_eq!(arranged, eframe::egui::pos2(-80.0, 92.0));
    }

    #[test]
    fn viewport_bounding_beats_anchor_distance() {
        let existing = vec![
            crate::placement::arrangement::PlacementArrangementInput::new(
                0,
                eframe::egui::pos2(0.0, 0.0),
                eframe::egui::vec2(240_f32, 80_f32),
            ),
        ];
        let viewport = eframe::egui::Rect::from_min_max(
            eframe::egui::pos2(100.0, -60.0),
            eframe::egui::pos2(260.0, 60.0),
        );

        // The anchor sits on the lower slot, which is off screen.
        let arranged = arrange_rect_with_existing_layout(
            &existing,
            eframe::egui::pos2(-80.0, 92.0),
            viewport,
            80.0,
            80.0,
            12.0,
        );

        assert_eq!(arranged, eframe::egui::pos2(172.0, 0.0));
    }
}
