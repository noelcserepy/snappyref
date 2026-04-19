mod app;
mod canvas;
mod data;
mod jobs;
mod native_capture;
mod perf;
mod screenshot;
#[cfg(feature = "semantic-search")]
mod semantic_search;
mod storage;
mod theme;

pub fn run() -> eframe::Result<()> {
    let config = app::AppConfig::from_env();
    if config.perf_report_requested() {
        if let Err(error) = perf::print_report() {
            return Err(eframe::Error::AppCreation(Box::new(std::io::Error::other(
                error,
            ))));
        }
        return Ok(());
    }
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([960.0, 640.0])
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
            egui_extras::install_image_loaders(&cc.egui_ctx);
            Ok(Box::new(app::ReferenceApp::new(cc, config.clone())))
        }),
    )
}
