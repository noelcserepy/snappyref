//! Shared app fixtures, full-app rendering, and inspect artifact recording.
use crate::app::SnappyRefApp;
use crate::data::{
    Asset, CanvasAnnotationKind, CanvasStrokeWidth, CanvasTextSize, Placement, PlacementState,
};
use crate::image_preparation::PreparedPlacementImage;
use crate::library::storage::Library;
use crate::search::palette::{
    PALETTE_WIDTH, SEARCH_OVERLAY_TOP_MARGIN, search_result_grid_layout, search_result_tile_height,
};
use crate::search::semantic::{LocalModelStoreStatus, SearchResultAssetEntry};
use crate::ui::sidebar::{block_sidebar_input, show_sidebar, sidebar_rect};
use eframe::egui;
use egui_kittest::Harness;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, Instant};

pub(super) const INSPECT_DELETE_KEY: egui::Key = if cfg!(target_os = "macos") {
    egui::Key::Backspace
} else {
    egui::Key::Delete
};

pub(super) fn test_app(library: Option<Library>) -> SnappyRefApp {
    let mut app = SnappyRefApp::detached(None, 8192, crate::updater::UpdateConfig::for_tests());
    if let Some(library) = &library {
        app.board_editing.persistence =
            crate::active_board::persistence::DocumentPersistence::queued(library.clone());
    }
    app.library = library;
    app.refresh_board_summaries();
    app
}

pub(super) fn key_press_input(modifiers: egui::Modifiers, key: egui::Key) -> egui::RawInput {
    egui::RawInput {
        events: vec![
            egui::Event::ModifiersChanged(modifiers),
            egui::Event::Key {
                key,
                physical_key: Some(key),
                pressed: true,
                repeat: false,
                modifiers,
            },
        ],
        ..Default::default()
    }
}

pub(super) fn test_screen_rect() -> egui::Rect {
    egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 800.0))
}

pub(super) fn render_sidebar_for_test(ctx: &egui::Context, app: &mut SnappyRefApp) {
    let screen_rect = test_screen_rect();
    if app.should_block_sidebar_input() {
        block_sidebar_input(ctx, sidebar_rect(screen_rect));
    }
    let board_summaries = app.board_editing.documents.summaries();
    let about = app.about_view();
    let semantic_search = app.semantic_search_settings_status();
    let sidebar_result = show_sidebar(
        ctx,
        screen_rect,
        app.sidebar_open,
        !app.modal_open(),
        &mut app.sidebar_tab,
        &mut app.settings,
        &mut app.canvas,
        board_summaries,
        app.active_board_id,
        &about,
        &semantic_search,
    );
    let board_action = if app.modal_open() {
        None
    } else {
        sidebar_result.board_action
    };
    app.handle_board_action(board_action);
    app.handle_semantic_search_settings_action(sidebar_result.semantic_search_action, ctx);
    if sidebar_result.update_action_clicked {
        app.handle_update_action_click(ctx);
    }
}

/// A file handle as the winit integration would deliver it to `RawInput::dropped_files`.
#[derive(Debug)]
struct TestDroppedFile(std::path::PathBuf);

impl egui::DroppedFile for TestDroppedFile {
    fn path(&self) -> &std::path::Path {
        &self.0
    }

    fn bytes(&self) -> Result<Vec<u8>, String> {
        fs::read(&self.0).map_err(|error| error.to_string())
    }
}

pub(super) fn dropped_file(path: impl Into<std::path::PathBuf>) -> egui::DroppedFileHandle {
    std::sync::Arc::new(TestDroppedFile(path.into()))
}

pub(super) fn app_harness<'a>(
    app: SnappyRefApp,
    mut render: impl FnMut(&egui::Context, &mut SnappyRefApp) + 'a,
) -> Harness<'a, SnappyRefApp> {
    Harness::builder()
        .with_size(egui::vec2(1200.0, 800.0))
        .with_pixels_per_point(1.0)
        .with_os(egui::os::OperatingSystem::Nix)
        .with_theme(egui::Theme::Dark)
        .with_max_steps(4)
        .build_ui_state(
            move |ui, app| {
                let ctx = ui.ctx().clone();
                crate::ui::theme::configure_theme(&ctx);
                render(&ctx, app);
            },
            app,
        )
}

pub(super) fn app_full_harness(app: SnappyRefApp) -> Harness<'static, SnappyRefApp> {
    app_full_harness_with_step_dt(app, 1.0 / 4.0)
}

pub(super) fn app_full_harness_with_step_dt(
    app: SnappyRefApp,
    step_dt: f32,
) -> Harness<'static, SnappyRefApp> {
    app_full_harness_with_startup(step_dt, move |_| app)
}

/// Builds the app inside the eframe creation context, before the first frame,
/// like `SnappyRefApp::new`. Use it when startup work (worker creation,
/// session load) must precede the first frame's persistence.
pub(super) fn app_full_harness_with_startup(
    step_dt: f32,
    startup: impl FnOnce(&egui::Context) -> SnappyRefApp + 'static,
) -> Harness<'static, SnappyRefApp> {
    Harness::builder()
        .with_size(egui::vec2(1200.0, 800.0))
        .with_pixels_per_point(1.0)
        .with_os(egui::os::OperatingSystem::Nix)
        .with_theme(egui::Theme::Dark)
        .with_max_steps(4)
        .with_step_dt(step_dt)
        .build_eframe(move |cc| {
            crate::ui::theme::configure_theme(&cc.egui_ctx);
            startup(&cc.egui_ctx)
        })
}

pub(super) fn drag_test_pos<State>(harness: &Harness<'_, State>, from: egui::Pos2, to: egui::Pos2) {
    harness.hover_at(from);
    harness.drag_at(from);
    harness.hover_at(to);
    harness.drop_at(to);
}

pub(super) struct InspectScenario {
    name: String,
    pub(super) artifact_dir: PathBuf,
    events: Vec<serde_json::Value>,
    assertions: Vec<serde_json::Value>,
    frames: Vec<String>,
    passed: bool,
}

pub(super) struct InspectReport {
    pub(super) passed: bool,
    pub(super) artifact_dir: PathBuf,
}

pub(super) struct InspectFrameStats {
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) transparent_pixels: usize,
    pub(super) unique_colors: usize,
}

pub(super) struct InspectProgressConfig {
    pub(super) start_frame_index: usize,
    pub(super) timeout: Duration,
    pub(super) poll_interval: Duration,
    pub(super) fact_name: &'static str,
    pub(super) timeout_fact_name: &'static str,
    pub(super) terminal_assertion: &'static str,
    pub(super) timeout_assertion: &'static str,
}

pub(super) struct InspectImportSample {
    pub(super) name: &'static str,
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) color: [u8; 4],
    pub(super) center: egui::Pos2,
}

struct InspectSampleImport {
    name: &'static str,
    width: u32,
    height: u32,
    color: [u8; 4],
    reservation: crate::placement::content_resolution::ContentResolutionReservation,
    loading_state: PlacementState,
}

impl InspectScenario {
    pub(super) fn new(name: impl Into<String>) -> Self {
        let name = name.into();
        let artifact_dir = inspect_artifact_dir(&name);
        Self {
            name,
            artifact_dir,
            events: Vec::new(),
            assertions: Vec::new(),
            frames: Vec::new(),
            passed: true,
        }
    }

    pub(super) fn action(&mut self, name: &'static str, screen_pos: egui::Pos2) {
        self.event(
            "action",
            serde_json::json!({
                "name": name,
                "screen": inspect_pos(screen_pos),
            }),
        );
    }

    pub(super) fn target(&mut self, name: &'static str, kind: &'static str, rect: egui::Rect) {
        self.event(
            "observation.target",
            serde_json::json!({
                "name": name,
                "kind": kind,
                "rect": inspect_rect(rect),
            }),
        );
    }

    pub(super) fn fact(&mut self, name: &'static str, value: serde_json::Value) {
        self.event(
            "observation.fact",
            serde_json::json!({
                "name": name,
                "value": value,
            }),
        );
    }

    pub(super) fn capture_click_hold_fact(
        &mut self,
        frame_name: &str,
        harness: &Harness<'_, SnappyRefApp>,
        placement_id: i64,
    ) {
        let now_seconds = harness.ctx.input(|input| input.time);
        let placement = harness.state().canvas.placement_for_id(placement_id);
        self.fact(
            "placement-click-hold.frame-state",
            serde_json::json!({
                "frame": frame_name,
                "time_seconds": now_seconds,
                "selected_placement_ids": harness.state().canvas.selected_placement_ids(),
                "placement_center": placement
                    .as_ref()
                    .map(|placement| inspect_pos(placement.state.geometry.center)),
                "selection_border_visible": harness
                    .state()
                    .canvas
                    .selection_border_visible(now_seconds),
            }),
        );
    }

    pub(super) fn assertion(&mut self, name: &'static str, passed: bool) {
        self.passed &= passed;
        let event = serde_json::json!({
            "type": "assertion",
            "name": name,
            "passed": passed,
        });
        self.assertions.push(event.clone());
        self.events.push(event);
    }

    pub(super) fn event(&mut self, event_type: &'static str, payload: serde_json::Value) {
        self.events.push(serde_json::json!({
            "type": event_type,
            "payload": payload,
        }));
    }

    pub(super) fn capture_screen(
        &mut self,
        name: &str,
        harness: &mut Harness<'_, SnappyRefApp>,
    ) -> InspectFrameStats {
        let raw = harness
            .render()
            .expect("inspect screen frame should render");
        let mut transparent_pixels = 0usize;
        let mut unique_colors = std::collections::HashSet::new();
        let mut image = image::RgbaImage::new(raw.width(), raw.height());
        let background = image::Rgba([
            crate::ui::theme::BACKGROUND.r(),
            crate::ui::theme::BACKGROUND.g(),
            crate::ui::theme::BACKGROUND.b(),
            255,
        ]);

        for (x, y, pixel) in raw.enumerate_pixels() {
            if pixel.0[3] < 255 {
                transparent_pixels += 1;
            }
            let composited = composite_pixel_over_background(*pixel, background);
            if unique_colors.len() <= 256 {
                unique_colors.insert(composited.0);
            }
            image.put_pixel(x, y, composited);
        }

        let frames_dir = self.artifact_dir.join("frames");
        fs::create_dir_all(&frames_dir).unwrap();
        let path = frames_dir.join(format!("{name}.png"));
        image.save(&path).expect("inspect frame should save");
        let stats = InspectFrameStats {
            width: image.width(),
            height: image.height(),
            transparent_pixels,
            unique_colors: unique_colors.len(),
        };
        self.frames.push(path.display().to_string());
        self.event(
            "frame",
            serde_json::json!({
                "name": name,
                "path": path,
                "source": "egui-kittest-full-app-render",
                "width": stats.width,
                "height": stats.height,
                "transparent_pixels": stats.transparent_pixels,
                "unique_colors_sample": stats.unique_colors,
            }),
        );
        stats
    }

    pub(super) fn drive_progress<Status>(
        &mut self,
        harness: &mut Harness<'_, SnappyRefApp>,
        config: InspectProgressConfig,
        mut sample_status: impl FnMut(&SnappyRefApp) -> Status,
        mut status_slug: impl FnMut(&Status) -> String,
        mut is_terminal: impl FnMut(&Status) -> bool,
        mut is_success: impl FnMut(&Status) -> bool,
    ) -> Status
    where
        Status: Clone + PartialEq + std::fmt::Debug,
    {
        let deadline = Instant::now() + config.timeout;
        let mut last_status = sample_status(harness.state());
        let mut frame_index = config.start_frame_index;

        loop {
            harness.step();
            let status = sample_status(harness.state());
            if status != last_status {
                self.fact(
                    config.fact_name,
                    serde_json::json!({
                        "status": format!("{status:?}"),
                    }),
                );
                self.capture_screen(
                    &format!("screen-{frame_index:03}-{}", status_slug(&status)),
                    harness,
                );
                frame_index += 1;
                last_status = status.clone();
            }

            if is_terminal(&status) {
                self.assertion(config.terminal_assertion, is_success(&status));
                return status;
            }

            if Instant::now() >= deadline {
                self.fact(
                    config.timeout_fact_name,
                    serde_json::json!({
                        "status": format!("{status:?}"),
                    }),
                );
                self.capture_screen("screen-timeout", harness);
                self.assertion(config.timeout_assertion, false);
                return status;
            }

            std::thread::sleep(config.poll_interval);
        }
    }

    pub(super) fn drive_import_lifecycle(
        &mut self,
        harness: &mut Harness<'_, SnappyRefApp>,
        samples: &[InspectImportSample],
    ) {
        self.capture_screen("screen-000-before-import", harness);

        let mut imports = Vec::new();
        for sample in samples {
            let reservation = harness
                .state_mut()
                .reserve_import(
                    Some((sample.width, sample.height)),
                    sample.center,
                    crate::data::ContentResolutionSource::Unknown {
                        description: sample.name.to_owned(),
                    },
                )
                .unwrap();
            let placement_id = reservation.placement_id;
            let loading_state = harness
                .state()
                .canvas
                .placement_for_id(placement_id)
                .expect("loading placement should be on canvas")
                .state
                .clone();
            imports.push(InspectSampleImport {
                name: sample.name,
                width: sample.width,
                height: sample.height,
                color: sample.color,
                reservation,
                loading_state,
            });
        }

        harness.state_mut().status = "Queued sample image imports".to_owned();
        self.fact(
            "sample-import.loading",
            serde_json::json!({
                "count": imports.len(),
                "pending": harness.state().placement_creation.pending_len(),
            }),
        );
        harness.step();
        self.capture_screen("screen-001-during-import-loading", harness);
        self.assertion(
            "loading placeholders do not count as painted images",
            harness.state().frame_perf.canvas.image_pass.painted_images == 0,
        );

        let ctx = harness.ctx.clone();
        let active_board_id = harness.state().active_board_id;
        for import in &imports {
            let loaded = inspect_sample_loaded_item(
                import.name,
                import.width,
                import.height,
                import.color,
                import.reservation.queue_id,
                active_board_id,
                import.loading_state.clone(),
            );
            harness
                .state_mut()
                .install_test_prepared_image(&ctx, loaded);
        }
        self.fact(
            "sample-import.loaded",
            serde_json::json!({
                "count": imports.len(),
                "pending": harness.state().placement_creation.pending_len(),
            }),
        );
        harness.step();
        self.capture_screen("screen-002-after-import-loaded", harness);
        self.assertion(
            "loaded sample textures count as painted images",
            harness.state().frame_perf.canvas.image_pass.painted_images == samples.len(),
        );

        self.assertion(
            "sample imports created loading placements",
            imports.len() == samples.len()
                && harness.state().canvas.all_placement_ids().len() == samples.len(),
        );
        self.assertion(
            "sample imports finished loading",
            harness.state().placement_creation.pending_len() == 0,
        );
    }

    pub(super) fn finish(self) -> InspectReport {
        let artifact_dir = self.artifact_dir;
        fs::create_dir_all(&artifact_dir).unwrap();

        let events_path = artifact_dir.join("events.ndjson");
        let mut events_file = fs::File::create(&events_path).unwrap();
        for event in &self.events {
            writeln!(events_file, "{event}").unwrap();
        }

        let first_failure = self
            .assertions
            .iter()
            .find(|assertion| assertion["passed"] == false)
            .and_then(|assertion| assertion["name"].as_str())
            .map(str::to_owned);
        // Cargo shows this on failure, so the failed names appear without opening the report.
        for assertion in self.assertions.iter().filter(|a| a["passed"] == false) {
            eprintln!("inspect assertion failed: {}", assertion["name"]);
        }
        let report = serde_json::json!({
            "scenario": self.name,
            "status": if self.passed { "pass" } else { "fail" },
            "artifact_dir": artifact_dir,
            "first_failure": first_failure,
            "assertions": self.assertions,
            "frames": self.frames,
        });
        fs::write(
            artifact_dir.join("report.json"),
            serde_json::to_string_pretty(&report).unwrap(),
        )
        .unwrap();
        fs::write(
            artifact_dir
                .parent()
                .expect("scenario artifact dir should have parent")
                .join("latest.txt"),
            artifact_dir.display().to_string(),
        )
        .unwrap();

        InspectReport {
            passed: self.passed,
            artifact_dir,
        }
    }
}

fn inspect_artifact_dir(scenario: &str) -> PathBuf {
    let target_dir = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target"));
    let target_dir = if target_dir.is_absolute() {
        target_dir
    } else {
        std::env::current_dir().unwrap().join(target_dir)
    };
    target_dir
        .join("snappyref-inspect")
        .join(scenario)
        .join(format!(
            "run-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
}

pub(super) fn inspect_pos(pos: egui::Pos2) -> serde_json::Value {
    serde_json::json!({
        "x": pos.x,
        "y": pos.y,
    })
}

fn inspect_vec2(vec: egui::Vec2) -> serde_json::Value {
    serde_json::json!({
        "x": vec.x,
        "y": vec.y,
    })
}

pub(super) fn inspect_rect(rect: egui::Rect) -> serde_json::Value {
    serde_json::json!({
        "min": inspect_pos(rect.min),
        "max": inspect_pos(rect.max),
        "center": inspect_pos(rect.center()),
        "size": inspect_vec2(rect.size()),
    })
}

pub(super) fn inspect_placement(placement: &Placement) -> serde_json::Value {
    serde_json::json!({
        "id": placement.state.id,
        "asset_id": placement.asset_id(),
        "center": inspect_pos(placement.state.geometry.center),
        "size": inspect_vec2(placement.state.geometry.scaled_size()),
        "scale": placement.state.geometry.scale,
    })
}

pub(super) fn inspect_status_slug(status: &LocalModelStoreStatus) -> String {
    match status {
        LocalModelStoreStatus::Missing => "missing".to_owned(),
        LocalModelStoreStatus::Downloading { percent } => format!("downloading-{percent:03}"),
        LocalModelStoreStatus::Installed => "installed".to_owned(),
        LocalModelStoreStatus::Failed(_) => "failed".to_owned(),
    }
}

fn inspect_sample_loaded_item(
    name: &str,
    width: u32,
    height: u32,
    color: [u8; 4],
    queue_id: u64,
    target_board_id: i64,
    state: PlacementState,
) -> PreparedPlacementImage {
    let asset_id = state.id;
    let asset = Asset {
        id: asset_id,
        original_path: PathBuf::from(name),
        filename: name.to_owned(),
        mime_type: "image/png".to_owned(),
        width,
        height,
    };
    let pixel_count = width as usize * height as usize;
    PreparedPlacementImage {
        target_board_id,
        queue_id: Some(queue_id),
        preview_rgba: std::iter::repeat_n(color, pixel_count).flatten().collect(),
        preview_width: width,
        preview_height: height,
        origin: crate::image_preparation::ImportOrigin::Path,
        ..loaded_record(Placement::loaded(state, asset))
    }
}

pub(super) fn inspect_prepared_loaded_placement(
    placement: Placement,
    color: [u8; 4],
    preview_width: u32,
    preview_height: u32,
    queue_id: Option<u64>,
    origin: crate::image_preparation::ImportOrigin,
) -> PreparedPlacementImage {
    PreparedPlacementImage {
        queue_id,
        preview_rgba: solid_rgba(color, preview_width, preview_height),
        preview_width,
        preview_height,
        origin,
        ..loaded_record(placement)
    }
}

pub(super) fn inspect_search_result_entry(placement: &Placement) -> SearchResultAssetEntry {
    let asset = placement.require_loaded_asset();
    SearchResultAssetEntry {
        asset_id: asset.id,
        asset_preview_path: Some(asset.original_path.clone()),
        asset_width: asset.width,
        asset_height: asset.height,
    }
}

pub(super) fn inspect_search_result_tile_center(index: usize, result_count: usize) -> egui::Pos2 {
    let body_h_pad = 12.0_f32;
    let grid_inner_width = PALETTE_WIDTH - body_h_pad * 2.0;
    let grid_layout = search_result_grid_layout(grid_inner_width, result_count);
    let columns = grid_layout.columns.max(1);
    let row = index / columns;
    let column = index % columns;
    let gap = crate::ui::theme::SPACE_2;
    let overlay_left = test_screen_rect().center().x - PALETTE_WIDTH / 2.0;
    let grid_left = overlay_left + body_h_pad;
    let chrome_height = SEARCH_OVERLAY_TOP_MARGIN + 53.0;
    let grid_top = test_screen_rect().top() + chrome_height + body_h_pad;
    egui::pos2(
        grid_left + column as f32 * (grid_layout.tile_width + gap) + grid_layout.tile_width / 2.0,
        grid_top
            + row as f32 * (search_result_tile_height() + gap)
            + search_result_tile_height() / 2.0,
    )
}

pub(super) fn inspect_world_to_screen(world: egui::Pos2, rect: egui::Rect) -> egui::Pos2 {
    rect.center() + world.to_vec2()
}

pub(super) fn inspect_click_at(
    harness: &Harness<'_, SnappyRefApp>,
    pos: egui::Pos2,
    modifiers: egui::Modifiers,
) {
    harness.event_modifiers(
        egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers,
        },
        modifiers,
    );
    harness.event_modifiers(
        egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers,
        },
        modifiers,
    );
}

pub(super) fn inspect_bright_red_pixels_in_rect(path: &PathBuf, rect: egui::Rect) -> usize {
    let image = image::open(path)
        .expect("inspect frame should exist")
        .to_rgba8();
    let left = rect.left().floor().max(0.0) as u32;
    let top = rect.top().floor().max(0.0) as u32;
    let right = rect.right().ceil().min(image.width() as f32) as u32;
    let bottom = rect.bottom().ceil().min(image.height() as f32) as u32;
    let mut count = 0;
    for y in top..bottom {
        for x in left..right {
            let [r, g, b, a] = image.get_pixel(x, y).0;
            if r >= 240 && g <= 40 && b <= 40 && a >= 200 {
                count += 1;
            }
        }
    }
    count
}

pub(super) fn inspect_command_modifier() -> egui::Modifiers {
    egui::Modifiers {
        command: true,
        ..Default::default()
    }
}

/// Text submitted to the renderer, including painter-only controls without AccessKit labels.
pub(super) fn inspect_displayed_text(harness: &Harness<'_, SnappyRefApp>) -> Vec<String> {
    fn collect(shape: &egui::epaint::Shape, clip: egui::Rect, out: &mut Vec<String>) {
        match shape {
            egui::epaint::Shape::Text(text) if clip.intersects(text.visual_bounding_rect()) => {
                out.push(text.galley.job.text.clone())
            }
            egui::epaint::Shape::Vec(shapes) => {
                for shape in shapes {
                    collect(shape, clip, out);
                }
            }
            _ => {}
        }
    }
    let mut text = Vec::new();
    for shape in &harness.output().shapes {
        collect(&shape.shape, shape.clip_rect, &mut text);
    }
    text
}

pub(super) fn check_compact_placements(
    scenario: &mut InspectScenario,
    before: &[Placement],
    app: &SnappyRefApp,
) {
    let mut after = app.canvas.committed_placements();
    after.sort_by_key(|p| p.id());
    let mut before = before.to_vec();
    before.sort_by_key(|p| p.id());
    scenario.assertion(
        "Q keeps every placement and asset identity",
        after
            .iter()
            .map(|p| (p.id(), p.require_loaded_asset().id))
            .collect::<Vec<_>>()
            == before
                .iter()
                .map(|p| (p.id(), p.require_loaded_asset().id))
                .collect::<Vec<_>>(),
    );
    scenario.assertion(
        "Q changes the seeded arrangement",
        after
            .iter()
            .zip(&before)
            .any(|(a, b)| a.state.geometry.center.distance(b.state.geometry.center) > 1.0),
    );
    let gap = app.settings.placement_gap;
    scenario.assertion(
        "Q leaves the requested gap between every pair",
        after.iter().enumerate().all(|(i, a)| {
            after[i + 1..].iter().all(|b| {
                let a = a.visual_bounds();
                let b = b.visual_bounds();
                let dx = a.left().max(b.left()) - a.right().min(b.right());
                let dy = a.top().max(b.top()) - a.bottom().min(b.bottom());
                dx >= gap - 0.05 || dy >= gap - 0.05
            })
        }),
    );
    scenario.fact(
        "compact.placements",
        serde_json::json!(after.iter().map(inspect_placement).collect::<Vec<_>>()),
    );
}

pub(super) fn compact_selection_fact(app: &SnappyRefApp) -> serde_json::Value {
    let mut centers = app
        .canvas
        .selected_placement_ids()
        .into_iter()
        .filter_map(|id| {
            app.canvas
                .placement_for_id(id)
                .map(|placement| (id, placement.state.geometry.center))
        })
        .collect::<Vec<_>>();
    centers.sort_by_key(|left| left.0);
    let mut xs = centers
        .iter()
        .map(|(_, center)| center.x)
        .collect::<Vec<_>>();
    let mut ys = centers
        .iter()
        .map(|(_, center)| center.y)
        .collect::<Vec<_>>();
    xs.sort_by(|left, right| left.total_cmp(right));
    ys.sort_by(|left, right| left.total_cmp(right));
    let column_count = compact_axis_bucket_count(xs);
    let row_count = compact_axis_bucket_count(ys);
    let max_column_count = compact_axis_max_bucket_size(
        centers
            .iter()
            .map(|(_, center)| center.x)
            .collect::<Vec<_>>(),
    );
    let bounds = app.canvas.selected_bounds_world();
    let bounds_aspect = bounds
        .map(|bounds| bounds.width().max(bounds.height()) / bounds.width().min(bounds.height()))
        .unwrap_or(1.0);
    serde_json::json!({
        "selected_count": centers.len(),
        "column_count": column_count,
        "row_count": row_count,
        "max_column_count": max_column_count,
        "bounds_aspect": bounds_aspect,
        "bounds_world": bounds.map(inspect_rect),
        "centers": centers
            .iter()
            .map(|(id, center)| serde_json::json!({
                "id": id,
                "center": inspect_pos(*center),
            }))
            .collect::<Vec<_>>(),
    })
}

fn compact_axis_bucket_count(mut values: Vec<f32>) -> usize {
    let mut buckets = 0usize;
    let mut last = None::<f32>;
    for value in values.drain(..) {
        if last.is_none_or(|previous| (value - previous).abs() > 1.0) {
            buckets += 1;
            last = Some(value);
        }
    }
    buckets
}

fn compact_axis_max_bucket_size(mut values: Vec<f32>) -> usize {
    values.sort_by(|left, right| left.total_cmp(right));
    let mut max_bucket = 0usize;
    let mut current_bucket = 0usize;
    let mut last = None::<f32>;
    for value in values {
        if last.is_none_or(|previous| (value - previous).abs() > 1.0) {
            max_bucket = max_bucket.max(current_bucket);
            current_bucket = 1;
            last = Some(value);
        } else {
            current_bucket += 1;
        }
    }
    max_bucket.max(current_bucket)
}

pub(super) fn add_inspect_compact_image(
    harness: &mut Harness<'_, SnappyRefApp>,
    ctx: &egui::Context,
    id: i64,
    width: u32,
    height: u32,
    color: [u8; 4],
    center: egui::Pos2,
) {
    let asset = Asset {
        id,
        original_path: PathBuf::from(format!("compact-fixture-{id}.png")),
        filename: format!("compact-fixture-{id}.png"),
        mime_type: "image/png".to_owned(),
        width,
        height,
    };
    let placement = Placement::loaded(
        PlacementState::with_display_size(
            id,
            center,
            egui::vec2(width as f32, height as f32),
            1.0,
            0.0,
            id as i32,
            false,
        ),
        asset,
    );
    harness.state_mut().canvas.add_prepared_placement_image(
        ctx,
        PreparedPlacementImage {
            preview_rgba: solid_rgba(color, width, height),
            preview_width: width,
            preview_height: height,
            ..loaded_record(placement)
        },
    );
}

pub(super) fn solid_rgba(color: [u8; 4], width: u32, height: u32) -> Vec<u8> {
    std::iter::repeat_n(color, width as usize * height as usize)
        .flatten()
        .collect()
}

fn composite_pixel_over_background(
    foreground: image::Rgba<u8>,
    background: image::Rgba<u8>,
) -> image::Rgba<u8> {
    let alpha = foreground.0[3] as f32 / 255.0;
    let inverse_alpha = 1.0 - alpha;
    image::Rgba([
        (foreground.0[0] as f32 * alpha + background.0[0] as f32 * inverse_alpha).round() as u8,
        (foreground.0[1] as f32 * alpha + background.0[1] as f32 * inverse_alpha).round() as u8,
        (foreground.0[2] as f32 * alpha + background.0[2] as f32 * inverse_alpha).round() as u8,
        255,
    ])
}

pub(super) fn test_viewport_world() -> egui::Rect {
    egui::Rect::from_min_max(egui::pos2(-240.0, -180.0), egui::pos2(240.0, 180.0))
}

pub(super) fn test_loaded_item(placement_id: i64, target_board_id: i64) -> PreparedPlacementImage {
    let asset = Asset {
        id: placement_id,
        original_path: PathBuf::from(format!("fixture-{placement_id}.png")),
        filename: format!("fixture-{placement_id}.png"),
        mime_type: "image/png".to_owned(),
        width: 1,
        height: 1,
    };
    let placement = Placement::loaded(
        PlacementState::new(
            placement_id,
            eframe::egui::pos2(0.0, 0.0),
            1.0,
            0.0,
            0,
            false,
        ),
        asset,
    );
    PreparedPlacementImage {
        target_board_id,
        queue_id: Some(placement_id as u64),
        preview_rgba: vec![255, 255, 255, 255],
        origin: crate::image_preparation::ImportOrigin::Clipboard,
        ..loaded_record(placement)
    }
}

/// Like `test_loaded_item`, but the Asset really exists in `library`, so the
/// queued Document unit for the Placement can satisfy the asset foreign key.
pub(super) fn persisted_loaded_item(
    library: &Library,
    placement_id: i64,
    target_board_id: i64,
) -> PreparedPlacementImage {
    let stored = library
        .store_rgba_image(
            &format!("fixture-{placement_id}"),
            &[255, 255, 255, 255],
            1,
            1,
        )
        .unwrap();
    let placement = crate::data::Placement::loaded(
        crate::data::PlacementState::new(
            placement_id,
            eframe::egui::pos2(0.0, 0.0),
            1.0,
            0.0,
            0,
            false,
        ),
        library
            .register_original(&stored, "image/png", 1, 1, 4, None)
            .unwrap(),
    );
    PreparedPlacementImage {
        placement,
        ..test_loaded_item(placement_id, target_board_id)
    }
}

/// A worker-prepared record for `placement` on board 1 with a 1x1 red
/// preview, as if the existing item had just loaded from the library.
pub(super) fn loaded_record(placement: Placement) -> PreparedPlacementImage {
    PreparedPlacementImage {
        target_board_id: 1,
        queue_id: None,
        placement,
        preview_rgba: vec![255, 0, 0, 255],
        preview_width: 1,
        preview_height: 1,
        animated_preview: None,
        preview_texture: crate::image_preparation::ImagePrepPreviewTexture::Cpu,
        loaded_lod: crate::data::CanvasImageLod::Working,
        existing_load_diagnostics: None,
        origin: crate::image_preparation::ImportOrigin::Existing,
        trace: None,
    }
}

pub(super) fn add_loaded_record(app: &mut SnappyRefApp, ctx: &egui::Context, placement: Placement) {
    app.canvas
        .add_prepared_placement_image(ctx, loaded_record(placement));
}

pub(super) fn test_canvas_annotation(id: i64, start: egui::Pos2) -> crate::data::CanvasAnnotation {
    crate::data::CanvasAnnotation::new(
        id,
        CanvasAnnotationKind::Line,
        start,
        egui::pos2(start.x + 40.0, start.y + 30.0),
        String::new(),
        CanvasTextSize::M,
        CanvasStrokeWidth::M,
        crate::data::CanvasAnnotationColor::Gold,
        false,
    )
}

impl SnappyRefApp {
    pub(super) fn install_test_prepared_image(
        &mut self,
        ctx: &egui::Context,
        loaded: crate::image_preparation::PreparedPlacementImage,
    ) {
        self.accept_preparation_result(
            ctx,
            crate::image_preparation::ImagePrepResult::Loaded(Box::new(loaded)),
        );
    }
}
