use eframe::egui::{self, FontData, FontDefinitions, FontFamily, FontId, TextStyle};

const DM_SANS_VARIABLE: &[u8] = include_bytes!("../assets/fonts/DMSans-Variable.ttf");

const SHADE_20: egui::Color32 = color(0xAE, 0xAC, 0xA4);
const SHADE_30: egui::Color32 = color(0x99, 0x97, 0x90);
const SHADE_40: egui::Color32 = color(0x83, 0x81, 0x7B);
const SHADE_60: egui::Color32 = color(0x57, 0x56, 0x52);
const SHADE_70: egui::Color32 = color(0x41, 0x41, 0x3E);
const SHADE_80: egui::Color32 = color(0x2C, 0x2B, 0x29);
const SHADE_90: egui::Color32 = color(0x16, 0x16, 0x15);

const DISPLAY_TEXT_STYLE_NAME: &str = "display";

/// Tailwind-like 4px spacing scale for layout-driven hierarchy.
///
/// Usage:
/// - `SPACE_1` / `SPACE_2` for dense control gaps and overlay insets
/// - `SPACE_3` / `SPACE_4` for default control padding
/// - `SPACE_5` / `SPACE_6` for grid and section spacing
#[allow(dead_code)]
pub const SPACE_1: f32 = 4.0;
pub const SPACE_2: f32 = 8.0;
pub const SPACE_3: f32 = 12.0;
pub const SPACE_4: f32 = 16.0;
pub const SPACE_5: f32 = 20.0;
pub const SPACE_6: f32 = 24.0;
#[allow(dead_code)]
pub const SPACE_8: f32 = 32.0;

/// Standard drawer and panel padding.
pub const PANEL_PADDING: egui::Margin = egui::Margin {
    left: SPACE_4 as i8,
    right: SPACE_4 as i8,
    top: SPACE_4 as i8,
    bottom: SPACE_4 as i8,
};

/// Preferred gap between stacked sections.
pub const SECTION_GAP: f32 = SPACE_6;

/// Standard control height for compact toolbar and drawer controls.
pub const CONTROL_HEIGHT: f32 = 28.0;

pub const CONTROL_RADIUS: u8 = 8;
pub const CARD_RADIUS: u8 = 12;

/// Surface roles.
///
/// Usage:
/// - `BACKGROUND` for the canvas and app-level backdrop
/// - `SURFACE` for panels, drawers, and resting controls
/// - `SURFACE_ELEVATED` for hover states and selected surfaces that need separation
pub const BACKGROUND: egui::Color32 = SHADE_90;
pub const SURFACE: egui::Color32 = SHADE_80;
pub const SURFACE_ELEVATED: egui::Color32 = SHADE_70;

/// Border and text roles.
///
/// Usage:
/// - `BORDER` for quiet separators and resting control outlines
/// - `BORDER_STRONG` when a control needs stronger affordance
/// - `TEXT` for primary readable copy and overlay strokes
/// - `TEXT_MUTED` for helper text and secondary values
/// - `TEXT_SUBTLE` for section labels, table headers, and low-emphasis metadata
/// - `ACCENT` for active fills and the few states that need emphasis without adding a new hue
pub const BORDER: egui::Color32 = SHADE_60;
pub const BORDER_STRONG: egui::Color32 = SHADE_40;
pub const TEXT: egui::Color32 = color(0xDA, 0xD7, 0xCD);
pub const TEXT_MUTED: egui::Color32 = SHADE_30;
pub const TEXT_SUBTLE: egui::Color32 = SHADE_60;
pub const ACCENT: egui::Color32 = SHADE_20;

pub fn configure_theme(ctx: &egui::Context) {
    configure_fonts(ctx);
    ctx.set_theme(egui::Theme::Dark);
    ctx.all_styles_mut(apply_style);
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

fn apply_style(style: &mut egui::Style) {
    style.text_styles = [
        (display_text_style(), FontId::proportional(16.0)),
        (TextStyle::Heading, FontId::proportional(13.0)),
        (TextStyle::Body, FontId::proportional(12.0)),
        (TextStyle::Button, FontId::proportional(11.0)),
        (TextStyle::Small, FontId::proportional(10.0)),
        (TextStyle::Monospace, FontId::monospace(10.0)),
    ]
    .into();
    style.visuals = egui::Visuals::dark();
    style.visuals.override_text_color = Some(TEXT);
    style.visuals.panel_fill = BACKGROUND;
    style.visuals.window_fill = SURFACE;
    style.visuals.extreme_bg_color = BACKGROUND;
    style.visuals.faint_bg_color = SURFACE;
    style.visuals.code_bg_color = SURFACE;
    style.visuals.window_corner_radius = egui::CornerRadius::ZERO;
    style.visuals.menu_corner_radius = egui::CornerRadius::ZERO;
    style.visuals.window_stroke = stroke_subtle();
    style.visuals.widgets.noninteractive.bg_fill = BACKGROUND;
    style.visuals.widgets.noninteractive.bg_stroke = stroke_subtle();
    style.visuals.widgets.noninteractive.corner_radius = egui::CornerRadius::ZERO;
    style.visuals.widgets.noninteractive.fg_stroke = stroke_strong();
    style.visuals.widgets.inactive.bg_fill = SURFACE;
    style.visuals.widgets.inactive.bg_stroke = stroke_subtle();
    style.visuals.widgets.inactive.corner_radius = egui::CornerRadius::same(CONTROL_RADIUS);
    style.visuals.widgets.inactive.fg_stroke = stroke_strong();
    style.visuals.widgets.hovered.bg_fill = SURFACE_ELEVATED;
    style.visuals.widgets.hovered.bg_stroke = stroke_strong();
    style.visuals.widgets.hovered.corner_radius = egui::CornerRadius::same(CONTROL_RADIUS);
    style.visuals.widgets.hovered.fg_stroke = stroke_strong();
    style.visuals.widgets.active.bg_fill = BORDER;
    style.visuals.widgets.active.bg_stroke = egui::Stroke::new(1.0, ACCENT);
    style.visuals.widgets.active.corner_radius = egui::CornerRadius::same(CONTROL_RADIUS);
    style.visuals.widgets.active.fg_stroke = stroke_strong();
    style.visuals.selection.bg_fill = SURFACE_ELEVATED;
    style.visuals.selection.stroke = stroke_strong();
    style.visuals.hyperlink_color = ACCENT;
    style.spacing.item_spacing = egui::vec2(SPACE_3, SPACE_3);
    style.spacing.button_padding = egui::vec2(SPACE_3, SPACE_2);
}

pub fn display_text_style() -> TextStyle {
    TextStyle::Name(DISPLAY_TEXT_STYLE_NAME.into())
}

pub fn stroke_subtle() -> egui::Stroke {
    egui::Stroke::new(1.0, BORDER)
}

pub fn stroke_strong() -> egui::Stroke {
    egui::Stroke::new(1.0, TEXT)
}

const fn color(r: u8, g: u8, b: u8) -> egui::Color32 {
    egui::Color32::from_rgb(r, g, b)
}
