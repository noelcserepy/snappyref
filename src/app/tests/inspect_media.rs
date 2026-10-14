use super::*;

/// Optional release check: use the locally prepared, licensed image fixture set.
#[test]
#[ignore = "requires SNAPPYREF_MEDIA_FIXTURES with the release image fixture set"]
fn inspect_external_image_format_gallery_writes_artifact() {
    let fixtures = PathBuf::from(
        std::env::var_os("SNAPPYREF_MEDIA_FIXTURES").expect("set SNAPPYREF_MEDIA_FIXTURES"),
    );
    let mut scenario = InspectScenario::new("external-image-format-gallery");
    let names = [
        "quadrants.png",
        "quadrants.jpg",
        "quadrants.webp",
        "quadrants.gif",
        "quadrants.tiff",
        "quadrants.bmp",
        "shapes.svg",
        "overexposed-gradient.exr",
        "cmyk_layers.jxl",
        "example.avif",
        "example.heic",
        "hopper_merged.psd",
        "file6.jp2",
        "transparent.png",
        "tiny.png",
        "large.png",
        "orientation-6.jpg",
        "orientation-2.tiff",
        "animated.gif",
        "animated-webp-2x2.webp",
    ];
    for (page, names) in names.chunks(4).enumerate() {
        let root = TempRoot::new("snappyref-media-gallery");
        let library = Library::initialize(root.to_path_buf()).unwrap();
        let mut items = Vec::new();
        for (slot, name) in names.iter().enumerate() {
            let center = egui::pos2(
                if slot % 2 == 0 { -260. } else { 260. },
                if slot < 2 { -150. } else { 150. },
            );
            let mut item =
                crate::image_preparation::import_path_sync(&library, fixtures.join(name), center)
                    .unwrap_or_else(|error| panic!("{name}: {error}"));
            let size = item.placement.state.geometry.size;
            item.placement.state.geometry.scale = (280. / size.x).min(180. / size.y);
            item.placement = library.fixture_placement(1, item.placement).unwrap();
            scenario.event("fixture", serde_json::json!({"page": page + 1, "slot": slot, "file": name, "size": [size.x,size.y]}));
            items.push(item);
        }
        let mut app = test_app(Some(library.clone()));
        let labels = names
            .iter()
            .map(|name| name.to_string())
            .collect::<Vec<_>>();
        let mut harness = app_full_harness_with_startup(1. / 60., move |ctx| {
            egui_extras::install_image_loaders(ctx);
            for (item, label) in items.into_iter().zip(labels) {
                let center = item.placement.state.geometry.center;
                app.canvas.add_prepared_placement_image(ctx, item);
                app.canvas
                    .add_annotation(crate::data::CanvasAnnotation::new(
                        library.allocate_document_id(),
                        CanvasAnnotationKind::Textbox,
                        center + egui::vec2(-180., -130.),
                        center + egui::vec2(180., -96.),
                        label,
                        CanvasTextSize::S,
                        CanvasStrokeWidth::M,
                        crate::data::CanvasAnnotationColor::Default,
                        false,
                    ));
            }
            app.canvas.select_content(&[], &[]);
            app
        });
        for _ in 0..4 {
            harness.step();
        }
        scenario.fact(
            "external-gallery.placements",
            serde_json::json!(
                harness
                    .state()
                    .canvas
                    .committed_placements()
                    .iter()
                    .map(inspect_placement)
                    .collect::<Vec<_>>()
            ),
        );
        scenario.capture_screen(&format!("page-{}-labeled", page + 1), &mut harness);
        if page == 1 {
            scenario.assertion(
                "SVG renders its red circle",
                inspect_bright_red_pixels_in_rect(
                    &scenario.artifact_dir.join("frames/page-2-labeled.png"),
                    egui::Rect::from_min_max(egui::pos2(200., 460.), egui::pos2(480., 640.)),
                ) > 3000,
            );
        }
        let geometry = harness
            .state()
            .canvas
            .committed_placements()
            .iter()
            .map(|p| p.state.geometry)
            .collect::<Vec<_>>();
        let ids = harness.state().canvas.all_placement_ids();
        harness.state_mut().canvas.select_placements(&ids);
        harness
            .state_mut()
            .canvas
            .apply_viewport_state(&crate::data::ViewportState {
                camera_pan_x: 2500.0,
                camera_pan_y: 1700.0,
                camera_zoom: 0.25,
                ..Default::default()
            });
        harness.step();
        scenario.capture_screen(&format!("page-{}-before-fit", page + 1), &mut harness);
        harness.key_press(egui::Key::Space);
        for _ in 0..40 {
            harness.step();
        }
        scenario.capture_screen(&format!("page-{}-fit", page + 1), &mut harness);
        scenario.assertion(
            "Space fits the external fixtures without changing image geometry",
            harness.state().canvas.viewport_state().camera_zoom > 1.0
                && harness
                    .state()
                    .canvas
                    .selection_screen_rect(test_screen_rect())
                    .is_some_and(|bounds| {
                        test_screen_rect().contains(bounds.min)
                            && test_screen_rect().contains(bounds.max)
                    })
                && harness
                    .state()
                    .canvas
                    .committed_placements()
                    .iter()
                    .map(|p| p.state.geometry)
                    .collect::<Vec<_>>()
                    == geometry,
        );
    }
    scenario.finish();
}

/// README board screenshot: public-domain references in justified rows.
/// After visual changes, quantize `frames/board.png` to 256 colors and save it
/// as `assets/readme/board.png`.
#[test]
fn inspect_readme_board_writes_artifact() {
    const ROW_WIDTH: f32 = 1600.;
    const GAP: f32 = 12.;
    let fixtures =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/images/readme-board");
    let mut scenario = InspectScenario::new("readme-board");
    let root = TempRoot::new("snappyref-inspect-readme-board");
    let library = Library::initialize(root.to_path_buf()).unwrap();
    let rows: [&[&str]; 3] = [
        &["tetons-snake-river", "great-wave", "woman-water-pitcher"],
        &[
            "matterhorn",
            "neue-burg-staircase",
            "hall-of-justice",
            "wheat-field-cypresses",
        ],
        &[
            "earthrise",
            "evening-snow-kanbara",
            "bgk-staircase",
            "merced-river",
        ],
    ];
    let mut items = Vec::new();
    let mut top = 0.;
    for row in rows {
        let row_items = row
            .iter()
            .map(|name| {
                let path = fixtures.join(format!("{name}.jpg"));
                crate::image_preparation::import_path_sync(&library, path, egui::Pos2::ZERO)
                    .unwrap_or_else(|error| panic!("{name}: {error}"))
            })
            .collect::<Vec<_>>();
        let aspect_sum: f32 = row_items
            .iter()
            .map(|item| item.placement.state.geometry.size.x / item.placement.state.geometry.size.y)
            .sum();
        let height = (ROW_WIDTH - GAP * (row.len() - 1) as f32) / aspect_sum;
        let mut left = 0.;
        for mut item in row_items {
            let geometry = &mut item.placement.state.geometry;
            geometry.scale = height / geometry.size.y;
            let width = geometry.size.x * geometry.scale;
            geometry.center = egui::pos2(left + width / 2., top + height / 2.);
            left += width + GAP;
            item.placement = library.fixture_placement(1, item.placement).unwrap();
            items.push(item);
        }
        top += height + GAP;
    }
    let placed = items.len();
    let mut app = test_app(Some(library));
    let mut harness = app_full_harness_with_startup(0.25, move |ctx| {
        for item in items {
            app.canvas.add_prepared_placement_image(ctx, item);
        }
        app
    });
    harness.step();
    let ids = harness.state().canvas.all_placement_ids();
    harness.state_mut().canvas.select_placements(&ids);
    harness.key_press(egui::Key::Space);
    harness.step();
    harness.state_mut().canvas.select_content(&[], &[]);
    // Outlast the "Focused selection" status message.
    for _ in 0..20 {
        harness.step();
    }
    let stats = scenario.capture_screen("board", &mut harness);
    scenario.assertion("all references placed", ids.len() == placed);
    scenario.assertion("board frame is opaque", stats.transparent_pixels == 0);
    scenario.finish();
}

/// A tiny oriented RGB TIFF. The six colored pixels make rotations and mirrors visible.
fn inspect_oriented_tiff(orientation: u16) -> Vec<u8> {
    let entries = [
        (256, 4, 1, 3),
        (257, 4, 1, 2),
        (258, 3, 3, 134),
        (259, 3, 1, 1),
        (262, 3, 1, 2),
        (273, 4, 1, 140),
        (274, 3, 1, u32::from(orientation)),
        (277, 3, 1, 3),
        (278, 4, 1, 2),
        (279, 4, 1, 18),
    ];
    let mut bytes = b"II\x2a\x00\x08\x00\x00\x00".to_vec();
    bytes.extend_from_slice(&10_u16.to_le_bytes());
    for (tag, kind, count, value) in entries {
        bytes.extend_from_slice(&(tag as u16).to_le_bytes());
        bytes.extend_from_slice(&(kind as u16).to_le_bytes());
        bytes.extend_from_slice(&(count as u32).to_le_bytes());
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend_from_slice(&[8, 0, 8, 0, 8, 0]);
    bytes.extend_from_slice(&[
        255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 0, 0, 255, 255, 255, 0, 255,
    ]);
    bytes
}

#[test]
fn inspect_image_format_gallery_writes_artifact() {
    let mut scenario = InspectScenario::new("image-format-gallery");
    let root = TempRoot::new("snappyref-literal-format-gallery");
    let red_green = vec![255, 0, 0, 255, 0, 255, 0, 255];
    let mut fixtures = Vec::new();
    for extension in ["png", "bmp", "tiff", "webp", "gif"] {
        let path = root.join(format!("red-green.{extension}"));
        image::save_buffer(&path, &red_green, 2, 1, image::ColorType::Rgba8).unwrap();
        fixtures.push((path, 2, 1, red_green.clone()));
    }
    let checked_in =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/images/formats");
    for name in [
        "red-green.ff",
        "red-green.hdr",
        "red-green.ico",
        "red-green.jxl",
    ] {
        fixtures.push((checked_in.join(name), 2, 1, red_green.clone()));
    }
    fixtures.push((
        checked_in.join("red-green.exr"),
        2,
        1,
        vec![186, 69, 69, 255, 69, 186, 69, 255],
    ));
    #[cfg(target_os = "macos")]
    for name in ["red-green.psd", "red-green.jp2"] {
        fixtures.push((checked_in.join(name), 2, 1, red_green.clone()));
    }
    for (orientation, width, height, pixels) in [
        (
            2,
            3,
            2,
            vec![
                0, 0, 255, 255, 0, 255, 0, 255, 255, 0, 0, 255, 255, 0, 255, 255, 0, 255, 255, 255,
                255, 255, 0, 255,
            ],
        ),
        (
            6,
            2,
            3,
            vec![
                255, 255, 0, 255, 255, 0, 0, 255, 0, 255, 255, 255, 0, 255, 0, 255, 255, 0, 255,
                255, 0, 0, 255, 255,
            ],
        ),
    ] {
        let path = root.join(format!("orientation-{orientation}.tiff"));
        std::fs::write(&path, inspect_oriented_tiff(orientation)).unwrap();
        fixtures.push((path, width, height, pixels));
    }
    for (page, fixtures) in fixtures.chunks(4).enumerate() {
        let library = Library::initialize(root.join(format!("library-{page}"))).unwrap();
        let mut items = Vec::new();
        for (slot, (path, width, height, pixels)) in fixtures.iter().enumerate() {
            let center = egui::pos2(
                if slot % 2 == 0 { -260.0 } else { 260.0 },
                if slot < 2 { -150.0 } else { 150.0 },
            );
            let mut item =
                crate::image_preparation::import_path_sync(&library, path.clone(), center).unwrap();
            scenario.fact("gallery.decoded",serde_json::json!({"file":path.file_name().unwrap().to_string_lossy(),"width":item.preview_width,"height":item.preview_height,"pixels":item.preview_rgba}));
            scenario.assertion(
                "import preserves literal decoded dimensions and pixels",
                item.preview_width == *width
                    && item.preview_height == *height
                    && item.preview_rgba == *pixels
                    && item.placement.require_loaded_asset().width == *width
                    && item.placement.require_loaded_asset().height == *height,
            );
            item.placement.state.geometry.scale =
                (280.0 / *width as f32).min(180.0 / *height as f32);
            item.placement = library.fixture_placement(1, item.placement).unwrap();
            items.push(item);
        }
        let mut app = test_app(Some(library));
        app.canvas
            .apply_viewport_state(&crate::data::ViewportState {
                camera_pan_x: 0.0,
                camera_pan_y: 0.0,
                camera_zoom: 1.0,
                ..Default::default()
            });
        let mut harness = app_full_harness_with_startup(1.0 / 60.0, move |ctx| {
            for item in items {
                app.canvas.add_prepared_placement_image(ctx, item);
            }
            app
        });
        for _ in 0..4 {
            harness.step();
        }
        let frame = format!("page-{page}-decoded");
        scenario.capture_screen(&frame, &mut harness);
        let screen = image::open(
            scenario
                .artifact_dir
                .join("frames")
                .join(format!("{frame}.png")),
        )
        .unwrap()
        .to_rgba8();
        for (slot, (_, width, height, pixels)) in fixtures.iter().enumerate() {
            let center = egui::pos2(
                if slot % 2 == 0 { 340.0 } else { 860.0 },
                if slot < 2 { 250.0 } else { 550.0 },
            );
            let scale = (280.0 / *width as f32).min(180.0 / *height as f32);
            let min = center - egui::vec2(*width as f32, *height as f32) * scale * 0.5;
            let matches = pixels
                .as_chunks::<4>()
                .0
                .iter()
                .enumerate()
                .all(|(index, expected)| {
                    let x = min.x + (index as u32 % *width) as f32 * scale + scale * 0.5;
                    let y = min.y + (index as u32 / *width) as f32 * scale + scale * 0.5;
                    screen
                        .get_pixel(x as u32, y as u32)
                        .0
                        .iter()
                        .zip(expected)
                        .all(|(actual, expected)| actual.abs_diff(*expected) <= 4)
                });
            scenario.assertion(
                "canvas renders literal pixel positions within texture filtering tolerance",
                matches,
            );
        }
        let original = harness.state().canvas.committed_placements();
        let ids = harness.state().canvas.all_placement_ids();
        harness.state_mut().canvas.select_placements(&ids);
        harness
            .state_mut()
            .canvas
            .apply_viewport_state(&crate::data::ViewportState {
                camera_pan_x: 2500.0,
                camera_pan_y: 1700.0,
                camera_zoom: 0.25,
                ..Default::default()
            });
        harness.step();
        scenario.capture_screen(&format!("page-{page}-before-space"), &mut harness);
        harness.key_press(egui::Key::Space);
        for _ in 0..40 {
            harness.step();
        }
        let view = harness.state().canvas.viewport_state();
        scenario.assertion(
            "Space moves the offscreen selection into the viewport",
            view.camera_zoom > 1.0
                && harness
                    .state()
                    .canvas
                    .selection_screen_rect(test_screen_rect())
                    .is_some_and(|bounds| {
                        test_screen_rect().contains(bounds.min)
                            && test_screen_rect().contains(bounds.max)
                    }),
        );
        scenario.assertion(
            "Space preserves every image identity and geometry",
            harness
                .state()
                .canvas
                .committed_placements()
                .iter()
                .map(|p| (p.id(), p.asset_id(), p.state.geometry))
                .collect::<Vec<_>>()
                == original
                    .iter()
                    .map(|p| (p.id(), p.asset_id(), p.state.geometry))
                    .collect::<Vec<_>>(),
        );
        scenario.capture_screen(&format!("page-{page}-after-space"), &mut harness);
    }
    scenario.finish();
}
