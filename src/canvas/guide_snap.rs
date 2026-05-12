use eframe::egui;

use super::GuideSnapIndicator;

pub(super) const ACQUIRE_DISTANCE_SCREEN: f32 = 8.0;
pub(super) const RELEASE_DISTANCE_SCREEN: f32 = 12.0;
pub(super) const VIEWPORT_PADDING_SCREEN: f32 = 64.0;
const EPSILON: f32 = 0.001;

pub(super) struct GuideSnapRequest<'a> {
    pub(super) desired_rect: egui::Rect,
    pub(super) candidates: &'a [egui::Rect],
    pub(super) gap: f32,
    pub(super) threshold: f32,
}

pub(super) struct GuideSnapResult {
    pub(super) rect: egui::Rect,
    pub(super) indicators: Vec<GuideSnapIndicator>,
}

#[derive(Clone, Copy, Debug)]
struct GuideAxisSnap {
    nudge: f32,
    distance: f32,
    coord: f32,
    kind: GuideSnapKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GuideAxis {
    Horizontal,
    Vertical,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GuideSnapKind {
    Edge,
    Center,
    Adjacency,
}

pub(super) fn threshold_world(zoom: f32, previously_snapped: bool) -> f32 {
    let threshold_screen = if previously_snapped {
        RELEASE_DISTANCE_SCREEN
    } else {
        ACQUIRE_DISTANCE_SCREEN
    };
    threshold_screen / zoom.max(0.01)
}

pub(super) fn snap_rect(request: GuideSnapRequest<'_>) -> Option<GuideSnapResult> {
    if request.candidates.is_empty() {
        return None;
    }

    let mut best_x =
        best_axis_alignment_snap_x(request.desired_rect, request.candidates, request.threshold);
    consider_horizontal_adjacency_snaps(
        request.desired_rect,
        request.candidates,
        request.gap,
        request.threshold,
        &mut best_x,
    );
    let mut best_y =
        best_axis_alignment_snap_y(request.desired_rect, request.candidates, request.threshold);
    consider_vertical_adjacency_snaps(
        request.desired_rect,
        request.candidates,
        request.gap,
        request.threshold,
        &mut best_y,
    );

    let nudge = egui::vec2(
        best_x.map(|snap| snap.nudge).unwrap_or(0.0),
        best_y.map(|snap| snap.nudge).unwrap_or(0.0),
    );
    if nudge.length_sq() <= EPSILON {
        return None;
    }
    let snapped_rect = request.desired_rect.translate(nudge);
    let mut indicators = Vec::new();
    if let Some(snap) = best_x {
        indicators.push(guide_indicator_for_axis(
            GuideAxis::Vertical,
            snap.coord,
            snap.kind,
            snapped_rect,
            request.candidates,
        ));
    }
    if let Some(snap) = best_y {
        indicators.push(guide_indicator_for_axis(
            GuideAxis::Horizontal,
            snap.coord,
            snap.kind,
            snapped_rect,
            request.candidates,
        ));
    }
    if let (Some(x_snap), Some(y_snap)) = (best_x, best_y) {
        add_guide_intersection_marks(&mut indicators, egui::pos2(x_snap.coord, y_snap.coord));
    }
    Some(GuideSnapResult {
        rect: snapped_rect,
        indicators,
    })
}

fn best_axis_alignment_snap_x(
    desired_rect: egui::Rect,
    candidates: &[egui::Rect],
    threshold: f32,
) -> Option<GuideAxisSnap> {
    let desired_coords = [
        (desired_rect.left(), false),
        (desired_rect.center().x, true),
        (desired_rect.right(), false),
    ];
    let mut best = None;
    for candidate in candidates {
        let candidate_coords = [
            (candidate.left(), false),
            (candidate.center().x, true),
            (candidate.right(), false),
        ];
        for (desired, desired_center) in desired_coords {
            for (target, target_center) in candidate_coords {
                if desired_center != target_center {
                    continue;
                }
                let kind = if desired_center {
                    GuideSnapKind::Center
                } else {
                    GuideSnapKind::Edge
                };
                best = consider_axis_snap(best, target - desired, threshold, target, kind);
            }
        }
    }
    best
}

fn best_axis_alignment_snap_y(
    desired_rect: egui::Rect,
    candidates: &[egui::Rect],
    threshold: f32,
) -> Option<GuideAxisSnap> {
    let desired_coords = [
        (desired_rect.top(), false),
        (desired_rect.center().y, true),
        (desired_rect.bottom(), false),
    ];
    let mut best = None;
    for candidate in candidates {
        let candidate_coords = [
            (candidate.top(), false),
            (candidate.center().y, true),
            (candidate.bottom(), false),
        ];
        for (desired, desired_center) in desired_coords {
            for (target, target_center) in candidate_coords {
                if desired_center != target_center {
                    continue;
                }
                let kind = if desired_center {
                    GuideSnapKind::Center
                } else {
                    GuideSnapKind::Edge
                };
                best = consider_axis_snap(best, target - desired, threshold, target, kind);
            }
        }
    }
    best
}

fn consider_horizontal_adjacency_snaps(
    desired_rect: egui::Rect,
    candidates: &[egui::Rect],
    gap: f32,
    threshold: f32,
    best: &mut Option<GuideAxisSnap>,
) {
    for candidate in candidates {
        if vertical_overlap_span(desired_rect, *candidate) <= EPSILON {
            continue;
        }
        for nudge in [
            candidate.right() + gap - desired_rect.left(),
            candidate.left() - gap - desired_rect.right(),
        ] {
            let coord = if nudge > 0.0 {
                desired_rect.left() + nudge
            } else {
                desired_rect.right() + nudge
            };
            *best = consider_axis_snap(*best, nudge, threshold, coord, GuideSnapKind::Adjacency);
        }
    }
}

fn consider_vertical_adjacency_snaps(
    desired_rect: egui::Rect,
    candidates: &[egui::Rect],
    gap: f32,
    threshold: f32,
    best: &mut Option<GuideAxisSnap>,
) {
    for candidate in candidates {
        if horizontal_overlap_span(desired_rect, *candidate) <= EPSILON {
            continue;
        }
        for nudge in [
            candidate.bottom() + gap - desired_rect.top(),
            candidate.top() - gap - desired_rect.bottom(),
        ] {
            let coord = if nudge > 0.0 {
                desired_rect.top() + nudge
            } else {
                desired_rect.bottom() + nudge
            };
            *best = consider_axis_snap(*best, nudge, threshold, coord, GuideSnapKind::Adjacency);
        }
    }
}

fn consider_axis_snap(
    best: Option<GuideAxisSnap>,
    nudge: f32,
    threshold: f32,
    coord: f32,
    kind: GuideSnapKind,
) -> Option<GuideAxisSnap> {
    let distance = nudge.abs();
    if distance > threshold || distance <= EPSILON {
        return best;
    }
    match best {
        Some(best) if best.distance <= distance => Some(best),
        _ => Some(GuideAxisSnap {
            nudge,
            distance,
            coord,
            kind,
        }),
    }
}

fn vertical_overlap_span(left: egui::Rect, right: egui::Rect) -> f32 {
    (left.bottom().min(right.bottom()) - left.top().max(right.top())).max(0.0)
}

fn horizontal_overlap_span(left: egui::Rect, right: egui::Rect) -> f32 {
    (left.right().min(right.right()) - left.left().max(right.left())).max(0.0)
}

fn guide_indicator_for_axis(
    axis: GuideAxis,
    coord: f32,
    kind: GuideSnapKind,
    snapped_rect: egui::Rect,
    candidates: &[egui::Rect],
) -> GuideSnapIndicator {
    let mut marks = Vec::new();
    collect_axis_marks(axis, coord, kind, snapped_rect, &mut marks);
    for candidate in candidates {
        collect_axis_marks(axis, coord, kind, *candidate, &mut marks);
    }
    dedupe_points(&mut marks);

    if marks.is_empty() {
        return GuideSnapIndicator {
            segments: Vec::new(),
            marks,
        };
    }

    let segment = match axis {
        GuideAxis::Vertical => {
            let min_y = marks
                .iter()
                .map(|point| point.y)
                .fold(f32::INFINITY, f32::min);
            let max_y = marks
                .iter()
                .map(|point| point.y)
                .fold(f32::NEG_INFINITY, f32::max);
            (egui::pos2(coord, min_y), egui::pos2(coord, max_y))
        }
        GuideAxis::Horizontal => {
            let min_x = marks
                .iter()
                .map(|point| point.x)
                .fold(f32::INFINITY, f32::min);
            let max_x = marks
                .iter()
                .map(|point| point.x)
                .fold(f32::NEG_INFINITY, f32::max);
            (egui::pos2(min_x, coord), egui::pos2(max_x, coord))
        }
    };

    GuideSnapIndicator {
        segments: vec![segment],
        marks,
    }
}

fn collect_axis_marks(
    axis: GuideAxis,
    coord: f32,
    kind: GuideSnapKind,
    rect: egui::Rect,
    marks: &mut Vec<egui::Pos2>,
) {
    for point in rect_snap_points(rect, kind) {
        let aligned = match axis {
            GuideAxis::Vertical => (point.x - coord).abs() <= EPSILON,
            GuideAxis::Horizontal => (point.y - coord).abs() <= EPSILON,
        };
        if aligned {
            marks.push(point);
        }
    }
}

fn rect_snap_points(rect: egui::Rect, kind: GuideSnapKind) -> Vec<egui::Pos2> {
    match kind {
        GuideSnapKind::Edge | GuideSnapKind::Adjacency => {
            vec![
                rect.left_top(),
                rect.right_top(),
                rect.left_bottom(),
                rect.right_bottom(),
            ]
        }
        GuideSnapKind::Center => vec![
            egui::pos2(rect.center().x, rect.top()),
            egui::pos2(rect.left(), rect.center().y),
            rect.center(),
            egui::pos2(rect.right(), rect.center().y),
            egui::pos2(rect.center().x, rect.bottom()),
        ],
    }
}

fn dedupe_points(points: &mut Vec<egui::Pos2>) {
    let mut deduped = Vec::with_capacity(points.len());
    for point in points.drain(..) {
        if deduped
            .iter()
            .any(|existing: &egui::Pos2| existing.distance(point) <= EPSILON)
        {
            continue;
        }
        deduped.push(point);
    }
    *points = deduped;
}

fn add_guide_intersection_marks(indicators: &mut [GuideSnapIndicator], point: egui::Pos2) {
    for indicator in indicators {
        indicator.marks.push(point);
        dedupe_points(&mut indicator.marks);
    }
}
