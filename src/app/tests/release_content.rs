use super::*;

#[test]
fn group_copy_duplicate_delete_ungroup_and_paste_history_preserve_structure() {
    let root = TempRoot::new("snappyref-release-group-copy");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let other = library.fixture_board("B").unwrap();
    let ctx = egui::Context::default();
    let mut app = test_app(Some(library.clone()));
    app.board_editing.persistence =
        crate::active_board::persistence::DocumentPersistence::queued(library.clone());
    let mut ids = Vec::new();
    for i in 0..3 {
        let path = library
            .store_rgba_image(
                &format!("group-copy-{i}"),
                &[i as u8 * 60, 100, 200, 255],
                1,
                1,
            )
            .unwrap();
        let placement = Placement::loaded(
            PlacementState::new(
                library.allocate_document_id(),
                egui::Pos2::ZERO,
                1.,
                0.,
                0,
                false,
            ),
            library
                .register_original(&path, "image/png", 1, 1, 4, None)
                .unwrap(),
        );
        let mut item = loaded_record(placement);
        item.placement.state.geometry.center = egui::pos2(i as f32 * 180., 0.);
        item.placement.state.geometry.size = egui::vec2(100., 70.);
        item.placement = library.fixture_placement(1, item.placement).unwrap();
        ids.push(item.placement.id());
        app.canvas.add_prepared_placement_image(&ctx, item);
    }
    app.canvas.select_placements(&ids);
    app.group_selection();
    assert_eq!(app.canvas.groups()[0].flow, crate::data::GroupFlow::Wrap);
    let mut group = app.canvas.groups()[0].clone();
    group.name = "Faces".into();
    group.flow = crate::data::GroupFlow::Column;
    group.gap = 17.;
    group.padding = 23.;
    app.canvas.edit_group(group).unwrap();
    app.persist_canvas_changes();
    let original = app.canvas.groups()[0].clone();
    let payload = app.copy_selected_board_payload().unwrap();
    let original_geometry: Vec<_> = ids
        .iter()
        .map(|id| app.canvas.placement_for_id(*id).unwrap().state.geometry)
        .collect();
    app.duplicate_selection_with_gap();
    assert_eq!(app.canvas.groups().len(), 2);
    let duplicate = app.canvas.groups()[1].clone();
    assert!(duplicate.members.iter().all(|id| !ids.contains(id)));
    assert_eq!(duplicate.name, original.name);
    assert_eq!(duplicate.flow, original.flow);
    assert_eq!((duplicate.gap, duplicate.padding), (17., 23.));
    let mut edited = duplicate.clone();
    edited.name = "Independent".into();
    edited.flow = crate::data::GroupFlow::Row;
    app.canvas.edit_group(edited).unwrap();
    app.persist_canvas_changes();
    assert_eq!(app.canvas.groups()[0], original);
    assert_eq!(
        ids.iter()
            .map(|id| app.canvas.placement_for_id(*id).unwrap().state.geometry)
            .collect::<Vec<_>>(),
        original_geometry
    );
    app.canvas.select_group(duplicate.id);
    app.delete_selected_items_from_board(test_screen_rect());
    assert_eq!(app.canvas.groups(), std::slice::from_ref(&original));
    assert!(
        duplicate
            .members
            .iter()
            .all(|id| app.canvas.placement_for_id(*id).is_none())
    );
    app.undo_document_history(&ctx);
    assert_eq!(app.canvas.groups().len(), 2);
    assert!(
        duplicate
            .members
            .iter()
            .all(|id| app.canvas.placement_for_id(*id).is_some())
    );
    app.canvas.select_group(duplicate.id);
    let before_ungroup: Vec<_> = duplicate
        .members
        .iter()
        .map(|id| app.canvas.placement_for_id(*id).unwrap().state.geometry)
        .collect();
    app.ungroup_selection();
    assert_eq!(app.canvas.groups(), std::slice::from_ref(&original));
    assert_eq!(
        duplicate
            .members
            .iter()
            .map(|id| app.canvas.placement_for_id(*id).unwrap().state.geometry)
            .collect::<Vec<_>>(),
        before_ungroup
    );

    app.import_worker = Some(crate::image_preparation::ImagePrepWorker::new(
        library.clone(),
        ctx.clone(),
        None,
    ));
    app.switch_to_board(other.id);
    app.paste_board_payload_at(&payload, egui::pos2(0., 0.))
        .unwrap();
    let pasted = app.canvas.groups()[0].clone();
    assert_eq!(
        (
            pasted.name.as_str(),
            pasted.flow,
            pasted.gap,
            pasted.padding
        ),
        ("Faces", original.flow, 17., 23.)
    );
    for (index, id) in pasted.members.iter().enumerate() {
        let geometry = app.canvas.placement_for_id(*id).unwrap().state.geometry;
        assert_eq!(
            geometry.scaled_size(),
            original_geometry[index].scaled_size()
        );
        if index > 0 {
            assert_eq!(
                geometry.center
                    - app
                        .canvas
                        .placement_for_id(pasted.members[0])
                        .unwrap()
                        .state
                        .geometry
                        .center,
                original_geometry[index].center - original_geometry[0].center
            );
        }
    }
    app.paste_board_payload_at(&payload, egui::pos2(0., 0.))
        .unwrap();
    assert_eq!(app.canvas.groups().len(), 2);
    let second = app.canvas.groups()[1].clone();
    for new_id in &second.members {
        for old_id in &pasted.members {
            assert!(
                !app.canvas
                    .placement_for_id(*new_id)
                    .unwrap()
                    .visual_bounds()
                    .intersects(
                        app.canvas
                            .placement_for_id(*old_id)
                            .unwrap()
                            .visual_bounds()
                    )
            );
        }
    }

    app.undo_document_history(&ctx);
    assert_eq!(app.canvas.groups(), std::slice::from_ref(&pasted));
    app.undo_document_history(&ctx);
    assert!(app.canvas.groups().is_empty());
    assert!(app.canvas.all_placement_ids().is_empty());
    app.redo_document_history(&ctx);
    assert_eq!(app.canvas.groups(), &[pasted]);
    app.switch_to_board(1);
    assert_eq!(app.canvas.groups(), &[original]);
    assert_eq!(
        ids.iter()
            .map(|id| app.canvas.placement_for_id(*id).unwrap().state.geometry)
            .collect::<Vec<_>>(),
        original_geometry
    );
    app.canvas.select_content(&[], &[]);
    assert!(app.copy_selected_board_payload().is_none());
    let annotation = test_canvas_annotation(library.allocate_document_id(), egui::pos2(600., 0.));
    let annotation_id = annotation.id;
    app.canvas.add_annotation(annotation);
    app.canvas.select_content(&[], &[annotation_id]);
    app.group_selection();
    assert_eq!(app.canvas.groups().len(), 1);
}

#[test]
fn inspect_annotation_creation_editing_and_styles_writes_artifact() {
    let mut scenario = InspectScenario::new("annotation-editing");
    let root = TempRoot::new("snappyref-release-annotations");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    // Press/release frames must fit egui's double-click interval.
    let mut harness = app_full_harness_with_step_dt(test_app(Some(library)), 1. / 60.);
    let screen = test_screen_rect();
    let center = screen.center();
    // Each gesture gets its own empty region and checks the directed endpoints.
    for (key, delta, offset) in [
        (egui::Key::A, egui::vec2(90., 0.), egui::vec2(-300., -190.)),
        (egui::Key::A, egui::vec2(-90., 0.), egui::vec2(-80., -130.)),
        (egui::Key::A, egui::vec2(0., 70.), egui::vec2(60., -190.)),
        (egui::Key::A, egui::vec2(0., -70.), egui::vec2(180., -120.)),
        (egui::Key::L, egui::vec2(90., 30.), egui::vec2(250., -190.)),
    ] {
        let from = center + offset;
        harness.key_down(key);
        harness.step();
        drag(&mut harness, from, from + delta, egui::Modifiers::NONE);
        harness.key_up(key);
        harness.step();
        let id = *harness.state().canvas.all_annotation_ids().last().unwrap();
        let annotation = harness.state().canvas.annotation_for_id(id).unwrap();
        scenario.assertion(
            "draw shortcut creates the requested segment kind",
            (annotation.kind)
                == (if key == egui::Key::A {
                    CanvasAnnotationKind::Arrow
                } else {
                    CanvasAnnotationKind::Line
                }),
        );
        scenario.assertion(
            "segment endpoints follow the drag delta",
            (annotation.end() - annotation.start() - delta).length() < 1.,
        );
    }
    let segment_ids = harness.state().canvas.all_annotation_ids();
    let untouched = harness
        .state()
        .canvas
        .annotation_for_id(segment_ids[1])
        .unwrap();
    harness
        .state_mut()
        .canvas
        .select_content(&[], &segment_ids[..1]);
    harness.step();
    let selected = |harness: &Harness<'_, SnappyRefApp>| {
        harness
            .state()
            .canvas
            .annotation_for_id(segment_ids[0])
            .unwrap()
    };
    for (key, width) in [
        (egui::Key::Num1, CanvasStrokeWidth::S),
        (egui::Key::Num5, CanvasStrokeWidth::Xxl),
        (egui::Key::Num3, CanvasStrokeWidth::L),
    ] {
        shortcut(&mut harness, egui::Modifiers::SHIFT, key);
        scenario.assertion(
            "stroke control applies the chosen width",
            (selected(&harness).stroke_width) == (width),
        );
    }
    for (key, color) in [
        egui::Key::Num1,
        egui::Key::Num2,
        egui::Key::Num3,
        egui::Key::Num4,
        egui::Key::Num5,
    ]
    .into_iter()
    .zip(crate::data::CanvasAnnotationColor::ALL)
    {
        shortcut(&mut harness, egui::Modifiers::NONE, key);
        scenario.assertion(
            "color control applies the chosen color",
            (selected(&harness).color) == (color),
        );
    }
    for kind in [CanvasAnnotationKind::Line, CanvasAnnotationKind::Arrow] {
        shortcut(&mut harness, egui::Modifiers::NONE, egui::Key::H);
        scenario.assertion(
            "kind control applies the chosen segment kind",
            (selected(&harness).kind) == (kind),
        );
    }
    scenario.assertion(
        "style edits leave the other segment unchanged",
        (harness
            .state()
            .canvas
            .annotation_for_id(segment_ids[1])
            .unwrap())
            == (untouched),
    );
    for id in [segment_ids[0], segment_ids[4]] {
        harness.state_mut().canvas.select_content(&[], &[id]);
        harness.step();
        let before = harness.state().canvas.annotation_for_id(id).unwrap();
        let midpoint = inspect_world_to_screen(before.start().lerp(before.end(), 0.5), screen);
        drag(
            &mut harness,
            midpoint,
            midpoint + egui::vec2(0., 45.),
            egui::Modifiers::NONE,
        );
        let moved = harness.state().canvas.annotation_for_id(id).unwrap();
        scenario.assertion(
            "segment body drag moves its start by 45",
            (moved.start() - before.start() - egui::vec2(0., 45.)).length() < 1.,
        );
        scenario.assertion(
            "segment body drag moves its end by 45",
            (moved.end() - before.end() - egui::vec2(0., 45.)).length() < 1.,
        );
    }
    for key in [egui::Key::A, egui::Key::L] {
        harness.key_down(key);
        harness.step();
        inspect_click_at(
            &harness,
            center + egui::vec2(-300., -50.),
            egui::Modifiers::NONE,
        );
        harness.step();
        harness.key_up(key);
        harness.step();
    }
    scenario.assertion(
        "tiny A and L clicks create no segment",
        (harness.state().canvas.all_annotation_ids()) == (segment_ids),
    );
    scenario.assertion(
        "directed arrows and line create, tiny gestures leave no segment",
        segment_ids.len() == 5,
    );
    scenario.capture_screen("segments", &mut harness);
    let note_point = center + egui::vec2(-280., 40.);
    harness.hover_at(note_point);
    harness.step();
    shortcut(&mut harness, egui::Modifiers::NONE, egui::Key::T);
    harness.step();
    let text = "Grüezi 日本語\nsecond line gqwe alt";
    harness.event(egui::Event::Text(text.into()));
    harness.step();
    let note_id = *harness.state().canvas.all_annotation_ids().last().unwrap();
    scenario.assertion(
        "text edit stores the entered Unicode text",
        (harness
            .state()
            .canvas
            .annotation_for_id(note_id)
            .unwrap()
            .text)
            == (text),
    );
    // Style keys type text while the label is open; Escape leaves it selected.
    shortcut(&mut harness, egui::Modifiers::NONE, egui::Key::Escape);
    scenario.assertion(
        "Escape releases text editing",
        !harness.ctx.egui_wants_keyboard_input(),
    );
    for (key, size) in [
        (egui::Key::Num1, CanvasTextSize::S),
        (egui::Key::Num5, CanvasTextSize::Xxl),
        (egui::Key::Num3, CanvasTextSize::L),
    ] {
        shortcut(&mut harness, egui::Modifiers::SHIFT, key);
        scenario.assertion(
            "text size control applies the chosen size",
            (harness
                .state()
                .canvas
                .annotation_for_id(note_id)
                .unwrap()
                .text_size)
                == (size),
        );
    }
    shortcut(&mut harness, egui::Modifiers::NONE, egui::Key::Num2);
    scenario.assertion(
        "text color control applies coral",
        (harness
            .state()
            .canvas
            .annotation_for_id(note_id)
            .unwrap()
            .color)
            == (crate::data::CanvasAnnotationColor::Coral),
    );
    harness.hover_at(center + egui::vec2(350., 230.));
    shortcut(&mut harness, egui::Modifiers::NONE, egui::Key::Enter);
    harness.step();
    scenario.assertion(
        "Enter starts editing the selected label",
        harness.ctx.egui_wants_keyboard_input(),
    );
    shortcut(&mut harness, CMD, egui::Key::A);
    harness.event(egui::Event::Text("gqwe Åß\nreplacement".into()));
    harness.step();
    shortcut(&mut harness, egui::Modifiers::NONE, egui::Key::Backspace);
    scenario.assertion(
        "text shortcuts and Backspace edit the Unicode content",
        (harness
            .state()
            .canvas
            .annotation_for_id(note_id)
            .unwrap()
            .text)
            == ("gqwe Åß\nreplacemen"),
    );
    scenario.assertion(
        "typing shortcut letters creates no extra annotations",
        (harness.state().canvas.all_annotation_ids().len()) == (6),
    );
    scenario.assertion(
        "typing G creates no group",
        harness.state().canvas.groups().is_empty(),
    );
    inspect_click_at(
        &harness,
        center + egui::vec2(350., 230.),
        egui::Modifiers::NONE,
    );
    harness.step();
    scenario.assertion(
        "outside click releases text editing",
        !harness.ctx.egui_wants_keyboard_input(),
    );

    shortcut(&mut harness, CMD, egui::Key::Z);
    scenario.assertion(
        "click-away session undoes to its original text",
        harness
            .state()
            .canvas
            .annotation_for_id(note_id)
            .unwrap()
            .text
            == "Grüezi 日本語\nsecond line gqwe alt",
    );
    shortcut(
        &mut harness,
        egui::Modifiers { shift: true, ..CMD },
        egui::Key::Z,
    );
    scenario.assertion(
        "click-away session redoes its replacement and Backspace",
        harness
            .state()
            .canvas
            .annotation_for_id(note_id)
            .unwrap()
            .text
            == "gqwe Åß\nreplacemen",
    );
    scenario.capture_screen("text-edited", &mut harness);
    let from = center + egui::vec2(70., 100.);
    harness.key_down(egui::Key::S);
    harness.step();
    drag(
        &mut harness,
        from,
        from + egui::vec2(260., 70.),
        egui::Modifiers::NONE,
    );
    harness.key_up(egui::Key::S);
    harness.step();
    let id = *harness.state().canvas.all_annotation_ids().last().unwrap();
    scenario.assertion(
        "S drag creates a rectangle",
        (harness.state().canvas.annotation_for_id(id).unwrap().kind)
            == (CanvasAnnotationKind::Rectangle),
    );
    harness.state_mut().canvas.select_content(&[], &[id]);
    harness.step();
    let before = harness.state().canvas.annotation_for_id(id).unwrap();
    let corner = inspect_world_to_screen(before.end(), screen);
    drag(
        &mut harness,
        corner,
        corner + egui::vec2(60., 30.),
        egui::Modifiers::NONE,
    );
    let resized = harness.state().canvas.annotation_for_id(id).unwrap();
    scenario.assertion(
        "rectangle corner drag expands its bounds",
        resized.end().x > before.end().x + 40.,
    );
    let body = inspect_world_to_screen(resized.end() - egui::vec2(40., 20.), screen);
    drag(
        &mut harness,
        body,
        body + egui::vec2(-30., 50.),
        egui::Modifiers::NONE,
    );
    let moved = harness.state().canvas.annotation_for_id(id).unwrap();
    scenario.assertion(
        "rectangle body drag moves by the pointer delta",
        (moved.start() - resized.start() - egui::vec2(-30., 50.)).length() < 1.,
    );

    scenario.capture_screen("rectangle-moved", &mut harness);

    harness.state_mut().canvas.select_content(&[], &[note_id]);
    harness.step();
    let label = harness.state().canvas.annotation_for_id(note_id).unwrap();
    let body = inspect_world_to_screen(label.start().lerp(label.end(), 0.5), screen);
    drag(
        &mut harness,
        body,
        body + egui::vec2(-30., 50.),
        egui::Modifiers::NONE,
    );
    let moved = harness.state().canvas.annotation_for_id(note_id).unwrap();
    scenario.assertion(
        "label body drag moves by the pointer delta",
        (moved.start() - label.start() - egui::vec2(-30., 50.)).length() < 1.,
    );
    let text_point = inspect_world_to_screen(moved.start().lerp(moved.end(), 0.5), screen);
    for _ in 0..40 {
        harness.step();
    }
    harness.hover_at(text_point);
    for _ in 0..2 {
        for pressed in [true, false] {
            harness.event(egui::Event::PointerButton {
                pos: text_point,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            });
            harness.step();
        }
    }
    scenario.assertion(
        "double click edits the moved label",
        harness.ctx.egui_wants_keyboard_input(),
    );
    shortcut(&mut harness, CMD, egui::Key::A);
    harness.event(egui::Event::Text("edited after moving".into()));
    harness.step();
    scenario.assertion(
        "moved label accepts replacement text",
        (harness
            .state()
            .canvas
            .annotation_for_id(note_id)
            .unwrap()
            .text)
            == ("edited after moving"),
    );
    scenario.capture_screen("text-editing-after-move", &mut harness);
    shortcut(&mut harness, egui::Modifiers::NONE, egui::Key::Escape);
    scenario.assertion(
        "Escape releases editing of the moved label",
        !harness.ctx.egui_wants_keyboard_input(),
    );

    shortcut(&mut harness, CMD, egui::Key::Z);
    scenario.assertion(
        "Escape session undoes to its original text",
        harness
            .state()
            .canvas
            .annotation_for_id(note_id)
            .unwrap()
            .text
            == "gqwe Åß\nreplacemen",
    );
    shortcut(
        &mut harness,
        egui::Modifiers { shift: true, ..CMD },
        egui::Key::Z,
    );
    scenario.assertion(
        "Escape session redoes the moved label text",
        harness
            .state()
            .canvas
            .annotation_for_id(note_id)
            .unwrap()
            .text
            == "edited after moving",
    );
    scenario.finish();
}

#[test]
fn inspect_annotation_endpoint_drag_writes_artifact() {
    let mut scenario = InspectScenario::new("annotation-endpoint-drag");
    let root = TempRoot::new("snappyref-endpoint-drag");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let mut app = test_app(Some(library.clone()));
    let mut ids = Vec::new();
    for (kind, y) in [
        (CanvasAnnotationKind::Line, -120.),
        (CanvasAnnotationKind::Arrow, 120.),
    ] {
        let annotation = library
            .fixture_annotation(
                kind,
                egui::pos2(-180., y),
                egui::pos2(180., y),
                "",
                CanvasTextSize::M,
                CanvasStrokeWidth::M,
                crate::data::CanvasAnnotationColor::Default,
            )
            .unwrap();
        ids.push(annotation.id);
        app.canvas.add_annotation(annotation);
    }
    let mut harness = app_full_harness_with_step_dt(app, 1. / 60.);
    let screen = test_screen_rect();
    for id in ids {
        let annotation = harness.state().canvas.annotation_for_id(id).unwrap();
        inspect_click_at(
            &harness,
            inspect_world_to_screen(annotation.start().lerp(annotation.end(), 0.5), screen),
            egui::Modifiers::NONE,
        );
        harness.step();
        for start in [true, false] {
            let before = harness.state().canvas.annotation_for_id(id).unwrap();
            let point = if start { before.start() } else { before.end() };
            let from = inspect_world_to_screen(point, screen);
            let delta = egui::vec2(if start { -40. } else { 40. }, 30.);
            scenario.action("drag selected segment endpoint", from + delta);
            drag(&mut harness, from, from + delta, egui::Modifiers::NONE);
            let after = harness.state().canvas.annotation_for_id(id).unwrap();
            scenario.assertion(
                "endpoint drag preserves the opposite endpoint",
                (if start { after.end() } else { after.start() })
                    == (if start { before.end() } else { before.start() }),
            );
            scenario.assertion(
                "dragged endpoint follows the pointer delta",
                ((if start { after.start() } else { after.end() }) - point - delta).length() < 0.1,
            );
            shortcut(&mut harness, CMD, egui::Key::Z);
            scenario.assertion(
                "one Undo restores both endpoints",
                (harness.state().canvas.annotation_for_id(id).unwrap()) == (before),
            );
            shortcut(
                &mut harness,
                egui::Modifiers { shift: true, ..CMD },
                egui::Key::Z,
            );
            scenario.assertion(
                "Redo restores the edited endpoints",
                (harness.state().canvas.annotation_for_id(id).unwrap()) == (after),
            );
        }
    }

    scenario.capture_screen("edited-endpoints", &mut harness);
    scenario.finish();
}

#[test]
fn group_picker_arrows_remember_destination_and_cancel_typed_shortcuts() {
    let root = TempRoot::new("snappyref-picker-navigation");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let items: Vec<_> = (0..2)
        .map(|_| persisted_loaded_item(&library, library.allocate_document_id(), 1))
        .collect();
    let ids: Vec<_> = items.iter().map(|item| item.placement.id()).collect();
    let groups: Vec<_> = ["Faces", "Shapes"]
        .into_iter()
        .enumerate()
        .map(|(index, name)| crate::data::Group {
            id: library.allocate_document_id(),
            name: name.into(),
            members: vec![],
            container: crate::data::RectGeometry::from_bounds(
                egui::Rect::from_min_size(
                    egui::pos2(index as f32 * 400. - 400., -150.),
                    egui::vec2(250., 150.),
                ),
                0,
            ),
            flow: crate::data::GroupFlow::Wrap,
            gap: 12.,
            layout: crate::data::GroupLayoutSettings {
                row_gap: 12.0,
                ..Default::default()
            },
            padding: 16.,
        })
        .collect();
    let mut app = test_app(Some(library));
    let mut harness = app_full_harness_with_startup(1. / 60., move |ctx| {
        for item in items {
            app.canvas.add_prepared_placement_image(ctx, item);
        }
        app.canvas.restore_groups(groups);
        app
    });
    harness.state_mut().canvas.select_placements(&ids[..1]);
    shortcut(&mut harness, egui::Modifiers::NONE, egui::Key::G);
    harness.step();
    shortcut(&mut harness, egui::Modifiers::NONE, egui::Key::ArrowUp);
    shortcut(&mut harness, egui::Modifiers::NONE, egui::Key::Enter);
    assert_eq!(harness.state().canvas.groups()[1].members, ids[..1]);
    harness.state_mut().canvas.select_placements(&ids[1..]);
    shortcut(&mut harness, egui::Modifiers::NONE, egui::Key::G);
    harness.step();
    shortcut(&mut harness, egui::Modifiers::NONE, egui::Key::Enter);
    assert_eq!(harness.state().canvas.groups()[1].members, ids);
    harness.state_mut().canvas.select_placements(&ids[..1]);
    shortcut(&mut harness, egui::Modifiers::NONE, egui::Key::G);
    harness.step();
    shortcut(&mut harness, egui::Modifiers::NONE, egui::Key::ArrowDown);
    shortcut(&mut harness, egui::Modifiers::NONE, egui::Key::Enter);
    assert_eq!(harness.state().canvas.groups()[0].members, ids[..1]);
    assert_eq!(harness.state().canvas.groups()[1].members, ids[1..]);
    for cancel_button in [false, true] {
        let before = harness.state().canvas.groups().to_vec();
        shortcut(&mut harness, egui::Modifiers::NONE, egui::Key::G);
        harness.step();
        for (key, text) in [
            (egui::Key::G, "g"),
            (egui::Key::Q, "q"),
            (egui::Key::W, "w"),
            (egui::Key::E, "e"),
        ] {
            shortcut(&mut harness, egui::Modifiers::NONE, key);
            harness.event(egui::Event::Text(text.into()));
            harness.step();
        }
        assert_eq!(
            harness
                .get_all_by_role(egui::accesskit::Role::TextInput)
                .into_iter()
                .find(|node| node.is_focused())
                .unwrap()
                .value()
                .as_deref(),
            Some("gqwe")
        );
        if cancel_button {
            harness.get_by_label("Cancel").click();
            harness.step();
        } else {
            shortcut(&mut harness, egui::Modifiers::NONE, egui::Key::Escape);
        }
        assert_eq!(harness.state().canvas.groups(), before);
        assert_eq!(harness.state().canvas.all_placement_ids().len(), 2);
    }
}

#[test]
fn inspect_annotation_text_sessions_writes_artifact() {
    fn reopen(path: PathBuf) -> Harness<'static, SnappyRefApp> {
        Harness::builder()
            .with_size(egui::vec2(1200., 800.))
            .with_pixels_per_point(1.)
            .with_os(egui::os::OperatingSystem::Nix)
            .with_theme(egui::Theme::Dark)
            .with_max_steps(4)
            .with_step_dt(1. / 60.)
            .build_eframe(move |cc| {
                crate::ui::theme::configure_theme(&cc.egui_ctx);
                let config = AppConfig {
                    library_root: Ok(path),
                    no_startup_focus: true,
                    perf_log: false,
                    perf_decode_path: None,
                    perf_scenario: None,
                    perf_corpus: None,
                    perf_run_dir: None,
                    update_smoke_install: false,
                    update_smoke_status_path: None,
                };
                let startup = config.prepare_startup();
                let mut app = SnappyRefApp::new(cc, config, startup);
                app.update_service =
                    crate::updater::UpdateService::new(crate::updater::UpdateConfig::for_tests());
                app
            })
    }
    let mut scenario = InspectScenario::new("annotation-text-sessions");
    let root = TempRoot::new("snappyref-text-sessions");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let annotation = library
        .fixture_annotation(
            CanvasAnnotationKind::Textbox,
            egui::pos2(-180., -40.),
            egui::pos2(20., 40.),
            "before",
            CanvasTextSize::L,
            CanvasStrokeWidth::M,
            crate::data::CanvasAnnotationColor::Default,
        )
        .unwrap();
    let id = annotation.id;
    let mut app = test_app(Some(library.clone()));
    app.canvas.add_annotation(annotation);
    app.settings.semantic_search_first_launch_prompt_seen = true;
    let mut harness = app_full_harness_with_step_dt(app, 1. / 60.);
    harness.hover_at(test_screen_rect().center() + egui::vec2(300., 200.));
    harness.step();
    scenario.capture_screen("before-edit", &mut harness);
    shortcut(&mut harness, egui::Modifiers::NONE, egui::Key::Num2);
    shortcut(&mut harness, egui::Modifiers::NONE, egui::Key::Enter);
    harness.step();
    shortcut(&mut harness, CMD, egui::Key::A);
    for character in ["h", "e", "l", "l", "o"] {
        harness.event(egui::Event::Text(character.into()));
        harness.step();
    }
    scenario.capture_screen("hello-editing", &mut harness);
    shortcut(&mut harness, egui::Modifiers::NONE, egui::Key::Enter);
    scenario.assertion(
        "plain Enter commits hello",
        !harness.ctx.egui_wants_keyboard_input()
            && harness.state().canvas.annotation_for_id(id).unwrap().text == "hello",
    );
    shortcut(&mut harness, CMD, egui::Key::Z);
    let undone = harness.state().canvas.annotation_for_id(id).unwrap().text;
    scenario.fact("one-undo-text", serde_json::json!(undone));
    scenario.assertion(
        "one Undo restores before after five typing frames",
        undone == "before",
    );
    scenario.capture_screen("one-undo", &mut harness);
    shortcut(
        &mut harness,
        egui::Modifiers { shift: true, ..CMD },
        egui::Key::Z,
    );
    scenario.assertion(
        "Redo restores hello",
        harness.state().canvas.annotation_for_id(id).unwrap().text == "hello",
    );
    harness.state_mut().persist_canvas_changes();
    harness.state_mut().flush_persistence();
    let mut restored = reopen(root.to_path_buf());
    restored.hover_at(test_screen_rect().center() + egui::vec2(300., 200.));
    restored.step();
    shortcut(&mut restored, CMD, egui::Key::Z);
    scenario.assertion(
        "reopened history undoes the whole session to before",
        restored.state().canvas.annotation_for_id(id).unwrap().text == "before",
    );
    shortcut(
        &mut restored,
        egui::Modifiers { shift: true, ..CMD },
        egui::Key::Z,
    );
    scenario.assertion(
        "reopened history redoes hello",
        restored.state().canvas.annotation_for_id(id).unwrap().text == "hello",
    );

    restored.state_mut().flush_persistence();
    drop(restored);
    scenario.action(
        "type two lines with Shift Enter",
        test_screen_rect().center(),
    );
    shortcut(&mut harness, egui::Modifiers::NONE, egui::Key::Enter);
    harness.step();
    shortcut(&mut harness, CMD, egui::Key::A);
    harness.event(egui::Event::Text("first".into()));
    harness.step();
    shortcut(&mut harness, egui::Modifiers::SHIFT, egui::Key::Enter);
    harness.event(egui::Event::Text("second".into()));
    harness.step();
    harness.step();
    scenario.capture_screen("two-lines-editing", &mut harness);
    scenario.assertion(
        "Shift Enter inserts a newline and keeps editing",
        harness.ctx.egui_wants_keyboard_input()
            && harness.state().canvas.annotation_for_id(id).unwrap().text == "first\nsecond",
    );
    shortcut(&mut harness, egui::Modifiers::NONE, egui::Key::Enter);
    scenario.assertion(
        "Enter commits the two lines",
        !harness.ctx.egui_wants_keyboard_input()
            && harness.state().canvas.annotation_for_id(id).unwrap().text == "first\nsecond",
    );
    harness.state_mut().persist_canvas_changes();
    harness.state_mut().flush_persistence();
    let stored = library
        .load_canvas_annotations()
        .unwrap()
        .into_iter()
        .find(|a| a.id == id)
        .unwrap();
    scenario.fact("stored-multiline-text", serde_json::json!(stored.text));
    scenario.assertion(
        "the stored text contains the typed newline",
        stored.text == "first\nsecond",
    );
    scenario.capture_screen("two-lines-committed", &mut harness);
    shortcut(&mut harness, CMD, egui::Key::Z);
    scenario.assertion(
        "one Undo of the second session restores hello",
        harness.state().canvas.annotation_for_id(id).unwrap().text == "hello",
    );
    // Exit is an explicit submission even when the editor still has focus.
    shortcut(&mut harness, egui::Modifiers::NONE, egui::Key::Enter);
    harness.step();
    shortcut(&mut harness, CMD, egui::Key::A);
    for text in ["saved", " on exit"] {
        harness.event(egui::Event::Text(text.into()));
        harness.step();
    }
    scenario.capture_screen("uncommitted-at-exit", &mut harness);
    eframe::App::on_exit(harness.state_mut());
    let reopened = Library::initialize(root.to_path_buf()).unwrap();
    let stored = reopened
        .load_canvas_annotations()
        .unwrap()
        .into_iter()
        .find(|a| a.id == id)
        .unwrap();
    scenario.assertion(
        "exit saves the unfinished typing session",
        stored.text == "saved on exit",
    );
    let mut restored = reopen(root.to_path_buf());
    restored.hover_at(test_screen_rect().center() + egui::vec2(300., 200.));
    restored.step();
    shortcut(&mut restored, CMD, egui::Key::Z);
    scenario.assertion(
        "exit stores one Undo step back to hello",
        restored.state().canvas.annotation_for_id(id).unwrap().text == "hello",
    );
    shortcut(&mut restored, egui::Modifiers::NONE, egui::Key::Num1);
    shortcut(&mut restored, egui::Modifiers::NONE, egui::Key::Num2);
    shortcut(&mut restored, egui::Modifiers::NONE, egui::Key::Enter);
    restored.step();
    shortcut(&mut restored, CMD, egui::Key::A);
    for text in ["first", "second"] {
        restored.event(egui::Event::Text(text.into()));
        restored.step();
    }
    shortcut(&mut restored, CMD, egui::Key::Z);
    scenario.assertion(
        "Undo while typing preserves the previous Coral style edit",
        restored.state().canvas.annotation_for_id(id).unwrap().color
            == crate::data::CanvasAnnotationColor::Coral,
    );
    scenario.assertion(
        "the focused editor undoes its text",
        restored.state().canvas.annotation_for_id(id).unwrap().text == "hello",
    );
    shortcut(
        &mut restored,
        egui::Modifiers { shift: true, ..CMD },
        egui::Key::Z,
    );
    scenario.assertion(
        "the focused editor redoes its text",
        restored.state().canvas.annotation_for_id(id).unwrap().text == "firstsecond",
    );
    shortcut(&mut restored, egui::Modifiers::NONE, egui::Key::Escape);
    shortcut(&mut restored, CMD, egui::Key::Z);
    scenario.assertion(
        "committed focused-editor changes undo in one step",
        restored.state().canvas.annotation_for_id(id).unwrap().text == "hello",
    );
    // A board Redo must not overwrite a newly typed draft.
    shortcut(&mut restored, egui::Modifiers::NONE, egui::Key::Num1);
    shortcut(&mut restored, egui::Modifiers::NONE, egui::Key::Num2);
    shortcut(&mut restored, CMD, egui::Key::Z);
    shortcut(&mut restored, egui::Modifiers::NONE, egui::Key::Enter);
    restored.step();
    shortcut(&mut restored, CMD, egui::Key::A);
    for text in ["first", "second"] {
        restored.event(egui::Event::Text(text.into()));
        restored.step();
    }
    shortcut(
        &mut restored,
        egui::Modifiers { shift: true, ..CMD },
        egui::Key::Z,
    );
    let draft = restored.state().canvas.annotation_for_id(id).unwrap();
    scenario.assertion(
        "Redo while typing preserves the new draft and Default style",
        draft.text == "firstsecond" && draft.color == crate::data::CanvasAnnotationColor::Default,
    );
    shortcut(&mut restored, egui::Modifiers::NONE, egui::Key::Escape);
    shortcut(&mut restored, CMD, egui::Key::Z);
    scenario.assertion(
        "the draft committed after stale Redo undoes to hello",
        restored.state().canvas.annotation_for_id(id).unwrap().text == "hello",
    );
    shortcut(&mut restored, egui::Modifiers::NONE, egui::Key::Enter);
    restored.step();
    shortcut(&mut restored, CMD, egui::Key::A);
    shortcut(&mut restored, egui::Modifiers::NONE, egui::Key::Backspace);
    shortcut(&mut restored, egui::Modifiers::NONE, egui::Key::Enter);
    scenario.assertion(
        "an empty committed label is removed",
        restored.state().canvas.annotation_for_id(id).is_none(),
    );
    shortcut(&mut restored, CMD, egui::Key::Z);
    scenario.assertion(
        "one Undo restores the label cleared during editing",
        restored
            .state()
            .canvas
            .annotation_for_id(id)
            .is_some_and(|a| a.text == "hello"),
    );
    scenario.capture_screen("undo-cleared-label", &mut restored);
    scenario.finish();
}
