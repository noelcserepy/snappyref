use eframe::egui;

const SNAP_EPSILON: f32 = 0.5;

/// Axis-aligned bounds used by move snapping.
///
/// Snapping currently operates on displayed axis-aligned rectangles rather than
/// rotated polygon geometry.
#[derive(Clone, Copy, Debug)]
pub(crate) struct SnapObstacle {
    rect: egui::Rect,
}

impl SnapObstacle {
    pub(crate) fn from_center_size(center: egui::Pos2, size: egui::Vec2) -> Self {
        Self {
            rect: egui::Rect::from_center_size(center, size),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct MoveSnapRequest {
    pub desired_rect: egui::Rect,
    pub drag_delta: egui::Vec2,
    pub gap: f32,
    pub max_snap_distance: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct MoveSnapCandidate {
    pub rect: egui::Rect,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SnapKind {
    Alignment,
    Adjacency,
}

#[derive(Clone, Copy, Debug)]
struct AxisSnap {
    center: f32,
    kind: SnapKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum PlacementClass {
    Aligned,
    EdgeAdjacent,
    CornerPocket,
}

#[derive(Clone, Copy, Debug)]
struct SnapFacts {
    placement_class: PlacementClass,
    adjacency_axes: usize,
    contact_span: f32,
    alignment_span: f32,
    support_rects: usize,
    intent_penalty: usize,
    correction_distance_sq: f32,
}

#[derive(Clone, Copy, Debug)]
struct EvaluatedCandidate {
    rect: egui::Rect,
    facts: SnapFacts,
    generation_index: usize,
}

pub(crate) fn best_move_snap_candidate(
    obstacles: &[SnapObstacle],
    request: &MoveSnapRequest,
) -> Option<MoveSnapCandidate> {
    if obstacles.is_empty() {
        return None;
    }

    let x_snaps = collect_x_snaps(obstacles, request);
    let y_snaps = collect_y_snaps(obstacles, request);

    let mut candidates = Vec::new();
    let mut generation_index = 0_usize;

    for x_snap in &x_snaps {
        if let Some(candidate) =
            evaluate_generated_candidate(Some(*x_snap), None, request, obstacles, generation_index)
        {
            candidates.push(candidate);
        }
        generation_index += 1;
    }

    for y_snap in &y_snaps {
        if let Some(candidate) =
            evaluate_generated_candidate(None, Some(*y_snap), request, obstacles, generation_index)
        {
            candidates.push(candidate);
        }
        generation_index += 1;
    }

    for x_snap in &x_snaps {
        for y_snap in &y_snaps {
            if let Some(candidate) = evaluate_generated_candidate(
                Some(*x_snap),
                Some(*y_snap),
                request,
                obstacles,
                generation_index,
            ) {
                candidates.push(candidate);
            }
            generation_index += 1;
        }
    }

    dedupe_candidates(&mut candidates);
    candidates
        .into_iter()
        .reduce(|best, candidate| {
            if is_better(candidate, best) {
                candidate
            } else {
                best
            }
        })
        .map(|candidate| MoveSnapCandidate {
            rect: candidate.rect,
        })
}

pub(crate) fn validate_move_snap_candidate(
    obstacles: &[SnapObstacle],
    request: &MoveSnapRequest,
    rect: egui::Rect,
) -> Option<MoveSnapCandidate> {
    evaluate_candidate_rect(rect, None, None, request, obstacles, usize::MAX).map(|candidate| {
        MoveSnapCandidate {
            rect: candidate.rect,
        }
    })
}

fn collect_x_snaps(obstacles: &[SnapObstacle], request: &MoveSnapRequest) -> Vec<AxisSnap> {
    let width = request.desired_rect.width();
    let center_x = request.desired_rect.center().x;
    let mut snaps = Vec::new();

    for obstacle in obstacles {
        let candidates = [
            AxisSnap {
                center: obstacle.rect.left() + width * 0.5,
                kind: SnapKind::Alignment,
            },
            AxisSnap {
                center: obstacle.rect.center().x,
                kind: SnapKind::Alignment,
            },
            AxisSnap {
                center: obstacle.rect.right() - width * 0.5,
                kind: SnapKind::Alignment,
            },
            AxisSnap {
                center: obstacle.rect.right() + request.gap + width * 0.5,
                kind: SnapKind::Adjacency,
            },
            AxisSnap {
                center: obstacle.rect.left() - request.gap - width * 0.5,
                kind: SnapKind::Adjacency,
            },
        ];
        for candidate in candidates {
            if (candidate.center - center_x).abs() <= request.max_snap_distance {
                snaps.push(candidate);
            }
        }
    }

    dedupe_axis_snaps(&mut snaps);
    snaps
}

fn collect_y_snaps(obstacles: &[SnapObstacle], request: &MoveSnapRequest) -> Vec<AxisSnap> {
    let height = request.desired_rect.height();
    let center_y = request.desired_rect.center().y;
    let mut snaps = Vec::new();

    for obstacle in obstacles {
        let candidates = [
            AxisSnap {
                center: obstacle.rect.top() + height * 0.5,
                kind: SnapKind::Alignment,
            },
            AxisSnap {
                center: obstacle.rect.center().y,
                kind: SnapKind::Alignment,
            },
            AxisSnap {
                center: obstacle.rect.bottom() - height * 0.5,
                kind: SnapKind::Alignment,
            },
            AxisSnap {
                center: obstacle.rect.bottom() + request.gap + height * 0.5,
                kind: SnapKind::Adjacency,
            },
            AxisSnap {
                center: obstacle.rect.top() - request.gap - height * 0.5,
                kind: SnapKind::Adjacency,
            },
        ];
        for candidate in candidates {
            if (candidate.center - center_y).abs() <= request.max_snap_distance {
                snaps.push(candidate);
            }
        }
    }

    dedupe_axis_snaps(&mut snaps);
    snaps
}

fn dedupe_axis_snaps(snaps: &mut Vec<AxisSnap>) {
    let mut deduped = Vec::with_capacity(snaps.len());
    for snap in snaps.drain(..) {
        if deduped
            .iter()
            .any(|existing: &AxisSnap| (existing.center - snap.center).abs() <= SNAP_EPSILON)
        {
            continue;
        }
        deduped.push(snap);
    }
    *snaps = deduped;
}

fn evaluate_generated_candidate(
    x_snap: Option<AxisSnap>,
    y_snap: Option<AxisSnap>,
    request: &MoveSnapRequest,
    obstacles: &[SnapObstacle],
    generation_index: usize,
) -> Option<EvaluatedCandidate> {
    let center = egui::pos2(
        x_snap
            .map(|snap| snap.center)
            .unwrap_or(request.desired_rect.center().x),
        y_snap
            .map(|snap| snap.center)
            .unwrap_or(request.desired_rect.center().y),
    );
    let rect = egui::Rect::from_center_size(center, request.desired_rect.size());
    if rect.center().distance(request.desired_rect.center()) <= SNAP_EPSILON {
        return None;
    }
    evaluate_candidate_rect(rect, x_snap, y_snap, request, obstacles, generation_index)
}

fn evaluate_candidate_rect(
    rect: egui::Rect,
    x_snap: Option<AxisSnap>,
    y_snap: Option<AxisSnap>,
    request: &MoveSnapRequest,
    obstacles: &[SnapObstacle],
    generation_index: usize,
) -> Option<EvaluatedCandidate> {
    if candidate_overlaps_any(rect, obstacles, request.gap) {
        return None;
    }
    let correction_distance_sq = rect.center().distance_sq(request.desired_rect.center());
    if correction_distance_sq > request.max_snap_distance * request.max_snap_distance {
        return None;
    }

    let measured = measure_candidate_support(rect, obstacles, request.gap)?;
    let facts = SnapFacts {
        placement_class: classify_candidate(&measured)?,
        adjacency_axes: usize::from(measured.horizontal_adjacency_supports > 0)
            + usize::from(measured.vertical_adjacency_supports > 0),
        contact_span: measured.contact_span,
        alignment_span: measured.alignment_span,
        support_rects: measured.support_rects,
        intent_penalty: intent_penalty(x_snap, y_snap, request.drag_delta),
        correction_distance_sq,
    };

    Some(EvaluatedCandidate {
        rect,
        facts,
        generation_index,
    })
}

#[derive(Clone, Copy, Debug)]
struct MeasuredSupport {
    snug_corner_supports: usize,
    horizontal_adjacency_supports: usize,
    vertical_adjacency_supports: usize,
    adjacency_supports: usize,
    alignment_supports: usize,
    support_rects: usize,
    contact_span: f32,
    alignment_span: f32,
}

fn classify_candidate(support: &MeasuredSupport) -> Option<PlacementClass> {
    if support.snug_corner_supports > 0 {
        Some(PlacementClass::CornerPocket)
    } else if support.adjacency_supports > 0 {
        Some(PlacementClass::EdgeAdjacent)
    } else if support.alignment_supports > 0 {
        Some(PlacementClass::Aligned)
    } else {
        None
    }
}

fn measure_candidate_support(
    candidate_rect: egui::Rect,
    obstacles: &[SnapObstacle],
    gap: f32,
) -> Option<MeasuredSupport> {
    let mut left_supports_top_corner = false;
    let mut left_supports_bottom_corner = false;
    let mut right_supports_top_corner = false;
    let mut right_supports_bottom_corner = false;
    let mut top_supports_left_corner = false;
    let mut top_supports_right_corner = false;
    let mut bottom_supports_left_corner = false;
    let mut bottom_supports_right_corner = false;
    let mut horizontal_adjacency_supports = 0_usize;
    let mut vertical_adjacency_supports = 0_usize;
    let mut adjacency_supports = 0_usize;
    let mut alignment_supports = 0_usize;
    let mut support_rects = 0_usize;
    let mut contact_span = 0.0_f32;
    let mut alignment_span = 0.0_f32;

    for obstacle in obstacles {
        let mut supported = false;

        let horizontal_span = horizontal_contact_span(candidate_rect, obstacle.rect, gap);
        if horizontal_span > SNAP_EPSILON {
            horizontal_adjacency_supports += 1;
            adjacency_supports += 1;
            contact_span += horizontal_span;
            if (candidate_rect.left() - (obstacle.rect.right() + gap)).abs() <= SNAP_EPSILON {
                left_supports_top_corner |= rect_contains_y(obstacle.rect, candidate_rect.top());
                left_supports_bottom_corner |=
                    rect_contains_y(obstacle.rect, candidate_rect.bottom());
            }
            if (candidate_rect.right() - (obstacle.rect.left() - gap)).abs() <= SNAP_EPSILON {
                right_supports_top_corner |= rect_contains_y(obstacle.rect, candidate_rect.top());
                right_supports_bottom_corner |=
                    rect_contains_y(obstacle.rect, candidate_rect.bottom());
            }
            supported = true;
        }

        let vertical_span = vertical_contact_span(candidate_rect, obstacle.rect, gap);
        if vertical_span > SNAP_EPSILON {
            vertical_adjacency_supports += 1;
            adjacency_supports += 1;
            contact_span += vertical_span;
            if (candidate_rect.top() - (obstacle.rect.bottom() + gap)).abs() <= SNAP_EPSILON {
                top_supports_left_corner |= rect_contains_x(obstacle.rect, candidate_rect.left());
                top_supports_right_corner |= rect_contains_x(obstacle.rect, candidate_rect.right());
            }
            if (candidate_rect.bottom() - (obstacle.rect.top() - gap)).abs() <= SNAP_EPSILON {
                bottom_supports_left_corner |=
                    rect_contains_x(obstacle.rect, candidate_rect.left());
                bottom_supports_right_corner |=
                    rect_contains_x(obstacle.rect, candidate_rect.right());
            }
            supported = true;
        }

        for span in [
            vertical_alignment_span(candidate_rect, obstacle.rect),
            horizontal_alignment_span(candidate_rect, obstacle.rect),
        ] {
            if span > SNAP_EPSILON {
                alignment_supports += 1;
                alignment_span += span;
                supported = true;
            }
        }

        if supported {
            support_rects += 1;
        }
    }

    let snug_corner_supports = usize::from(left_supports_top_corner && top_supports_left_corner)
        + usize::from(right_supports_top_corner && top_supports_right_corner)
        + usize::from(left_supports_bottom_corner && bottom_supports_left_corner)
        + usize::from(right_supports_bottom_corner && bottom_supports_right_corner);

    Some(MeasuredSupport {
        snug_corner_supports,
        horizontal_adjacency_supports,
        vertical_adjacency_supports,
        adjacency_supports,
        alignment_supports,
        support_rects,
        contact_span,
        alignment_span,
    })
}

fn rect_contains_x(rect: egui::Rect, x: f32) -> bool {
    rect.left() - SNAP_EPSILON <= x && x <= rect.right() + SNAP_EPSILON
}

fn rect_contains_y(rect: egui::Rect, y: f32) -> bool {
    rect.top() - SNAP_EPSILON <= y && y <= rect.bottom() + SNAP_EPSILON
}

fn horizontal_contact_span(candidate_rect: egui::Rect, existing_rect: egui::Rect, gap: f32) -> f32 {
    let left_gap = (candidate_rect.left() - (existing_rect.right() + gap)).abs() <= SNAP_EPSILON;
    let right_gap = (candidate_rect.right() - (existing_rect.left() - gap)).abs() <= SNAP_EPSILON;
    if !(left_gap || right_gap) {
        return 0.0;
    }
    overlap_span(
        candidate_rect.top(),
        candidate_rect.bottom(),
        existing_rect.top(),
        existing_rect.bottom(),
    )
}

fn vertical_contact_span(candidate_rect: egui::Rect, existing_rect: egui::Rect, gap: f32) -> f32 {
    let top_gap = (candidate_rect.top() - (existing_rect.bottom() + gap)).abs() <= SNAP_EPSILON;
    let bottom_gap = (candidate_rect.bottom() - (existing_rect.top() - gap)).abs() <= SNAP_EPSILON;
    if !(top_gap || bottom_gap) {
        return 0.0;
    }
    overlap_span(
        candidate_rect.left(),
        candidate_rect.right(),
        existing_rect.left(),
        existing_rect.right(),
    )
}

fn vertical_alignment_span(candidate_rect: egui::Rect, existing_rect: egui::Rect) -> f32 {
    let aligned = [
        (candidate_rect.left() - existing_rect.left()).abs() <= SNAP_EPSILON,
        (candidate_rect.center().x - existing_rect.center().x).abs() <= SNAP_EPSILON,
        (candidate_rect.right() - existing_rect.right()).abs() <= SNAP_EPSILON,
    ]
    .into_iter()
    .any(|value| value);
    if !aligned {
        return 0.0;
    }
    overlap_span(
        candidate_rect.top(),
        candidate_rect.bottom(),
        existing_rect.top(),
        existing_rect.bottom(),
    )
}

fn horizontal_alignment_span(candidate_rect: egui::Rect, existing_rect: egui::Rect) -> f32 {
    let aligned = [
        (candidate_rect.top() - existing_rect.top()).abs() <= SNAP_EPSILON,
        (candidate_rect.center().y - existing_rect.center().y).abs() <= SNAP_EPSILON,
        (candidate_rect.bottom() - existing_rect.bottom()).abs() <= SNAP_EPSILON,
    ]
    .into_iter()
    .any(|value| value);
    if !aligned {
        return 0.0;
    }
    overlap_span(
        candidate_rect.left(),
        candidate_rect.right(),
        existing_rect.left(),
        existing_rect.right(),
    )
}

fn overlap_span(start_a: f32, end_a: f32, start_b: f32, end_b: f32) -> f32 {
    (end_a.min(end_b) - start_a.max(start_b)).max(0.0)
}

fn intent_penalty(
    x_snap: Option<AxisSnap>,
    y_snap: Option<AxisSnap>,
    drag_delta: egui::Vec2,
) -> usize {
    let horizontal_dominant = drag_delta.x.abs() > drag_delta.y.abs() * 1.2;
    let vertical_dominant = drag_delta.y.abs() > drag_delta.x.abs() * 1.2;
    if !horizontal_dominant && !vertical_dominant {
        return 0;
    }

    let has_horizontal_adjacency = x_snap.is_some_and(|snap| snap.kind == SnapKind::Adjacency);
    let has_vertical_adjacency = y_snap.is_some_and(|snap| snap.kind == SnapKind::Adjacency);
    match (horizontal_dominant, vertical_dominant) {
        (true, false) if !has_horizontal_adjacency && has_vertical_adjacency => 1,
        (false, true) if !has_vertical_adjacency && has_horizontal_adjacency => 1,
        _ => 0,
    }
}

fn dedupe_candidates(candidates: &mut Vec<EvaluatedCandidate>) {
    let mut deduped = Vec::with_capacity(candidates.len());
    for candidate in candidates.drain(..) {
        if deduped.iter().any(|existing: &EvaluatedCandidate| {
            existing.rect.center().distance(candidate.rect.center()) <= SNAP_EPSILON
        }) {
            continue;
        }
        deduped.push(candidate);
    }
    *candidates = deduped;
}

fn is_better(candidate: EvaluatedCandidate, best: EvaluatedCandidate) -> bool {
    let left = candidate.facts;
    let right = best.facts;

    if left.placement_class != right.placement_class {
        return left.placement_class > right.placement_class;
    }
    if left.adjacency_axes != right.adjacency_axes {
        return left.adjacency_axes > right.adjacency_axes;
    }
    if (left.correction_distance_sq - right.correction_distance_sq).abs() > SNAP_EPSILON {
        return left.correction_distance_sq < right.correction_distance_sq;
    }
    if (left.contact_span - right.contact_span).abs() > SNAP_EPSILON {
        return left.contact_span > right.contact_span;
    }
    if left.support_rects != right.support_rects {
        return left.support_rects > right.support_rects;
    }
    if (left.alignment_span - right.alignment_span).abs() > SNAP_EPSILON {
        return left.alignment_span > right.alignment_span;
    }
    if left.intent_penalty != right.intent_penalty {
        return left.intent_penalty < right.intent_penalty;
    }
    candidate.generation_index < best.generation_index
}

fn candidate_overlaps_any(
    candidate_rect: egui::Rect,
    obstacles: &[SnapObstacle],
    gap: f32,
) -> bool {
    obstacles
        .iter()
        .any(|obstacle| rects_overlap_with_gap(candidate_rect, obstacle.rect, gap))
}

fn rects_overlap_with_gap(left: egui::Rect, right: egui::Rect, gap: f32) -> bool {
    left.left() < right.right() + gap
        && left.right() > right.left() - gap
        && left.top() < right.bottom() + gap
        && left.bottom() > right.top() - gap
}

#[cfg(test)]
mod tests {
    use super::{
        MoveSnapRequest, SnapObstacle, best_move_snap_candidate, validate_move_snap_candidate,
    };
    use eframe::egui;

    #[test]
    fn move_snap_supports_snapping_on_all_four_sides() {
        let existing = [obstacle(0.0, 0.0, 100.0, 100.0)];
        let size = egui::vec2(100.0, 100.0);
        let gap = 24.0;

        let right = best_move_snap_candidate(
            &existing,
            &request(
                egui::Rect::from_center_size(egui::pos2(72.0, 0.0), size),
                egui::vec2(60.0, 0.0),
                gap,
            ),
        )
        .expect("right snap");
        assert_eq!(right.rect.center(), egui::pos2(124.0, 0.0));

        let left = best_move_snap_candidate(
            &existing,
            &request(
                egui::Rect::from_center_size(egui::pos2(-72.0, 0.0), size),
                egui::vec2(-60.0, 0.0),
                gap,
            ),
        )
        .expect("left snap");
        assert_eq!(left.rect.center(), egui::pos2(-124.0, 0.0));

        let below = best_move_snap_candidate(
            &existing,
            &request(
                egui::Rect::from_center_size(egui::pos2(0.0, 72.0), size),
                egui::vec2(0.0, 60.0),
                gap,
            ),
        )
        .expect("below snap");
        assert_eq!(below.rect.center(), egui::pos2(0.0, 124.0));

        let above = best_move_snap_candidate(
            &existing,
            &request(
                egui::Rect::from_center_size(egui::pos2(0.0, -72.0), size),
                egui::vec2(0.0, -60.0),
                gap,
            ),
        )
        .expect("above snap");
        assert_eq!(above.rect.center(), egui::pos2(0.0, -124.0));
    }

    #[test]
    fn move_snap_prefers_corner_completion_with_stronger_contact() {
        let existing = [
            obstacle(-170.0, 150.0, 120.0, 180.0),
            obstacle(10.0, -120.0, 220.0, 120.0),
        ];
        let desired_rect =
            egui::Rect::from_center_size(egui::pos2(70.0, 36.0), egui::vec2(180.0, 140.0));

        let candidate = best_move_snap_candidate(
            &existing,
            &MoveSnapRequest {
                desired_rect,
                drag_delta: egui::vec2(24.0, -88.0),
                gap: 24.0,
                max_snap_distance: 160.0,
            },
        )
        .expect("expected snap candidate");

        let left_rect =
            egui::Rect::from_center_size(egui::pos2(-170.0, 150.0), egui::vec2(120.0, 180.0));
        let top_rect =
            egui::Rect::from_center_size(egui::pos2(10.0, -120.0), egui::vec2(220.0, 120.0));

        assert_eq!(candidate.rect.left() - left_rect.right(), 24.0);
        assert_eq!(candidate.rect.top(), -36.0);
        assert_eq!(candidate.rect.top() - top_rect.bottom(), 24.0);
    }

    #[test]
    fn move_snap_prefers_nearby_l_corner_over_far_single_axis_target() {
        let existing = [
            obstacle(40.0, -120.0, 220.0, 120.0),
            obstacle(120.0, 10.0, 100.0, 100.0),
        ];
        let desired_rect =
            egui::Rect::from_center_size(egui::pos2(-8.0, 20.0), egui::vec2(100.0, 100.0));

        let candidate = best_move_snap_candidate(
            &existing,
            &MoveSnapRequest {
                desired_rect,
                drag_delta: egui::vec2(-40.0, 40.0),
                gap: 24.0,
                max_snap_distance: 160.0,
            },
        )
        .expect("expected l-corner snap");

        assert_eq!(candidate.rect.center(), egui::pos2(-4.0, 14.0));
    }

    #[test]
    fn move_snap_uses_selection_bounds_not_single_item_size() {
        let existing = [obstacle(-140.0, -140.0, 240.0, 100.0)];
        let selection_rect =
            egui::Rect::from_min_max(egui::pos2(-260.0, 10.0), egui::pos2(-20.0, 90.0));

        let candidate = best_move_snap_candidate(
            &existing,
            &MoveSnapRequest {
                desired_rect: selection_rect,
                drag_delta: egui::vec2(0.0, -80.0),
                gap: 24.0,
                max_snap_distance: 160.0,
            },
        )
        .expect("expected group snap");

        assert_eq!(candidate.rect.width(), selection_rect.width());
        assert_eq!(candidate.rect.center().x, selection_rect.center().x);
        assert_eq!(candidate.rect.center().y, -26.0);
    }

    #[test]
    fn validate_move_snap_candidate_accepts_supported_corner_preview() {
        let existing = [
            obstacle(-170.0, 150.0, 120.0, 180.0),
            obstacle(10.0, -120.0, 220.0, 120.0),
        ];
        let desired_rect =
            egui::Rect::from_center_size(egui::pos2(70.0, 36.0), egui::vec2(180.0, 140.0));
        let previous_rect =
            egui::Rect::from_center_size(egui::pos2(4.0, 34.0), desired_rect.size());
        let previous = validate_move_snap_candidate(
            &existing,
            &MoveSnapRequest {
                desired_rect,
                drag_delta: egui::vec2(24.0, -88.0),
                gap: 24.0,
                max_snap_distance: 160.0,
            },
            previous_rect,
        )
        .expect("expected previous candidate");
        assert_eq!(previous.rect.center(), previous_rect.center());
    }

    fn request(desired_rect: egui::Rect, drag_delta: egui::Vec2, gap: f32) -> MoveSnapRequest {
        MoveSnapRequest {
            desired_rect,
            drag_delta,
            gap,
            max_snap_distance: 160.0,
        }
    }

    fn obstacle(x: f32, y: f32, width: f32, height: f32) -> SnapObstacle {
        SnapObstacle::from_center_size(egui::pos2(x, y), egui::vec2(width, height))
    }
}
