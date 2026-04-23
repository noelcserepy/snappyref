use crate::data::BoardLayoutItem;
use eframe::egui;

const SNAP_EPSILON: f32 = 0.5;

#[derive(Clone, Copy, Debug)]
pub(crate) struct MoveSnapRequest {
    pub desired_rect: egui::Rect,
    pub drag_delta: egui::Vec2,
    pub previous_snapped_rect: Option<egui::Rect>,
    pub gap: f32,
    pub max_snap_distance: f32,
    pub hysteresis_distance: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct MoveSnapCandidate {
    pub rect: egui::Rect,
}

#[derive(Clone, Copy, Debug)]
struct LayoutRect {
    rect: egui::Rect,
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

#[derive(Clone, Copy, Debug)]
struct CandidateMetrics {
    snapped_axes: usize,
    adjacency_supports: usize,
    alignment_supports: usize,
    support_rects: usize,
    contact_span: f32,
    alignment_span: f32,
    intent_penalty: usize,
    correction_distance_sq: f32,
}

#[derive(Clone, Copy, Debug)]
struct EvaluatedCandidate {
    rect: egui::Rect,
    metrics: CandidateMetrics,
    generation_index: usize,
}

pub(crate) fn best_move_snap_candidate(
    existing_items: &[BoardLayoutItem],
    request: &MoveSnapRequest,
) -> Option<MoveSnapCandidate> {
    if existing_items.is_empty() {
        return None;
    }

    let existing_rects = existing_items
        .iter()
        .map(displayed_layout_rect)
        .collect::<Vec<_>>();
    let x_snaps = collect_x_snaps(&existing_rects, request);
    let y_snaps = collect_y_snaps(&existing_rects, request);

    let mut candidates = Vec::new();
    let mut generation_index = 0_usize;

    for x_snap in &x_snaps {
        if let Some(candidate) = evaluate_generated_candidate(
            Some(*x_snap),
            None,
            request,
            &existing_rects,
            generation_index,
        ) {
            candidates.push(candidate);
        }
        generation_index += 1;
    }

    for y_snap in &y_snaps {
        if let Some(candidate) = evaluate_generated_candidate(
            None,
            Some(*y_snap),
            request,
            &existing_rects,
            generation_index,
        ) {
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
                &existing_rects,
                generation_index,
            ) {
                candidates.push(candidate);
            }
            generation_index += 1;
        }
    }

    dedupe_candidates(&mut candidates);
    let mut best = candidates.into_iter().reduce(|best, candidate| {
        if is_better(candidate, best) {
            candidate
        } else {
            best
        }
    });

    if let Some(previous_rect) = request.previous_snapped_rect
        && let Some(previous) =
            evaluate_existing_candidate(previous_rect, request, &existing_rects, usize::MAX)
    {
        best = match best {
            Some(best_candidate)
                if best_candidate
                    .rect
                    .center()
                    .distance(previous.rect.center())
                    <= request.hysteresis_distance =>
            {
                Some(previous)
            }
            Some(best_candidate) => Some(if is_better(previous, best_candidate) {
                previous
            } else {
                best_candidate
            }),
            None => Some(previous),
        };
    }

    best.map(|candidate| MoveSnapCandidate {
        rect: candidate.rect,
    })
}

fn collect_x_snaps(existing_rects: &[LayoutRect], request: &MoveSnapRequest) -> Vec<AxisSnap> {
    let width = request.desired_rect.width();
    let center_x = request.desired_rect.center().x;
    let mut snaps = Vec::new();

    for existing in existing_rects {
        let candidates = [
            AxisSnap {
                center: existing.rect.left() + width * 0.5,
                kind: SnapKind::Alignment,
            },
            AxisSnap {
                center: existing.rect.center().x,
                kind: SnapKind::Alignment,
            },
            AxisSnap {
                center: existing.rect.right() - width * 0.5,
                kind: SnapKind::Alignment,
            },
            AxisSnap {
                center: existing.rect.right() + request.gap + width * 0.5,
                kind: SnapKind::Adjacency,
            },
            AxisSnap {
                center: existing.rect.left() - request.gap - width * 0.5,
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

fn collect_y_snaps(existing_rects: &[LayoutRect], request: &MoveSnapRequest) -> Vec<AxisSnap> {
    let height = request.desired_rect.height();
    let center_y = request.desired_rect.center().y;
    let mut snaps = Vec::new();

    for existing in existing_rects {
        let candidates = [
            AxisSnap {
                center: existing.rect.top() + height * 0.5,
                kind: SnapKind::Alignment,
            },
            AxisSnap {
                center: existing.rect.center().y,
                kind: SnapKind::Alignment,
            },
            AxisSnap {
                center: existing.rect.bottom() - height * 0.5,
                kind: SnapKind::Alignment,
            },
            AxisSnap {
                center: existing.rect.bottom() + request.gap + height * 0.5,
                kind: SnapKind::Adjacency,
            },
            AxisSnap {
                center: existing.rect.top() - request.gap - height * 0.5,
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
    existing_rects: &[LayoutRect],
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
    evaluate_candidate_rect(
        rect,
        x_snap,
        y_snap,
        request,
        existing_rects,
        generation_index,
    )
}

fn evaluate_existing_candidate(
    rect: egui::Rect,
    request: &MoveSnapRequest,
    existing_rects: &[LayoutRect],
    generation_index: usize,
) -> Option<EvaluatedCandidate> {
    evaluate_candidate_rect(rect, None, None, request, existing_rects, generation_index)
}

fn evaluate_candidate_rect(
    rect: egui::Rect,
    x_snap: Option<AxisSnap>,
    y_snap: Option<AxisSnap>,
    request: &MoveSnapRequest,
    existing_rects: &[LayoutRect],
    generation_index: usize,
) -> Option<EvaluatedCandidate> {
    if candidate_overlaps_any(rect, existing_rects, request.gap) {
        return None;
    }
    let correction_distance_sq = rect.center().distance_sq(request.desired_rect.center());
    if correction_distance_sq > request.max_snap_distance * request.max_snap_distance {
        return None;
    }

    let (adjacency_supports, alignment_supports, support_rects, contact_span, alignment_span) =
        measure_candidate_support(rect, existing_rects, request.gap);
    let metrics = CandidateMetrics {
        snapped_axes: x_snap.is_some() as usize + y_snap.is_some() as usize,
        adjacency_supports,
        alignment_supports,
        support_rects,
        contact_span,
        alignment_span,
        intent_penalty: intent_penalty(x_snap, y_snap, request.drag_delta),
        correction_distance_sq,
    };

    Some(EvaluatedCandidate {
        rect,
        metrics,
        generation_index,
    })
}

fn measure_candidate_support(
    candidate_rect: egui::Rect,
    existing_rects: &[LayoutRect],
    gap: f32,
) -> (usize, usize, usize, f32, f32) {
    let mut adjacency_supports = 0_usize;
    let mut alignment_supports = 0_usize;
    let mut support_rects = 0_usize;
    let mut contact_span = 0.0_f32;
    let mut alignment_span = 0.0_f32;

    for existing in existing_rects {
        let mut supported = false;

        for span in [
            horizontal_contact_span(candidate_rect, existing.rect, gap),
            vertical_contact_span(candidate_rect, existing.rect, gap),
        ] {
            if span > SNAP_EPSILON {
                adjacency_supports += 1;
                contact_span += span;
                supported = true;
            }
        }

        for span in [
            vertical_alignment_span(candidate_rect, existing.rect),
            horizontal_alignment_span(candidate_rect, existing.rect),
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

    (
        adjacency_supports,
        alignment_supports,
        support_rects,
        contact_span,
        alignment_span,
    )
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
    let left = candidate.metrics;
    let right = best.metrics;

    if left.adjacency_supports != right.adjacency_supports {
        return left.adjacency_supports > right.adjacency_supports;
    }
    if (left.contact_span - right.contact_span).abs() > SNAP_EPSILON {
        return left.contact_span > right.contact_span;
    }
    if (left.correction_distance_sq - right.correction_distance_sq).abs() > SNAP_EPSILON {
        return left.correction_distance_sq < right.correction_distance_sq;
    }
    if left.snapped_axes != right.snapped_axes {
        return left.snapped_axes > right.snapped_axes;
    }
    if left.support_rects != right.support_rects {
        return left.support_rects > right.support_rects;
    }
    if left.alignment_supports != right.alignment_supports {
        return left.alignment_supports > right.alignment_supports;
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

fn displayed_layout_rect(record: &BoardLayoutItem) -> LayoutRect {
    LayoutRect {
        rect: egui::Rect::from_center_size(
            egui::pos2(record.x, record.y),
            egui::vec2(record.width.max(0.0), record.height.max(0.0)),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::{MoveSnapRequest, best_move_snap_candidate};
    use crate::data::BoardLayoutItem;
    use eframe::egui;

    #[test]
    fn move_snap_supports_snapping_on_all_four_sides() {
        let existing = [layout_item(1, 0.0, 0.0, 100.0, 100.0)];
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
            layout_item(1, -170.0, 150.0, 120.0, 180.0),
            layout_item(2, 10.0, -120.0, 220.0, 120.0),
        ];
        let desired_rect =
            egui::Rect::from_center_size(egui::pos2(70.0, 36.0), egui::vec2(180.0, 140.0));

        let candidate = best_move_snap_candidate(
            &existing,
            &MoveSnapRequest {
                desired_rect,
                drag_delta: egui::vec2(24.0, -88.0),
                previous_snapped_rect: None,
                gap: 24.0,
                max_snap_distance: 160.0,
                hysteresis_distance: 16.0,
            },
        )
        .expect("expected snap candidate");

        assert_eq!(candidate.rect.top(), -36.0);
    }

    #[test]
    fn move_snap_uses_selection_bounds_not_single_item_size() {
        let existing = [layout_item(1, 0.0, -140.0, 260.0, 100.0)];
        let selection_rect =
            egui::Rect::from_min_max(egui::pos2(-120.0, 10.0), egui::pos2(120.0, 90.0));

        let candidate = best_move_snap_candidate(
            &existing,
            &MoveSnapRequest {
                desired_rect: selection_rect,
                drag_delta: egui::vec2(0.0, -80.0),
                previous_snapped_rect: None,
                gap: 24.0,
                max_snap_distance: 160.0,
                hysteresis_distance: 16.0,
            },
        )
        .expect("expected group snap");

        assert_eq!(candidate.rect.width(), selection_rect.width());
        assert_eq!(candidate.rect.center().y, -26.0);
    }

    #[test]
    fn move_snap_keeps_previous_candidate_within_hysteresis_window() {
        let existing = [
            layout_item(1, -120.0, 0.0, 100.0, 100.0),
            layout_item(2, 120.0, 0.0, 100.0, 100.0),
        ];
        let desired_rect =
            egui::Rect::from_center_size(egui::pos2(0.0, 0.0), egui::vec2(100.0, 100.0));
        let previous_rect =
            egui::Rect::from_center_size(egui::pos2(-4.0, 124.0), desired_rect.size());

        let candidate = best_move_snap_candidate(
            &existing,
            &MoveSnapRequest {
                desired_rect,
                drag_delta: egui::vec2(0.0, 80.0),
                previous_snapped_rect: Some(previous_rect),
                gap: 24.0,
                max_snap_distance: 160.0,
                hysteresis_distance: 12.0,
            },
        )
        .expect("expected hysteresis candidate");

        assert_eq!(candidate.rect.center(), previous_rect.center());
    }

    fn request(desired_rect: egui::Rect, drag_delta: egui::Vec2, gap: f32) -> MoveSnapRequest {
        MoveSnapRequest {
            desired_rect,
            drag_delta,
            previous_snapped_rect: None,
            gap,
            max_snap_distance: 160.0,
            hysteresis_distance: 12.0,
        }
    }

    fn layout_item(id: i64, x: f32, y: f32, width: f32, height: f32) -> BoardLayoutItem {
        BoardLayoutItem {
            id,
            x,
            y,
            width,
            height,
        }
    }
}
