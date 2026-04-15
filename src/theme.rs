use eframe::egui::{self, FontData, FontDefinitions, FontFamily};

const DM_SANS_VARIABLE: &[u8] = include_bytes!("../assets/fonts/DMSans-Variable.ttf");

#[allow(dead_code)]
pub const SHADE_BASE_HEX: &str = "#DAD7CD";
#[allow(dead_code)]
pub const SHADE_10_HEX: &str = "#C4C2B9";
#[allow(dead_code)]
pub const SHADE_20_HEX: &str = "#AEACA4";
#[allow(dead_code)]
pub const SHADE_30_HEX: &str = "#999790";
#[allow(dead_code)]
pub const SHADE_40_HEX: &str = "#83817B";
#[allow(dead_code)]
pub const SHADE_50_HEX: &str = "#6D6C67";
#[allow(dead_code)]
pub const SHADE_60_HEX: &str = "#575652";
#[allow(dead_code)]
pub const SHADE_70_HEX: &str = "#41413E";
#[allow(dead_code)]
pub const SHADE_80_HEX: &str = "#2C2B29";
#[allow(dead_code)]
pub const SHADE_90_HEX: &str = "#161615";

#[allow(dead_code)]
pub const SHADE_BASE: egui::Color32 = color(0xDA, 0xD7, 0xCD);
#[allow(dead_code)]
pub const SHADE_10: egui::Color32 = color(0xC4, 0xC2, 0xB9);
pub const SHADE_20: egui::Color32 = color(0xAE, 0xAC, 0xA4);
pub const SHADE_30: egui::Color32 = color(0x99, 0x97, 0x90);
pub const SHADE_40: egui::Color32 = color(0x83, 0x81, 0x7B);
#[allow(dead_code)]
pub const SHADE_50: egui::Color32 = color(0x6D, 0x6C, 0x67);
pub const SHADE_60: egui::Color32 = color(0x57, 0x56, 0x52);
pub const SHADE_70: egui::Color32 = color(0x41, 0x41, 0x3E);
pub const SHADE_80: egui::Color32 = color(0x2C, 0x2B, 0x29);
pub const SHADE_90: egui::Color32 = color(0x16, 0x16, 0x15);

pub fn configure_theme(ctx: &egui::Context) {
    configure_fonts(ctx);

    let mut style = (*ctx.global_style()).clone();
    style.visuals = egui::Visuals::dark();
    style.visuals.override_text_color = Some(SHADE_BASE);
    style.visuals.panel_fill = SHADE_90;
    style.visuals.window_fill = SHADE_80;
    style.visuals.extreme_bg_color = SHADE_90;
    style.visuals.faint_bg_color = SHADE_80;
    style.visuals.code_bg_color = SHADE_80;
    style.visuals.window_corner_radius = egui::CornerRadius::ZERO;
    style.visuals.menu_corner_radius = egui::CornerRadius::ZERO;
    style.visuals.window_stroke = egui::Stroke::new(1.0, SHADE_60);
    style.visuals.widgets.noninteractive.bg_fill = SHADE_90;
    style.visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, SHADE_60);
    style.visuals.widgets.noninteractive.corner_radius = egui::CornerRadius::ZERO;
    style.visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, SHADE_BASE);
    style.visuals.widgets.inactive.bg_fill = SHADE_80;
    style.visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, SHADE_60);
    style.visuals.widgets.inactive.corner_radius = egui::CornerRadius::ZERO;
    style.visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, SHADE_BASE);
    style.visuals.widgets.hovered.bg_fill = SHADE_70;
    style.visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, SHADE_40);
    style.visuals.widgets.hovered.corner_radius = egui::CornerRadius::ZERO;
    style.visuals.widgets.hovered.fg_stroke = egui::Stroke::new(1.0, SHADE_BASE);
    style.visuals.widgets.active.bg_fill = SHADE_60;
    style.visuals.widgets.active.bg_stroke = egui::Stroke::new(1.0, SHADE_30);
    style.visuals.widgets.active.corner_radius = egui::CornerRadius::ZERO;
    style.visuals.widgets.active.fg_stroke = egui::Stroke::new(1.0, SHADE_BASE);
    style.visuals.selection.bg_fill = SHADE_70;
    style.visuals.selection.stroke = egui::Stroke::new(1.0, SHADE_BASE);
    style.visuals.hyperlink_color = SHADE_10;
    style.spacing.item_spacing = egui::vec2(10.0, 10.0);
    style.spacing.button_padding = egui::vec2(12.0, 8.0);
    ctx.set_global_style(style);
}

fn configure_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    let dm_sans_name = "dm_sans_variable".to_owned();

    fonts.font_data.insert(
        dm_sans_name.clone(),
        FontData::from_static(DM_SANS_VARIABLE).into(),
    );

    fonts
        .families
        .get_mut(&FontFamily::Proportional)
        .expect("default proportional font family")
        .insert(0, dm_sans_name.clone());
    fonts
        .families
        .get_mut(&FontFamily::Monospace)
        .expect("default monospace font family")
        .insert(0, dm_sans_name);

    ctx.set_fonts(fonts);
}

const fn color(r: u8, g: u8, b: u8) -> egui::Color32 {
    egui::Color32::from_rgb(r, g, b)
}
