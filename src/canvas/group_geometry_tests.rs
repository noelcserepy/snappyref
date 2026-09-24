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

fn images_canvas(sizes: &[(f32, f32)]) -> CanvasState {
    let mut canvas = CanvasState::default();
    for (id, &(w, h)) in (1..).zip(sizes) {
        canvas.document.placements.push(Placement::loaded(
            PlacementState {
                id,
                geometry: RectGeometry::new(egui::Pos2::ZERO, egui::vec2(w, h), 1.0, 0.0, 0),
                archived: false,
            },
            Asset {
                id: 10 + id,
                original_path: "image.png".into(),
                filename: "image.png".into(),
                mime_type: "image/png".into(),
                width: w as u32,
                height: h as u32,
            },
        ));
    }
    let ids = (1..=sizes.len() as i64).collect::<Vec<_>>();
    canvas.create_named_group(7, "Group".into(), &ids).unwrap();
    canvas
}

#[test]
fn absolute_members_do_not_decide_insertion_slot() {
    let mut canvas = images_canvas(&[(120.0, 60.0); 3]);
    canvas.group_edit(|s| s.setFlow(7, "Row")).unwrap();
    canvas.group_edit(|s| s.toggleAbsolute(7, 1, 0)).unwrap();
    let [b, c] = [2, 3].map(|id| canvas.group_member_geometry(id).unwrap().center);
    let mut a = canvas.group_member_geometry(1).unwrap();
    a.center.x = c.x + 500.0;
    canvas.set_group_member_geometry(1, a);
    let between = egui::pos2((b.x + c.x) / 2.0, b.y);
    assert_eq!(canvas.group_insertion_slot(7, 4, between), 2);
}

fn assert_clip_keeps_sizes(canvas: &mut CanvasState) {
    let sizes = |canvas: &CanvasState| {
        [1, 2].map(|id| canvas.group_member_geometry(id).unwrap().scaled_size())
    };
    let before = sizes(canvas);
    for _ in 0..2 {
        canvas
            .group_edit(|s| {
                s.selectGroup(7);
                s.useDock(Control::Clip)
            })
            .unwrap();
        assert_eq!(sizes(canvas), before);
    }
}

#[test]
fn cross_fill_image_width_follows_its_row_not_the_last_render() {
    let mut wrap = images_canvas(&[(80.0, 100.0), (60.0, 60.0)]);
    wrap.group_edit(|s| {
        s.setAxisMode(7, 0, AxisMode::Fixed(170.0));
        s.setAxisMode(7, 1, AxisMode::Fixed(120.0));
        s.selectGroup(7);
        s.useDock(Control::Padding(10.0));
        s.useDock(Control::Gap(10.0));
        s.useDock(Control::RowGap(10.0));
        s.setMemberMode(7, 2, 1, true)
    })
    .unwrap();
    let filled = wrap.group_member_geometry(2).unwrap().scaled_size();
    assert_eq!(filled, egui::vec2(100.0, 100.0));
    assert_clip_keeps_sizes(&mut wrap);

    let mut row = images_canvas(&[(100.0, 100.0), (100.0, 100.0)]);
    row.group_edit(|s| {
        s.setFlow(7, "Row");
        s.setAxisMode(7, 0, AxisMode::Fixed(300.0));
        s.setAxisMode(7, 1, AxisMode::Fixed(100.0));
        s.selectGroup(7);
        s.useDock(Control::Padding(0.0));
        s.useDock(Control::Gap(0.0));
        s.setMemberMode(7, 1, 1, true);
        s.setMemberMode(7, 2, 0, true);
        s.setAxisMode(7, 1, AxisMode::Fixed(200.0))
    })
    .unwrap();
    let size = |id| row.group_member_geometry(id).unwrap().scaled_size();
    assert_eq!(
        [size(1), size(2)],
        [egui::vec2(200.0, 200.0), egui::vec2(100.0, 100.0)]
    );
    assert_clip_keeps_sizes(&mut row);
}

#[test]
fn batch_transfer_freezes_every_member_at_its_source_size() {
    for create in [true, false] {
        let mut canvas = images_canvas(&[(200.0, 100.0), (200.0, 100.0)]);
        canvas
            .group_edit(|s| {
                s.setFlow(7, "Row");
                s.setAxisMode(7, 0, AxisMode::Fixed(220.0));
                s.selectGroup(7);
                s.useDock(Control::Padding(10.0));
                s.useDock(Control::Gap(10.0));
                s.setMemberMode(7, 1, 0, true);
                s.setMemberMode(7, 2, 0, true)
            })
            .unwrap();
        let size = |canvas: &CanvasState| {
            [1, 2].map(|id| canvas.group_member_geometry(id).unwrap().scaled_size())
        };
        let before = size(&canvas);
        assert_eq!(before, [egui::vec2(95.0, 47.5); 2]);
        if create {
            canvas.create_named_group(8, "B".into(), &[1, 2]).unwrap();
        } else {
            canvas.document.placements.push(Placement::loaded(
                PlacementState {
                    id: 3,
                    geometry: RectGeometry::new(
                        egui::pos2(0.0, 500.0),
                        egui::vec2(50.0, 50.0),
                        1.0,
                        0.0,
                        0,
                    ),
                    archived: false,
                },
                Asset {
                    id: 13,
                    original_path: "image.png".into(),
                    filename: "image.png".into(),
                    mime_type: "image/png".into(),
                    width: 50,
                    height: 50,
                },
            ));
            canvas.create_named_group(8, "B".into(), &[3]).unwrap();
            canvas
                .group_edit(|s| {
                    s.setFlow(8, "Row");
                    s.setAxisMode(8, 0, AxisMode::Hug)
                })
                .unwrap();
            canvas.add_members_to_group(8, &[1, 2]).unwrap();
        }
        assert_eq!(size(&canvas), before, "create={create}");
    }
}

#[test]
fn filled_textbox_renders_its_allocated_size() {
    let mut canvas = CanvasState::default();
    canvas.add_annotation(crate::data::CanvasAnnotation::new(
        1,
        crate::data::CanvasAnnotationKind::Textbox,
        egui::pos2(0.0, 0.0),
        egui::pos2(240.0, 56.0),
        String::new(),
        Default::default(),
        Default::default(),
        Default::default(),
        false,
        false,
    ));
    canvas.create_named_group(7, "Group".into(), &[1]).unwrap();
    canvas
        .group_edit(|s| {
            s.setFlow(7, "Row");
            s.setAxisMode(7, 0, AxisMode::Fixed(120.0));
            s.selectGroup(7);
            s.useDock(Control::Padding(10.0));
            s.setMemberMode(7, 1, 0, true)
        })
        .unwrap();
    for (padding, width) in [(10.0, 100.0), (60.0, 0.0)] {
        canvas
            .group_edit(|s| {
                s.selectGroup(7);
                s.useDock(Control::Padding(padding))
            })
            .unwrap();
        let textbox = canvas.annotation_by_id(1).unwrap();
        assert_eq!(
            crate::canvas::interaction::textbox_world_rect(textbox).width(),
            width
        );
    }
}

#[test]
fn member_editing_reaches_visible_overflow_only() {
    use crate::canvas::interaction::{hit_test_canvas_item, hit_test_member};
    let mut canvas = images_canvas(&[(150.0, 100.0)]);
    canvas
        .group_edit(|s| s.setAxisMode(7, 0, AxisMode::Fixed(80.0)))
        .unwrap();
    let viewport = egui::Rect::from_center_size(egui::Pos2::ZERO, egui::vec2(2000.0, 2000.0));
    let container = canvas.groups()[0].container.unrotated_bounds();
    let image = canvas.group_member_geometry(1).unwrap().visual_bounds();
    let world = egui::pos2(container.max.x + 20.0, image.center().y);
    assert!(image.contains(world));
    let pointer = canvas.world_to_screen(world, viewport);
    for clip in [false, true] {
        assert_eq!(hit_test_canvas_item(&canvas, viewport, pointer), None);
        assert_eq!(
            hit_test_member(&canvas, viewport, pointer),
            (!clip).then_some(crate::canvas::CanvasHit::Placement(1))
        );
        canvas
            .group_edit(|s| {
                s.selectGroup(7);
                s.useDock(Control::Clip)
            })
            .unwrap();
    }
}
