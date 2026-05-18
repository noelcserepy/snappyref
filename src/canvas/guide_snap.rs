use eframe::egui;

use super::GuideSnapIndicator;

pub(crate) const ACQUIRE_DISTANCE_SCREEN: f32 = 8.0;
pub(crate) const RELEASE_DISTANCE_SCREEN: f32 = 12.0;
pub(crate) const VIEWPORT_PADDING_SCREEN: f32 = 64.0;
const EPSILON: f32 = 0.001;

pub(crate) struct GuideSnapRequest<'a> {
    pub(crate) desired_rect: egui::Rect,
    pub(crate) candidates: &'a [GuideSnapCandidate],
    pub(crate) gap: f32,
    pub(crate) threshold: f32,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct GuideSnapCandidate {
    visual_rect: egui::Rect,
}

impl GuideSnapCandidate {
    pub(crate) fn new(visual_rect: egui::Rect) -> Self {
        Self { visual_rect }
    }
}

pub(crate) struct GuideSnapResult {
    pub(crate) rect: egui::Rect,
    pub(crate) indicators: Vec<GuideSnapIndicator>,
}

pub(crate) struct GuideResizeSnapRequest<'a> {
    pub(crate) desired_rect: egui::Rect,
    pub(crate) anchor: egui::Pos2,
    pub(crate) base_display_size: egui::Vec2,
    pub(crate) min_scale: f32,
    pub(crate) candidates: &'a [GuideSnapCandidate],
    pub(crate) gap: f32,
    pub(crate) threshold: f32,
}

pub(crate) struct GuideUniformScaleSnapRequest<'a> {
    pub(crate) origin: egui::Pos2,
    pub(crate) start_rect: egui::Rect,
    pub(crate) desired_factor: f32,
    pub(crate) min_factor: f32,
    pub(crate) candidates: &'a [GuideSnapCandidate],
    pub(crate) gap: f32,
    pub(crate) threshold: f32,
}

#[derive(Clone, Copy, Debug)]
struct GuideAxisSnap {
    nudge: f32,
    distance: f32,
    coord: f32,
    kind: GuideSnapKind,
    candidate_rect: egui::Rect,
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
    GapSpacing,
}

pub(crate) fn threshold_world(zoom: f32, previously_snapped: bool) -> f32 {
    let threshold_screen = if previously_snapped {
        RELEASE_DISTANCE_SCREEN
    } else {
        ACQUIRE_DISTANCE_SCREEN
    };
    threshold_screen / zoom.max(0.01)
}

pub(crate) fn snap_rect(request: GuideSnapRequest<'_>) -> Option<GuideSnapResult> {
    if request.candidates.is_empty() {
        return None;
    }

    let x_snaps = axis_snaps(
        GuideAxis::Vertical,
        request.desired_rect,
        request.candidates,
        request.gap,
        request.threshold,
    );
    let y_snaps = axis_snaps(
        GuideAxis::Horizontal,
        request.desired_rect,
        request.candidates,
        request.gap,
        request.threshold,
    );
    let best_x = best_snap_for_axis(&x_snaps);
    let best_y = best_snap_for_axis(&y_snaps);

    let nudge = egui::vec2(
        best_x.map(|snap| snap.nudge).unwrap_or(0.0),
        best_y.map(|snap| snap.nudge).unwrap_or(0.0),
    );
    if nudge.length_sq() <= EPSILON {
        return None;
    }
    let snapped_rect = request.desired_rect.translate(nudge);
    let mut indicators = Vec::new();
    for snap in co_winning_axis_snaps(&x_snaps, best_x) {
        indicators.push(guide_indicator_for_axis(
            GuideAxis::Vertical,
            snap.coord,
            snap,
            snapped_rect,
        ));
    }
    for snap in co_winning_axis_snaps(&y_snaps, best_y) {
        indicators.push(guide_indicator_for_axis(
            GuideAxis::Horizontal,
            snap.coord,
            snap,
            snapped_rect,
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

pub(crate) fn snap_resize(request: GuideResizeSnapRequest<'_>) -> Option<GuideSnapResult> {
    if request.candidates.is_empty() {
        return None;
    }

    let handle = resize_handle_from_anchor(request.anchor, request.desired_rect)?;
    let mut best = None;
    for candidate in request.candidates {
        let eligibility = GuideContactEligibility::new(
            request.desired_rect,
            candidate.visual_rect,
            request.gap,
            request.threshold,
        );
        for target in resize_snap_targets(
            request.desired_rect,
            candidate.visual_rect,
            request.gap,
            eligibility,
            handle,
            request.threshold,
        ) {
            best = consider_resize_snap(best, target, &request);
        }
    }

    let snap = best?;
    let rect = resize_rect_from_anchor(
        request.anchor,
        request.base_display_size,
        snap.scale,
        handle,
    );
    Some(GuideSnapResult {
        rect,
        indicators: vec![guide_indicator_for_axis(
            snap.axis,
            snap.coord,
            snap.axis_snap,
            rect,
        )],
    })
}

pub(crate) fn snap_uniform_scale(
    request: GuideUniformScaleSnapRequest<'_>,
) -> Option<GuideSnapResult> {
    if request.candidates.is_empty() {
        return None;
    }

    let desired_rect =
        uniform_scale_rect(request.origin, request.start_rect, request.desired_factor);
    let mut snaps = Vec::new();
    for candidate in request.candidates {
        let eligibility = GuideContactEligibility::new(
            desired_rect,
            candidate.visual_rect,
            request.gap,
            request.threshold,
        );
        for target in uniform_scale_snap_targets(
            request.start_rect,
            desired_rect,
            candidate.visual_rect,
            request.gap,
            eligibility,
            request.threshold,
        ) {
            consider_uniform_scale_snap(&mut snaps, target, &request);
        }
    }

    let best = snaps
        .iter()
        .min_by(|left, right| left.distance.total_cmp(&right.distance))
        .copied()?;
    let rect = uniform_scale_rect(request.origin, request.start_rect, best.factor);
    let mut indicators = Vec::new();
    for snap in snaps {
        if (snap.factor - best.factor).abs() <= EPSILON
            && (snap.distance - best.distance).abs() <= request.threshold
        {
            indicators.push(guide_indicator_for_axis(
                snap.axis,
                snap.coord,
                snap.axis_snap,
                rect,
            ));
        }
    }
    dedupe_indicators(&mut indicators);

    Some(GuideSnapResult { rect, indicators })
}

#[derive(Clone, Copy, Debug)]
struct GuideResizeSnap {
    scale: f32,
    distance: f32,
    axis: GuideAxis,
    coord: f32,
    axis_snap: GuideAxisSnap,
}

#[derive(Clone, Copy, Debug)]
struct GuideResizeTarget {
    axis: GuideAxis,
    desired_coord: f32,
    target_coord: f32,
    kind: GuideSnapKind,
    candidate_rect: egui::Rect,
}

#[derive(Clone, Copy, Debug)]
struct GuideUniformScaleSnap {
    factor: f32,
    distance: f32,
    axis: GuideAxis,
    coord: f32,
    axis_snap: GuideAxisSnap,
}

#[derive(Clone, Copy, Debug)]
struct GuideUniformScaleTarget {
    axis: GuideAxis,
    desired_coord: f32,
    start_coord: f32,
    target_coord: f32,
    kind: GuideSnapKind,
    candidate_rect: egui::Rect,
}

#[derive(Clone, Copy, Debug)]
struct ResizeHandleFromAnchor {
    x_sign: f32,
    y_sign: f32,
}

fn axis_snaps(
    axis: GuideAxis,
    desired_rect: egui::Rect,
    candidates: &[GuideSnapCandidate],
    gap: f32,
    threshold: f32,
) -> Vec<GuideAxisSnap> {
    let mut snaps = Vec::new();
    for candidate in candidates {
        let eligibility =
            GuideContactEligibility::new(desired_rect, candidate.visual_rect, gap, threshold);
        for target in axis_snap_targets(axis, desired_rect, candidate.visual_rect, gap, eligibility)
        {
            if let Some(snap) = axis_snap_for_target(target, threshold) {
                snaps.push(snap);
            }
        }
    }
    snaps
}

fn best_snap_for_axis(snaps: &[GuideAxisSnap]) -> Option<GuideAxisSnap> {
    snaps
        .iter()
        .min_by(|left, right| left.distance.total_cmp(&right.distance))
        .copied()
}

fn co_winning_axis_snaps(
    snaps: &[GuideAxisSnap],
    best: Option<GuideAxisSnap>,
) -> Vec<GuideAxisSnap> {
    let Some(best) = best else {
        return Vec::new();
    };
    snaps
        .iter()
        .copied()
        .filter(|snap| {
            (snap.distance - best.distance).abs() <= EPSILON
                && (snap.nudge - best.nudge).abs() <= EPSILON
                && snap.kind == best.kind
        })
        .collect()
}

#[derive(Clone, Copy, Debug)]
struct GuideContactEligibility {
    x_contact: bool,
    y_contact: bool,
}

impl GuideContactEligibility {
    fn new(desired_rect: egui::Rect, candidate_rect: egui::Rect, gap: f32, threshold: f32) -> Self {
        let gap = gap.max(0.0);
        let x_contact = [
            candidate_rect.right() + gap - desired_rect.left(),
            candidate_rect.left() - gap - desired_rect.right(),
        ]
        .into_iter()
        .any(|nudge| nudge.abs() <= threshold)
            && intervals_near_or_overlap(
                desired_rect.top(),
                desired_rect.bottom(),
                candidate_rect.top(),
                candidate_rect.bottom(),
                threshold,
            );
        let y_contact = [
            candidate_rect.bottom() + gap - desired_rect.top(),
            candidate_rect.top() - gap - desired_rect.bottom(),
        ]
        .into_iter()
        .any(|nudge| nudge.abs() <= threshold)
            && intervals_near_or_overlap(
                desired_rect.left(),
                desired_rect.right(),
                candidate_rect.left(),
                candidate_rect.right(),
                threshold,
            );

        Self {
            x_contact,
            y_contact,
        }
    }
}

fn intervals_near_or_overlap(
    left_min: f32,
    left_max: f32,
    right_min: f32,
    right_max: f32,
    threshold: f32,
) -> bool {
    left_min <= right_max + threshold && left_max >= right_min - threshold
}

#[derive(Clone, Copy, Debug)]
struct GuideAxisTarget {
    desired_coord: f32,
    target_coord: f32,
    kind: GuideSnapKind,
    candidate_rect: egui::Rect,
}

fn axis_snap_targets(
    axis: GuideAxis,
    desired_rect: egui::Rect,
    candidate_rect: egui::Rect,
    gap: f32,
    eligibility: GuideContactEligibility,
) -> Vec<GuideAxisTarget> {
    let gap = gap.max(0.0);
    let mut targets = Vec::with_capacity(5);
    match axis {
        GuideAxis::Vertical => {
            if eligibility.y_contact {
                targets.push(GuideAxisTarget {
                    desired_coord: desired_rect.left(),
                    target_coord: candidate_rect.left(),
                    kind: GuideSnapKind::Edge,
                    candidate_rect,
                });
                targets.push(GuideAxisTarget {
                    desired_coord: desired_rect.center().x,
                    target_coord: candidate_rect.center().x,
                    kind: GuideSnapKind::Center,
                    candidate_rect,
                });
                targets.push(GuideAxisTarget {
                    desired_coord: desired_rect.right(),
                    target_coord: candidate_rect.right(),
                    kind: GuideSnapKind::Edge,
                    candidate_rect,
                });
            }
            if eligibility.x_contact {
                targets.push(GuideAxisTarget {
                    desired_coord: desired_rect.left(),
                    target_coord: candidate_rect.right() + gap,
                    kind: GuideSnapKind::GapSpacing,
                    candidate_rect,
                });
                targets.push(GuideAxisTarget {
                    desired_coord: desired_rect.right(),
                    target_coord: candidate_rect.left() - gap,
                    kind: GuideSnapKind::GapSpacing,
                    candidate_rect,
                });
            }
        }
        GuideAxis::Horizontal => {
            if eligibility.x_contact {
                targets.push(GuideAxisTarget {
                    desired_coord: desired_rect.top(),
                    target_coord: candidate_rect.top(),
                    kind: GuideSnapKind::Edge,
                    candidate_rect,
                });
                targets.push(GuideAxisTarget {
                    desired_coord: desired_rect.center().y,
                    target_coord: candidate_rect.center().y,
                    kind: GuideSnapKind::Center,
                    candidate_rect,
                });
                targets.push(GuideAxisTarget {
                    desired_coord: desired_rect.bottom(),
                    target_coord: candidate_rect.bottom(),
                    kind: GuideSnapKind::Edge,
                    candidate_rect,
                });
            }
            if eligibility.y_contact {
                targets.push(GuideAxisTarget {
                    desired_coord: desired_rect.top(),
                    target_coord: candidate_rect.bottom() + gap,
                    kind: GuideSnapKind::GapSpacing,
                    candidate_rect,
                });
                targets.push(GuideAxisTarget {
                    desired_coord: desired_rect.bottom(),
                    target_coord: candidate_rect.top() - gap,
                    kind: GuideSnapKind::GapSpacing,
                    candidate_rect,
                });
            }
        }
    }
    targets
}

fn axis_snap_for_target(target: GuideAxisTarget, threshold: f32) -> Option<GuideAxisSnap> {
    let nudge = target.target_coord - target.desired_coord;
    let distance = nudge.abs();
    if distance > threshold || distance <= EPSILON {
        return None;
    }
    Some(GuideAxisSnap {
        nudge,
        distance,
        coord: target.target_coord,
        kind: target.kind,
        candidate_rect: target.candidate_rect,
    })
}

fn resize_snap_targets(
    desired_rect: egui::Rect,
    candidate_rect: egui::Rect,
    gap: f32,
    eligibility: GuideContactEligibility,
    handle: ResizeHandleFromAnchor,
    threshold: f32,
) -> Vec<GuideResizeTarget> {
    let gap = gap.max(0.0);
    let mut targets = Vec::with_capacity(4);
    if intervals_near_or_overlap(
        desired_rect.top(),
        desired_rect.bottom(),
        candidate_rect.top(),
        candidate_rect.bottom(),
        threshold,
    ) {
        let desired_coord = if handle.x_sign > 0.0 {
            desired_rect.right()
        } else {
            desired_rect.left()
        };
        targets.push(GuideResizeTarget {
            axis: GuideAxis::Vertical,
            desired_coord,
            target_coord: candidate_rect.left(),
            kind: GuideSnapKind::Edge,
            candidate_rect,
        });
        targets.push(GuideResizeTarget {
            axis: GuideAxis::Vertical,
            desired_coord,
            target_coord: candidate_rect.right(),
            kind: GuideSnapKind::Edge,
            candidate_rect,
        });
    }
    if eligibility.x_contact {
        let desired_coord = if handle.x_sign > 0.0 {
            desired_rect.right()
        } else {
            desired_rect.left()
        };
        targets.push(GuideResizeTarget {
            axis: GuideAxis::Vertical,
            desired_coord,
            target_coord: if handle.x_sign > 0.0 {
                candidate_rect.left() - gap
            } else {
                candidate_rect.right() + gap
            },
            kind: GuideSnapKind::GapSpacing,
            candidate_rect,
        });
    }
    if intervals_near_or_overlap(
        desired_rect.left(),
        desired_rect.right(),
        candidate_rect.left(),
        candidate_rect.right(),
        threshold,
    ) {
        let desired_coord = if handle.y_sign > 0.0 {
            desired_rect.bottom()
        } else {
            desired_rect.top()
        };
        targets.push(GuideResizeTarget {
            axis: GuideAxis::Horizontal,
            desired_coord,
            target_coord: candidate_rect.top(),
            kind: GuideSnapKind::Edge,
            candidate_rect,
        });
        targets.push(GuideResizeTarget {
            axis: GuideAxis::Horizontal,
            desired_coord,
            target_coord: candidate_rect.bottom(),
            kind: GuideSnapKind::Edge,
            candidate_rect,
        });
    }
    if eligibility.y_contact {
        let desired_coord = if handle.y_sign > 0.0 {
            desired_rect.bottom()
        } else {
            desired_rect.top()
        };
        targets.push(GuideResizeTarget {
            axis: GuideAxis::Horizontal,
            desired_coord,
            target_coord: if handle.y_sign > 0.0 {
                candidate_rect.top() - gap
            } else {
                candidate_rect.bottom() + gap
            },
            kind: GuideSnapKind::GapSpacing,
            candidate_rect,
        });
    }
    targets
}

fn consider_resize_snap(
    best: Option<GuideResizeSnap>,
    target: GuideResizeTarget,
    request: &GuideResizeSnapRequest<'_>,
) -> Option<GuideResizeSnap> {
    let distance = (target.target_coord - target.desired_coord).abs();
    if distance > request.threshold || distance <= EPSILON {
        return best;
    }

    let scale = match target.axis {
        GuideAxis::Vertical => {
            (target.target_coord - request.anchor.x).abs()
                / request.base_display_size.x.max(EPSILON)
        }
        GuideAxis::Horizontal => {
            (target.target_coord - request.anchor.y).abs()
                / request.base_display_size.y.max(EPSILON)
        }
    };
    if scale < request.min_scale {
        return best;
    }

    let axis_snap = GuideAxisSnap {
        nudge: target.target_coord - target.desired_coord,
        distance,
        coord: target.target_coord,
        kind: target.kind,
        candidate_rect: target.candidate_rect,
    };
    match best {
        Some(best) if best.distance <= distance => Some(best),
        _ => Some(GuideResizeSnap {
            scale,
            distance,
            axis: target.axis,
            coord: target.target_coord,
            axis_snap,
        }),
    }
}

fn uniform_scale_snap_targets(
    start_rect: egui::Rect,
    desired_rect: egui::Rect,
    candidate_rect: egui::Rect,
    gap: f32,
    eligibility: GuideContactEligibility,
    threshold: f32,
) -> Vec<GuideUniformScaleTarget> {
    let gap = gap.max(0.0);
    let mut targets = Vec::with_capacity(12);
    if intervals_near_or_overlap(
        desired_rect.top(),
        desired_rect.bottom(),
        candidate_rect.top(),
        candidate_rect.bottom(),
        threshold,
    ) {
        for (desired_coord, start_coord) in [
            (desired_rect.left(), start_rect.left()),
            (desired_rect.right(), start_rect.right()),
        ] {
            for target_coord in [candidate_rect.left(), candidate_rect.right()] {
                targets.push(GuideUniformScaleTarget {
                    axis: GuideAxis::Vertical,
                    desired_coord,
                    start_coord,
                    target_coord,
                    kind: GuideSnapKind::Edge,
                    candidate_rect,
                });
            }
        }
    }
    if eligibility.x_contact {
        targets.push(GuideUniformScaleTarget {
            axis: GuideAxis::Vertical,
            desired_coord: desired_rect.left(),
            start_coord: start_rect.left(),
            target_coord: candidate_rect.right() + gap,
            kind: GuideSnapKind::GapSpacing,
            candidate_rect,
        });
        targets.push(GuideUniformScaleTarget {
            axis: GuideAxis::Vertical,
            desired_coord: desired_rect.right(),
            start_coord: start_rect.right(),
            target_coord: candidate_rect.left() - gap,
            kind: GuideSnapKind::GapSpacing,
            candidate_rect,
        });
    }

    if intervals_near_or_overlap(
        desired_rect.left(),
        desired_rect.right(),
        candidate_rect.left(),
        candidate_rect.right(),
        threshold,
    ) {
        for (desired_coord, start_coord) in [
            (desired_rect.top(), start_rect.top()),
            (desired_rect.bottom(), start_rect.bottom()),
        ] {
            for target_coord in [candidate_rect.top(), candidate_rect.bottom()] {
                targets.push(GuideUniformScaleTarget {
                    axis: GuideAxis::Horizontal,
                    desired_coord,
                    start_coord,
                    target_coord,
                    kind: GuideSnapKind::Edge,
                    candidate_rect,
                });
            }
        }
    }
    if eligibility.y_contact {
        targets.push(GuideUniformScaleTarget {
            axis: GuideAxis::Horizontal,
            desired_coord: desired_rect.top(),
            start_coord: start_rect.top(),
            target_coord: candidate_rect.bottom() + gap,
            kind: GuideSnapKind::GapSpacing,
            candidate_rect,
        });
        targets.push(GuideUniformScaleTarget {
            axis: GuideAxis::Horizontal,
            desired_coord: desired_rect.bottom(),
            start_coord: start_rect.bottom(),
            target_coord: candidate_rect.top() - gap,
            kind: GuideSnapKind::GapSpacing,
            candidate_rect,
        });
    }
    targets
}

fn consider_uniform_scale_snap(
    snaps: &mut Vec<GuideUniformScaleSnap>,
    target: GuideUniformScaleTarget,
    request: &GuideUniformScaleSnapRequest<'_>,
) {
    let distance = (target.target_coord - target.desired_coord).abs();
    if distance > request.threshold || distance <= EPSILON {
        return;
    }

    let denominator = match target.axis {
        GuideAxis::Vertical => target.start_coord - request.origin.x,
        GuideAxis::Horizontal => target.start_coord - request.origin.y,
    };
    if denominator.abs() <= EPSILON {
        return;
    }

    let factor = match target.axis {
        GuideAxis::Vertical => (target.target_coord - request.origin.x) / denominator,
        GuideAxis::Horizontal => (target.target_coord - request.origin.y) / denominator,
    };
    if factor < request.min_factor {
        return;
    }

    let axis_snap = GuideAxisSnap {
        nudge: target.target_coord - target.desired_coord,
        distance,
        coord: target.target_coord,
        kind: target.kind,
        candidate_rect: target.candidate_rect,
    };
    snaps.push(GuideUniformScaleSnap {
        factor,
        distance,
        axis: target.axis,
        coord: target.target_coord,
        axis_snap,
    });
}

fn uniform_scale_rect(origin: egui::Pos2, start_rect: egui::Rect, factor: f32) -> egui::Rect {
    egui::Rect::from_min_max(
        egui::pos2(
            origin.x + (start_rect.left() - origin.x) * factor,
            origin.y + (start_rect.top() - origin.y) * factor,
        ),
        egui::pos2(
            origin.x + (start_rect.right() - origin.x) * factor,
            origin.y + (start_rect.bottom() - origin.y) * factor,
        ),
    )
}

fn resize_handle_from_anchor(
    anchor: egui::Pos2,
    desired_rect: egui::Rect,
) -> Option<ResizeHandleFromAnchor> {
    let center = desired_rect.center();
    let x_sign = if anchor.x < center.x { 1.0 } else { -1.0 };
    let y_sign = if anchor.y < center.y { 1.0 } else { -1.0 };
    if (anchor.x - center.x).abs() <= EPSILON || (anchor.y - center.y).abs() <= EPSILON {
        return None;
    }
    Some(ResizeHandleFromAnchor { x_sign, y_sign })
}

fn resize_rect_from_anchor(
    anchor: egui::Pos2,
    base_display_size: egui::Vec2,
    scale: f32,
    handle: ResizeHandleFromAnchor,
) -> egui::Rect {
    let handle_pos = egui::pos2(
        anchor.x + handle.x_sign * base_display_size.x * scale,
        anchor.y + handle.y_sign * base_display_size.y * scale,
    );
    egui::Rect::from_two_pos(anchor, handle_pos)
}

fn guide_indicator_for_axis(
    axis: GuideAxis,
    coord: f32,
    snap: GuideAxisSnap,
    snapped_rect: egui::Rect,
) -> GuideSnapIndicator {
    let mut marks = Vec::new();
    let snapped_kind = match snap.kind {
        GuideSnapKind::Edge | GuideSnapKind::GapSpacing => GuideSnapKind::Edge,
        GuideSnapKind::Center => GuideSnapKind::Center,
    };
    collect_axis_marks(axis, coord, snapped_kind, snapped_rect, &mut marks);
    collect_candidate_marks(axis, coord, snap, &mut marks);
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

fn collect_candidate_marks(
    axis: GuideAxis,
    coord: f32,
    snap: GuideAxisSnap,
    marks: &mut Vec<egui::Pos2>,
) {
    match snap.kind {
        GuideSnapKind::Edge | GuideSnapKind::Center => {
            collect_axis_marks(axis, coord, snap.kind, snap.candidate_rect, marks);
        }
        GuideSnapKind::GapSpacing => match axis {
            GuideAxis::Vertical => {
                marks.push(egui::pos2(coord, snap.candidate_rect.top()));
                marks.push(egui::pos2(coord, snap.candidate_rect.bottom()));
            }
            GuideAxis::Horizontal => {
                marks.push(egui::pos2(snap.candidate_rect.left(), coord));
                marks.push(egui::pos2(snap.candidate_rect.right(), coord));
            }
        },
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
        GuideSnapKind::Edge | GuideSnapKind::GapSpacing => vec![
            rect.left_top(),
            rect.right_top(),
            rect.left_bottom(),
            rect.right_bottom(),
        ],
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

fn dedupe_indicators(indicators: &mut Vec<GuideSnapIndicator>) {
    let mut deduped = Vec::with_capacity(indicators.len());
    for indicator in indicators.drain(..) {
        if deduped
            .iter()
            .any(|existing: &GuideSnapIndicator| existing.segments == indicator.segments)
        {
            continue;
        }
        deduped.push(indicator);
    }
    *indicators = deduped;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uniform_scale_snap_scales_edge_to_gap_target() {
        let start_rect = egui::Rect::from_center_size(egui::Pos2::ZERO, egui::vec2(100.0, 100.0));
        let candidates = [GuideSnapCandidate::new(egui::Rect::from_min_size(
            egui::pos2(124.0, -50.0),
            egui::vec2(100.0, 100.0),
        ))];

        let result = snap_uniform_scale(GuideUniformScaleSnapRequest {
            origin: egui::Pos2::ZERO,
            start_rect,
            desired_factor: 1.96,
            min_factor: 0.1,
            candidates: &candidates,
            gap: 24.0,
            threshold: 8.0,
        })
        .expect("expected uniform scale snap");

        assert_eq!(result.rect.right(), 100.0);
        assert_eq!(result.rect.width(), 200.0);
        assert_eq!(result.indicators.len(), 1);
    }

    #[test]
    fn rect_snap_reports_all_equal_edge_guides_for_the_chosen_nudge() {
        let desired_rect =
            egui::Rect::from_min_size(egui::pos2(120.0, 4.0), egui::vec2(100.0, 100.0));
        let candidates = [GuideSnapCandidate::new(egui::Rect::from_min_size(
            egui::pos2(0.0, 0.0),
            egui::vec2(100.0, 100.0),
        ))];

        let result = snap_rect(GuideSnapRequest {
            desired_rect,
            candidates: &candidates,
            gap: 24.0,
            threshold: 8.0,
        })
        .expect("expected gap snap plus co-winning horizontal guides");

        assert_eq!(result.rect.left(), 124.0);
        assert_eq!(result.rect.top(), 0.0);
        assert_eq!(result.indicators.len(), 3);
        assert!(result.indicators.iter().any(|indicator| {
            indicator.segments == vec![(egui::pos2(0.0, 0.0), egui::pos2(224.0, 0.0))]
        }));
        assert!(result.indicators.iter().any(|indicator| {
            indicator.segments == vec![(egui::pos2(0.0, 100.0), egui::pos2(224.0, 100.0))]
        }));
    }

    #[test]
    fn uniform_scale_snap_rejects_factor_below_minimum() {
        let start_rect = egui::Rect::from_center_size(egui::Pos2::ZERO, egui::vec2(100.0, 100.0));
        let candidates = [GuideSnapCandidate::new(egui::Rect::from_min_size(
            egui::pos2(34.0, -50.0),
            egui::vec2(100.0, 100.0),
        ))];

        let result = snap_uniform_scale(GuideUniformScaleSnapRequest {
            origin: egui::Pos2::ZERO,
            start_rect,
            desired_factor: 0.24,
            min_factor: 0.5,
            candidates: &candidates,
            gap: 24.0,
            threshold: 8.0,
        });

        assert!(result.is_none());
    }

    #[test]
    fn resize_snap_scales_right_edge_to_gap_target() {
        let desired_rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(98.0, 98.0));
        let candidates = [GuideSnapCandidate::new(egui::Rect::from_min_size(
            egui::pos2(124.0, 0.0),
            egui::vec2(100.0, 100.0),
        ))];

        let result = snap_resize(GuideResizeSnapRequest {
            desired_rect,
            anchor: desired_rect.left_top(),
            base_display_size: egui::vec2(100.0, 100.0),
            min_scale: 0.1,
            candidates: &candidates,
            gap: 24.0,
            threshold: 8.0,
        })
        .expect("expected resize snap");

        assert_eq!(result.rect.right(), 100.0);
        assert_eq!(result.rect.bottom(), 100.0);
        assert_eq!(result.indicators.len(), 1);
    }

    #[test]
    fn resize_snap_scales_right_edge_to_normal_edge_target() {
        let desired_rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(98.0, 98.0));
        let candidates = [GuideSnapCandidate::new(egui::Rect::from_min_size(
            egui::pos2(100.0, 0.0),
            egui::vec2(100.0, 100.0),
        ))];

        let result = snap_resize(GuideResizeSnapRequest {
            desired_rect,
            anchor: desired_rect.left_top(),
            base_display_size: egui::vec2(100.0, 100.0),
            min_scale: 0.1,
            candidates: &candidates,
            gap: 24.0,
            threshold: 8.0,
        })
        .expect("expected resize snap");

        assert_eq!(result.rect.right(), 100.0);
        assert_eq!(result.rect.bottom(), 100.0);
        assert_eq!(result.indicators.len(), 1);
    }

    #[test]
    fn resize_snap_scales_bottom_edge_to_gap_target() {
        let desired_rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(98.0, 98.0));
        let candidates = [GuideSnapCandidate::new(egui::Rect::from_min_size(
            egui::pos2(0.0, 124.0),
            egui::vec2(100.0, 100.0),
        ))];

        let result = snap_resize(GuideResizeSnapRequest {
            desired_rect,
            anchor: desired_rect.left_top(),
            base_display_size: egui::vec2(100.0, 100.0),
            min_scale: 0.1,
            candidates: &candidates,
            gap: 24.0,
            threshold: 8.0,
        })
        .expect("expected resize snap");

        assert_eq!(result.rect.right(), 100.0);
        assert_eq!(result.rect.bottom(), 100.0);
        assert_eq!(result.indicators.len(), 1);
    }

    #[test]
    fn resize_snap_scales_bottom_edge_to_normal_edge_target() {
        let desired_rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(98.0, 98.0));
        let candidates = [GuideSnapCandidate::new(egui::Rect::from_min_size(
            egui::pos2(0.0, 100.0),
            egui::vec2(100.0, 100.0),
        ))];

        let result = snap_resize(GuideResizeSnapRequest {
            desired_rect,
            anchor: desired_rect.left_top(),
            base_display_size: egui::vec2(100.0, 100.0),
            min_scale: 0.1,
            candidates: &candidates,
            gap: 24.0,
            threshold: 8.0,
        })
        .expect("expected resize snap");

        assert_eq!(result.rect.right(), 100.0);
        assert_eq!(result.rect.bottom(), 100.0);
        assert_eq!(result.indicators.len(), 1);
    }

    #[test]
    fn resize_snap_ignores_far_cross_axis_candidate() {
        let desired_rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(98.0, 98.0));
        let candidates = [GuideSnapCandidate::new(egui::Rect::from_min_size(
            egui::pos2(124.0, 260.0),
            egui::vec2(100.0, 100.0),
        ))];

        let result = snap_resize(GuideResizeSnapRequest {
            desired_rect,
            anchor: desired_rect.left_top(),
            base_display_size: egui::vec2(100.0, 100.0),
            min_scale: 0.1,
            candidates: &candidates,
            gap: 24.0,
            threshold: 8.0,
        });

        assert!(result.is_none());
    }

    #[test]
    fn resize_snap_allows_scale_below_one_when_above_min_scale() {
        let desired_rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(298.0, 298.0));
        let candidates = [GuideSnapCandidate::new(egui::Rect::from_min_size(
            egui::pos2(300.0, 0.0),
            egui::vec2(100.0, 100.0),
        ))];

        let result = snap_resize(GuideResizeSnapRequest {
            desired_rect,
            anchor: desired_rect.left_top(),
            base_display_size: egui::vec2(1000.0, 1000.0),
            min_scale: 0.024,
            candidates: &candidates,
            gap: 24.0,
            threshold: 8.0,
        })
        .expect("expected resize snap below asset scale 1.0");

        assert_eq!(result.rect.right(), 300.0);
        assert_eq!(result.rect.bottom(), 300.0);
        assert_eq!(result.indicators.len(), 1);
    }

    #[test]
    fn resize_snap_preserves_aspect_ratio() {
        let desired_rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(96.0, 48.0));
        let candidates = [GuideSnapCandidate::new(egui::Rect::from_min_size(
            egui::pos2(124.0, 0.0),
            egui::vec2(100.0, 50.0),
        ))];

        let result = snap_resize(GuideResizeSnapRequest {
            desired_rect,
            anchor: desired_rect.left_top(),
            base_display_size: egui::vec2(100.0, 50.0),
            min_scale: 0.1,
            candidates: &candidates,
            gap: 24.0,
            threshold: 8.0,
        })
        .expect("expected resize snap");

        assert_eq!(result.rect.size(), egui::vec2(100.0, 50.0));
    }
}
