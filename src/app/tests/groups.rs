use super::*;
use egui_kittest::kittest::NodeT;

#[path = "release_content.rs"]
mod release_content;

const CMD: egui::Modifiers = egui::Modifiers {
    command: true,
    ..egui::Modifiers::NONE
};

fn shortcut(harness: &mut Harness<'_, SnappyRefApp>, modifiers: egui::Modifiers, key: egui::Key) {
    for pressed in [true, false] {
        harness.event_modifiers(
            egui::Event::Key {
                key,
                physical_key: Some(key),
                pressed,
                repeat: false,
                modifiers,
            },
            modifiers,
        );
    }
    harness.step();
}

fn drag(
    harness: &mut Harness<'_, SnappyRefApp>,
    from: egui::Pos2,
    to: egui::Pos2,
    modifiers: egui::Modifiers,
) {
    harness.event_modifiers(egui::Event::PointerMoved(from), modifiers);
    harness.event_modifiers(
        egui::Event::PointerButton {
            pos: from,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers,
        },
        modifiers,
    );
    for _ in 0..8 {
        harness.step();
    }
    std::thread::sleep(Duration::from_millis(120));
    harness.event_modifiers(egui::Event::PointerMoved(to), modifiers);
    harness.step();
    harness.event_modifiers(
        egui::Event::PointerButton {
            pos: to,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers,
        },
        modifiers,
    );
    harness.step();
}

#[test]
fn inspect_group_operations_writes_artifact() {
    let mut scenario = InspectScenario::new("group-operations");
    let root = TempRoot::new("snappyref-inspect-group-operations");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let mut items = Vec::new();
    let mut ids = Vec::new();
    scenario.event("seed", serde_json::json!({"placements": 3, "purpose": "Group shortcuts, membership, bottom dock, Free layout and clipping"}));
    for (i, color) in [
        [78, 126, 226, 255],
        [226, 160, 78, 255],
        [106, 176, 116, 255],
    ]
    .into_iter()
    .enumerate()
    {
        let path = library
            .store_rgba_image(&format!("group-{i}"), &solid_rgba(color, 120, 90), 120, 90)
            .unwrap();
        let placement = library
            .fixture_placement(
                library.load_user_session().unwrap().active_board_id,
                crate::data::Placement::loaded(
                    crate::data::PlacementState::new(
                        library.allocate_document_id(),
                        egui::pos2(-180.0 + i as f32 * 180.0, 100.0),
                        1.0,
                        0.0,
                        0,
                        false,
                    ),
                    library
                        .register_original(&path, "image/png", 120, 90, 43200, None)
                        .unwrap(),
                ),
            )
            .unwrap();
        ids.push(placement.id());
        items.push(inspect_prepared_loaded_placement(
            placement,
            color,
            120,
            90,
            None,
            crate::image_preparation::ImportOrigin::Existing,
        ));
    }
    let mut app = test_app(Some(library.clone()));
    app.board_editing.persistence =
        crate::active_board::persistence::DocumentPersistence::queued(library.clone());
    let mut harness = app_full_harness_with_startup(1.0 / 60.0, move |ctx| {
        for item in items {
            app.canvas.add_prepared_placement_image(ctx, item);
        }
        app
    });
    harness.step();
    let screen = test_screen_rect();
    let center = |harness: &Harness<'_, SnappyRefApp>, id| {
        inspect_world_to_screen(
            harness
                .state()
                .canvas
                .placement_for_id(id)
                .unwrap()
                .state
                .geometry
                .center,
            screen,
        )
    };
    let first = center(&harness, ids[0]);
    inspect_click_at(&harness, first, egui::Modifiers::NONE);
    harness.step();
    let second = center(&harness, ids[1]);
    inspect_click_at(&harness, second, egui::Modifiers::SHIFT);
    harness.step();
    scenario.action("create Group with command G", second);
    shortcut(&mut harness, CMD, egui::Key::G);
    scenario.assertion(
        "create shortcut groups selected placements",
        harness.state().canvas.groups().len() == 1
            && harness.state().canvas.groups()[0].members == ids[..2],
    );
    if harness.state().canvas.groups().is_empty() {
        let report = scenario.finish();
        panic!("Group creation failed: {}", report.artifact_dir.display());
    }
    let group_id = harness.state().canvas.groups()[0].id;
    scenario.capture_screen("screen-001-created", &mut harness);

    let before = harness.state().canvas.groups()[0].clone();
    let from = center(&harness, ids[0]);
    let to = from + egui::vec2(0.0, 80.0);
    scenario.action("drag selected Group", to);
    harness.hover_at(from);
    harness.drag_at(from);
    for _ in 0..8 {
        harness.step();
    }
    std::thread::sleep(Duration::from_millis(120));
    harness.hover_at(to);
    harness.step();
    harness.drop_at(to);
    harness.step();
    let moved = harness.state().canvas.groups()[0].clone();
    scenario.assertion(
        "Group drag moves container",
        moved.container.center.distance(before.container.center) > 10.0,
    );
    scenario.assertion(
        "Group drag retains all members selected",
        harness.state().canvas.selected_group_ids() == vec![group_id]
            && harness.state().canvas.selected_placement_ids().len() == 2,
    );

    scenario.action("select Column control", to);
    harness.get_by_label("Column").click();
    harness.step();
    scenario.assertion(
        "Column control reflows Group",
        harness.state().canvas.groups()[0].flow == crate::data::GroupFlow::Column,
    );
    scenario.capture_screen("screen-002-column", &mut harness);

    let member = center(&harness, ids[0]);
    scenario.action("double click member to edit", member);
    harness.hover_at(member);
    for _ in 0..40 {
        harness.step();
    }
    for _ in 0..2 {
        for pressed in [true, false] {
            harness.event(egui::Event::PointerButton {
                pos: member,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            });
            harness.step();
            scenario.event("member-pointer", serde_json::json!({ "pressed": pressed, "time": harness.ctx.input(|i| i.time), "double": harness.ctx.input(|i| i.pointer.button_double_clicked(egui::PointerButton::Primary)), "click": harness.ctx.input(|i| i.pointer.any_click()), "groups": harness.state().canvas.selected_group_ids(), "placements": harness.state().canvas.selected_placement_ids() }));
        }
    }
    scenario.event("member-double-click-state", serde_json::json!({
        "pointer_double_clicked": harness.ctx.input(|i| i.pointer.button_double_clicked(egui::PointerButton::Primary)),
        "selected_placements": harness.state().canvas.selected_placement_ids(),
        "selected_groups": harness.state().canvas.selected_group_ids(),
        "member_screen": [member.x,member.y],
    }));
    scenario.capture_screen("screen-002-member-edit", &mut harness);
    scenario.assertion(
        "double click selects one member",
        harness.state().canvas.selected_placement_ids() == vec![ids[0]]
            && harness.state().canvas.selected_group_ids().is_empty(),
    );
    let reorder_from = center(&harness, ids[0]);
    let reorder_to = center(&harness, ids[1]) + egui::vec2(0.0, 20.0);
    scenario.action("drag first member after second", reorder_to);
    drag(
        &mut harness,
        reorder_from,
        reorder_to,
        egui::Modifiers::NONE,
    );
    scenario.assertion(
        "member drag reorders ordered membership",
        harness.state().canvas.groups()[0].members == vec![ids[1], ids[0]],
    );
    let grouped = harness.state().canvas.groups().to_vec();
    let geometries = |harness: &Harness<'_, SnappyRefApp>| {
        ids.iter()
            .map(|id| {
                harness
                    .state()
                    .canvas
                    .placement_for_id(*id)
                    .unwrap()
                    .state
                    .geometry
            })
            .collect::<Vec<_>>()
    };
    let before_detach = geometries(&harness);
    let detach_from = center(&harness, ids[1]);
    let detach_to = detach_from + egui::vec2(250.0, 0.0);
    scenario.action("drag member outside Group", detach_to);
    drag(&mut harness, detach_from, detach_to, egui::Modifiers::NONE);
    scenario.assertion(
        "outside drop removes membership and keeps placement at drop",
        harness.state().canvas.groups()[0].members == vec![ids[0]]
            && center(&harness, ids[1]).distance(detach_to) < 1.0
            && harness.state().canvas.selected_placement_ids() == vec![ids[1]],
    );
    scenario.assertion(
        "remaining member closes the gap and container shrinks",
        geometries(&harness)[0].center.y < before_detach[0].center.y
            && harness.state().canvas.groups()[0].container.size.y < grouped[0].container.size.y,
    );
    scenario.capture_screen("screen-002-member-detached", &mut harness);
    let detached_groups = harness.state().canvas.groups().to_vec();
    let detached_geometry = geometries(&harness);
    shortcut(&mut harness, CMD, egui::Key::Z);
    scenario.assertion(
        "one Undo restores membership and all placement positions",
        harness.state().canvas.groups() == grouped && geometries(&harness) == before_detach,
    );
    shortcut(
        &mut harness,
        egui::Modifiers { shift: true, ..CMD },
        egui::Key::Z,
    );
    scenario.assertion(
        "Redo restores detached placement and reflowed Group",
        harness.state().canvas.groups() == detached_groups
            && geometries(&harness) == detached_geometry,
    );
    let last_member = center(&harness, ids[0]);
    for _ in 0..40 {
        harness.step();
    }
    for _ in 0..2 {
        inspect_click_at(&harness, last_member, egui::Modifiers::NONE);
        harness.step();
    }
    let last_drop = last_member + egui::vec2(250.0, 0.0);
    scenario.action("drag last member outside Group", last_drop);
    drag(&mut harness, last_member, last_drop, egui::Modifiers::NONE);
    scenario.assertion(
        "last member detaches and empty Group keeps its configuration",
        harness.state().canvas.groups()[0].members.is_empty()
            && harness.state().canvas.groups()[0].name == detached_groups[0].name
            && harness.state().canvas.groups()[0].container == detached_groups[0].container
            && center(&harness, ids[0]).distance(last_drop) < 1.0,
    );
    scenario.capture_screen("screen-002-empty-after-detach", &mut harness);
    shortcut(&mut harness, CMD, egui::Key::Z);
    scenario.assertion(
        "Undo last member drop restores the Group",
        harness.state().canvas.groups() == detached_groups
            && geometries(&harness) == detached_geometry,
    );
    shortcut(&mut harness, CMD, egui::Key::Z);
    // Continue the existing Group controls checks after restoring the members.
    let member = center(&harness, ids[0]);
    for _ in 0..40 {
        harness.step();
    }
    for _ in 0..2 {
        inspect_click_at(&harness, member, egui::Modifiers::NONE);
        harness.step();
    }
    shortcut(&mut harness, egui::Modifiers::NONE, egui::Key::Escape);
    scenario.assertion(
        "Escape returns to Group selection",
        harness.state().canvas.selected_group_ids() == vec![group_id],
    );

    harness.get_by_label("Wrap").click();
    harness.step();
    let before_resize = harness.state().canvas.groups()[0].clone();
    let member_size = harness
        .state()
        .canvas
        .placement_for_id(ids[0])
        .unwrap()
        .state
        .geometry
        .scaled_size();
    let resize_from =
        inspect_world_to_screen(before_resize.container.unrotated_bounds().max, screen);
    scenario.action("drag container bottom-right narrower", resize_from);
    drag(
        &mut harness,
        resize_from,
        resize_from - egui::vec2(220.0, 0.0),
        egui::Modifiers::NONE,
    );
    let resized = harness.state().canvas.groups()[0].clone();
    scenario.assertion(
        "container resize changes width without scaling member",
        resized.container.scaled_size().x < before_resize.container.scaled_size().x
            && harness
                .state()
                .canvas
                .placement_for_id(ids[0])
                .unwrap()
                .state
                .geometry
                .scaled_size()
                == member_size,
    );
    scenario.assertion(
        "narrow container leaves fixed member visibly overflowing",
        harness
            .state()
            .canvas
            .placement_for_id(ids[0])
            .unwrap()
            .visual_bounds()
            .max
            .x
            > resized.container.unrotated_bounds().max.x,
    );
    scenario.capture_screen("screen-002-overflow", &mut harness);
    let scale_from = center(&harness, ids[1]);
    scenario.action("command alt drag scales Group", scale_from);
    drag(
        &mut harness,
        scale_from,
        scale_from + egui::vec2(30.0, 0.0),
        egui::Modifiers { alt: true, ..CMD },
    );
    scenario.assertion(
        "uniform scale grows container and members",
        harness.state().canvas.groups()[0].container.scaled_size().x
            > resized.container.scaled_size().x
            && harness
                .state()
                .canvas
                .placement_for_id(ids[0])
                .unwrap()
                .state
                .geometry
                .scaled_size()
                .x
                > member_size.x,
    );
    scenario.capture_screen("screen-002-resized-scaled", &mut harness);

    // Empty destinations are seeded. The picker must expose and fill them.
    let mut empty = harness.state().canvas.groups()[0].clone();
    empty.id = group_id + 100;
    empty.name = "Empty destination".into();
    empty.members.clear();
    empty.container.center.x += 350.0;
    let mut groups = harness.state().canvas.groups().to_vec();
    groups.push(empty.clone());
    harness.state_mut().canvas.restore_groups(groups);
    let third = center(&harness, ids[2]);
    inspect_click_at(&harness, third, egui::Modifiers::NONE);
    harness.step();
    let before_cancel = harness.state().canvas.groups().to_vec();
    scenario.action("context menu Add to group then cancel", third);
    harness.hover_at(third);
    for pressed in [true, false] {
        harness.event(egui::Event::PointerButton {
            pos: third,
            button: egui::PointerButton::Secondary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
        harness.step();
    }
    harness.get_by_label("Add to group").click();
    for _ in 0..4 {
        harness.step();
    }
    let _ = harness.get_by_role(egui::accesskit::Role::TextInput);
    scenario.event(
        "context-menu-picker",
        serde_json::json!({"search_prompt_visible": true}),
    );
    harness.get_by_label("Cancel").click();
    harness.step();
    scenario.assertion(
        "picker cancel leaves membership unchanged",
        harness.state().canvas.groups() == before_cancel,
    );

    scenario.action("search empty destination then Enter", third);
    shortcut(&mut harness, egui::Modifiers::NONE, egui::Key::G);
    harness.step();
    harness.event(egui::Event::Text("Empty".into()));
    harness.step();
    scenario.capture_screen("screen-003-picker", &mut harness);
    let before_text_shortcut = harness.state().canvas.groups().to_vec();
    shortcut(&mut harness, CMD, egui::Key::G);
    scenario.assertion(
        "Group shortcut while typing does not create a Group",
        harness.state().canvas.groups() == before_text_shortcut,
    );
    shortcut(&mut harness, egui::Modifiers::NONE, egui::Key::Enter);
    scenario.assertion(
        "Enter adds existing placement to empty destination",
        harness
            .state()
            .canvas
            .groups()
            .iter()
            .find(|g| g.id == empty.id)
            .unwrap()
            .members
            == vec![ids[2]],
    );
    let after_add = harness.state().canvas.groups().to_vec();
    scenario.action("repeat add to same destination", third);
    shortcut(&mut harness, egui::Modifiers::NONE, egui::Key::G);
    harness.step();
    shortcut(&mut harness, egui::Modifiers::NONE, egui::Key::Enter);
    scenario.assertion(
        "repeated add does not duplicate or reorder",
        harness.state().canvas.groups() == after_add,
    );
    scenario.assertion(
        "picker does not duplicate placements",
        harness.state().canvas.all_placement_ids().len() == 3,
    );
    harness.state_mut().canvas.select_group(group_id);
    harness.step();
    harness.get_by_label("Free").click();
    for _ in 0..4 {
        harness.step();
    }
    scenario.assertion(
        "Free is available in the bottom dock",
        harness
            .state()
            .canvas
            .groups()
            .iter()
            .find(|g| g.id == group_id)
            .unwrap()
            .flow
            == crate::data::GroupFlow::Free,
    );
    harness.get_by_label("Clip").click();
    for _ in 0..4 {
        harness.step();
    }
    scenario.assertion(
        "Clip is a saved group setting",
        harness
            .state()
            .canvas
            .groups()
            .iter()
            .find(|g| g.id == group_id)
            .unwrap()
            .layout
            .clip,
    );
    harness.get_by_label("Wrap").click();
    harness.step();
    scenario.capture_screen("screen-004-complete", &mut harness);
    let report = scenario.finish();
    assert!(
        report.passed,
        "inspect Group scenario failed: {}",
        report.artifact_dir.display()
    );
}

#[test]
fn group_document_history_survives_transfer_delete_restart_and_board_archive() {
    let root = TempRoot::new("snappyref-group-document-history");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let other_board = library.fixture_board("Other").unwrap();
    let mut app = test_app(Some(library.clone()));
    app.board_editing.persistence =
        crate::active_board::persistence::DocumentPersistence::queued(library.clone());
    let ctx = egui::Context::default();
    let mut ids = Vec::new();
    for i in 0..3 {
        let path = library
            .store_rgba_image(
                &format!("history-{i}"),
                &solid_rgba([60 + i as u8 * 40, 90, 120, 255], 40, 30),
                40,
                30,
            )
            .unwrap();
        let placement = library
            .fixture_placement(
                library.load_user_session().unwrap().active_board_id,
                crate::data::Placement::loaded(
                    crate::data::PlacementState::new(
                        library.allocate_document_id(),
                        egui::pos2(i as f32 * 60.0, 0.0),
                        1.0,
                        0.0,
                        0,
                        false,
                    ),
                    library
                        .register_original(&path, "image/png", 40, 30, 4800, None)
                        .unwrap(),
                ),
            )
            .unwrap();
        ids.push(placement.id());
        app.canvas.add_unresolved_placement(placement);
    }
    let record = |app: &mut SnappyRefApp| {
        app.record_pending_document_changes_history("Edit group");
        app.board_editing
            .accept_created_batch(&mut app.canvas, app.active_board_id, None)
            .unwrap();
    };
    app.canvas.select_placements(&ids[..2]);
    let first = library.allocate_document_id();
    app.canvas.create_group(first).unwrap();
    record(&mut app);
    let created = app.canvas.groups().to_vec();
    let mut renamed = created[0].clone();
    renamed.name = "Study".into();
    app.canvas.edit_group(renamed).unwrap();
    record(&mut app);
    app.canvas.select_placements(&ids[2..]);
    let second = library.allocate_document_id();
    app.canvas.create_group(second).unwrap();
    record(&mut app);
    let before_transfer = app.canvas.groups().to_vec();
    let before_geometry: Vec<_> = ids
        .iter()
        .map(|id| app.canvas.placement_for_id(*id).unwrap().state.geometry)
        .collect();
    app.canvas.add_members_to_group(second, &[ids[0]]).unwrap();
    record(&mut app);
    let transferred = app.canvas.groups().to_vec();
    assert_eq!(transferred[0].members, vec![ids[1]]);
    assert_eq!(transferred[1].members, vec![ids[2], ids[0]]);
    app.undo_document_history(&ctx);
    assert_eq!(app.canvas.groups(), before_transfer);
    assert_eq!(
        ids.iter()
            .map(|id| app.canvas.placement_for_id(*id).unwrap().state.geometry)
            .collect::<Vec<_>>(),
        before_geometry
    );
    app.redo_document_history(&ctx);
    assert_eq!(app.canvas.groups(), transferred);
    app.canvas.select_placements(&[ids[1]]);
    app.delete_selected_items_from_board(test_screen_rect());
    assert!(app.canvas.groups()[0].members.is_empty());
    app.undo_document_history(&ctx);
    assert_eq!(app.canvas.groups(), transferred);
    app.redo_document_history(&ctx);
    let deleted = app.canvas.groups().to_vec();
    app.flush_persistence();
    let reopened = Library::initialize(root.to_path_buf()).unwrap();
    assert_eq!(reopened.load_groups().unwrap(), deleted);
    let mut restored = test_app(Some(reopened.clone()));
    restored.board_editing.persistence =
        crate::active_board::persistence::DocumentPersistence::queued(reopened.clone());
    restored.board_editing.history = reopened.load_document_history().unwrap();
    let snapshot = reopened.load_board_document(1).unwrap();
    restored
        .board_lifecycle()
        .open_board_document_snapshot(Some(&ctx), &snapshot)
        .unwrap();
    restored.undo_document_history(&ctx);
    assert_eq!(restored.canvas.groups(), transferred);
    restored.redo_document_history(&ctx);
    assert_eq!(restored.canvas.groups(), deleted);
    restored.switch_to_board(other_board.id);
    assert!(restored.canvas.groups().is_empty());
    restored.switch_to_board(1);
    assert_eq!(restored.canvas.groups(), deleted);
    restored.request_board_delete(1, "My Board".into());
    restored.confirm_pending_board_delete(&ctx);
    assert_eq!(restored.active_board_id, other_board.id);
    restored.undo_document_history(&ctx);
    assert_eq!(restored.active_board_id, 1);
    assert_eq!(restored.canvas.groups(), deleted);
    restored.flush_persistence();
    let durable = reopened.load_board_document(1).unwrap_or_else(|error| {
        panic!(
            "{error}; app status={}; persistence={:?}",
            restored.status,
            restored.board_editing.persistence.take_error()
        )
    });
    assert_eq!(durable.groups, deleted);
}

#[test]
fn inspect_group_picker_rename_and_create_writes_artifact() {
    let mut scenario = InspectScenario::new("group-picker-rename-create");
    let root = TempRoot::new("snappyref-group-picker-rename-create");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let mut items = Vec::new();
    let mut ids = Vec::new();
    for i in 0..2 {
        let mut item = persisted_loaded_item(&library, library.allocate_document_id(), 1);
        item.placement.state.geometry.center = egui::pos2(i as f32 * 260.0 - 130.0, 90.0);
        item.placement.state.geometry.size = egui::vec2(120.0, 90.0);
        item.placement = library.fixture_placement(1, item.placement).unwrap();
        ids.push(item.placement.id());
        items.push(item);
    }
    let mut app = test_app(Some(library.clone()));
    let selected = ids[..1].to_vec();
    let mut harness = app_full_harness_with_startup(1.0 / 60.0, move |ctx| {
        for item in items {
            app.canvas.add_prepared_placement_image(ctx, item);
        }
        app.canvas.select_placements(&selected);
        app.group_selection();
        app
    });
    let group_id = harness.state().canvas.groups()[0].id;
    harness.get_by_label("Group name").click();
    harness.step();
    shortcut(&mut harness, CMD, egui::Key::A);
    harness.event(egui::Event::Text("Faces gqwe".into()));
    harness.step();
    let other = harness.state().canvas.placement_for_id(ids[1]).unwrap();
    let other_point = inspect_world_to_screen(other.state.geometry.center, test_screen_rect());
    inspect_click_at(&harness, other_point, egui::Modifiers::NONE);
    harness.step();
    shortcut(&mut harness, egui::Modifiers::NONE, egui::Key::G);
    harness.step();
    assert_eq!(harness.state().canvas.groups()[0].name, "Faces gqwe");
    let _ = harness.get_by_label("Faces gqwe");
    scenario.assertion(
        "click-away rename appears in picker",
        harness.state().canvas.groups()[0].name == "Faces gqwe",
    );
    scenario.capture_screen("renamed-destination", &mut harness);
    harness.event(egui::Event::Text("New references".into()));
    harness.step();
    harness.step();
    scenario.capture_screen("create-new-action", &mut harness);
    shortcut(&mut harness, egui::Modifiers::NONE, egui::Key::Escape);
    assert_eq!(harness.state().canvas.groups().len(), 1);
    shortcut(&mut harness, egui::Modifiers::NONE, egui::Key::G);
    harness.step();
    harness.event(egui::Event::Text("New references".into()));
    harness.step();
    harness
        .get_by_label("Create new \"New references\"")
        .click();
    harness.step();
    let created = harness
        .state()
        .canvas
        .groups()
        .iter()
        .find(|g| g.id != group_id)
        .unwrap()
        .clone();
    scenario.assertion(
        "create names and groups the selected placement once",
        created.name == "New references"
            && created.members == ids[1..]
            && created.flow == crate::data::GroupFlow::Wrap,
    );
    assert_eq!(created.members, ids[1..]);
    shortcut(&mut harness, CMD, egui::Key::Z);
    assert_eq!(harness.state().canvas.groups().len(), 1);
    shortcut(
        &mut harness,
        egui::Modifiers { shift: true, ..CMD },
        egui::Key::Z,
    );
    assert_eq!(
        harness.state().canvas.groups().len(),
        2,
        "status={}",
        harness.state().status
    );
    harness.state_mut().flush_persistence();
    assert_eq!(
        library.load_groups().unwrap(),
        harness.state().canvas.groups()
    );
    scenario.assertion(
        "new Wrap group keeps Fixed width and Hug height",
        created.layout.fixed == [true, false]
            && created.layout.align == [0, 0]
            && !created.layout.clip,
    );
    scenario.assertion(
        "create is one undoable durable edit",
        library.load_groups().unwrap().len() == 2,
    );
    // Open with a long query at a constrained width to check the real popup.
    shortcut(&mut harness, egui::Modifiers::NONE, egui::Key::G);
    harness.step();
    harness.event(egui::Event::Text(
        "A long group name for architectural details and reference studies".into(),
    ));
    harness.step();
    harness.set_size(egui::vec2(480.0, 420.0));
    harness.step();
    scenario.capture_screen("narrow-create-action", &mut harness);
    shortcut(&mut harness, egui::Modifiers::NONE, egui::Key::Enter);
    assert_eq!(harness.state().canvas.groups().len(), 3);
    scenario.assertion(
        "Enter creates the typed destination when no group matches",
        harness.state().canvas.groups().last().unwrap().name
            == "A long group name for architectural details and reference studies",
    );

    let report = scenario.finish();
    assert!(report.passed, "{}", report.artifact_dir.display());
}

#[test]
fn copied_group_survives_dedupe_and_reopen() {
    let root = TempRoot::new("clipboard-legacy-dedupe");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let other = library.fixture_board("B").unwrap();
    let mut items = Vec::new();
    for _ in 0..3 {
        let mut item = persisted_loaded_item(&library, library.allocate_document_id(), 1);
        item.placement = library.fixture_placement(1, item.placement).unwrap();
        items.push(item);
    }
    let ids = items
        .iter()
        .map(|item| item.placement.id())
        .collect::<Vec<_>>();
    let ctx = egui::Context::default();
    let mut app = test_app(Some(library.clone()));
    app.import_worker = Some(crate::image_preparation::ImagePrepWorker::new(
        library.clone(),
        ctx.clone(),
        None,
    ));
    for item in items {
        app.canvas.add_prepared_placement_image(&ctx, item);
    }
    app.canvas.select_placements(&ids);
    app.group_selection();
    let payload = app.copy_selected_board_payload().unwrap();
    app.switch_to_board(other.id);
    // Merged IDs must not be reused for unrelated newly imported pixels.
    let path = library
        .store_rgba_image("unrelated", &[255, 0, 0, 255], 1, 1)
        .unwrap();
    let fresh = library
        .register_original(&path, "image/png", 1, 1, 4, None)
        .unwrap();
    assert!(fresh.id > payload.placements.iter().map(|p| p.asset_id).max().unwrap());
    app.paste_board_payload_at(&payload, egui::pos2(300., 0.))
        .unwrap();
    assert_eq!(app.canvas.groups()[0].members.len(), 3);
    assert_eq!(app.canvas.all_placement_ids().len(), 3);
    app.flush_persistence();
    drop(app);
    let reopened = Library::initialize(root.to_path_buf()).unwrap();
    let mut app = test_app(Some(reopened.clone()));
    app.import_worker = Some(crate::image_preparation::ImagePrepWorker::new(
        reopened.clone(),
        ctx,
        None,
    ));
    app.switch_to_board(other.id);
    app.paste_board_payload_at(&payload, egui::pos2(600., 0.))
        .unwrap();
    assert_eq!(app.canvas.groups().last().unwrap().members.len(), 3);
}

fn dock_fixture() -> (TempRoot, Harness<'static, SnappyRefApp>, Vec<i64>) {
    let root = TempRoot::new("snappyref-dock-artboards");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let mut items = Vec::new();
    let mut ids = Vec::new();
    for (i, (height, color)) in [
        (190, [59, 53, 50, 255]),
        (110, [74, 67, 64, 255]),
        (150, [47, 58, 58, 255]),
        (120, [61, 51, 68, 255]),
        (150, [68, 64, 58, 255]),
    ]
    .into_iter()
    .enumerate()
    {
        let path = library
            .store_rgba_image(
                &format!("dock-{i}"),
                &solid_rgba(color, 150, height),
                150,
                height,
            )
            .unwrap();
        let placement = library
            .fixture_placement(
                1,
                crate::data::Placement::loaded(
                    crate::data::PlacementState::new(
                        library.allocate_document_id(),
                        egui::pos2(i as f32 * 160.0 - 184.0, -119.0),
                        1.0,
                        0.0,
                        0,
                        false,
                    ),
                    library
                        .register_original(
                            &path,
                            "image/png",
                            150,
                            height,
                            (150 * height * 4) as u64,
                            None,
                        )
                        .unwrap(),
                ),
            )
            .unwrap();
        ids.push(placement.id());
        items.push(inspect_prepared_loaded_placement(
            placement,
            color,
            150,
            height,
            None,
            crate::image_preparation::ImportOrigin::Existing,
        ));
    }
    let mut app = test_app(Some(library));
    let selected = ids.clone();
    let mut harness = app_full_harness_with_startup(1.0 / 60.0, move |ctx| {
        for item in items {
            app.canvas.add_prepared_placement_image(ctx, item);
        }
        app.canvas.select_placements(&selected);
        app.group_selection();
        let mut group = app.canvas.groups()[0].clone();
        group.name = "Faces".into();
        group.gap = 24.0;
        group.padding = 16.0;
        group.layout.row_gap = 24.0;
        group.container.size.x = 530.0;
        app.canvas.edit_group(group).unwrap();
        app
    });
    harness.set_size(egui::vec2(1280.0, 800.0));
    for _ in 0..20 {
        for _ in 0..4 {
            harness.step();
        }
    }
    (root, harness, ids)
}

#[test]
fn group_paste_undo_removes_only_pasted_members() {
    let (_root, mut h, ids) = dock_fixture();
    let ctx = h.ctx.clone();
    let group = h.state().canvas.groups()[0].id;
    h.state_mut().canvas.select_content(&ids[..2], &[]);
    let payload = h.state().copy_selected_board_payload().unwrap();
    let app = h.state_mut();
    app.canvas.select_group(group);
    app.paste_board_payload_at(&payload, egui::pos2(700., 100.))
        .unwrap();
    assert_eq!(app.canvas.groups()[0].members.len(), 7);
    app.undo_document_history(&ctx);
    assert_eq!(app.canvas.all_placement_ids().len(), 5);
    assert_eq!(app.canvas.groups()[0].members, ids);
    let mut repeated = payload;
    repeated.stack_order[1] = repeated.stack_order[0].clone();
    app.canvas.select_content(&[], &[]);
    app.paste_board_payload_at(&repeated, egui::pos2(700., 100.))
        .unwrap();
    app.undo_document_history(&ctx);
    assert_eq!(app.canvas.all_placement_ids().len(), 5);
}

#[test]
fn member_paste_redo_restores_reflowed_members() {
    let (_root, mut h, ids) = dock_fixture();
    let ctx = h.ctx.clone();
    h.state_mut().canvas.select_content(&ids[..1], &[]);
    let payload = h.state().copy_selected_board_payload().unwrap();
    let first = h.state().canvas.placement_for_id(ids[0]).unwrap();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 800.0));
    let pos = inspect_world_to_screen(first.state.geometry.center, screen);
    h.event_modifiers(egui::Event::PointerMoved(pos), CMD);
    inspect_click_at(&h, pos, CMD);
    h.step();
    assert!(h.state().canvas.current_group_dock().member() == Some(ids[0]));
    let geometry = |h: &Harness<'_, SnappyRefApp>| {
        h.state()
            .canvas
            .all_placement_ids()
            .into_iter()
            .map(|id| {
                h.state()
                    .canvas
                    .placement_for_id(id)
                    .unwrap()
                    .state
                    .geometry
            })
            .collect::<Vec<_>>()
    };
    let before = geometry(&h);
    h.state_mut()
        .paste_board_payload_at(&payload, egui::pos2(700., 100.))
        .unwrap();
    let pasted = geometry(&h);
    assert_eq!(pasted.len(), 6);
    h.state_mut().undo_document_history(&ctx);
    for _ in 0..8 {
        h.step();
    }
    h.state_mut().redo_document_history(&ctx);
    for _ in 0..8 {
        h.step();
    }
    assert_eq!(geometry(&h), pasted);
    h.state_mut().undo_document_history(&ctx);
    assert_eq!(geometry(&h), before);
    assert_eq!(h.state().canvas.groups()[0].members, ids);
}

#[test]
fn member_delete_undo_restores_reflowed_textbox() {
    let (_root, mut h, ids) = dock_fixture();
    let ctx = h.ctx.clone();
    let app = h.state_mut();
    let note = app.library.as_ref().unwrap().new_canvas_annotation(
        CanvasAnnotationKind::Textbox,
        egui::pos2(-200.0, 200.0),
        egui::pos2(40.0, 256.0),
        "note".to_owned(),
        CanvasTextSize::M,
        CanvasStrokeWidth::M,
        crate::data::CanvasAnnotationColor::Sky,
        true,
    );
    app.canvas.add_annotation(note.clone());
    let group = app.canvas.groups()[0].id;
    app.canvas.add_members_to_group(group, &[note.id]).unwrap();
    for _ in 0..8 {
        h.step();
    }
    let center = |h: &Harness<'_, SnappyRefApp>| {
        h.state()
            .canvas
            .annotation_for_id(note.id)
            .unwrap()
            .geometry
    };
    let before = center(&h);
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 800.0));
    let first = h.state().canvas.placement_for_id(ids[0]).unwrap();
    let pos = inspect_world_to_screen(first.state.geometry.center, screen);
    h.event_modifiers(egui::Event::PointerMoved(pos), CMD);
    inspect_click_at(&h, pos, CMD);
    h.step();
    h.state_mut()
        .delete_selected_items_from_board(test_screen_rect());
    for _ in 0..8 {
        h.step();
    }
    assert_ne!(center(&h), before);
    h.state_mut().undo_document_history(&ctx);
    for _ in 0..8 {
        h.step();
    }
    assert_eq!(center(&h), before);
}

#[test]
fn dragging_a_member_moves_every_selected_group() {
    let (_root, mut h, ids) = dock_fixture();
    let app = h.state_mut();
    app.canvas.select_content(&ids[3..], &[]);
    app.group_selection();
    let note = app.library.as_ref().unwrap().new_canvas_annotation(
        CanvasAnnotationKind::Textbox,
        egui::pos2(-200.0, 200.0),
        egui::pos2(40.0, 256.0),
        "note".to_owned(),
        CanvasTextSize::M,
        CanvasStrokeWidth::M,
        crate::data::CanvasAnnotationColor::Sky,
        true,
    );
    app.canvas.add_annotation(note.clone());
    app.canvas.select_content(&[], &[note.id]);
    app.group_selection();
    app.canvas.select_all_content();
    let containers = |h: &Harness<'_, SnappyRefApp>| {
        h.state()
            .canvas
            .groups()
            .iter()
            .map(|g| g.container.center)
            .collect::<Vec<_>>()
    };
    let before = containers(&h);
    assert_eq!(before.len(), 3);
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 800.0));
    let from = inspect_world_to_screen(
        h.state()
            .canvas
            .placement_for_id(ids[0])
            .unwrap()
            .state
            .geometry
            .center,
        screen,
    );
    drag(
        &mut h,
        from,
        from + egui::vec2(90.0, 60.0),
        egui::Modifiers::NONE,
    );
    let moved = containers(&h)
        .iter()
        .zip(&before)
        .map(|(after, before)| *after - *before)
        .collect::<Vec<_>>();
    assert_eq!(moved, [egui::vec2(90.0, 60.0); 3]);
    assert_eq!(
        h.state().canvas.annotation_for_id(note.id).unwrap().start(),
        note.start() + egui::vec2(90.0, 60.0)
    );
    assert_eq!(h.state().canvas.selected_group_ids().len(), 3);

    // A drag from the textbox moves only textboxes itself; image Groups still follow.
    let from = inspect_world_to_screen(egui::pos2(10.0, 228.0) + egui::vec2(90.0, 60.0), screen);
    drag(
        &mut h,
        from,
        from + egui::vec2(90.0, 60.0),
        egui::Modifiers::NONE,
    );
    let moved = containers(&h)
        .iter()
        .zip(&before)
        .map(|(after, before)| *after - *before)
        .collect::<Vec<_>>();
    assert_eq!(moved, [egui::vec2(180.0, 120.0); 3]);
    let payload = h.state().copy_selected_board_payload().unwrap();
    assert_eq!(payload.placements.len(), 5);
    assert_eq!(payload.groups.len(), 3);
}

#[test]
fn dragging_a_loose_textbox_leaves_the_selected_group() {
    let (_root, mut h, _ids) = dock_fixture();
    let app = h.state_mut();
    let note = app.library.as_ref().unwrap().new_canvas_annotation(
        CanvasAnnotationKind::Textbox,
        egui::pos2(-200.0, 200.0),
        egui::pos2(40.0, 256.0),
        "note".to_owned(),
        CanvasTextSize::M,
        CanvasStrokeWidth::M,
        crate::data::CanvasAnnotationColor::Sky,
        true,
    );
    app.canvas.add_annotation(note.clone());
    let group = app.canvas.groups()[0].id;
    app.canvas.select_group(group);
    h.step();
    let before = h.state().canvas.groups()[0].container.center;
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 800.0));
    let from = inspect_world_to_screen(egui::pos2(10.0, 228.0), screen);
    drag(
        &mut h,
        from,
        from + egui::vec2(90.0, 60.0),
        egui::Modifiers::NONE,
    );
    assert_eq!(
        h.state().canvas.annotation_for_id(note.id).unwrap().start(),
        note.start() + egui::vec2(90.0, 60.0)
    );
    assert_eq!(h.state().canvas.groups()[0].container.center, before);
}

#[test]
fn group_overflow_reaches_member_editing_only_while_visible() {
    let (_root, mut h, ids) = dock_fixture();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 800.0));
    let mut group = h.state().canvas.groups()[0].clone();
    group.container.size.x = 80.0;
    h.state_mut().canvas.edit_group(group).unwrap();
    let bounds = h.state().canvas.groups()[0].container.unrotated_bounds();
    let first = h
        .state()
        .canvas
        .placement_for_id(ids[0])
        .unwrap()
        .state
        .geometry;
    assert!(first.visual_bounds().max.x > bounds.max.x + 20.0);
    let overflow = inspect_world_to_screen(egui::pos2(bounds.max.x + 20.0, first.center.y), screen);
    for clip in [false, true] {
        let mut group = h.state().canvas.groups()[0].clone();
        group.layout.clip = clip;
        let app = h.state_mut();
        app.canvas.edit_group(group).unwrap();
        app.canvas.select_content(&[], &[]);
        inspect_click_at(&h, overflow, if clip { egui::Modifiers::NONE } else { CMD });
        for _ in 0..4 {
            h.step();
        }
        let canvas = &h.state().canvas;
        if clip {
            assert!(canvas.selected_group_ids().is_empty());
            assert!(canvas.selected_placement_ids().is_empty());
        } else {
            assert_eq!(canvas.current_group_dock().member(), Some(ids[0]));
        }
    }
    // Crop, like every item hit test, only reaches visible pixels.
    h.event(egui::Event::Key {
        key: egui::Key::C,
        physical_key: Some(egui::Key::C),
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    });
    h.step();
    inspect_click_at(&h, overflow, egui::Modifiers::NONE);
    for _ in 0..4 {
        h.step();
    }
    assert!(h.state().canvas.selected_placement_ids().is_empty());
}

#[test]
fn inspect_group_dock_writes_artifact() {
    let mut scenario = InspectScenario::new("group-dock");
    let (_root, mut harness, ids) = dock_fixture();
    scenario.event("seed",serde_json::json!({"members":5,"viewport":[1280,800],"flow":"Wrap","width":530,"padding":16,"gap":24}));
    let card = harness
        .ctx
        .memory(|m| m.area_rect(egui::Id::new("group-card")))
        .unwrap();
    scenario.assertion(
        "card sits center-right, 16 pixels from the canvas edge",
        card.right() == 1264.0 && (card.center().y - 400.0).abs() < 0.5,
    );
    // The card reads top to bottom: identity, flow, alignment beside spacing
    // and spread, then the frame.
    let inventory = [
        "Group name",
        "Row",
        "Column",
        "Wrap",
        "Free",
        "Align left, top",
        "Gap",
        "Row gap",
        "Off",
        "Between",
        "Evenly",
        "Around",
        "W size",
        "W hug contents",
        "H size",
        "H hug contents",
        "Padding",
        "Clip",
    ];
    let mut previous = egui::pos2(f32::NEG_INFINITY, f32::NEG_INFINITY);
    for label in inventory {
        let rect = harness.get_by_label(label).rect();
        scenario.event(
            "control",
            serde_json::json!({"label":label,"rect":format!("{rect:?}")}),
        );
        assert!(rect.width() > 10.0, "Visible control: {label}");
        assert!(card.contains_rect(rect), "Inside card: {label}");
        assert!(
            rect.top() > previous.y || rect.left() > previous.x,
            "Control order: {label}"
        );
        previous = rect.left_top();
    }
    scenario.assertion("group inventory follows card order", true);
    // Every pad cell keeps one grid rect, so the larger selected glyph never
    // shifts its neighbours.
    let cells: Vec<_> = (0..9)
        .map(|i| {
            harness
                .get_by_label(&format!(
                    "Align {}, {}",
                    ["left", "center", "right"][i % 3],
                    ["top", "center", "bottom"][i / 3]
                ))
                .rect()
        })
        .collect();
    let step = cells[1].left() - cells[0].left();
    scenario.assertion(
        "packed pad cells sit on a fixed grid",
        cells.iter().enumerate().all(|(i, r)| {
            (r.size() - cells[0].size()).length() < 0.01
                && (r.left() - cells[0].left() - (i % 3) as f32 * step).abs() < 0.01
                && (r.top() - cells[0].top() - (i / 3) as f32 * step).abs() < 0.01
        }),
    );
    for (label, draft, blur, expected) in [
        ("Gap", "1.5", false, 24.0),
        ("Row gap", "1.5", true, 24.0),
        ("Gap", "-3", true, -3.0),
        ("Row gap", "+7", false, 7.0),
        ("Padding", "16.5", false, 16.5),
    ] {
        scenario.event(
            "submit numeric draft",
            serde_json::json!({
                "field": label, "draft": draft, "submission": if blur { "blur" } else { "Enter" }
            }),
        );
        harness.get_by_label(label).click();
        harness.step();
        shortcut(&mut harness, CMD, egui::Key::A);
        harness.event(egui::Event::Text(draft.into()));
        harness.step();
        if blur {
            harness.get_by_label("Group name").click();
        } else {
            shortcut(&mut harness, egui::Modifiers::NONE, egui::Key::Enter);
        }
        for _ in 0..4 {
            harness.step();
        }
        let group = &harness.state().canvas.groups()[0];
        let actual = match label {
            "Gap" => group.gap,
            "Row gap" => group.layout.row_gap,
            _ => group.padding,
        };
        scenario.assertion(
            "numeric draft preserves integer gaps and decimal padding",
            actual == expected,
        );
        scenario.assertion(
            "numeric field releases focus and remains enabled",
            !harness.get_by_label(label).is_focused()
                && !harness.get_by_label(label).accesskit_node().is_disabled(),
        );
    }
    scenario.capture_screen("numeric-draft-validation", &mut harness);
    // The handle is the icon just left of the field. Its press commits the
    // open draft, and the scrub starts from that draft.
    let padding = harness.state().canvas.groups()[0].padding;
    harness.get_by_label("Padding").click();
    harness.step();
    shortcut(&mut harness, CMD, egui::Key::A);
    harness.event(egui::Event::Text("100".into()));
    harness.step();
    let handle = harness.get_by_label("Padding").rect().left_center() - egui::vec2(13.0, 0.0);
    harness.event(egui::Event::PointerMoved(handle));
    harness.step();
    for (dx, pressed) in [
        (0.0, Some(true)),
        (15.0, None),
        (30.0, None),
        (30.0, Some(false)),
    ] {
        harness.event(egui::Event::PointerMoved(handle + egui::vec2(dx, 0.0)));
        if let Some(pressed) = pressed {
            harness.event(egui::Event::PointerButton {
                pos: handle + egui::vec2(dx, 0.0),
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            });
        }
        harness.step();
    }
    harness.step();
    let scrubbed = harness.state().canvas.groups()[0].padding;
    let ctx = harness.ctx.clone();
    harness.state_mut().undo_document_history(&ctx);
    harness.step();
    let undone = harness.state().canvas.groups()[0].padding;
    // Undo drops the Group selection.
    let group = harness.state().canvas.groups()[0].id;
    harness.state_mut().canvas.select_group(group);
    scenario.assertion(
        "scrubbing from a committed draft is one undoable edit",
        scrubbed == 130.0 && undone == padding,
    );
    // H hugs in the fixture, so submitting its unchanged size would freeze it.
    let height = harness.state().canvas.groups()[0].container.scaled_size().y;
    harness.get_by_label("H size").click();
    harness.step();
    harness.get_by_label("Group name").click();
    for _ in 0..4 {
        harness.step();
    }
    scenario.assertion(
        "unchanged size blur keeps its axis mode",
        !harness.state().canvas.groups()[0].layout.fixed[1]
            && harness.state().canvas.groups()[0].container.scaled_size().y == height,
    );
    for (field, draft, clicked) in [
        ("Group name", "Renamed", "Row"),
        ("Group name", "Faces", "Wrap"),
        ("Gap", "13", "Free"),
        ("W size", "700", "Clip"),
        ("Padding", "18.5", "Clip"),
    ] {
        if field == "W size" {
            harness.get_by_label("Wrap").click();
        }
        // The card relayouts a frame after a flow change.
        for _ in 0..4 {
            harness.step();
        }
        harness.get_by_label(field).click();
        harness.step();
        assert!(harness.get_by_label(field).is_focused(), "focus {field}");
        shortcut(&mut harness, CMD, egui::Key::A);
        harness.event(egui::Event::Text(draft.into()));
        harness.step();
        let pos = harness.get_by_label(clicked).rect().center();
        harness.event(egui::Event::PointerMoved(pos));
        for pressed in [true, false] {
            harness.event(egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            });
        }
        // Both pointer transitions reach one frame, alongside the field's blur.
        harness.step();
        let group = &harness.state().canvas.groups()[0];
        let committed = match field {
            "Group name" => group.name == draft,
            "Gap" => group.gap == 13.,
            "W size" => group.container.scaled_size().x == 700.,
            _ => group.padding == 18.5,
        };
        let clicked_applied = match clicked {
            "Row" => group.flow == crate::data::GroupFlow::Row,
            "Wrap" => group.flow == crate::data::GroupFlow::Wrap,
            "Free" => group.flow == crate::data::GroupFlow::Free,
            _ => group.layout.clip == (field == "W size"),
        };
        scenario.event(
            "same-frame blur and command",
            serde_json::json!({"field":field,"draft":draft,"clicked":clicked,"committed":committed,"clicked_applied":clicked_applied,"width":group.container.scaled_size().x,"clip":group.layout.clip}),
        );
        scenario.assertion(
            "blur commits before clicked command in either widget order",
            committed && clicked_applied,
        );
    }
    scenario.capture_screen("same-frame-submissions", &mut harness);
    harness.get_by_label("Between").click();
    for _ in 0..4 {
        harness.step();
    }
    let main = harness.state().canvas.groups()[0].layout.align[0];
    harness.get_by_label("Align bottom").click();
    for _ in 0..4 {
        harness.step();
    }
    scenario.assertion(
        "spread pad picks a cross-axis band and keeps the main axis",
        harness.state().canvas.groups()[0].layout.align == [main, 2]
            && harness.query_by_label("Align left, top").is_none(),
    );
    harness.event(egui::Event::PointerGone);
    for _ in 0..16 {
        harness.step();
    }
    scenario.capture_screen("spread-bands", &mut harness);
    harness.get_by_label("Column").click();
    for _ in 0..4 {
        harness.step();
    }
    scenario.assertion(
        "Column bands pick the horizontal cross axis",
        harness.query_by_label("Align right").is_some(),
    );
    scenario.capture_screen("column-bands", &mut harness);
    harness.get_by_label("Free").click();
    for _ in 0..4 {
        harness.step();
    }
    scenario.assertion(
        "Free dims gap, alignment and spread",
        harness.get_by_label("Gap").accesskit_node().is_disabled()
            && harness
                .get_by_label("Align top")
                .accesskit_node()
                .is_disabled()
            && harness
                .get_by_label("Evenly")
                .accesskit_node()
                .is_disabled(),
    );
    scenario.capture_screen("free-dimmed", &mut harness);
    harness.get_by_label("Wrap").click();
    for _ in 0..4 {
        harness.step();
    }
    scenario.assertion(
        "Wrap refuses width Hug",
        harness
            .get_by_label("W hug contents")
            .accesskit_node()
            .is_disabled(),
    );
    harness.get_by_label("Group name").click();
    for _ in 0..4 {
        harness.step();
    }
    assert!(harness.get_by_label("Group name").is_focused());
    harness.event(egui::Event::Text("draft".into()));
    for _ in 0..4 {
        harness.step();
    }
    shortcut(&mut harness, egui::Modifiers::NONE, egui::Key::Escape);
    scenario.assertion(
        "Esc discards name draft",
        harness.state().canvas.groups()[0].name == "Faces"
            && !harness.state().canvas.selected_group_ids().is_empty(),
    );
    let group = harness.state().canvas.groups()[0].id;
    harness.state_mut().canvas.select_placements(&ids);
    let selected = harness.state().canvas.selected_placement_ids();
    harness.state_mut().canvas.delete_placements(&selected);
    harness.state_mut().canvas.select_group(group);
    for _ in 0..4 {
        harness.step();
    }
    scenario.assertion(
        "empty group retains card",
        harness.state().canvas.groups()[0].members.is_empty()
            && harness.query_by_label("Ungroup").is_some(),
    );
    scenario.capture_screen("empty-group", &mut harness);
    let report = scenario.finish();
    assert!(report.passed, "{}", report.artifact_dir.display());
}

#[test]
fn inspect_group_dock_narrow_writes_artifact() {
    let mut scenario = InspectScenario::new("group-dock-narrow");
    let (_root, mut harness, _) = dock_fixture();
    harness.set_size(egui::vec2(640.0, 480.0));
    for _ in 0..16 {
        harness.step();
    }
    scenario.capture_screen("clamped-card", &mut harness);
    let canvas = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(640.0, 480.0)).shrink(16.0);
    for label in ["Group name", "Ungroup", "Align left, top", "Around", "Clip"] {
        let rect = harness.get_by_label(label).rect();
        scenario.assertion(label, canvas.contains_rect(rect));
    }
    harness.get_by_label("Ungroup").click();
    for _ in 0..8 {
        harness.step();
    }
    scenario.assertion(
        "clamped card accepts clicks",
        harness.state().canvas.groups().is_empty(),
    );
    let report = scenario.finish();
    assert!(report.passed, "{}", report.artifact_dir.display());
}

#[test]
fn inspect_member_dock_writes_artifact() {
    let mut scenario = InspectScenario::new("member-dock");
    let (_root, mut harness, ids) = dock_fixture();
    let mut group = harness.state().canvas.groups()[0].clone();
    group.layout.fixed[1] = true;
    harness.state_mut().canvas.edit_group(group).unwrap();
    let center = harness
        .state()
        .canvas
        .placement_for_id(ids[2])
        .unwrap()
        .state
        .geometry
        .center;
    inspect_click_at(
        &harness,
        inspect_world_to_screen(
            center,
            egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 800.0)),
        ),
        CMD,
    );
    for _ in 0..4 {
        for _ in 0..4 {
            harness.step();
        }
    }
    let card = harness
        .ctx
        .memory(|m| m.area_rect(egui::Id::new("group-card")))
        .unwrap();
    // The member card reads top to bottom from the group card's spot.
    let inventory = ["Faces", "Detach", "Fill width", "Fill height", "Absolute"];
    let mut previous = egui::pos2(f32::NEG_INFINITY, f32::NEG_INFINITY);
    for label in inventory {
        let rect = harness.get_by_label(label).rect();
        assert!(card.contains_rect(rect), "Inside card: {label}");
        assert!(
            rect.top() > previous.y || rect.left() > previous.x,
            "Control order: {label}"
        );
        previous = rect.left_top();
    }
    scenario.assertion(
        "member card replaces the group card in place",
        card.right() == 1264.0,
    );
    harness.get_by_label("Fill height").click();
    for _ in 0..4 {
        harness.step();
    }
    scenario.assertion(
        "member inventory replaces group inventory",
        harness.query_by_label("Ungroup").is_none(),
    );
    harness.event(egui::Event::PointerGone);
    for _ in 0..16 {
        harness.step();
    }
    scenario.capture_screen("member-dock", &mut harness);
    harness.get_by_label("Absolute").click();
    for _ in 0..4 {
        harness.step();
    }
    scenario.assertion(
        "Absolute retains and dims Fill",
        harness
            .get_by_label("Fill height")
            .accesskit_node()
            .is_disabled(),
    );
    scenario.capture_screen("absolute-dimmed", &mut harness);
    harness.get_by_label("Absolute").click();
    for _ in 0..4 {
        harness.step();
    }
    harness.get_by_label("Faces").click();
    for _ in 0..4 {
        harness.step();
    }
    scenario.assertion(
        "Group name returns to the group card",
        harness.query_by_label("Ungroup").is_some(),
    );
    harness.get_by_label("H hug contents").click();
    for _ in 0..4 {
        harness.step();
    }
    let center = harness
        .state()
        .canvas
        .placement_for_id(ids[2])
        .unwrap()
        .state
        .geometry
        .center;
    inspect_click_at(
        &harness,
        inspect_world_to_screen(
            center,
            egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 800.0)),
        ),
        CMD,
    );
    for _ in 0..4 {
        harness.step();
    }
    scenario.assertion(
        "Hug parent refuses member Fill",
        harness
            .get_by_label("Fill height")
            .accesskit_node()
            .is_disabled(),
    );
    harness.get_by_label("Detach").click();
    for _ in 0..4 {
        harness.step();
    }
    for _ in 0..20 {
        harness.step();
    }
    scenario.assertion(
        "Detach hides dock and retains loose selection",
        harness.query_by_label("Detach").is_none()
            && harness.state().canvas.selected_placement_ids() == [ids[2]],
    );
    let report = scenario.finish();
    assert!(report.passed, "{}", report.artifact_dir.display());
}

#[test]
/// Board activation flushes a scrub in progress instead of dropping it.
fn scrub_survives_board_switch() {
    let (_root, mut harness, _ids) = dock_fixture();
    let other = harness
        .state()
        .library
        .as_ref()
        .unwrap()
        .fixture_board("Second")
        .unwrap()
        .id;
    harness.step();
    let padding = harness.state().canvas.groups()[0].padding;
    let handle = harness.get_by_label("Padding").rect().left_center() - egui::vec2(13.0, 0.0);
    harness.event(egui::Event::PointerMoved(handle));
    harness.step();
    for (dx, pressed) in [(0.0, Some(true)), (15.0, None), (30.0, None)] {
        harness.event(egui::Event::PointerMoved(handle + egui::vec2(dx, 0.0)));
        if let Some(pressed) = pressed {
            harness.event(egui::Event::PointerButton {
                pos: handle + egui::vec2(dx, 0.0),
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            });
        }
        harness.step();
    }
    let edited = harness.state().canvas.groups()[0].padding;
    let first = harness.state().active_board_id;
    harness.state_mut().switch_to_board(other);
    harness.step();
    let second = harness.state().active_board_id;
    harness.event(egui::Event::PointerButton {
        pos: handle + egui::vec2(30.0, 0.0),
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    });
    harness.step();
    harness.state_mut().switch_to_board(first);
    harness.step();
    harness.step();
    let back = harness.state().canvas.groups()[0].padding;
    assert_ne!(first, second);
    assert_eq!(edited, padding + 30.0);
    assert_eq!(back, edited);
}

/// Captures the context menu over an empty board, a selected Group, and a
/// multi-image selection with one row hovered.
#[test]
fn inspect_context_menu_writes_artifact() {
    let mut scenario = InspectScenario::new("context-menu");
    let (_root, mut harness, ids) = dock_fixture();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 800.0));
    let group_id = harness.state().canvas.groups()[0].id;
    let at = |h: &Harness<'_, SnappyRefApp>, id: i64| {
        inspect_world_to_screen(
            h.state()
                .canvas
                .placement_for_id(id)
                .unwrap()
                .state
                .geometry
                .center,
            screen,
        )
    };
    let open = |h: &mut Harness<'_, SnappyRefApp>, pos: egui::Pos2| {
        h.state_mut().context_menu_pos = None;
        h.hover_at(pos);
        h.step();
        for pressed in [true, false] {
            h.event(egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Secondary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            });
            h.step();
        }
        for _ in 0..16 {
            h.step();
        }
    };

    harness.state_mut().canvas.select_content(&[], &[]);
    open(&mut harness, egui::pos2(180.0, 120.0));
    scenario.assertion(
        "menu opens on empty board",
        harness.state().context_menu_pos.is_some(),
    );
    scenario.capture_screen("empty", &mut harness);

    harness.state_mut().canvas.select_group(group_id);
    let pos = at(&harness, ids[0]) - egui::vec2(0.0, 60.0);
    open(&mut harness, pos);
    scenario.capture_screen("group", &mut harness);

    harness.state_mut().context_menu_pos = None;
    harness.state_mut().canvas.select_group(group_id);
    harness.state_mut().ungroup_selection();
    for _ in 0..4 {
        harness.step();
    }
    harness.state_mut().canvas.select_content(&ids[0..3], &[]);
    let pos = at(&harness, ids[1]) - egui::vec2(40.0, 40.0);
    open(&mut harness, pos);
    let row = harness.get_by_label("Duplicate").rect();
    harness.hover_at(row.center());
    for _ in 0..12 {
        harness.step();
    }
    scenario.assertion(
        "multi-image selection keeps all three images",
        harness.state().canvas.selected_placement_ids().len() == 3,
    );
    scenario.capture_screen("selection-hover", &mut harness);

    let report = scenario.finish();
    assert!(
        report.passed,
        "inspect scenario failed; artifact={}",
        report.artifact_dir.display()
    );
}
