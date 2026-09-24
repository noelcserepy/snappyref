//! Uniform Group scaling. Document callers own membership, history, and persistence.
use std::collections::HashSet;

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
    use crate::data::GroupFlow;
    use eframe::egui::{Pos2, Rect, vec2};

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
    fn uniform_scale_preserves_relative_geometry() {
        let (group, mut members) = fixture(&[(60.0, 20.0), (30.0, 40.0)]);
        members[0].1.center = Pos2::new(50.0, 40.0);
        members[1].1.center = Pos2::new(110.0, 50.0);
        let scaled = scale_group(&group, &members, 2.0).unwrap();
        assert_eq!(scaled.gap, 10.0);
        assert_eq!(scaled.padding, 20.0);
        assert_eq!(
            scaled.container.scaled_size(),
            group.container.scaled_size() * 2.0
        );
        assert_eq!(
            scaled.members[1].1.center - scaled.members[0].1.center,
            (members[1].1.center - members[0].1.center) * 2.0
        );
    }

    #[test]
    fn invalid_inputs_are_rejected() {
        let (mut group, members) = fixture(&[]);
        assert_eq!(
            scale_group(&group, &members, 1.0).unwrap().container,
            group.container
        );
        for gap in [f32::NAN, f32::INFINITY] {
            group.gap = gap;
            assert_eq!(
                scale_group(&group, &members, 1.0),
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
            scale_group(&group, &members[..0], 1.0),
            Err(GroupLayoutError::InvalidGeometry)
        );
    }
}
