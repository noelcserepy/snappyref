//! Ordered Group geometry. Document callers own membership, history, and persistence.
use std::collections::HashSet;

use eframe::egui::{Pos2, vec2};

use crate::data::{Group, GroupFlow, PlacementId, RectGeometry};

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct GroupLayout {
    pub container: RectGeometry,
    pub members: Vec<(PlacementId, RectGeometry)>,
    pub spacing: f32,
    pub padding: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GroupLayoutError {
    InvalidGeometry,
    InvalidMembership,
}

/// Reflows fixed-size members from the container's top-left. Only incoming
/// members may shrink to fit. Existing oversized members deliberately overflow.
pub(crate) fn layout_group(
    group: &Group,
    members: &[(PlacementId, RectGeometry)],
    incoming: &[PlacementId],
) -> Result<GroupLayout, GroupLayoutError> {
    validate(group, members)?;
    let incoming: HashSet<_> = incoming.iter().copied().collect();
    if incoming.iter().any(|id| !group.members.contains(id)) {
        return Err(GroupLayoutError::InvalidMembership);
    }
    let bounds = group.container.unrotated_bounds();
    let width = bounds.width() - 2.0 * group.padding;
    let origin = bounds.min + vec2(group.padding, group.padding);
    let mut result = GroupLayout {
        container: group.container,
        members: Vec::with_capacity(members.len()),
        spacing: group.spacing,
        padding: group.padding,
    };
    let (mut x, mut y, mut row_height) = (0.0_f32, 0.0_f32, 0.0_f32);
    let mut row_has_member = false;
    let mut content_height = 0.0_f32;
    for &(id, mut geometry) in members {
        let mut size = geometry.visual_bounds().size();
        if incoming.contains(&id) && size.x > width {
            resize_member(&mut geometry, width / size.x)?;
            size = geometry.visual_bounds().size();
        }
        if group.flow == GroupFlow::Wrap && row_has_member && x + size.x > width {
            x = 0.0;
            y += row_height + group.spacing;
            row_height = 0.0;
        }
        geometry.center = origin + vec2(x, y) + size * 0.5;
        content_height = content_height.max(y + size.y);
        row_height = row_height.max(size.y);
        match group.flow {
            GroupFlow::Row | GroupFlow::Wrap => x += size.x + group.spacing,
            GroupFlow::Column => y += size.y + group.spacing,
        }
        row_has_member = true;
        if !valid_geometry(geometry) {
            return Err(GroupLayoutError::InvalidGeometry);
        }
        result.members.push((id, geometry));
    }
    // Empty Groups keep their existing usable height and all configuration.
    if !members.is_empty() {
        let height = content_height + 2.0 * group.padding;
        result.container.size.y = height / result.container.scale;
        result.container.center = Pos2::new(bounds.center().x, bounds.min.y + height * 0.5);
    }
    if !valid_geometry(result.container) {
        return Err(GroupLayoutError::InvalidGeometry);
    }
    Ok(result)
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
        spacing: group.spacing * factor,
        padding: group.padding * factor,
    };
    if !valid_geometry(result.container)
        || result
            .members
            .iter()
            .any(|&(_, geometry)| !valid_geometry(geometry))
        || !result.spacing.is_finite()
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
        && geometry.size.x > 0.0
        && geometry.size.y > 0.0
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
        || !group.spacing.is_finite()
        || group.spacing < 0.0
        || !group.padding.is_finite()
        || group.padding < 0.0
        || group.container.scaled_size().x - 2.0 * group.padding <= 0.0
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
            spacing: 5.0,
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
    fn incoming_fits_but_container_resize_preserves_existing_sizes() {
        let (mut group, members) = fixture(&[(200.0, 80.0), (200.0, 100.0)]);
        let result = layout_group(&group, &members, &[1]).unwrap();
        assert_eq!(result.members[0].1.size, members[0].1.size);
        assert_eq!(result.members[1].1.scaled_size(), vec2(100.0, 50.0));
        group.container.size.x = 70.0;
        let narrow = layout_group(&group, &result.members, &[]).unwrap();
        assert_eq!(narrow.members[1].1.scaled_size(), vec2(100.0, 50.0));
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
            assert_eq!(scaled.spacing, 10.0);
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
        for spacing in [-1.0, f32::NAN, f32::INFINITY] {
            group.spacing = spacing;
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
        group.spacing = 0.0;
        group.padding = 60.0;
        assert_eq!(
            layout_group(&group, &members, &[]),
            Err(GroupLayoutError::InvalidGeometry)
        );
    }
}
