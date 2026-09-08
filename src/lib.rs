mod active_board;
mod app;
mod canvas;
mod data;
mod diagnostics;
mod image_preparation;
mod library;
mod local_data_location;
mod placement;
mod platform;
mod search;
mod sources;
#[cfg(test)]
mod test_support;
mod ui;
pub mod updater;

pub fn run() -> eframe::Result<()> {
    #[cfg(target_os = "windows")]
    if updater::windows_self_replace::launch_args_contain_runner_flag(std::env::args().skip(1)) {
        let exit_code = updater::windows_self_replace::run_runner_from_args();
        std::process::exit(exit_code);
    }

    if std::env::args()
        .skip(1)
        .any(|argument| argument == "--imageprep")
    {
        if let Err(error) = image_preparation::print_imageprep_output_from_args() {
            return Err(eframe::Error::AppCreation(Box::new(std::io::Error::other(
                error,
            ))));
        }
        return Ok(());
    }

    if std::env::args()
        .skip(1)
        .any(|argument| argument == "--semantic-embed-server")
    {
        if let Err(error) = search::semantic::run_embed_image_server() {
            return Err(eframe::Error::AppCreation(Box::new(std::io::Error::other(
                error,
            ))));
        }
        return Ok(());
    }

    let config = app::AppConfig::from_env();
    if let Some(path) = config.perf_decode_path() {
        if let Err(error) = diagnostics::perf::print_file_decode_report(path) {
            return Err(eframe::Error::AppCreation(Box::new(std::io::Error::other(
                error,
            ))));
        }
        return Ok(());
    }
    let viewport_size = config.startup_viewport_size();
    let startup = config.prepare_startup();
    let startup_focus_enabled = config.startup_focus_enabled();
    let mut viewport = eframe::egui::ViewportBuilder::default()
        .with_inner_size([viewport_size.x, viewport_size.y])
        .with_icon(app_icon())
        .with_fullsize_content_view(true)
        .with_title_shown(false)
        .with_titlebar_shown(false)
        .with_titlebar_buttons_shown(true)
        .with_active(startup_focus_enabled);
    if let Some(window) = startup.window() {
        if let (Some(width), Some(height)) = (window.inner_width, window.inner_height) {
            viewport = viewport.with_inner_size([width, height]);
        }
        if let (Some(x), Some(y)) = (window.outer_x, window.outer_y) {
            viewport = viewport.with_position([x, y]);
        }
    }
    if startup.always_on_top() {
        viewport = viewport.with_window_level(eframe::egui::viewport::WindowLevel::AlwaysOnTop);
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
            ui::theme::configure_theme(&cc.egui_ctx);
            cc.egui_ctx.options_mut(|o| o.zoom_with_keyboard = false);
            egui_extras::install_image_loaders(&cc.egui_ctx);
            Ok(Box::new(app::SnappyRefApp::new(cc, config, startup)))
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
