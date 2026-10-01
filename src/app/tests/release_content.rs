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
        assert_eq!(
            annotation.kind,
            if key == egui::Key::A {
                CanvasAnnotationKind::Arrow
            } else {
                CanvasAnnotationKind::Line
            }
        );
        assert!((annotation.end() - annotation.start() - delta).length() < 1.);
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
    for (label, width) in [
        ("S", CanvasStrokeWidth::S),
        ("M", CanvasStrokeWidth::M),
        ("L", CanvasStrokeWidth::L),
    ] {
        harness.get_by_label(label).click();
        harness.step();
        assert_eq!(
            harness
                .state()
                .canvas
                .annotation_for_id(segment_ids[0])
                .unwrap()
                .stroke_width,
            width
        );
    }
    for color in [
        crate::data::CanvasAnnotationColor::Default,
        crate::data::CanvasAnnotationColor::Coral,
        crate::data::CanvasAnnotationColor::Gold,
        crate::data::CanvasAnnotationColor::Mint,
        crate::data::CanvasAnnotationColor::Sky,
    ] {
        harness
            .state_mut()
            .canvas
            .apply_color_to_selected_annotation(color);
        assert_eq!(
            harness
                .state()
                .canvas
                .annotation_for_id(segment_ids[0])
                .unwrap()
                .color,
            color
        );
    }
    assert_eq!(
        harness
            .state()
            .canvas
            .annotation_for_id(segment_ids[1])
            .unwrap(),
        untouched
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
        assert!((moved.start() - before.start() - egui::vec2(0., 45.)).length() < 1.);
        assert!((moved.end() - before.end() - egui::vec2(0., 45.)).length() < 1.);
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
    assert_eq!(harness.state().canvas.all_annotation_ids(), segment_ids);
    scenario.assertion(
        "directed arrows and line create, tiny gestures leave no segment",
        segment_ids.len() == 5,
    );
    scenario.capture_screen("segments", &mut harness);
    let note_point = center + egui::vec2(-280., 40.);
    harness.key_down(egui::Key::T);
    harness.step();
    inspect_click_at(&harness, note_point, egui::Modifiers::NONE);
    harness.step();
    harness.key_up(egui::Key::T);
    harness.step();
    harness.step();
    let text = "Grüezi 日本語\nsecond line gqwe alt";
    harness.event(egui::Event::Text(text.into()));
    harness.step();
    let note_id = *harness.state().canvas.all_annotation_ids().last().unwrap();
    assert_eq!(
        harness
            .state()
            .canvas
            .annotation_for_id(note_id)
            .unwrap()
            .text,
        text
    );
    for (label, size) in [
        ("S", CanvasTextSize::S),
        ("M", CanvasTextSize::M),
        ("L", CanvasTextSize::L),
        ("XL", CanvasTextSize::Xl),
    ] {
        harness.get_by_label(label).click();
        harness.step();
        assert_eq!(
            harness
                .state()
                .canvas
                .annotation_for_id(note_id)
                .unwrap()
                .text_size,
            size
        );
    }
    let before_bg = harness
        .state()
        .canvas
        .annotation_for_id(note_id)
        .unwrap()
        .background_enabled;
    for color in [
        crate::data::CanvasAnnotationColor::Default,
        crate::data::CanvasAnnotationColor::Coral,
        crate::data::CanvasAnnotationColor::Gold,
        crate::data::CanvasAnnotationColor::Mint,
        crate::data::CanvasAnnotationColor::Sky,
    ] {
        harness
            .state_mut()
            .canvas
            .apply_color_to_editing_textbox(color);
        assert_eq!(
            harness
                .state()
                .canvas
                .annotation_for_id(note_id)
                .unwrap()
                .color,
            color
        );
    }
    harness.get_by_label("BG").click();
    harness.step();
    assert_eq!(
        harness
            .state()
            .canvas
            .annotation_for_id(note_id)
            .unwrap()
            .background_enabled,
        !before_bg
    );
    shortcut(&mut harness, egui::Modifiers::NONE, egui::Key::Escape);
    assert!(!harness.ctx.egui_wants_keyboard_input());
    harness.state_mut().canvas.select_content(&[], &[note_id]);
    shortcut(&mut harness, egui::Modifiers::NONE, egui::Key::Enter);
    harness.step();
    assert!(harness.ctx.egui_wants_keyboard_input());
    shortcut(&mut harness, CMD, egui::Key::A);
    harness.event(egui::Event::Text("gqwe Åß\nreplacement".into()));
    harness.step();
    shortcut(&mut harness, egui::Modifiers::NONE, egui::Key::Backspace);
    assert_eq!(
        harness
            .state()
            .canvas
            .annotation_for_id(note_id)
            .unwrap()
            .text,
        "gqwe Åß\nreplacemen"
    );
    assert_eq!(harness.state().canvas.all_annotation_ids().len(), 6);
    assert!(harness.state().canvas.groups().is_empty());
    inspect_click_at(
        &harness,
        center + egui::vec2(350., 230.),
        egui::Modifiers::NONE,
    );
    harness.step();
    assert!(!harness.ctx.egui_wants_keyboard_input());
    scenario.assertion(
        "multiline Unicode, size, BG, Enter, Escape and text Backspace stay editor owned",
        true,
    );
    scenario.capture_screen("text-edited", &mut harness);
    // Drag creation uses the same accepted textbox path, with a different default.
    harness.state_mut().settings.default_textbox_background = true;
    let from = center + egui::vec2(70., 100.);
    harness.key_down(egui::Key::T);
    harness.step();
    drag(
        &mut harness,
        from,
        from + egui::vec2(260., 70.),
        egui::Modifiers::NONE,
    );
    harness.key_up(egui::Key::T);
    harness.step();
    harness.step();
    harness.event(egui::Event::Text("drag created".into()));
    harness.step();
    shortcut(&mut harness, egui::Modifiers::NONE, egui::Key::Escape);
    let id = *harness.state().canvas.all_annotation_ids().last().unwrap();
    assert_ne!(id, note_id);
    assert!(
        harness
            .state()
            .canvas
            .annotation_for_id(id)
            .unwrap()
            .background_enabled
    );
    assert_eq!(
        harness.state().canvas.annotation_for_id(id).unwrap().text,
        "drag created"
    );
    assert_eq!(
        harness
            .state()
            .canvas
            .annotation_for_id(note_id)
            .unwrap()
            .background_enabled,
        !before_bg
    );
    scenario.assertion(
        "drag creates text using current background default without changing old text",
        true,
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
    assert!(resized.end().x > before.end().x + 40.);
    assert_eq!(resized.text, before.text);
    let body = inspect_world_to_screen(resized.end() - egui::vec2(40., 20.), screen);
    drag(
        &mut harness,
        body,
        body + egui::vec2(-30., 50.),
        egui::Modifiers::NONE,
    );
    let moved = harness.state().canvas.annotation_for_id(id).unwrap();
    assert!((moved.start() - resized.start() - egui::vec2(-30., 50.)).length() < 1.);
    assert_eq!(moved.text, before.text);
    let text_point = inspect_world_to_screen(moved.start() + egui::vec2(20., 20.), screen);
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
    assert!(harness.ctx.egui_wants_keyboard_input());
    shortcut(&mut harness, CMD, egui::Key::A);
    harness.event(egui::Event::Text("edited after moving".into()));
    harness.step();
    assert_eq!(
        harness.state().canvas.annotation_for_id(id).unwrap().text,
        "edited after moving"
    );
    scenario.capture_screen("text-editing-after-move", &mut harness);
    shortcut(&mut harness, egui::Modifiers::NONE, egui::Key::Escape);
    assert!(!harness.ctx.egui_wants_keyboard_input());
    scenario.assertion(
        "textbox corner resize and body move preserve text, double-click edits it, Escape commits",
        true,
    );

    scenario.capture_screen("text-drag-created", &mut harness);
    assert!(scenario.finish().passed);
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
                false,
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
            assert_eq!(
                if start { after.end() } else { after.start() },
                if start { before.end() } else { before.start() }
            );
            assert!(
                ((if start { after.start() } else { after.end() }) - point - delta).length() < 0.1
            );
            shortcut(&mut harness, CMD, egui::Key::Z);
            assert_eq!(
                harness.state().canvas.annotation_for_id(id).unwrap(),
                before
            );
            shortcut(
                &mut harness,
                egui::Modifiers { shift: true, ..CMD },
                egui::Key::Z,
            );
            assert_eq!(harness.state().canvas.annotation_for_id(id).unwrap(), after);
        }
    }
    scenario.assertion(
        "line and arrow endpoints move independently with one undoable edit per drag",
        true,
    );
    scenario.capture_screen("edited-endpoints", &mut harness);
    assert!(scenario.finish().passed);
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
