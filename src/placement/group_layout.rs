//! Ordered Group geometry. Document callers own membership, history, and persistence.
use std::collections::HashSet;

#[cfg(test)]
use eframe::egui::{Pos2, vec2};

#[cfg(test)]
use crate::data::GroupFlow;
use crate::data::{Group, PlacementId, RectGeometry};

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct GroupLayout {
    pub container: RectGeometry,
    pub members: Vec<(PlacementId, RectGeometry)>,
    pub gap: f32,
    pub padding: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GroupLayoutError {
    InvalidGeometry,
    InvalidMembership,
}

/// Reflows fixed-size members from the container's top-left. Only incoming
/// members may shrink to fit. Existing oversized members deliberately overflow.
#[cfg(test)]
fn layout_group(
    group: &Group,
    members: &[(PlacementId, RectGeometry)],
    incoming: &[PlacementId],
) -> Result<GroupLayout, GroupLayoutError> {
    validate(group, members)?;
    let incoming: HashSet<_> = incoming.iter().copied().collect();
    if incoming.iter().any(|id| !group.members.contains(id)) {
        return Err(GroupLayoutError::InvalidMembership);
    }
    let pixels = super::group_pixels::Pixels {
        rects: members.iter().copied().collect(),
        ..Default::default()
    };
    let mut board = crate::grouping::Board::default();
    board
        .groups
        .insert(group.id, super::group_pixels::config(group));
    for &(id, geometry) in members {
        let mode = group.layout.members.get(&id).copied().unwrap_or_default();
        let mut size = geometry.visual_bounds().size();
        if incoming.contains(&id)
            && !mode.absolute
            && !mode.fill[0]
            && !mode.fill[1]
            && group.layout.fixed[0]
            && group.flow != GroupFlow::Free
            && size.x > board.groups[&group.id].capacity(0)
        {
            size *= board.groups[&group.id].capacity(0) / size.x;
        }
        board.members.insert(
            id,
            crate::grouping::Member {
                kind: "Image".into(),
                size: size.into(),
                fill: (0..2).filter(|&a| mode.fill[a]).collect(),
                absolute: mode.absolute,
            },
        );
    }
    let layout = pixels.layout(&board, group.id);
    let bounds = group.container.unrotated_bounds();
    let mut origin = bounds.min;
    let mut size = bounds.size();
    if !members.is_empty() {
        for a in 0..2 {
            if !group.layout.fixed[a] && !board.flowing(group.id).is_empty() {
                size[a] = layout.content[a].max(1.0) + group.padding * 2.0;
            }
        }
        if group.flow == GroupFlow::Free
            && let Some(member_bounds) = board
                .flowing(group.id)
                .iter()
                .map(|m| pixels.rects[m].visual_bounds())
                .reduce(|a, b| a.union(b))
        {
            for a in 0..2 {
                if !group.layout.fixed[a] {
                    origin[a] = member_bounds.min[a] - group.padding;
                }
            }
        }
    }
    let container = RectGeometry::from_bounds(
        eframe::egui::Rect::from_min_size(origin, size),
        group.container.z_index,
    );
    let mut output = Vec::new();
    for &(id, mut geometry) in members {
        let mode = group.layout.members.get(&id).copied().unwrap_or_default();
        if !mode.absolute && group.flow != GroupFlow::Free {
            let desired = layout.sizes[&id];
            let current = geometry.visual_bounds().size();
            let factor = if current.x > 0.0 {
                desired.x / current.x
            } else {
                1.0
            };
            if factor >= 0.0 {
                resize_member(&mut geometry, factor)?;
            }
            geometry.center =
                origin + vec2(group.padding, group.padding) + layout.offsets[&id] + desired * 0.5;
        }
        output.push((id, geometry));
    }
    Ok(GroupLayout {
        container,
        members: output,
        gap: group.gap,
        padding: group.padding,
    })
}

/// Scales around the container's top-left without reflowing its ordered layout.
pub(crate) fn scale_group(
    group: &Group,
    members: &[(PlacementId, RectGeometry)],
    factor: f32,
) -> Result<GroupLayout, GroupLayoutError> {
    validate(group, members)?;
    if !factor.is_finite() || factor <= 0.0 {
        return Err(GroupLayoutError::InvalidGeometry);
    }
    let anchor = group.container.unrotated_bounds().min;
    let transform = |mut geometry: RectGeometry| {
        geometry.center = anchor + (geometry.center - anchor) * factor;
        resize_member(&mut geometry, factor)?;
        Ok(geometry)
    };
    let result = GroupLayout {
        container: transform(group.container)?,
        members: members
            .iter()
            .map(|&(id, geometry)| transform(geometry).map(|geometry| (id, geometry)))
            .collect::<Result<_, GroupLayoutError>>()?,
        gap: group.gap * factor,
        padding: group.padding * factor,
    };
    if !valid_geometry(result.container)
        || result
            .members
            .iter()
            .any(|&(_, geometry)| !valid_geometry(geometry))
        || !result.gap.is_finite()
        || !result.padding.is_finite()
    {
        return Err(GroupLayoutError::InvalidGeometry);
    }
    Ok(result)
}

fn resize_member(geometry: &mut RectGeometry, factor: f32) -> Result<(), GroupLayoutError> {
    // RectGeometry clamps base dimensions to one and scale to .01. Normalize
    // explicitly so fitting tiny references still preserves their aspect ratio.
    let displayed = geometry.scaled_size() * factor;
    if displayed.min_elem() == 0.0 {
        geometry.size = displayed;
        geometry.scale = 1.0;
        return Ok(());
    }
    if !displayed.is_finite() || displayed.min_elem() < 0.01 {
        return Err(GroupLayoutError::InvalidGeometry);
    }
    let scale = displayed.min_elem().min(1.0);
    geometry.size = displayed / scale;
    geometry.scale = scale;
    Ok(())
}

fn valid_geometry(geometry: RectGeometry) -> bool {
    geometry.center.is_finite()
        && geometry.size.is_finite()
        && geometry.size.x >= 0.0
        && geometry.size.y >= 0.0
        && geometry.scale.is_finite()
        && geometry.scale >= 0.01
        && geometry.rotation.is_finite()
        && geometry.visual_bounds().is_finite()
}

fn validate(
    group: &Group,
    members: &[(PlacementId, RectGeometry)],
) -> Result<(), GroupLayoutError> {
    if !valid_geometry(group.container)
        || group.container.rotation != 0.0
        || !group.gap.is_finite()
        || !group.padding.is_finite()
        || group.padding < 0.0
        || group.container.scaled_size().min_elem() <= 0.0
    {
        return Err(GroupLayoutError::InvalidGeometry);
    }
    let mut seen = HashSet::new();
    if group.members.len() != members.len()
        || group
            .members
            .iter()
            .zip(members)
            .any(|(&expected, &(id, _))| expected != id || !seen.insert(id))
    {
        return Err(GroupLayoutError::InvalidMembership);
    }
    if members
        .iter()
        .any(|&(_, geometry)| !valid_geometry(geometry))
    {
        return Err(GroupLayoutError::InvalidGeometry);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::Rect;

    fn fixture(sizes: &[(f32, f32)]) -> (Group, Vec<(PlacementId, RectGeometry)>) {
        let members: Vec<_> = sizes
            .iter()
            .enumerate()
            .map(|(i, &(w, h))| {
                (
                    i as i64,
                    RectGeometry::new(Pos2::ZERO, vec2(w, h), 1.0, 0.0, 0),
                )
            })
            .collect();
        let group = Group {
            id: 1,
            name: "Group 1".into(),
            members: members.iter().map(|m| m.0).collect(),
            container: RectGeometry::from_bounds(
                Rect::from_min_size(Pos2::new(10.0, 20.0), vec2(120.0, 100.0)),
                0,
            ),
            flow: GroupFlow::default(),
            layout: crate::data::GroupLayoutSettings {
                row_gap: 5.0,
                ..Default::default()
            },
            gap: 5.0,
            padding: 10.0,
        };
        (group, members)
    }

    #[test]
    fn append_keeps_existing_positions_even_with_a_taller_last_row_member() {
        let (mut group, members) = fixture(&[(40.0, 20.0), (40.0, 70.0), (40.0, 30.0)]);
        group.members.truncate(1);
        let before = layout_group(&group, &members[..1], &[]).unwrap();
        group.container = before.container;
        group.members = vec![0, 1, 2];
        let after = layout_group(&group, &members, &[1, 2]).unwrap();
        assert_eq!(before.members[0], after.members[0]);
        assert_eq!(
            after.members[1].1.visual_bounds().min,
            Pos2::new(65.0, 30.0)
        );
        assert_eq!(
            after.members[2].1.visual_bounds().min,
            Pos2::new(20.0, 105.0)
        );
        group.container = after.container;
        assert_eq!(layout_group(&group, &after.members, &[]).unwrap(), after);
    }

    #[test]
    fn flows_preserve_order_and_uniform_scale_preserves_relative_geometry() {
        let (mut group, members) = fixture(&[(60.0, 20.0), (30.0, 40.0)]);
        for flow in [GroupFlow::Row, GroupFlow::Column] {
            group.flow = flow;
            let layout = layout_group(&group, &members, &[]).unwrap();
            let first = layout.members[0].1.visual_bounds();
            let second = layout.members[1].1.visual_bounds();
            if flow == GroupFlow::Row {
                assert_eq!(second.min.x - first.max.x, 5.0);
            } else {
                assert_eq!(second.min.y - first.max.y, 5.0);
            }
            group.container = layout.container;
            let scaled = scale_group(&group, &layout.members, 2.0).unwrap();
            assert_eq!(scaled.gap, 10.0);
            assert_eq!(scaled.padding, 20.0);
            assert_eq!(
                scaled.container.scaled_size(),
                layout.container.scaled_size() * 2.0
            );
            assert_eq!(
                scaled.members[1].1.center - scaled.members[0].1.center,
                (layout.members[1].1.center - layout.members[0].1.center) * 2.0
            );
        }
        let (mut group, members) = fixture(&[(100.0, 100.0), (100.0, 100.0)]);
        group.container.size.y = 40.0;
        group.layout.fixed = [true, true];
        group.layout.auto = "Between".into();
        group.layout.members = (0..2)
            .map(|id| {
                (
                    id,
                    crate::data::MemberLayout {
                        fill: [true, true],
                        absolute: false,
                    },
                )
            })
            .collect();
        let fitted = layout_group(&group, &members, &[]).unwrap();
        assert_eq!(fitted.members[0].1.scaled_size(), vec2(20.0, 20.0));
        assert_eq!(
            fitted.members[1].1.center.x - fitted.members[0].1.center.x,
            50.0
        );
    }

    #[test]
    fn removal_closes_gap_and_rotated_members_do_not_overlap() {
        let (mut group, mut members) = fixture(&[(60.0, 20.0), (60.0, 20.0), (30.0, 20.0)]);
        members[0].1.rotation = std::f32::consts::FRAC_PI_2;
        let before = layout_group(&group, &members, &[]).unwrap();
        assert!(
            before.members[0].1.visual_bounds().max.x <= before.members[1].1.visual_bounds().min.x
        );
        group.members = vec![0, 2];
        let after = layout_group(&group, &[members[0], members[2]], &[]).unwrap();
        assert!(after.container.scaled_size().y < before.container.scaled_size().y);
        assert_eq!(
            after.members[1].1.visual_bounds().min.y,
            after.members[0].1.visual_bounds().min.y
        );
    }

    #[test]
    fn empty_is_valid_and_invalid_inputs_are_rejected() {
        let (mut group, members) = fixture(&[]);
        assert_eq!(
            layout_group(&group, &members, &[]).unwrap().container,
            group.container
        );
        for gap in [f32::NAN, f32::INFINITY] {
            group.gap = gap;
            assert_eq!(
                layout_group(&group, &members, &[]),
                Err(GroupLayoutError::InvalidGeometry)
            );
        }
        let (tiny, members) = fixture(&[(200.0, 1.0)]);
        assert_eq!(
            scale_group(&tiny, &members, 0.001),
            Err(GroupLayoutError::InvalidGeometry)
        );
        group.gap = 0.0;
        group.padding = -1.0;
        assert_eq!(
            layout_group(&group, &members, &[]),
            Err(GroupLayoutError::InvalidGeometry)
        );
    }
}
