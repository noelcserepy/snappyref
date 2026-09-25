use super::*;

#[test]
fn grouping_default_member_modes_are_inferred() {
    let root = crate::test_support::TempRoot::new("group-sparse-member-modes");
    let library = crate::library::storage::Library::initialize(root.to_path_buf()).unwrap();
    let path = library
        .store_rgba_image("member", &vec![255; 20 * 10 * 4], 20, 10)
        .unwrap();
    let asset = library
        .register_original(&path, "image/png", 20, 10, 800, None)
        .unwrap();
    let member = crate::data::Placement::loaded(
        crate::data::PlacementState::new(1, egui::Pos2::ZERO, 1., 0., 0, false),
        asset,
    );
    let mut canvas = CanvasState::default();
    canvas.add_unresolved_placement(member.clone());
    canvas.restore_groups(vec![Group {
        id: 2,
        name: "References".into(),
        members: vec![1],
        container: RectGeometry::new(egui::Pos2::ZERO, egui::vec2(100., 100.), 1., 0., 0),
        flow: GroupFlow::Row,
        gap: 8.,
        padding: 12.,
        layout: crate::data::GroupLayoutSettings {
            fixed: [true, true],
            ..Default::default()
        },
    }]);
    canvas
        .group_edit(|state| state.set_member_mode(2, 1, 0, true))
        .unwrap();
    assert!(canvas.groups()[0].layout.members[&1].fill[0]);
    canvas
        .group_edit(|state| state.set_member_mode(2, 1, 0, false))
        .unwrap();
    assert!(canvas.groups()[0].layout.members.is_empty());

    let mut snapshot = library.load_board_document(1).unwrap();
    snapshot.placements = vec![canvas.placement_for_id(1).unwrap()];
    snapshot.groups = canvas.groups().to_vec();
    snapshot.stack_order = vec![crate::data::CanvasStackItem::Placement(1)];
    library.fixture_board_snapshot(&snapshot, 1).unwrap();
    drop(library);
    let reopened = crate::library::storage::Library::initialize(root.to_path_buf()).unwrap();
    let restored = reopened.load_board_document(1).unwrap();
    assert!(restored.groups[0].layout.members.is_empty());
    let mut restored_canvas = CanvasState::default();
    restored_canvas.add_unresolved_placement(restored.placements[0].clone());
    restored_canvas.restore_groups(restored.groups);
    let projected = restored_canvas.grouping_state();
    assert_eq!(projected.board.members[&1].fill, [false, false]);
    assert!(!projected.board.members[&1].absolute);
}

#[test]
fn grouping_zero_fill_history_and_reopen() {
    use crate::active_board::history::{DocumentHistory, HistoryAction, HistoryChange};
    use crate::library::storage::{BoardDurabilityOperation, Library};
    let root = crate::test_support::TempRoot::new("group-zero-fill-history");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let path = library
        .store_rgba_image("wide", &vec![255; 20 * 10 * 4], 20, 10)
        .unwrap();
    let asset = library
        .register_original(&path, "image/png", 20, 10, 800, None)
        .unwrap();
    let member = library
        .fixture_placement(
            1,
            crate::data::Placement::loaded(
                crate::data::PlacementState::new(
                    library.allocate_document_id(),
                    egui::Pos2::ZERO,
                    1.,
                    0.,
                    0,
                    false,
                ),
                asset,
            ),
        )
        .unwrap();
    let id = member.id();
    let group_id = library.allocate_document_id();
    let mut canvas = CanvasState::default();
    canvas.add_unresolved_placement(member.clone());
    canvas.restore_groups(vec![Group {
        id: group_id,
        name: "Exhausted".into(),
        members: vec![id],
        container: RectGeometry::new(egui::Pos2::ZERO, egui::vec2(100., 100.), 1., 0., 0),
        flow: GroupFlow::Row,
        gap: 8.,
        padding: 50.,
        layout: crate::data::GroupLayoutSettings {
            fixed: [true, true],
            ..Default::default()
        },
    }]);
    canvas
        .group_edit(|state| state.set_member_mode(group_id, id, 0, true))
        .unwrap();
    let zero = canvas.placement_for_id(id).unwrap();
    assert_eq!(zero.state.geometry.size, egui::Vec2::ZERO);
    let mut history = DocumentHistory::default();
    history.record_applied(
        1,
        HistoryAction::document(
            "Fill".into(),
            vec![HistoryChange {
                before: Some(member.clone()),
                after: Some(zero.clone()),
            }],
            vec![],
            None,
        ),
    );
    library
        .persist_board_unit(
            &BoardDurabilityOperation::Document {
                board_id: 1,
                changes: super::super::CanvasDocumentChanges {
                    updated_placements: vec![zero.clone()],
                    groups: Some(canvas.groups().to_vec()),
                    ..Default::default()
                },
            },
            &history,
        )
        .unwrap();
    drop(library);
    let reopened = Library::initialize(root.to_path_buf()).unwrap();
    let restored = reopened.load_board_document(1).unwrap();
    assert_eq!(restored.placements[0].state.geometry, zero.state.geometry);
    let mut restored_history = reopened.load_document_history().unwrap();
    let undone = restored_history.undo(1).unwrap();
    let HistoryAction::Document(action) = undone.action else {
        panic!("document action")
    };
    assert_eq!(
        action.placements[0].before.as_ref().unwrap().state.geometry,
        member.state.geometry
    );
    assert_eq!(
        action.placements[0].after.as_ref().unwrap().state.geometry,
        zero.state.geometry
    );
    let redone = restored_history.redo(1).unwrap();
    let HistoryAction::Document(action) = redone.action else {
        panic!("document action")
    };
    assert_eq!(
        action.placements[0].after.as_ref().unwrap().state.geometry,
        zero.state.geometry
    );
    let mut recovered = CanvasState::default();
    recovered.add_unresolved_placement(restored.placements[0].clone());
    recovered.restore_groups(restored.groups);
    recovered
        .group_edit(|state| {
            state.set_axis_mode(group_id, 0, crate::grouping::AxisMode::Fixed(200.))
        })
        .unwrap();
    assert_eq!(
        recovered
            .placement_for_id(id)
            .unwrap()
            .scaled_display_size(),
        egui::vec2(100., 50.)
    );
    assert_eq!(
        recovered.placement_for_id(id).unwrap().asset_id(),
        member.asset_id()
    );
}
