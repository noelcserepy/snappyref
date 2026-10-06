//! Inspect scenarios for PureRef `.pur` exchange: dropping a file opens it as
//! a new board, and Cmd+Shift+E writes the active board back out.

use super::*;
use crate::data::RectGeometry;
use crate::pureref_file::{
    PureRefBoard, PureRefImage, PureRefImageItem, PureRefNote, PureRefPixelRect, PureRefTransform,
    read_board, write_board,
};
use eframe::egui::emath::Rot2;

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

fn step_until(
    harness: &mut Harness<'_, SnappyRefApp>,
    mut done: impl FnMut(&SnappyRefApp) -> bool,
) -> bool {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !done(harness.state()) {
        if Instant::now() > deadline {
            return false;
        }
        harness.step();
        std::thread::sleep(Duration::from_millis(5));
    }
    true
}

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/images/readme-board")
        .join(name)
}

fn embedded_image(name: &str) -> PureRefImage {
    let path = fixture(name);
    let (width, height) = image::image_dimensions(&path).unwrap();
    PureRefImage {
        bytes: Some(fs::read(&path).unwrap()),
        source: Some(path.display().to_string()),
        width,
        height,
    }
}

fn full_crop(image: &PureRefImage) -> PureRefPixelRect {
    PureRefPixelRect {
        x: 0.0,
        y: 0.0,
        width: f64::from(image.width),
        height: f64::from(image.height),
    }
}

fn scaled_rotation(scale: f64, angle: f64, dx: f64, dy: f64) -> PureRefTransform {
    let (sin, cos) = angle.sin_cos();
    PureRefTransform {
        m11: scale * cos,
        m12: scale * sin,
        m21: -scale * sin,
        m22: scale * cos,
        dx,
        dy,
    }
}

fn mapped(transform: PureRefTransform, x: f64, y: f64) -> egui::Pos2 {
    let (x, y) = transform.map_point(x, y);
    egui::pos2(x as f32, y as f32)
}

/// Where the canvas draws a corner of the placement, `corner` in -1..1 per axis.
fn drawn_corner(geometry: RectGeometry, corner: egui::Vec2) -> egui::Pos2 {
    geometry.center + Rot2::from_angle(geometry.rotation) * (geometry.scaled_size() / 2.0 * corner)
}

fn textbox_center(textbox: &crate::data::CanvasAnnotation) -> egui::Pos2 {
    egui::Rect::from_two_pos(textbox.start(), textbox.end()).center()
}

fn placements_back_to_front(app: &SnappyRefApp) -> Vec<Placement> {
    app.canvas
        .canvas_stack_order()
        .into_iter()
        .filter_map(|item| match item {
            CanvasStackItem::Placement(id) => app.canvas.placement_for_id(id),
            CanvasStackItem::Annotation(_) => None,
        })
        .collect()
}

fn textboxes(app: &SnappyRefApp) -> Vec<crate::data::CanvasAnnotation> {
    app.canvas
        .canvas_stack_order()
        .into_iter()
        .filter_map(|item| match item {
            CanvasStackItem::Annotation(id) => app.canvas.annotation_for_id(id),
            CanvasStackItem::Placement(_) => None,
        })
        .filter(|annotation| annotation.kind == CanvasAnnotationKind::Textbox)
        .collect()
}

fn images_loaded(app: &SnappyRefApp) -> bool {
    !app.canvas.has_pending_image_loads()
        && placements_back_to_front(app).iter().all(|placement| {
            app.canvas
                .preview_for_placement_id(placement.id())
                .is_some()
        })
}

fn drop_pureref_board(harness: &mut Harness<'_, SnappyRefApp>, path: PathBuf) -> bool {
    let board_before = harness.state().active_board_id;
    harness.input_mut().dropped_files = vec![dropped_file(path)];
    harness.step();
    step_until(harness, |app| {
        app.active_board_id != board_before && app.status.starts_with("Imported")
    })
}

#[test]
fn inspect_pureref_import_writes_artifact() {
    let mut scenario = InspectScenario::new("pureref-import");
    let root = TempRoot::new("snappyref-inspect-pureref-import");
    let library = Library::initialize(root.to_path_buf()).unwrap();

    let earthrise = embedded_image("earthrise.jpg");
    let staircase = embedded_image("bgk-staircase.jpg");
    let crop = PureRefPixelRect {
        x: f64::from(staircase.width / 4),
        y: f64::from(staircase.height / 4),
        width: f64::from(staircase.width / 2),
        height: f64::from(staircase.height / 2),
    };
    let item = |image: usize, transform, crop| PureRefImageItem {
        image,
        name: None,
        transform,
        crop,
        opacity: 1.0,
    };
    let board = PureRefBoard {
        image_items: vec![
            item(
                0,
                scaled_rotation(0.4, 0.0, -900.0, -300.0),
                full_crop(&earthrise),
            ),
            item(
                0,
                scaled_rotation(0.3, 30_f64.to_radians(), 0.0, -350.0),
                full_crop(&earthrise),
            ),
            item(1, scaled_rotation(0.5, 0.0, -700.0, 200.0), crop),
            item(
                1,
                PureRefTransform {
                    m11: -0.3,
                    m22: 0.3,
                    ..PureRefTransform::translation(700.0, 100.0)
                },
                full_crop(&staircase),
            ),
        ],
        images: vec![earthrise, staircase],
        notes: vec![PureRefNote {
            text: "Mood board\nsecond line".to_owned(),
            transform: PureRefTransform::translation(-900.0, -450.0),
        }],
    };
    fs::create_dir_all(&scenario.artifact_dir).unwrap();
    let pur_path = scenario.artifact_dir.join("Moodboard.pur");
    fs::write(&pur_path, write_board(&board).unwrap()).unwrap();
    scenario.fact("pur_file", serde_json::json!(pur_path));

    let mut app = test_app(Some(library.clone()));
    app.import_worker = Some(crate::image_preparation::ImagePrepWorker::new(
        library.clone(),
        egui::Context::default(),
        None,
    ));
    let mut harness = app_full_harness(app);
    let board_before = harness.state().active_board_id;
    harness.hover_at(test_screen_rect().center());
    harness.input_mut().hovered_files = vec![egui::HoveredFile {
        path: Some(pur_path.clone()),
        ..Default::default()
    }];
    harness.step();
    scenario.assertion(
        "hovering a .pur file shows no provisional placement",
        harness
            .state()
            .canvas
            .provisional_placements_in_selection_order()
            .is_empty(),
    );
    harness.input_mut().hovered_files.clear();

    let imported = drop_pureref_board(&mut harness, pur_path);
    scenario.assertion("dropping the .pur file opens a new board", imported);
    // The status clears on a wall-clock timer, so read it before images load.
    let status = harness.state().status.clone();
    scenario.fact("status", serde_json::json!(status));
    scenario.assertion(
        "status reports the flipped image",
        status.contains("1 flipped image imported unflipped"),
    );
    scenario.capture_screen("dropped", &mut harness);
    let loaded = step_until(&mut harness, images_loaded);
    scenario.assertion("every imported image loads", loaded);
    scenario.capture_screen("loaded", &mut harness);

    let app = harness.state();
    let board_id = app.active_board_id;
    scenario.assertion(
        "board is named after the file",
        app.board_summaries()
            .iter()
            .any(|summary| summary.board.id == board_id && summary.board.name == "Moodboard"),
    );

    let placements = placements_back_to_front(app);
    let geometry: Vec<RectGeometry> = placements.iter().map(|p| p.state.geometry).collect();
    scenario.fact("geometry", serde_json::json!(format!("{geometry:?}")));
    let close = |a: egui::Pos2, b: egui::Pos2| (a - b).length() < 1e-2;
    let unflipped_corners_match = placements.len() == 4
        && board.image_items[..3]
            .iter()
            .zip(&geometry)
            .all(|(item, geometry)| {
                let crop = item.crop;
                let corners = [
                    (egui::vec2(-1.0, -1.0), (crop.x, crop.y)),
                    (egui::vec2(1.0, -1.0), (crop.x + crop.width, crop.y)),
                    (
                        egui::vec2(1.0, 1.0),
                        (crop.x + crop.width, crop.y + crop.height),
                    ),
                ];
                corners.into_iter().all(|(corner, (x, y))| {
                    close(
                        drawn_corner(*geometry, corner),
                        mapped(item.transform, x, y),
                    )
                })
            });
    scenario.assertion(
        "position, scale, rotation and crop land where PureRef draws them",
        unflipped_corners_match,
    );
    let flipped = &board.image_items[3];
    let staircase = &board.images[1];
    scenario.assertion(
        "the flipped image keeps its center and size",
        geometry.get(3).is_some_and(|geometry| {
            close(
                geometry.center,
                mapped(
                    flipped.transform,
                    f64::from(staircase.width) / 2.0,
                    f64::from(staircase.height) / 2.0,
                ),
            ) && geometry.rotation == 0.0
                && (geometry.scaled_size()
                    - egui::vec2(staircase.width as f32, staircase.height as f32) * 0.3)
                    .length()
                    < 1e-2
        }),
    );

    let asset_ids: Vec<i64> = placements
        .iter()
        .map(|placement| placement.require_loaded_asset().id)
        .collect();
    scenario.assertion(
        "repeated image shares one asset; the crop gets its own",
        asset_ids.len() == 4
            && asset_ids[0] == asset_ids[1]
            && asset_ids[2] != asset_ids[3]
            && asset_ids[0] != asset_ids[2]
            && asset_ids[0] != asset_ids[3],
    );
    scenario.assertion(
        "the crop asset has the crop's size",
        placements.get(2).is_some_and(|placement| {
            let asset = placement.require_loaded_asset();
            (asset.width, asset.height) == (staircase.width / 2, staircase.height / 2)
                && image::image_dimensions(&asset.original_path).ok()
                    == Some((asset.width, asset.height))
        }),
    );
    let stack_kinds: Vec<bool> = app
        .canvas
        .canvas_stack_order()
        .iter()
        .map(|item| matches!(item, CanvasStackItem::Placement(_)))
        .collect();
    scenario.assertion(
        "images keep PureRef's order with the note on top",
        stack_kinds == [true, true, true, true, false],
    );
    let notes = textboxes(app);
    scenario.assertion(
        "the note becomes a textbox with its text",
        notes.len() == 1
            && notes[0].text == "Mood board\nsecond line"
            && close(textbox_center(&notes[0]), egui::pos2(-900.0, -450.0)),
    );

    harness.state_mut().flush_persistence();
    let stored = library.load_board_document(board_id).unwrap();
    scenario.assertion(
        "the board is durable",
        stored.board.name == "Moodboard"
            && stored.placements.len() == 4
            && stored.annotations.len() == 1
            && stored.stack_order == harness.state().canvas.canvas_stack_order(),
    );

    shortcut(&mut harness, CMD, egui::Key::Z);
    step_until(&mut harness, |app| app.active_board_id == board_before);
    scenario.assertion(
        "undo returns to the previous board",
        harness.state().active_board_id == board_before,
    );
    scenario.capture_screen("undone", &mut harness);

    let report = scenario.finish();
    assert!(report.passed, "{}", report.artifact_dir.display());
}

#[test]
fn inspect_pureref_export_writes_artifact() {
    let mut scenario = InspectScenario::new("pureref-export");
    let root = TempRoot::new("snappyref-inspect-pureref-export");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let board_id = library.load_user_session().unwrap().active_board_id;
    let sources = [
        ("earthrise.jpg", egui::pos2(-300.0, 0.0), 0.3, 0.4),
        ("bgk-staircase.jpg", egui::pos2(350.0, 80.0), 0.25, -0.2),
    ];
    let items: Vec<_> = sources
        .iter()
        .map(|(name, center, scale, rotation)| {
            let mut item =
                crate::image_preparation::import_path_sync(&library, fixture(name), *center)
                    .unwrap();
            let geometry = &mut item.placement.state.geometry;
            geometry.center = *center;
            geometry.scale *= scale;
            geometry.rotation = *rotation;
            item.placement = library.fixture_placement(board_id, item.placement).unwrap();
            item
        })
        .collect();
    let note = library.new_canvas_annotation(
        CanvasAnnotationKind::Textbox,
        egui::pos2(-450.0, -330.0),
        egui::pos2(-100.0, -260.0),
        "Export note".to_owned(),
        CanvasTextSize::L,
        CanvasStrokeWidth::M,
        crate::data::CanvasAnnotationColor::Default,
        false,
    );
    let line = test_canvas_annotation(library.allocate_document_id(), egui::pos2(200.0, -300.0));

    fs::create_dir_all(&scenario.artifact_dir).unwrap();
    let out = scenario.artifact_dir.join("exported.pur");
    let save_path = out.clone();
    let mut app = test_app(Some(library.clone()));
    app.import_worker = Some(crate::image_preparation::ImagePrepWorker::new(
        library.clone(),
        egui::Context::default(),
        None,
    ));
    app.pureref_save_path = Box::new(move |_, _| Some(save_path.clone()));
    let mut harness = app_full_harness_with_startup(0.25, move |ctx| {
        for item in items {
            app.canvas.add_prepared_placement_image(ctx, item);
        }
        app.canvas.add_annotation(note);
        app.canvas.add_annotation(line);
        app
    });
    harness.step();
    let placement_ids = harness.state().canvas.all_placement_ids();
    let annotation_ids = harness.state().canvas.all_annotation_ids();
    harness
        .state_mut()
        .canvas
        .select_content(&placement_ids, &annotation_ids);
    harness.key_press(egui::Key::Space);
    harness.step();
    harness.state_mut().canvas.select_content(&[], &[]);
    harness.step();
    scenario.capture_screen("board", &mut harness);
    let placements = placements_back_to_front(harness.state());
    let notes = textboxes(harness.state());

    shortcut(
        &mut harness,
        egui::Modifiers { shift: true, ..CMD },
        egui::Key::E,
    );
    let exported = step_until(&mut harness, |app| app.status.starts_with("Exported"));
    scenario.fact("status", serde_json::json!(harness.state().status));
    scenario.fact("pur_file", serde_json::json!(out));
    scenario.assertion("Cmd+Shift+E exports the board", exported);
    scenario.assertion(
        "status counts what was written and what was left out",
        harness
            .state()
            .status
            .starts_with("Exported 2 images, 1 note to exported.pur")
            && harness.state().status.contains("1 line or arrow left out"),
    );

    let board = read_board(&fs::read(&out).unwrap_or_default()).unwrap_or(PureRefBoard {
        images: Vec::new(),
        image_items: Vec::new(),
        notes: Vec::new(),
    });
    scenario.assertion(
        "each image is embedded byte for byte",
        board.images.len() == 2
            && board
                .images
                .iter()
                .zip(sources)
                .all(|(image, (name, ..))| image.bytes == fs::read(fixture(name)).ok()),
    );
    let close = |a: egui::Pos2, b: egui::Pos2| (a - b).length() < 1e-2;
    scenario.assertion(
        "item transforms draw each image over its placement",
        board.image_items.len() == 2
            && board
                .image_items
                .iter()
                .zip(&placements)
                .all(|(item, placement)| {
                    let (width, height) = (item.crop.width, item.crop.height);
                    let geometry = placement.state.geometry;
                    close(
                        mapped(item.transform, 0.0, 0.0),
                        drawn_corner(geometry, egui::vec2(-1.0, -1.0)),
                    ) && close(
                        mapped(item.transform, width, height),
                        drawn_corner(geometry, egui::vec2(1.0, 1.0)),
                    )
                }),
    );
    scenario.assertion(
        "the textbox becomes a note",
        board.notes.len() == 1
            && board.notes[0].text == "Export note"
            && close(
                mapped(board.notes[0].transform, 0.0, 0.0),
                textbox_center(&notes[0]),
            ),
    );

    let reimported = drop_pureref_board(&mut harness, out);
    scenario.assertion("the export imports back as a new board", reimported);
    step_until(&mut harness, images_loaded);
    scenario.capture_screen("reimported", &mut harness);
    let round_trip = placements_back_to_front(harness.state());
    scenario.assertion(
        "round trip keeps every placement's geometry",
        round_trip.len() == placements.len()
            && round_trip.iter().zip(&placements).all(|(after, before)| {
                let (after, before) = (after.state.geometry, before.state.geometry);
                close(after.center, before.center)
                    && (after.scaled_size() - before.scaled_size()).length() < 1e-2
                    && (after.rotation - before.rotation).abs() < 1e-4
            }),
    );
    let round_trip_notes = textboxes(harness.state());
    scenario.assertion(
        "round trip keeps the textbox text, center and text size",
        round_trip_notes.len() == 1
            && round_trip_notes[0].text == notes[0].text
            && round_trip_notes[0].text_size == notes[0].text_size
            && close(
                textbox_center(&round_trip_notes[0]),
                textbox_center(&notes[0]),
            ),
    );

    let report = scenario.finish();
    assert!(report.passed, "{}", report.artifact_dir.display());
}
