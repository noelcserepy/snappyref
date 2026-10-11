use super::*;

/// Labels, arrows, and rectangles on a board of real references, drawn the
/// way a user would: T and type, A-drag from a label, S-drag a region,
/// and number keys for color.
#[test]
fn inspect_annotation_board_writes_artifact() {
    let mut scenario = InspectScenario::new("annotation-board");
    let root = TempRoot::new("snappyref-inspect-annotation-board");
    let (mut harness, bounds) = readme_board(&root);
    let screen = test_screen_rect();
    // World and screen share a scale at the opening zoom.
    let at = |name: &str, u: f32, v: f32| {
        let rect = bounds[name];
        inspect_world_to_screen(
            rect.min + egui::vec2(rect.width() * u, rect.height() * v),
            screen,
        )
    };
    let center_of = |harness: &Harness<'_, SnappyRefApp>, id: i64| {
        let annotation = harness.state().canvas.annotation_for_id(id).unwrap();
        inspect_world_to_screen(annotation.start().lerp(annotation.end(), 0.5), screen)
    };

    // Matterhorn: a coral label with an arrow from its edge to the summit.
    let ridge = place_label(
        &mut harness,
        at("matterhorn", 0.08, 0.62),
        "North face light",
    );
    commit(&mut harness);
    color(&mut harness, 2);
    let from = center_of(&harness, ridge);
    hold_drag(
        &mut harness,
        egui::Key::A,
        from,
        at("matterhorn", 0.52, 0.22),
    );
    let arrow = last_annotation(&harness);
    let empty = screen.right_bottom() - egui::vec2(30., 30.);

    // Great Wave: a sky rectangle on the crest with its own label.
    hold_drag(
        &mut harness,
        egui::Key::S,
        at("great-wave", 0.04, 0.12),
        at("great-wave", 0.42, 0.58),
    );
    let crest = last_annotation(&harness);
    color(&mut harness, 5);
    place_label(&mut harness, at("great-wave", 0.05, 0.64), "Claw-like foam");
    commit(&mut harness);
    place_label(
        &mut harness,
        at("great-wave", 0.60, 0.86),
        "ref: Hokusai, c. 1831",
    );
    commit(&mut harness);
    color(&mut harness, 1);

    // Tetons: a gold region on the river bend, called out from a label.
    hold_drag(
        &mut harness,
        egui::Key::S,
        at("tetons-snake-river", 0.30, 0.55),
        at("tetons-snake-river", 0.78, 0.86),
    );
    color(&mut harness, 3);
    let bend = place_label(
        &mut harness,
        at("tetons-snake-river", 0.06, 0.08),
        "S-curve leads in",
    );
    commit(&mut harness);
    let from = center_of(&harness, bend);
    hold_drag(
        &mut harness,
        egui::Key::A,
        from,
        at("tetons-snake-river", 0.42, 0.6),
    );

    // Earthrise: a mint label with an arrow into the frame.
    let palette = place_label(&mut harness, at("earthrise", 0.52, 0.08), "Palette");
    commit(&mut harness);
    color(&mut harness, 4);
    let from = center_of(&harness, palette);
    hold_drag(&mut harness, egui::Key::A, from, at("earthrise", 0.4, 0.42));

    let count = harness.state().canvas.all_annotation_ids().len();
    scenario.fact("annotations", serde_json::json!(count));
    scenario.assertion("ten annotations drawn", count == 10);
    settle_at(&mut harness, Some(empty), empty);
    scenario.capture_screen("resting", &mut harness);

    let arrow_point = {
        let a = harness.state().canvas.annotation_for_id(arrow).unwrap();
        inspect_world_to_screen(a.start().lerp(a.end(), 0.6), screen)
    };
    settle_at(&mut harness, Some(arrow_point), empty);
    scenario.assertion(
        "a click on the arrow selects it",
        harness.state().canvas.selected_annotation_ids() == vec![arrow],
    );
    scenario.capture_screen("selected-arrow", &mut harness);

    let crest_edge = {
        let a = harness.state().canvas.annotation_for_id(crest).unwrap();
        inspect_world_to_screen(
            egui::pos2(a.start().x, (a.start().y + a.end().y) / 2.),
            screen,
        )
    };
    settle_at(&mut harness, Some(crest_edge), empty);
    scenario.assertion(
        "a click on the rectangle edge selects it",
        harness.state().canvas.selected_annotation_ids() == vec![crest],
    );
    scenario.capture_screen("selected-rectangle", &mut harness);

    let ridge_center = center_of(&harness, ridge);
    // Wait out the last click so the next two read as a double-click.
    settle_at(&mut harness, None, ridge_center);
    for _ in 0..2 {
        for pressed in [true, false] {
            harness.event(egui::Event::PointerButton {
                pos: ridge_center,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            });
            harness.step();
        }
    }
    shortcut(&mut harness, egui::Modifiers::NONE, egui::Key::End);
    harness.event(egui::Event::Text(" at dusk".into()));
    settle_at(&mut harness, None, empty);
    scenario.assertion(
        "double-click edits the label in place",
        harness.ctx.egui_wants_keyboard_input()
            && harness
                .state()
                .canvas
                .annotation_for_id(ridge)
                .unwrap()
                .text
                == "North face light at dusk",
    );
    scenario.capture_screen("editing-label", &mut harness);
    commit(&mut harness);

    inspect_click_at(&harness, empty, egui::Modifiers::NONE);
    harness.hover_at(screen.center());
    for _ in 0..3 {
        harness.event(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Line,
            delta: egui::vec2(0.0, -1.0),
            phase: egui::TouchPhase::Move,
            modifiers: egui::Modifiers::NONE,
        });
        harness.step();
    }
    settle_at(&mut harness, None, empty);
    let zoom = harness.state().canvas.zoom;
    scenario.fact("zoomed_out_zoom", serde_json::json!(zoom));
    scenario.assertion("the wheel zooms out", zoom < 0.7);
    scenario.capture_screen("zoomed-out", &mut harness);
    let report = scenario.finish();
    assert!(report.passed, "{}", report.artifact_dir.display());
}

fn last_annotation(harness: &Harness<'_, SnappyRefApp>) -> i64 {
    *harness.state().canvas.all_annotation_ids().last().unwrap()
}

fn hold_drag(
    harness: &mut Harness<'_, SnappyRefApp>,
    key: egui::Key,
    from: egui::Pos2,
    to: egui::Pos2,
) {
    harness.key_down(key);
    harness.step();
    drag(harness, from, to, egui::Modifiers::NONE);
    harness.key_up(key);
    harness.step();
}

/// T over `point`, then type. The label is editing until [`commit`]. A real
/// T press also sends its letter, which must not land in the label.
fn place_label(harness: &mut Harness<'_, SnappyRefApp>, point: egui::Pos2, text: &str) -> i64 {
    harness.hover_at(point);
    harness.step();
    harness.key_down(egui::Key::T);
    harness.event(egui::Event::Text("t".into()));
    harness.key_up(egui::Key::T);
    harness.step();
    harness.step();
    harness.event(egui::Event::Text(text.into()));
    harness.step();
    last_annotation(harness)
}

/// Presses color key 1 to 5 for the selection, or for the next annotation.
pub(super) fn color(harness: &mut Harness<'_, SnappyRefApp>, digit: usize) {
    let key = [
        egui::Key::Num1,
        egui::Key::Num2,
        egui::Key::Num3,
        egui::Key::Num4,
        egui::Key::Num5,
    ][digit - 1];
    shortcut(harness, egui::Modifiers::NONE, key);
}

fn commit(harness: &mut Harness<'_, SnappyRefApp>) {
    shortcut(harness, egui::Modifiers::NONE, egui::Key::Enter);
    harness.step();
}

/// Optionally clicks, then parks the pointer and lets motion settle.
fn settle_at(harness: &mut Harness<'_, SnappyRefApp>, click: Option<egui::Pos2>, park: egui::Pos2) {
    if let Some(point) = click {
        inspect_click_at(harness, point, egui::Modifiers::NONE);
        harness.step();
    }
    harness.hover_at(park);
    for _ in 0..20 {
        harness.step();
    }
}

/// Six readme-board references in two rows, at the opening zoom. Returns each
/// image's world rect by name.
fn readme_board(
    root: &TempRoot,
) -> (
    Harness<'static, SnappyRefApp>,
    std::collections::HashMap<&'static str, egui::Rect>,
) {
    const HEIGHT: f32 = 330.;
    const GAP: f32 = 12.;
    let fixtures =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/images/readme-board");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let rows: [(&[&'static str], f32); 2] = [
        (&["matterhorn", "great-wave", "woman-water-pitcher"], -342.),
        (&["tetons-snake-river", "earthrise", "hall-of-justice"], 0.),
    ];
    let mut items = Vec::new();
    let mut bounds = std::collections::HashMap::new();
    for (row, top) in rows {
        let row_items = row
            .iter()
            .map(|name| {
                let path = fixtures.join(format!("{name}.jpg"));
                crate::image_preparation::import_path_sync(&library, path, egui::Pos2::ZERO)
                    .unwrap_or_else(|error| panic!("{name}: {error}"))
            })
            .collect::<Vec<_>>();
        let widths = row_items
            .iter()
            .map(|item| {
                let size = item.placement.state.geometry.size;
                HEIGHT * size.x / size.y
            })
            .collect::<Vec<_>>();
        let mut left = -(widths.iter().sum::<f32>() + GAP * (row.len() - 1) as f32) / 2.;
        for ((mut item, width), name) in row_items.into_iter().zip(widths).zip(row.iter()) {
            let geometry = &mut item.placement.state.geometry;
            geometry.scale = HEIGHT / geometry.size.y;
            geometry.center = egui::pos2(left + width / 2., top + HEIGHT / 2.);
            bounds.insert(
                *name,
                egui::Rect::from_min_size(egui::pos2(left, top), egui::vec2(width, HEIGHT)),
            );
            left += width + GAP;
            item.placement = library.fixture_placement(1, item.placement).unwrap();
            items.push(item);
        }
    }
    let mut app = test_app(Some(library));
    let mut harness = app_full_harness_with_startup(1. / 60., move |ctx| {
        for item in items {
            app.canvas.add_prepared_placement_image(ctx, item);
        }
        app
    });
    for _ in 0..20 {
        harness.step();
    }
    (harness, bounds)
}

/// The keyboard style editor: a key row while annotations are selected, and a
/// HUD bottom-center for a moment after each style key.
#[test]
fn inspect_annotation_style_keys_writes_artifact() {
    let mut scenario = InspectScenario::new("annotation-style-keys");
    let root = TempRoot::new("snappyref-inspect-annotation-style-keys");
    let (mut harness, bounds) = readme_board(&root);
    let screen = test_screen_rect();
    let at = |name: &str, u: f32, v: f32| {
        let rect = bounds[name];
        inspect_world_to_screen(
            rect.min + egui::vec2(rect.width() * u, rect.height() * v),
            screen,
        )
    };
    let center_of = |harness: &Harness<'_, SnappyRefApp>, id: i64| {
        let annotation = harness.state().canvas.annotation_for_id(id).unwrap();
        inspect_world_to_screen(annotation.start().lerp(annotation.end(), 0.5), screen)
    };
    let along = |harness: &Harness<'_, SnappyRefApp>, id: i64, t: f32| {
        let annotation = harness.state().canvas.annotation_for_id(id).unwrap();
        inspect_world_to_screen(annotation.start().lerp(annotation.end(), t), screen)
    };
    let empty = screen.right_bottom() - egui::vec2(30., 30.);
    let annotation = |harness: &Harness<'_, SnappyRefApp>, id: i64| {
        harness
            .state()
            .canvas
            .annotation_for_id(id)
            .unwrap()
            .clone()
    };

    // The board from annotation-board, styled by keys only.
    let ridge = place_label(
        &mut harness,
        at("matterhorn", 0.08, 0.62),
        "North face light",
    );
    commit(&mut harness);
    color(&mut harness, 2);
    let from = center_of(&harness, ridge);
    hold_drag(
        &mut harness,
        egui::Key::A,
        from,
        at("matterhorn", 0.52, 0.22),
    );
    let arrow = last_annotation(&harness);
    hold_drag(
        &mut harness,
        egui::Key::S,
        at("great-wave", 0.04, 0.12),
        at("great-wave", 0.42, 0.58),
    );
    let crest = last_annotation(&harness);
    color(&mut harness, 5);
    place_label(&mut harness, at("great-wave", 0.05, 0.64), "Claw-like foam");
    commit(&mut harness);
    // Nothing selected: the keys set the next annotation's style.
    inspect_click_at(&harness, empty, egui::Modifiers::NONE);
    harness.step();
    color(&mut harness, 3);
    let next_color = harness.state().canvas.next_annotation_style().color;
    scenario.assertion(
        "with nothing selected, 3 sets the next color",
        next_color == crate::data::CanvasAnnotationColor::Gold,
    );
    hold(&mut harness);
    scenario.capture_screen("hud-next-color", &mut harness);
    hold_drag(
        &mut harness,
        egui::Key::S,
        at("tetons-snake-river", 0.30, 0.55),
        at("tetons-snake-river", 0.78, 0.86),
    );
    let bend = place_label(
        &mut harness,
        at("tetons-snake-river", 0.06, 0.08),
        "S-curve leads in",
    );
    commit(&mut harness);
    let from = center_of(&harness, bend);
    hold_drag(
        &mut harness,
        egui::Key::A,
        from,
        at("tetons-snake-river", 0.42, 0.6),
    );
    scenario.assertion(
        "the next style carries into new annotations",
        annotation(&harness, bend).color == crate::data::CanvasAnnotationColor::Gold,
    );
    // Crowded: a label low on the busy courthouse, near the canvas corner.
    color(&mut harness, 4);
    let steps = place_label(
        &mut harness,
        at("hall-of-justice", 0.52, 0.84),
        "Steps catch the shadow",
    );
    commit(&mut harness);
    let from = center_of(&harness, steps);
    hold_drag(
        &mut harness,
        egui::Key::A,
        from,
        at("hall-of-justice", 0.3, 0.62),
    );
    let steps_arrow = last_annotation(&harness);
    // Below the image, right where the key row sits.
    let palette = place_label(&mut harness, at("earthrise", 0.4, 1.06), "Palette");
    commit(&mut harness);
    settle_at(&mut harness, Some(empty), empty);
    scenario.capture_screen("resting", &mut harness);

    let point = center_of(&harness, ridge);
    settle_at(&mut harness, Some(point), empty);
    scenario.assertion(
        "a click selects the label",
        harness.state().canvas.selected_annotation_ids() == vec![ridge],
    );
    scenario.capture_screen("selected-label", &mut harness);

    shortcut(&mut harness, egui::Modifiers::NONE, egui::Key::Enter);
    shortcut(&mut harness, egui::Modifiers::NONE, egui::Key::End);
    harness.event(egui::Event::Text(" at dusk 2".into()));
    settle_at(&mut harness, None, empty);
    scenario.assertion(
        "while typing, digits are text",
        harness.ctx.egui_wants_keyboard_input()
            && annotation(&harness, ridge).text == "North face light at dusk 2"
            && annotation(&harness, ridge).color == crate::data::CanvasAnnotationColor::Coral,
    );
    scenario.capture_screen("editing-label", &mut harness);
    shortcut(&mut harness, egui::Modifiers::NONE, egui::Key::Escape);
    shortcut(&mut harness, egui::Modifiers::SHIFT, egui::Key::Num4);
    scenario.assertion(
        "Escape keeps the label selected, so Shift+4 reaches it",
        annotation(&harness, ridge).text_size == crate::data::CanvasTextSize::Xl,
    );
    hold(&mut harness);
    scenario.capture_screen("hud-label-size", &mut harness);

    let point = along(&harness, arrow, 0.6);
    settle_at(&mut harness, Some(point), empty);
    scenario.assertion(
        "a click selects the arrow",
        harness.state().canvas.selected_annotation_ids() == vec![arrow],
    );
    scenario.capture_screen("selected-arrow", &mut harness);
    color(&mut harness, 1);
    hold(&mut harness);
    scenario.assertion(
        "1 recolors the arrow",
        annotation(&harness, arrow).color == crate::data::CanvasAnnotationColor::Default,
    );
    scenario.capture_screen("hud-arrow-color", &mut harness);
    shortcut(&mut harness, egui::Modifiers::NONE, egui::Key::H);
    hold(&mut harness);
    scenario.assertion(
        "a tap of H takes the head off",
        annotation(&harness, arrow).kind == crate::data::CanvasAnnotationKind::Line,
    );
    scenario.capture_screen("hud-head-off", &mut harness);
    shortcut(&mut harness, egui::Modifiers::NONE, egui::Key::H);

    let crest_edge = {
        let a = annotation(&harness, crest);
        inspect_world_to_screen(
            egui::pos2(a.start().x, (a.start().y + a.end().y) / 2.),
            screen,
        )
    };
    settle_at(&mut harness, Some(crest_edge), empty);
    scenario.assertion(
        "a click on the rectangle edge selects it",
        harness.state().canvas.selected_annotation_ids() == vec![crest],
    );
    scenario.capture_screen("selected-rectangle", &mut harness);
    shortcut(&mut harness, egui::Modifiers::NONE, egui::Key::F);
    hold(&mut harness);
    let solid = annotation(&harness, crest).fill;
    scenario.capture_screen("hud-fill", &mut harness);
    shortcut(&mut harness, egui::Modifiers::NONE, egui::Key::F);
    scenario.assertion(
        "F steps a tinted rectangle to solid, then to no fill",
        solid == crate::data::CanvasRectangleFill::Solid
            && annotation(&harness, crest).fill == crate::data::CanvasRectangleFill::None,
    );

    inspect_click_at(&harness, center_of(&harness, ridge), egui::Modifiers::SHIFT);
    harness.step();
    inspect_click_at(
        &harness,
        along(&harness, arrow, 0.6),
        egui::Modifiers::SHIFT,
    );
    settle_at(&mut harness, None, empty);
    scenario.assertion(
        "shift-click builds a mixed selection",
        harness.state().canvas.selected_annotation_ids().len() == 3,
    );
    scenario.capture_screen("mixed-selection", &mut harness);
    shortcut(&mut harness, egui::Modifiers::SHIFT, egui::Key::Num5);
    hold(&mut harness);
    scenario.assertion(
        "Shift+5 sizes the label, the arrow, and the rectangle's border",
        annotation(&harness, ridge).text_size == crate::data::CanvasTextSize::Xxl
            && annotation(&harness, arrow).stroke_width == crate::data::CanvasStrokeWidth::Xxl
            && annotation(&harness, crest).stroke_width == crate::data::CanvasStrokeWidth::Xxl,
    );
    scenario.capture_screen("hud-mixed-size", &mut harness);

    let point = along(&harness, steps_arrow, 0.5);
    settle_at(&mut harness, Some(point), empty);
    scenario.capture_screen("selected-crowded-arrow", &mut harness);
    let point = center_of(&harness, palette);
    settle_at(&mut harness, Some(point), empty);
    scenario.capture_screen("selected-near-edge", &mut harness);
    color(&mut harness, 2);
    hold(&mut harness);
    scenario.capture_screen("hud-near-edge", &mut harness);
    shortcut(
        &mut harness,
        egui::Modifiers::NONE,
        super::support::INSPECT_DELETE_KEY,
    );
    scenario.assertion(
        "Delete removes the selected label",
        harness.state().canvas.annotation_for_id(palette).is_none(),
    );

    // A tap arms arrow mode. The next drag draws with the key long released.
    shortcut(&mut harness, egui::Modifiers::NONE, egui::Key::A);
    settle_at(&mut harness, None, at("earthrise", 0.5, 0.3));
    scenario.capture_screen("armed-arrow", &mut harness);
    let before = harness.state().canvas.all_annotation_ids().len();
    drag(
        &mut harness,
        at("earthrise", 0.7, 0.15),
        at("earthrise", 0.55, 0.4),
        egui::Modifiers::NONE,
    );
    drag(
        &mut harness,
        at("woman-water-pitcher", 0.3, 0.3),
        at("woman-water-pitcher", 0.6, 0.6),
        egui::Modifiers::NONE,
    );
    scenario.assertion(
        "an armed A draws one arrow, and the drag after it draws nothing",
        harness.state().canvas.all_annotation_ids().len() == before + 1
            && annotation(&harness, last_annotation(&harness)).kind
                == crate::data::CanvasAnnotationKind::Arrow,
    );

    let drawn = last_annotation(&harness);
    harness.state_mut().canvas.select_content(&[], &[drawn]);
    shortcut(&mut harness, egui::Modifiers::NONE, egui::Key::A);
    shortcut(&mut harness, egui::Modifiers::NONE, egui::Key::H);
    drag(
        &mut harness,
        at("woman-water-pitcher", 0.3, 0.3),
        at("woman-water-pitcher", 0.6, 0.6),
        egui::Modifiers::NONE,
    );
    scenario.assertion(
        "H on an armed arrow draws a line and leaves the arrow drawn before it alone",
        annotation(&harness, drawn).kind == crate::data::CanvasAnnotationKind::Arrow
            && annotation(&harness, last_annotation(&harness)).kind
                == crate::data::CanvasAnnotationKind::Line,
    );

    // The arrow starts on a label, so its start handle shows at the label's edge.
    let point = along(&harness, arrow, 0.6);
    settle_at(&mut harness, Some(point), empty);
    let handle = painted_handle(&harness, along(&harness, arrow, 0.));
    let drop = handle + egui::vec2(-60., 90.);
    drag(&mut harness, handle, drop, egui::Modifiers::NONE);
    let start = inspect_world_to_screen(annotation(&harness, arrow).start(), screen);
    scenario.fact(
        "endpoint_drop_miss",
        serde_json::json!(start.distance(drop)),
    );
    scenario.assertion(
        "an end dragged off a label lands where it is dropped",
        start.distance(drop) < 2.,
    );

    let report = scenario.finish();
    assert!(report.passed, "{}", report.artifact_dir.display());
}

/// The endpoint handle the last frame painted nearest a point.
fn painted_handle(harness: &Harness<'_, SnappyRefApp>, near: egui::Pos2) -> egui::Pos2 {
    fn handles(shape: &egui::Shape, found: &mut Vec<egui::Pos2>) {
        match shape {
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|shape| handles(shape, found)),
            egui::Shape::Circle(circle)
                if circle.radius == crate::ui::theme::SPACE_1
                    && circle.fill == crate::ui::theme::SURFACE =>
            {
                found.push(circle.center);
            }
            _ => {}
        }
    }
    let mut found = Vec::new();
    for clipped in &harness.output().shapes {
        handles(&clipped.shape, &mut found);
    }
    found
        .into_iter()
        .min_by(|a, b| a.distance(near).total_cmp(&b.distance(near)))
        .expect("a selected stroke paints its endpoint handles")
}

/// Lets the HUD finish its entrance without reaching its fade.
fn hold(harness: &mut Harness<'_, SnappyRefApp>) {
    for _ in 0..12 {
        harness.step();
    }
}
