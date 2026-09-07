use super::*;

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
    let mut app = test_app(Some(library.clone()));
    app.document_persistence =
        crate::active_board::persistence::DocumentPersistence::queued(library.clone());
    let mut harness = app_full_harness_with_step_dt(app, 1.0 / 60.0);
    let mut ids = Vec::new();
    scenario.event("seed", serde_json::json!({"placements": 3, "purpose": "Group shortcuts, movement, editing, picker and controls"}));
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
            .persist_imported_image(
                &path,
                "image/png",
                120,
                90,
                43200,
                egui::pos2(-180.0 + i as f32 * 180.0, 100.0),
                1.0,
            )
            .unwrap();
        ids.push(placement.id());
        let ctx = harness.ctx.clone();
        harness.state_mut().canvas.add_prepared_placement_image(
            &ctx,
            inspect_prepared_loaded_placement(
                placement,
                color,
                120,
                90,
                None,
                crate::jobs::ImportOrigin::Existing,
            ),
        );
    }
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
    let reorder_to = center(&harness, ids[1]) + egui::vec2(0.0, 60.0);
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
    let _ = harness.get_by_label("Find a group");
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
    let other_board = library.create_board("Other").unwrap();
    let mut app = test_app(Some(library.clone()));
    app.document_persistence =
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
            .persist_imported_image(
                &path,
                "image/png",
                40,
                30,
                4800,
                egui::pos2(i as f32 * 60.0, 0.0),
                1.0,
            )
            .unwrap();
        ids.push(placement.id());
        app.canvas.add_unresolved_placement(placement);
    }
    let record = |app: &mut SnappyRefApp| {
        app.record_pending_document_changes_history("Edit group");
        app.persist_recorded_canvas_changes();
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
    restored.document_persistence =
        crate::active_board::persistence::DocumentPersistence::queued(reopened.clone());
    restored.document_history = reopened.load_document_history().unwrap();
    let snapshot = reopened.load_board_document(1).unwrap();
    restored
        .open_board_document_snapshot(&ctx, &snapshot)
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
            restored.document_persistence.take_error()
        )
    });
    assert_eq!(durable.groups, deleted);
}
