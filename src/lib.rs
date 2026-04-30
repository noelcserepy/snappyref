mod app;
mod board_switch;
mod canvas;
mod data;
#[cfg(debug_assertions)]
mod design_system_preview;
mod document_persistence;
mod import_intake;
mod import_placement;
mod jobs;
mod layout_snap;
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

pub fn run() -> eframe::Result<()> {
    let config = app::AppConfig::from_env();
    if config.startup_trace_requested() {
        if let Err(error) = perf::print_startup_trace() {
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
    let viewport_size = config.startup_viewport_size();
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([viewport_size.x, viewport_size.y])
            .with_fullsize_content_view(true)
            .with_title_shown(false)
            .with_titlebar_shown(false)
            .with_titlebar_buttons_shown(true),
        ..Default::default()
    };

    eframe::run_native(
        "Reference",
        options,
        Box::new(|cc| {
            theme::configure_theme(&cc.egui_ctx);
            cc.egui_ctx.options_mut(|o| o.zoom_with_keyboard = false);
            egui_extras::install_image_loaders(&cc.egui_ctx);
            Ok(Box::new(app::ReferenceApp::new(cc, config.clone())))
        }),
    )
}
