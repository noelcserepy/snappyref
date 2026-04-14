mod app;
mod canvas;
mod data;
mod jobs;
mod storage;
mod theme;

pub fn run() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default().with_inner_size([960.0, 640.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Reference",
        options,
        Box::new(|cc| {
            theme::configure_theme(&cc.egui_ctx);
            Ok(Box::new(app::ReferenceApp::new(cc)))
        }),
    )
}
