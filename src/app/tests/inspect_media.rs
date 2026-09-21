use super::*;

/// Optional release check: use the locally prepared, licensed image fixture set.
#[test]
#[ignore = "requires SNAPPYREF_MEDIA_FIXTURES with the release image fixture set"]
fn inspect_image_format_gallery_writes_artifact() {
    let fixtures = PathBuf::from(
        std::env::var_os("SNAPPYREF_MEDIA_FIXTURES").expect("set SNAPPYREF_MEDIA_FIXTURES"),
    );
    let mut scenario = InspectScenario::new("image-format-gallery");
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
                        false,
                    ));
            }
            app.canvas.select_content(&[], &[]);
            app
        });
        for _ in 0..4 {
            harness.step();
        }
        scenario.assertion(
            [
                "page 1 decoded",
                "page 2 decoded",
                "page 3 decoded",
                "page 4 decoded",
                "page 5 decoded",
            ][page],
            harness.state().canvas.all_placement_ids().len() == 4,
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
        harness.key_press(egui::Key::Space);
        for _ in 0..40 {
            harness.step();
        }
        scenario.capture_screen(&format!("page-{}-fit", page + 1), &mut harness);
        scenario.assertion(
            [
                "page 1 fit",
                "page 2 fit",
                "page 3 fit",
                "page 4 fit",
                "page 5 fit",
            ][page],
            harness
                .state()
                .canvas
                .committed_placements()
                .iter()
                .map(|p| p.state.geometry)
                .collect::<Vec<_>>()
                == geometry,
        );
    }
    let report = scenario.finish();
    assert!(report.passed, "{}", report.artifact_dir.display());
}
