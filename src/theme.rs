use eframe::egui;

pub fn configure_theme(ctx: &egui::Context) {
    let mut style = (*ctx.global_style()).clone();
    style.spacing.item_spacing = egui::vec2(10.0, 10.0);
    style.spacing.button_padding = egui::vec2(12.0, 8.0);
    style.visuals.panel_fill = egui::Color32::from_rgb(16, 18, 24);
    style.visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(36, 40, 52);
    style.visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(56, 62, 82);
    style.visuals.widgets.active.bg_fill = egui::Color32::from_rgb(82, 96, 128);
    ctx.set_global_style(style);
}
