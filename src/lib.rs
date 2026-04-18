mod app;
mod canvas;
mod data;
mod jobs;
mod native_capture;
mod screenshot;
mod storage;
mod theme;

pub fn run() -> eframe::Result<()> {
    let config = app::AppConfig::from_env();
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
