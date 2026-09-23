use super::*;
use crate::data::{Asset, Placement, PlacementState};
use crate::grouping::{AxisMode, Control};

fn image_canvas(rotation: f32) -> CanvasState {
    let mut canvas = CanvasState::default();
    canvas.document.placements.push(Placement::loaded(
        PlacementState {
            id: 1,
            geometry: RectGeometry::new(
                egui::Pos2::ZERO,
                egui::vec2(240.0, 120.0),
                0.5,
                rotation,
                0,
            ),
            archived: false,
        },
        Asset {
            id: 11,
            original_path: "image.png".into(),
            filename: "image.png".into(),
            mime_type: "image/png".into(),
            width: 240,
            height: 120,
        },
    ));
    canvas.create_named_group(7, "Group".into(), &[1]).unwrap();
    canvas
}

#[test]
fn grouping_padding_exhausts_fixed_capacity() {
    let mut canvas = image_canvas(0.0);
    canvas
        .group_edit(|s| s.setAxisMode(7, 0, AxisMode::Fixed(100.0)))
        .unwrap();
    let fixed_member = canvas.group_member_geometry(1).unwrap().scaled_size();
    for padding in [50.0, 70.0, 20.0] {
        canvas
            .group_edit(|s| s.useDock(Control::Padding(padding)))
            .unwrap();
        assert_eq!(canvas.groups()[0].container.scaled_size().x, 100.0);
        assert_eq!(
            canvas.grouping_state().board.groups[&7].capacity(0),
            (100.0 - 2.0 * padding).max(0.0)
        );
        assert_eq!(
            canvas.group_member_geometry(1).unwrap().scaled_size(),
            fixed_member
        );
    }
    canvas
        .group_edit(|s| s.setMemberMode(7, 1, 0, true))
        .unwrap();
    canvas
        .group_edit(|s| s.useDock(Control::Padding(70.0)))
        .unwrap();
    assert_eq!(canvas.groups()[0].container.scaled_size().x, 100.0);
    assert_eq!(
        canvas.group_member_geometry(1).unwrap().scaled_size(),
        Vec2::ZERO
    );
}

#[test]
fn grouping_zero_fill_recovers_asset_ratio() {
    for rotation in [0.0, 0.37, std::f32::consts::FRAC_PI_2] {
        let mut canvas = image_canvas(rotation);
        canvas
            .group_edit(|s| s.setMemberMode(7, 1, 0, true))
            .unwrap();
        canvas
            .group_edit(|s| s.useDock(Control::Padding(200.0)))
            .unwrap();
        assert_eq!(
            canvas.group_member_geometry(1).unwrap().scaled_size(),
            Vec2::ZERO
        );
        canvas
            .group_edit(|s| s.useDock(Control::Padding(10.0)))
            .unwrap();
        let geometry = canvas.group_member_geometry(1).unwrap();
        assert!(geometry.size.is_finite());
        assert!(geometry.size.min_elem() > 0.0);
        assert!((geometry.size.x / geometry.size.y - 2.0).abs() < 0.0001);
        let capacity = canvas.grouping_state().board.groups[&7].capacity(0);
        assert!((geometry.visual_bounds().width() - capacity).abs() < 0.0001);
        assert_eq!(geometry.rotation, rotation);
        assert_eq!(canvas.document.placements[0].asset_id(), Some(11));
    }
}

#[test]
fn zero_fill_recovers_when_flow_or_absolute_resumes() {
    for absolute in [false, true] {
        let mut canvas = image_canvas(0.37);
        canvas
            .group_edit(|s| s.setMemberMode(7, 1, 0, true))
            .unwrap();
        canvas
            .group_edit(|s| s.useDock(Control::Padding(200.)))
            .unwrap();
        if absolute {
            canvas.group_edit(|s| s.toggleAbsolute(7, 1, 0)).unwrap();
        } else {
            canvas.group_edit(|s| s.setFlow(7, "Free")).unwrap();
        }
        canvas
            .group_edit(|s| s.useDock(Control::Padding(10.)))
            .unwrap();
        assert_eq!(canvas.group_member_geometry(1).unwrap().size, Vec2::ZERO);
        if absolute {
            canvas.group_edit(|s| s.toggleAbsolute(7, 1, 0)).unwrap();
        } else {
            canvas.group_edit(|s| s.setFlow(7, "Row")).unwrap();
        }
        let rect = canvas.group_member_geometry(1).unwrap();
        assert!(
            rect.size.min_elem() > 0.,
            "absolute={absolute}, rect={rect:?}, group={:?}",
            canvas.groups()[0]
        );
        assert!((rect.size.x / rect.size.y - 2.).abs() < 0.0001);
    }
}

#[test]
fn zero_fill_to_fixed_or_hug_freezes_complete_image_geometry() {
    for hug in [false, true] {
        for rotation in [0.0, 0.37] {
            let mut canvas = image_canvas(rotation);
            canvas.group_edit(|s| s.setFlow(7, "Row")).unwrap();
            canvas
                .group_edit(|s| s.setMemberMode(7, 1, 0, true))
                .unwrap();
            canvas
                .group_edit(|s| s.useDock(Control::Padding(200.)))
                .unwrap();
            let before = canvas.group_member_geometry(1).unwrap();
            assert_eq!(before.size, Vec2::ZERO);
            assert_eq!(canvas.grouping_state().board.members[&1].size, [0., 0.]);
            if hug {
                canvas
                    .group_edit(|s| s.setAxisMode(7, 0, AxisMode::Hug))
                    .unwrap();
            } else {
                canvas
                    .group_edit(|s| s.setMemberMode(7, 1, 0, false))
                    .unwrap();
            }
            let after = canvas.group_member_geometry(1).unwrap();
            assert_eq!(after.size, before.size);
            assert_eq!(after.scale, before.scale);
            assert_eq!(after.rotation, before.rotation);
            assert_eq!(canvas.groups()[0].container.size.y, 401.);
            assert!(canvas.groups()[0].layout.members.is_empty());
            canvas
                .group_edit(|s| s.useDock(Control::Padding(10.)))
                .unwrap();
            assert_eq!(canvas.group_member_geometry(1).unwrap().size, Vec2::ZERO);
            canvas
                .group_edit(|s| s.setAxisMode(7, 0, AxisMode::Fixed(100.)))
                .unwrap();
            canvas
                .group_edit(|s| s.setMemberMode(7, 1, 0, true))
                .unwrap();
            let recovered = canvas.group_member_geometry(1).unwrap();
            assert!(recovered.size.min_elem() > 0.);
            assert!((recovered.size.x / recovered.size.y - 2.).abs() < 0.0001);
        }
    }
}
