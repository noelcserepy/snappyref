use eframe::egui;

use crate::canvas::SelectedLayoutItem;
use crate::data::{Placement, PlacementId};

pub(crate) const SETTLE_BLOCK_GRACE_SCREEN: f32 = 1.0;
const COMPACT_NORMALIZE_ABSOLUTE_THRESHOLD: f32 = 1.0;
const COMPACT_NORMALIZE_RELATIVE_THRESHOLD: f32 = 0.05;
const COMPACT_NORMALIZE_OCCUPANCY_GAIN: f32 = 0.025;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SettleDirection {
    Left,
    Right,
    Up,
    Down,
}

#[derive(Clone, Debug)]
pub(crate) struct CompactInputItem {
    pub(crate) layout: SelectedLayoutItem,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct CompactInput {
    pub(crate) placement_id: PlacementId,
    pub(crate) bounds: egui::Rect,
}

impl CompactInput {
    pub(crate) fn layout_item(self) -> SelectedLayoutItem {
        SelectedLayoutItem::from_rect(self.placement_id, self.bounds)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct CompactOutput {
    pub(crate) placement_id: PlacementId,
    pub(crate) center: egui::Pos2,
}

impl Placement {
    pub(crate) fn compact_input(&self) -> CompactInput {
        CompactInput {
            placement_id: self.id(),
            bounds: self.visual_bounds(),
        }
    }

    pub(crate) fn apply_compact_output(&mut self, output: CompactOutput) {
        if output.placement_id == self.id() {
            self.state.geometry.center = output.center;
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct UserCompactLayout {
    pub(crate) placements: Vec<(i64, egui::Pos2)>,
    pub(crate) scale_factors: Vec<(i64, f32)>,
    pub(crate) width: f32,
    pub(crate) height: f32,
}

pub(crate) fn horizontal_layout(
    items: &[SelectedLayoutItem],
    gap: f32,
) -> Option<(Vec<(i64, egui::Pos2)>, f32, f32)> {
    if items.is_empty() {
        return None;
    }

    let max_height = items
        .iter()
        .map(|item| item.height())
        .fold(0.0_f32, f32::max);
    let mut x_cursor = 0.0;
    let mut placements = Vec::with_capacity(items.len());

    for item in items {
        let center = egui::pos2(
            x_cursor + item.width() * 0.5,
            max_height - item.height() * 0.5,
        );
        placements.push((item.id, center));
        x_cursor += item.width() + gap;
    }

    let total_width = (x_cursor - gap).max(0.0);
    Some((placements, total_width, max_height))
}

pub(crate) fn vertical_layout(
    items: &[SelectedLayoutItem],
    gap: f32,
) -> Option<(Vec<(i64, egui::Pos2)>, f32, f32)> {
    if items.is_empty() {
        return None;
    }

    let max_width = items
        .iter()
        .map(|item| item.width())
        .fold(0.0_f32, f32::max);
    let mut y_cursor = 0.0;
    let mut placements = Vec::with_capacity(items.len());

    for item in items {
        let center = egui::pos2(item.width() * 0.5, y_cursor + item.height() * 0.5);
        placements.push((item.id, center));
        y_cursor += item.height() + gap;
    }

    let total_height = (y_cursor - gap).max(0.0);
    Some((placements, max_width, total_height))
}

pub(crate) fn directional_settle_layout(
    items: &[SelectedLayoutItem],
    direction: SettleDirection,
    gap: f32,
    cross_axis_grace: f32,
) -> Option<Vec<(i64, egui::Pos2)>> {
    let bounds = layout_bounds_for_items(items)?;
    let blocking_gap = (gap - cross_axis_grace).max(0.0);
    let mut sorted = items.to_vec();
    sorted.sort_by(|left, right| settle_order(left, right, direction));

    let mut placements = Vec::with_capacity(sorted.len());
    let mut settled_rects = Vec::with_capacity(sorted.len());

    for item in sorted {
        let center =
            settled_center_for_item(&item, direction, bounds, gap, blocking_gap, &settled_rects);
        settled_rects.push(egui::Rect::from_center_size(
            center,
            egui::vec2(item.width(), item.height()),
        ));
        placements.push((item.id, center));
    }

    Some(placements)
}

pub(crate) fn layout_bounds_for_items(items: &[SelectedLayoutItem]) -> Option<egui::Rect> {
    let mut bounds = egui::Rect::NOTHING;
    let mut found = false;
    for item in items {
        let rect = item.rect();
        if found {
            bounds = bounds.union(rect);
        } else {
            bounds = rect;
            found = true;
        }
    }
    found.then_some(bounds)
}

fn settle_order(
    left: &SelectedLayoutItem,
    right: &SelectedLayoutItem,
    direction: SettleDirection,
) -> std::cmp::Ordering {
    let left_rect = layout_item_rect(left);
    let right_rect = layout_item_rect(right);
    match direction {
        SettleDirection::Right => right_rect
            .right()
            .total_cmp(&left_rect.right())
            .then_with(|| right.center().x.total_cmp(&left.center().x))
            .then_with(|| left.id.cmp(&right.id)),
        SettleDirection::Left => left_rect
            .left()
            .total_cmp(&right_rect.left())
            .then_with(|| left.center().x.total_cmp(&right.center().x))
            .then_with(|| left.id.cmp(&right.id)),
        SettleDirection::Down => right_rect
            .bottom()
            .total_cmp(&left_rect.bottom())
            .then_with(|| right.center().y.total_cmp(&left.center().y))
            .then_with(|| left.id.cmp(&right.id)),
        SettleDirection::Up => left_rect
            .top()
            .total_cmp(&right_rect.top())
            .then_with(|| left.center().y.total_cmp(&right.center().y))
            .then_with(|| left.id.cmp(&right.id)),
    }
}

fn layout_item_rect(item: &SelectedLayoutItem) -> egui::Rect {
    item.rect()
}

fn settled_center_for_item(
    item: &SelectedLayoutItem,
    direction: SettleDirection,
    bounds: egui::Rect,
    gap: f32,
    blocking_gap: f32,
    settled_rects: &[egui::Rect],
) -> egui::Pos2 {
    let half_width = item.width() * 0.5;
    let half_height = item.height() * 0.5;
    match direction {
        SettleDirection::Right => {
            let mut x = bounds.right() - half_width;
            let item_top = item.center().y - half_height;
            let item_bottom = item.center().y + half_height;
            for blocker in settled_rects {
                if intervals_within_gap(
                    item_top,
                    item_bottom,
                    blocker.top(),
                    blocker.bottom(),
                    blocking_gap,
                ) {
                    x = x.min(blocker.left() - gap - half_width);
                }
            }
            egui::pos2(x, item.center().y)
        }
        SettleDirection::Left => {
            let mut x = bounds.left() + half_width;
            let item_top = item.center().y - half_height;
            let item_bottom = item.center().y + half_height;
            for blocker in settled_rects {
                if intervals_within_gap(
                    item_top,
                    item_bottom,
                    blocker.top(),
                    blocker.bottom(),
                    blocking_gap,
                ) {
                    x = x.max(blocker.right() + gap + half_width);
                }
            }
            egui::pos2(x, item.center().y)
        }
        SettleDirection::Down => {
            let mut y = bounds.bottom() - half_height;
            let item_left = item.center().x - half_width;
            let item_right = item.center().x + half_width;
            for blocker in settled_rects {
                if intervals_within_gap(
                    item_left,
                    item_right,
                    blocker.left(),
                    blocker.right(),
                    blocking_gap,
                ) {
                    y = y.min(blocker.top() - gap - half_height);
                }
            }
            egui::pos2(item.center().x, y)
        }
        SettleDirection::Up => {
            let mut y = bounds.top() + half_height;
            let item_left = item.center().x - half_width;
            let item_right = item.center().x + half_width;
            for blocker in settled_rects {
                if intervals_within_gap(
                    item_left,
                    item_right,
                    blocker.left(),
                    blocker.right(),
                    blocking_gap,
                ) {
                    y = y.max(blocker.bottom() + gap + half_height);
                }
            }
            egui::pos2(item.center().x, y)
        }
    }
}

fn intervals_within_gap(
    left_min: f32,
    left_max: f32,
    right_min: f32,
    right_max: f32,
    gap: f32,
) -> bool {
    left_min <= right_max + gap && right_min <= left_max + gap
}

#[derive(Clone, Copy)]
enum CompactSort {
    MaxSide,
    Area,
    Perimeter,
    Height,
}

#[derive(Clone, Copy)]
enum PackHeuristic {
    BestShortSide,
    BestLongSide,
    BestArea,
}

#[derive(Clone)]
struct CompactCandidate {
    placements: Vec<(i64, egui::Pos2)>,
    width: f32,
    height: f32,
    occupancy: f32,
    aspect: f32,
    right_edge_raggedness: f32,
    area: f32,
}

pub(crate) fn best_compact_layout(
    items: &[SelectedLayoutItem],
    gap: f32,
) -> Option<(Vec<(i64, egui::Pos2)>, f32, f32)> {
    best_compact_candidate(items, gap)
        .map(|candidate| (candidate.placements, candidate.width, candidate.height))
}

fn best_compact_candidate(items: &[SelectedLayoutItem], gap: f32) -> Option<CompactCandidate> {
    if items.is_empty() {
        return None;
    }

    let sorts = [
        CompactSort::MaxSide,
        CompactSort::Area,
        CompactSort::Perimeter,
        CompactSort::Height,
    ];
    let heuristics = [
        PackHeuristic::BestShortSide,
        PackHeuristic::BestArea,
        PackHeuristic::BestLongSide,
    ];
    let mut best_layout: Option<CompactCandidate> = None;

    for sort in sorts {
        let sorted_items = sorted_compact_items(items, sort);
        for heuristic in heuristics {
            for bin_width in compact_candidate_widths(&sorted_items, gap) {
                let Some(candidate) =
                    maxrects_pack_layout(&sorted_items, bin_width, gap, heuristic)
                else {
                    continue;
                };
                if best_layout
                    .as_ref()
                    .is_none_or(|best| compact_candidate_better(&candidate, best))
                {
                    best_layout = Some(candidate);
                }
            }
        }
    }

    best_layout
}

pub(crate) fn best_user_compact_layout(
    items: &[CompactInputItem],
    gap: f32,
) -> Option<UserCompactLayout> {
    let baseline = compact_variant_result(
        items,
        vec![(1.0, ScaleCandidateKind::Original); items.len()],
        gap,
    )?;
    let width_variant = compact_axis_normalized_variant(items, Axis::Width, gap);
    let height_variant = compact_axis_normalized_variant(items, Axis::Height, gap);
    let mut best = baseline.clone();

    for candidate in [width_variant, height_variant].into_iter().flatten() {
        if compact_normalized_variant_beats(&candidate, &best, &baseline) {
            best = candidate;
        }
    }

    Some(UserCompactLayout {
        placements: best.candidate.placements,
        scale_factors: best
            .scale_factors
            .into_iter()
            .enumerate()
            .map(|(index, factor)| (items[index].layout.id, factor))
            .collect(),
        width: best.candidate.width,
        height: best.candidate.height,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Axis {
    Width,
    Height,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ScaleCandidateKind {
    Original,
    Tiny,
    Downscale,
    Upscale,
}

#[derive(Clone)]
struct CompactVariantResult {
    candidate: CompactCandidate,
    scale_factors: Vec<f32>,
    changed_count: usize,
    free_changes_only: bool,
    total_scale_delta: f32,
    upscale_delta: f32,
}

#[derive(Clone, Debug)]
struct SizeBucket {
    preferred: f32,
    members: usize,
}

fn compact_axis_normalized_variant(
    items: &[CompactInputItem],
    axis: Axis,
    gap: f32,
) -> Option<CompactVariantResult> {
    let axis_buckets = compact_size_buckets(items, axis);
    let other_buckets = compact_size_buckets(items, opposite_axis(axis));
    let factors = items
        .iter()
        .map(|item| compact_axis_scale_factor(item, axis, &axis_buckets, &other_buckets))
        .collect::<Vec<_>>();

    if factors
        .iter()
        .all(|(factor, _)| (*factor - 1.0).abs() <= f32::EPSILON)
    {
        return None;
    }

    compact_variant_result(items, factors, gap)
}

fn compact_variant_result(
    items: &[CompactInputItem],
    factors: Vec<(f32, ScaleCandidateKind)>,
    gap: f32,
) -> Option<CompactVariantResult> {
    let layout_items = items
        .iter()
        .zip(factors.iter())
        .map(|(item, (factor, _))| item.layout.with_size(item.layout.size() * *factor))
        .collect::<Vec<_>>();
    let candidate = best_compact_candidate(&layout_items, gap)?;
    let mut changed_count = 0;
    let mut free_changes_only = true;
    let mut total_scale_delta = 0.0;
    let mut upscale_delta = 0.0;
    let scale_factors = factors
        .into_iter()
        .map(|(factor, kind)| {
            let delta = (factor - 1.0).abs();
            if delta > 0.001 {
                changed_count += 1;
                if kind != ScaleCandidateKind::Tiny {
                    free_changes_only = false;
                    total_scale_delta += delta;
                }
                if kind == ScaleCandidateKind::Upscale {
                    upscale_delta += delta;
                }
            }
            factor
        })
        .collect::<Vec<_>>();

    Some(CompactVariantResult {
        candidate,
        scale_factors,
        changed_count,
        free_changes_only,
        total_scale_delta,
        upscale_delta,
    })
}

fn compact_normalized_variant_beats(
    candidate: &CompactVariantResult,
    best: &CompactVariantResult,
    baseline: &CompactVariantResult,
) -> bool {
    if candidate.changed_count == 0 {
        return false;
    }
    let earned_by_density = candidate.candidate.occupancy
        >= baseline.candidate.occupancy + COMPACT_NORMALIZE_OCCUPANCY_GAIN;
    let earned_by_free_cleanup = candidate.free_changes_only
        && candidate.candidate.occupancy + 0.001 >= baseline.candidate.occupancy;
    if !earned_by_density && !earned_by_free_cleanup {
        return false;
    }

    candidate.candidate.occupancy > best.candidate.occupancy + 0.001
        || ((candidate.candidate.occupancy - best.candidate.occupancy).abs() <= 0.001
            && (candidate.changed_count < best.changed_count
                || (candidate.changed_count == best.changed_count
                    && (candidate.total_scale_delta < best.total_scale_delta - 0.001
                        || ((candidate.total_scale_delta - best.total_scale_delta).abs()
                            <= 0.001
                            && candidate.upscale_delta < best.upscale_delta - 0.001)))))
}

fn compact_axis_scale_factor(
    item: &CompactInputItem,
    axis: Axis,
    axis_buckets: &[SizeBucket],
    other_buckets: &[SizeBucket],
) -> (f32, ScaleCandidateKind) {
    let dimension = compact_item_dimension(item, axis);
    let Some(bucket) = axis_buckets
        .iter()
        .filter(|bucket| bucket.members >= 2)
        .filter(|bucket| compact_dimensions_near(dimension, bucket.preferred))
        .min_by(|left, right| {
            (dimension - left.preferred)
                .abs()
                .total_cmp(&(dimension - right.preferred).abs())
        })
    else {
        return (1.0, ScaleCandidateKind::Original);
    };

    if dimension <= f32::EPSILON {
        return (1.0, ScaleCandidateKind::Original);
    }
    let factor = bucket.preferred / dimension;
    if (factor - 1.0).abs() <= 0.001
        || !compact_other_axis_remains_stable(item, opposite_axis(axis), factor, other_buckets)
    {
        return (1.0, ScaleCandidateKind::Original);
    }
    let kind = if (bucket.preferred - dimension).abs() <= COMPACT_NORMALIZE_ABSOLUTE_THRESHOLD {
        ScaleCandidateKind::Tiny
    } else if factor < 1.0 {
        ScaleCandidateKind::Downscale
    } else {
        ScaleCandidateKind::Upscale
    };
    (factor, kind)
}

fn compact_other_axis_remains_stable(
    item: &CompactInputItem,
    other_axis: Axis,
    factor: f32,
    other_buckets: &[SizeBucket],
) -> bool {
    let original = compact_item_dimension(item, other_axis);
    let normalized = original * factor;
    compact_dimensions_near(original, normalized)
        || other_buckets
            .iter()
            .filter(|bucket| bucket.members >= 2)
            .any(|bucket| compact_dimensions_near(normalized, bucket.preferred))
}

fn compact_size_buckets(items: &[CompactInputItem], axis: Axis) -> Vec<SizeBucket> {
    let mut dimensions = items
        .iter()
        .map(|item| compact_item_dimension(item, axis))
        .filter(|dimension| dimension.is_finite() && *dimension > 0.0)
        .collect::<Vec<_>>();
    dimensions.sort_by(|left, right| left.total_cmp(right));

    let mut buckets = Vec::new();
    let mut current = Vec::new();
    for dimension in dimensions {
        if current
            .last()
            .is_some_and(|last| !compact_dimensions_near(*last, dimension))
        {
            buckets.push(size_bucket_from_dimensions(&current));
            current.clear();
        }
        current.push(dimension);
    }
    if !current.is_empty() {
        buckets.push(size_bucket_from_dimensions(&current));
    }
    buckets
}

fn size_bucket_from_dimensions(dimensions: &[f32]) -> SizeBucket {
    let preferred = if dimensions.len() == 2 {
        dimensions[1]
    } else {
        dimensions[dimensions.len() / 2]
    };
    SizeBucket {
        preferred,
        members: dimensions.len(),
    }
}

fn compact_item_dimension(item: &CompactInputItem, axis: Axis) -> f32 {
    match axis {
        Axis::Width => item.layout.width(),
        Axis::Height => item.layout.height(),
    }
}

fn opposite_axis(axis: Axis) -> Axis {
    match axis {
        Axis::Width => Axis::Height,
        Axis::Height => Axis::Width,
    }
}

fn compact_dimensions_near(left: f32, right: f32) -> bool {
    let difference = (left - right).abs();
    difference <= COMPACT_NORMALIZE_ABSOLUTE_THRESHOLD
        || difference / right.abs().max(1.0) <= COMPACT_NORMALIZE_RELATIVE_THRESHOLD
}

pub(crate) fn compact_layout_for_sizes(
    sizes: &[egui::Vec2],
    gap: f32,
    center_world: egui::Pos2,
) -> Vec<(egui::Pos2, egui::Vec2)> {
    if sizes.is_empty() {
        return Vec::new();
    }
    if sizes.len() == 1 {
        return vec![(center_world, sizes[0])];
    }

    let items = sizes
        .iter()
        .enumerate()
        .map(|(index, size)| {
            SelectedLayoutItem::new(
                index as i64,
                egui::pos2(0.0, 0.0),
                egui::vec2(size.x, size.y),
            )
        })
        .collect::<Vec<_>>();

    let Some((placements, width, height)) = best_compact_layout(&items, gap) else {
        return sizes.iter().map(|size| (center_world, *size)).collect();
    };
    let local_center = egui::pos2(width * 0.5, height * 0.5);
    let mut ordered = sizes
        .iter()
        .map(|size| (center_world, *size))
        .collect::<Vec<_>>();
    for (id, placement) in placements {
        if let Some(slot) = ordered.get_mut(id as usize) {
            *slot = (
                center_world + (placement - local_center),
                sizes[id as usize],
            );
        }
    }
    ordered
}

pub(crate) fn balanced_grid_layout_for_sizes(
    sizes: &[egui::Vec2],
    gap: f32,
    center_world: egui::Pos2,
) -> Vec<(egui::Pos2, egui::Vec2)> {
    if sizes.is_empty() {
        return Vec::new();
    }
    if sizes.len() == 1 {
        return vec![(center_world, sizes[0])];
    }

    let average_width = sizes.iter().map(|size| size.x + gap).sum::<f32>() / sizes.len() as f32;
    let average_height = sizes.iter().map(|size| size.y + gap).sum::<f32>() / sizes.len() as f32;
    let columns = ((sizes.len() as f32 * average_height / average_width.max(1.0))
        .sqrt()
        .ceil() as usize)
        .clamp(1, sizes.len());
    let rows = sizes.len().div_ceil(columns);

    let mut column_widths = vec![0.0_f32; columns];
    let mut row_heights = vec![0.0_f32; rows];
    for (index, size) in sizes.iter().enumerate() {
        let column = index % columns;
        let row = index / columns;
        column_widths[column] = column_widths[column].max(size.x);
        row_heights[row] = row_heights[row].max(size.y);
    }

    let total_width = column_widths.iter().sum::<f32>() + gap * columns.saturating_sub(1) as f32;
    let total_height = row_heights.iter().sum::<f32>() + gap * rows.saturating_sub(1) as f32;
    let start_x = center_world.x - total_width * 0.5;
    let start_y = center_world.y - total_height * 0.5;

    let mut centers = Vec::with_capacity(sizes.len());
    let mut y = start_y;
    for (row, row_height) in row_heights.iter().copied().enumerate() {
        let mut x = start_x;
        for (column, column_width) in column_widths.iter().copied().enumerate() {
            let index = row * columns + column;
            if let Some(size) = sizes.get(index).copied() {
                centers.push((
                    egui::pos2(x + column_width * 0.5, y + row_height * 0.5),
                    size,
                ));
            }
            x += column_width + gap;
        }
        y += row_height + gap;
    }

    centers
}

fn sorted_compact_items(
    items: &[SelectedLayoutItem],
    sort: CompactSort,
) -> Vec<SelectedLayoutItem> {
    let mut sorted = items.to_vec();
    sorted.sort_by(|left, right| match sort {
        CompactSort::MaxSide => right
            .width()
            .max(right.height())
            .total_cmp(&left.width().max(left.height()))
            .then_with(|| {
                (right.width() * right.height()).total_cmp(&(left.width() * left.height()))
            })
            .then_with(|| left.id.cmp(&right.id)),
        CompactSort::Area => (right.width() * right.height())
            .total_cmp(&(left.width() * left.height()))
            .then_with(|| {
                right
                    .width()
                    .max(right.height())
                    .total_cmp(&left.width().max(left.height()))
            })
            .then_with(|| left.id.cmp(&right.id)),
        CompactSort::Perimeter => (right.width() + right.height())
            .total_cmp(&(left.width() + left.height()))
            .then_with(|| {
                (right.width() * right.height()).total_cmp(&(left.width() * left.height()))
            })
            .then_with(|| left.id.cmp(&right.id)),
        CompactSort::Height => right
            .height()
            .total_cmp(&left.height())
            .then_with(|| right.width().total_cmp(&left.width()))
            .then_with(|| left.id.cmp(&right.id)),
    });
    sorted
}

fn compact_candidate_widths(items: &[SelectedLayoutItem], gap: f32) -> Vec<f32> {
    let total_area = items
        .iter()
        .map(|item| (item.width() + gap) * (item.height() + gap))
        .sum::<f32>();
    let max_width = items
        .iter()
        .map(|item| item.width() + gap)
        .fold(0.0_f32, f32::max);
    let ideal_width = total_area.sqrt().max(max_width);
    let mut widths = [0.85, 0.95, 1.0, 1.05, 1.15]
        .into_iter()
        .map(|multiplier| (ideal_width * multiplier).max(max_width))
        .collect::<Vec<_>>();
    widths.push(max_width);
    let mut running_width = 0.0_f32;

    for item in items.iter().take(12) {
        let effective_width = item.width() + gap;
        running_width += effective_width;
        widths.push(running_width.max(max_width));
    }
    widths.push(
        items
            .iter()
            .map(|item| item.width() + gap)
            .sum::<f32>()
            .max(max_width),
    );

    widths.sort_by(|left, right| left.total_cmp(right));
    widths.dedup_by(|left, right| (*left - *right).abs() < 0.5);
    widths
}

fn maxrects_pack_layout(
    items: &[SelectedLayoutItem],
    bin_width: f32,
    gap: f32,
    heuristic: PackHeuristic,
) -> Option<CompactCandidate> {
    if items.is_empty() {
        return None;
    }

    let total_height = items
        .iter()
        .map(|item| item.height() + gap)
        .sum::<f32>()
        .max(
            items
                .iter()
                .map(|item| item.height() + gap)
                .fold(0.0_f32, f32::max),
        );
    let mut free_rects = vec![PackRect {
        x: 0.0,
        y: 0.0,
        width: bin_width,
        height: total_height,
    }];
    let mut placements = Vec::with_capacity(items.len());
    let mut padded_placements = Vec::with_capacity(items.len());

    for item in items {
        let padded = padded_item_rect(item, gap);
        if padded.width > bin_width + 0.001 {
            return None;
        }

        let Some(placed) = place_maxrects_item(&mut free_rects, padded, heuristic) else {
            return None;
        };
        placements.push((
            item.id,
            egui::pos2(
                placed.x + item.width() * 0.5,
                placed.y + item.height() * 0.5,
            ),
        ));
        padded_placements.push((item.id, placed));
    }

    normalize_placements_to_origin(items, &mut placements)?;
    let (layout_width, layout_height) = placed_bounds(items, &placements)?;
    let (padded_width, padded_height) = placed_padded_bounds(&padded_placements)?;
    let padded_area = items
        .iter()
        .map(|item| (item.width() + gap) * (item.height() + gap))
        .sum::<f32>();
    let score_area = (padded_width * padded_height).max(1.0);
    let occupancy = padded_area / score_area;
    let aspect = padded_width.max(padded_height) / padded_width.min(padded_height).max(1.0);
    Some(CompactCandidate {
        right_edge_raggedness: right_edge_raggedness(&padded_placements, padded_width),
        placements,
        width: layout_width,
        height: layout_height,
        occupancy,
        aspect,
        area: score_area,
    })
}

fn place_maxrects_item(
    free_rects: &mut Vec<PackRect>,
    padded: PackRect,
    heuristic: PackHeuristic,
) -> Option<PackRect> {
    let mut best_candidate: Option<(usize, PackRect, PackScore)> = None;

    for (index, free) in free_rects.iter().copied().enumerate() {
        if padded.width > free.width + 0.001 || padded.height > free.height + 0.001 {
            continue;
        }

        let placed = PackRect {
            x: free.x,
            y: free.y,
            width: padded.width,
            height: padded.height,
        };
        let score = pack_score(free, padded, heuristic);

        match best_candidate {
            Some((_, best_rect, best_score))
                if !pack_score_better(score, placed, best_score, best_rect) => {}
            _ => best_candidate = Some((index, placed, score)),
        }
    }

    let (candidate_index, placed, _) = best_candidate?;
    split_free_rects(free_rects, candidate_index, placed);
    Some(placed)
}

#[derive(Clone, Copy)]
struct PackScore {
    primary: f32,
    secondary: f32,
}

fn pack_score(free: PackRect, padded: PackRect, heuristic: PackHeuristic) -> PackScore {
    let leftover_width = free.width - padded.width;
    let leftover_height = free.height - padded.height;
    let short_side_fit = leftover_width.min(leftover_height);
    let long_side_fit = leftover_width.max(leftover_height);
    let area_fit = free.width * free.height - padded.width * padded.height;
    match heuristic {
        PackHeuristic::BestShortSide => PackScore {
            primary: short_side_fit,
            secondary: long_side_fit,
        },
        PackHeuristic::BestLongSide => PackScore {
            primary: long_side_fit,
            secondary: short_side_fit,
        },
        PackHeuristic::BestArea => PackScore {
            primary: area_fit,
            secondary: short_side_fit,
        },
    }
}

fn pack_score_better(
    score: PackScore,
    placed: PackRect,
    best_score: PackScore,
    best_rect: PackRect,
) -> bool {
    score.primary < best_score.primary - 0.001
        || ((score.primary - best_score.primary).abs() < 0.001
            && (score.secondary < best_score.secondary - 0.001
                || ((score.secondary - best_score.secondary).abs() < 0.001
                    && (placed.y < best_rect.y - 0.001
                        || ((placed.y - best_rect.y).abs() < 0.001
                            && placed.x < best_rect.x - 0.001)))))
}

fn split_free_rects(free_rects: &mut Vec<PackRect>, candidate_index: usize, placed: PackRect) {
    let mut next = Vec::with_capacity(free_rects.len() + 4);

    for (index, free) in free_rects.iter().copied().enumerate() {
        if index == candidate_index || rects_intersect(free, placed) {
            next.extend(split_free_rect(free, placed));
        } else {
            next.push(free);
        }
    }

    prune_free_rects(&mut next);
    *free_rects = next;
}

fn split_free_rect(free: PackRect, placed: PackRect) -> Vec<PackRect> {
    if !rects_intersect(free, placed) {
        return vec![free];
    }

    let mut result = Vec::with_capacity(4);

    if placed.y > free.y + 0.001 {
        result.push(PackRect {
            x: free.x,
            y: free.y,
            width: free.width,
            height: placed.y - free.y,
        });
    }

    if placed.bottom() < free.bottom() - 0.001 {
        result.push(PackRect {
            x: free.x,
            y: placed.bottom(),
            width: free.width,
            height: free.bottom() - placed.bottom(),
        });
    }

    if placed.x > free.x + 0.001 {
        result.push(PackRect {
            x: free.x,
            y: free.y,
            width: placed.x - free.x,
            height: free.height,
        });
    }

    if placed.right() < free.right() - 0.001 {
        result.push(PackRect {
            x: placed.right(),
            y: free.y,
            width: free.right() - placed.right(),
            height: free.height,
        });
    }

    result
}

fn prune_free_rects(free_rects: &mut Vec<PackRect>) {
    free_rects.retain(|rect| rect.width > 0.001 && rect.height > 0.001);

    loop {
        let mut changed = false;

        let mut index = 0;
        while index < free_rects.len() {
            let rect = free_rects[index];
            let contained = free_rects.iter().enumerate().any(|(other_index, other)| {
                other_index != index && rect_contained_in(rect, *other)
            });
            if contained {
                free_rects.remove(index);
                changed = true;
            } else {
                index += 1;
            }
        }

        let mut left_index = 0;
        while left_index < free_rects.len() {
            let mut right_index = left_index + 1;
            while right_index < free_rects.len() {
                if let Some(merged) =
                    merge_adjacent_free_rects(free_rects[left_index], free_rects[right_index])
                {
                    free_rects[left_index] = merged;
                    free_rects.remove(right_index);
                    changed = true;
                } else {
                    right_index += 1;
                }
            }
            left_index += 1;
        }

        if !changed {
            break;
        }
    }
}

fn rects_intersect(left: PackRect, right: PackRect) -> bool {
    left.x < right.right() - 0.001
        && left.right() > right.x + 0.001
        && left.y < right.bottom() - 0.001
        && left.bottom() > right.y + 0.001
}

fn rect_contained_in(inner: PackRect, outer: PackRect) -> bool {
    inner.x >= outer.x - 0.001
        && inner.y >= outer.y - 0.001
        && inner.right() <= outer.right() + 0.001
        && inner.bottom() <= outer.bottom() + 0.001
}

fn merge_adjacent_free_rects(left: PackRect, right: PackRect) -> Option<PackRect> {
    if (left.x - right.x).abs() < 0.001
        && (left.width - right.width).abs() < 0.001
        && (left.bottom() - right.y).abs() < 0.001
    {
        return Some(PackRect {
            x: left.x,
            y: left.y,
            width: left.width,
            height: left.height + right.height,
        });
    }

    if (left.x - right.x).abs() < 0.001
        && (left.width - right.width).abs() < 0.001
        && (right.bottom() - left.y).abs() < 0.001
    {
        return Some(PackRect {
            x: right.x,
            y: right.y,
            width: right.width,
            height: left.height + right.height,
        });
    }

    if (left.y - right.y).abs() < 0.001
        && (left.height - right.height).abs() < 0.001
        && (left.right() - right.x).abs() < 0.001
    {
        return Some(PackRect {
            x: left.x,
            y: left.y,
            width: left.width + right.width,
            height: left.height,
        });
    }

    if (left.y - right.y).abs() < 0.001
        && (left.height - right.height).abs() < 0.001
        && (right.right() - left.x).abs() < 0.001
    {
        return Some(PackRect {
            x: right.x,
            y: right.y,
            width: left.width + right.width,
            height: right.height,
        });
    }

    None
}

fn padded_item_rect(item: &SelectedLayoutItem, gap: f32) -> PackRect {
    PackRect {
        x: 0.0,
        y: 0.0,
        width: item.width() + gap,
        height: item.height() + gap,
    }
}

fn placed_bounds(
    items: &[SelectedLayoutItem],
    placements: &[(i64, egui::Pos2)],
) -> Option<(f32, f32)> {
    let mut min_x = f32::INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut max_y = f32::NEG_INFINITY;

    for (placement_id, center) in placements {
        let item = items.iter().find(|item| item.id == *placement_id)?;
        min_x = min_x.min(center.x - item.width() * 0.5);
        min_y = min_y.min(center.y - item.height() * 0.5);
        max_x = max_x.max(center.x + item.width() * 0.5);
        max_y = max_y.max(center.y + item.height() * 0.5);
    }

    Some(((max_x - min_x).max(0.0), (max_y - min_y).max(0.0)))
}

fn normalize_placements_to_origin(
    items: &[SelectedLayoutItem],
    placements: &mut [(i64, egui::Pos2)],
) -> Option<()> {
    let mut min_x = f32::INFINITY;
    let mut min_y = f32::INFINITY;

    for (placement_id, center) in placements.iter() {
        let item = items.iter().find(|item| item.id == *placement_id)?;
        min_x = min_x.min(center.x - item.width() * 0.5);
        min_y = min_y.min(center.y - item.height() * 0.5);
    }

    if !min_x.is_finite() || !min_y.is_finite() {
        return None;
    }

    let offset = egui::vec2(min_x, min_y);
    for (_, center) in placements {
        *center -= offset;
    }

    Some(())
}

fn placed_padded_bounds(placements: &[(i64, PackRect)]) -> Option<(f32, f32)> {
    let mut max_x = f32::NEG_INFINITY;
    let mut max_y = f32::NEG_INFINITY;

    for (_, rect) in placements {
        max_x = max_x.max(rect.right());
        max_y = max_y.max(rect.bottom());
    }

    max_x
        .is_finite()
        .then_some((max_x.max(0.0), max_y.max(0.0)))
}

fn right_edge_raggedness(placements: &[(i64, PackRect)], layout_width: f32) -> f32 {
    if placements.is_empty() {
        return 0.0;
    }

    placements
        .iter()
        .map(|(_, rect)| (layout_width - rect.right()).max(0.0))
        .sum::<f32>()
        / placements.len() as f32
}

fn compact_candidate_better(candidate: &CompactCandidate, best: &CompactCandidate) -> bool {
    const OCCUPANCY_DECIDES_THRESHOLD: f32 = 0.03;
    if candidate.occupancy > best.occupancy + OCCUPANCY_DECIDES_THRESHOLD {
        return true;
    }
    if best.occupancy > candidate.occupancy + OCCUPANCY_DECIDES_THRESHOLD {
        return false;
    }

    candidate.aspect < best.aspect - 0.001
        || ((candidate.aspect - best.aspect).abs() < 0.001
            && (candidate.occupancy > best.occupancy + 0.001
                || ((candidate.occupancy - best.occupancy).abs() < 0.001
                    && (candidate.right_edge_raggedness < best.right_edge_raggedness - 0.001
                        || ((candidate.right_edge_raggedness - best.right_edge_raggedness)
                            .abs()
                            < 0.001
                            && candidate.area < best.area - 0.001)))))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::{PlacementContent, PlacementLifecycle, PlacementState};

    fn placement(id: PlacementId, center: egui::Pos2) -> Placement {
        Placement {
            state: PlacementState::new(id, None, center, 2.0, 0.0, 0, false),
            asset: None,
            content: PlacementContent::Loading {
                source: crate::data::ContentResolutionSource::Unknown {
                    description: "test".to_owned(),
                },
                status_message: None,
            },
            lifecycle: PlacementLifecycle::Provisional,
            visual_size: egui::vec2(20.0, 10.0),
        }
    }

    #[test]
    fn compact_input_is_ephemeral_geometry_for_a_placement() {
        let record = placement(7, egui::pos2(50.0, 30.0));

        let input = record.compact_input();

        assert_eq!(input.placement_id, 7);
        assert_eq!(input.bounds.center(), egui::pos2(50.0, 30.0));
        assert_eq!(input.bounds.size(), egui::vec2(40.0, 20.0));
    }

    #[test]
    fn compact_output_applies_only_to_matching_placement() {
        let mut record = placement(7, egui::pos2(50.0, 30.0));

        record.apply_compact_output(CompactOutput {
            placement_id: 8,
            center: egui::pos2(10.0, 10.0),
        });
        assert_eq!(record.state.geometry.center, egui::pos2(50.0, 30.0));

        record.apply_compact_output(CompactOutput {
            placement_id: 7,
            center: egui::pos2(10.0, 10.0),
        });
        assert_eq!(record.state.geometry.center, egui::pos2(10.0, 10.0));
    }
}

#[derive(Clone, Copy, Debug)]
struct PackRect {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

impl PackRect {
    fn right(self) -> f32 {
        self.x + self.width
    }

    fn bottom(self) -> f32 {
        self.y + self.height
    }
}
