mod active_board_commands;
mod active_board_load;
mod app;
mod board_switch;
mod canvas;
mod crop_flow;
mod data;
#[cfg(debug_assertions)]
mod design_system_preview;
mod document_persistence;
mod import_intake;
mod jobs;
mod layout_snap;
mod library_location;
#[cfg(feature = "semantic-search")]
mod model_store;
mod native_capture;
mod native_drop;
#[cfg(feature = "semantic-search")]
mod palette;
mod perf;
mod residency;
mod screenshot;
#[cfg(feature = "semantic-search")]
mod search;
#[cfg(feature = "semantic-search")]
mod semantic_search;
mod sidebar;
mod storage;
mod theme;
mod ui_layout;
pub mod updater;

pub fn run() -> eframe::Result<()> {
    if std::env::args()
        .skip(1)
        .any(|argument| argument == "--rollback")
    {
        let exit_code = updater::rollback::run_cli();
        std::process::exit(exit_code);
    }

    if std::env::args()
        .skip(1)
        .any(|argument| argument == "--image-import-prepare")
    {
        if let Err(error) = jobs::print_image_import_prepare_output_from_args() {
            return Err(eframe::Error::AppCreation(Box::new(std::io::Error::other(
                error,
            ))));
        }
        return Ok(());
    }

    #[cfg(feature = "semantic-search")]
    if let Some(path) = std::env::args().skip(1).find_map(|argument| {
        argument
            .strip_prefix("--semantic-embed-image=")
            .map(str::to_owned)
    }) {
        if let Err(error) = semantic_search::print_embed_image_output(std::path::Path::new(&path)) {
            return Err(eframe::Error::AppCreation(Box::new(std::io::Error::other(
                error,
            ))));
        }
        return Ok(());
    }
    #[cfg(feature = "semantic-search")]
    if std::env::args()
        .skip(1)
        .any(|argument| argument == "--semantic-embed-server")
    {
        if let Err(error) = semantic_search::run_embed_image_server() {
            return Err(eframe::Error::AppCreation(Box::new(std::io::Error::other(
                error,
            ))));
        }
        return Ok(());
    }

    let config = app::AppConfig::from_env();
    if config.startup_trace_requested() {
        if let Err(error) = config.library_root().and_then(perf::print_startup_trace) {
            return Err(eframe::Error::AppCreation(Box::new(std::io::Error::other(
                error,
            ))));
        }
        return Ok(());
    }
    if config.perf_report_requested() {
        if let Err(error) = perf::print_report() {
            return Err(eframe::Error::AppCreation(Box::new(std::io::Error::other(
                error,
            ))));
        }
        return Ok(());
    }
    if let Some(path) = config.perf_decode_path() {
        if let Err(error) = perf::print_file_decode_report(path) {
            return Err(eframe::Error::AppCreation(Box::new(std::io::Error::other(
                error,
            ))));
        }
        return Ok(());
    }
    if let Some(path) = config.perf_import_path() {
        if let Err(error) = perf::print_file_import_report(path) {
            return Err(eframe::Error::AppCreation(Box::new(std::io::Error::other(
                error,
            ))));
        }
        return Ok(());
    }
    if config.perf_embedding_lod_requested() {
        if let Err(error) = config.library_root().and_then(|library_root| {
            perf::print_embedding_lod_report(library_root, config.perf_embedding_lod_limit())
        }) {
            return Err(eframe::Error::AppCreation(Box::new(std::io::Error::other(
                error,
            ))));
        }
        return Ok(());
    }
    let viewport_size = config.startup_viewport_size();
    let restored_window = config.restored_window_state();
    let startup_focus_enabled = config.startup_focus_enabled();
    let mut viewport = eframe::egui::ViewportBuilder::default()
        .with_inner_size([viewport_size.x, viewport_size.y])
        .with_icon(app_icon())
        .with_fullsize_content_view(true)
        .with_title_shown(false)
        .with_titlebar_shown(false)
        .with_titlebar_buttons_shown(true)
        .with_active(startup_focus_enabled);
    if let Some(window) = &restored_window {
        if let (Some(width), Some(height)) = (window.inner_width, window.inner_height) {
            viewport = viewport.with_inner_size([width, height]);
        }
        if let (Some(x), Some(y)) = (window.outer_x, window.outer_y) {
            viewport = viewport.with_position([x, y]);
        }
    }
    let mut options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    configure_startup_activation(&mut options, startup_focus_enabled);

    eframe::run_native(
        "SnappyRef",
        options,
        Box::new(|cc| {
            theme::configure_theme(&cc.egui_ctx);
            cc.egui_ctx.options_mut(|o| o.zoom_with_keyboard = false);
            egui_extras::install_image_loaders(&cc.egui_ctx);
            Ok(Box::new(app::SnappyRefApp::new(cc, config.clone())))
        }),
    )
}

#[cfg(target_os = "macos")]
fn configure_startup_activation(options: &mut eframe::NativeOptions, startup_focus_enabled: bool) {
    if startup_focus_enabled {
        return;
    }
    options.event_loop_builder = Some(Box::new(|builder| {
        use winit::platform::macos::EventLoopBuilderExtMacOS as _;
        builder.with_activate_ignoring_other_apps(false);
    }));
}

#[cfg(not(target_os = "macos"))]
fn configure_startup_activation(
    _options: &mut eframe::NativeOptions,
    _startup_focus_enabled: bool,
) {
}

fn app_icon() -> eframe::egui::IconData {
    eframe::icon_data::from_png_bytes(include_bytes!("../assets/app-icon/snappyref-128.png"))
        .expect("embedded SnappyRef app icon should be valid PNG")
}
