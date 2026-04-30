use eframe::egui::{self, FontData, FontDefinitions, FontFamily, FontId, TextStyle};

const DM_SANS_VARIABLE: &[u8] = include_bytes!("../assets/fonts/DMSans-Variable.ttf");

const SHELL_BG: egui::Color32 = color(0x0A, 0x0A, 0x0A);
const PANEL_BG: egui::Color32 = color(0x16, 0x15, 0x14);
const PANEL_BG_PALETTE: egui::Color32 = color(0x17, 0x16, 0x15);
const PANEL_BG_ELEVATED: egui::Color32 = color(0x22, 0x21, 0x1F);
const ACCENT_GOLD: egui::Color32 = color(0xE4, 0xCE, 0xB0);
const INDEXED_GREEN: egui::Color32 = color(0x7A, 0xC9, 0x7A);
const DESTRUCTIVE_RED: egui::Color32 = color(0xCA, 0x3C, 0x25);

const DISPLAY_TEXT_STYLE_NAME: &str = "display";

/// Tailwind-like 4px spacing scale for layout-driven hierarchy.
///
/// Usage:
/// - `SPACE_1` / `SPACE_2` for dense control gaps, optical insets, and inline rhythm
/// - `SPACE_3` / `SPACE_4` for default control padding and compact panel rhythm
/// - `SPACE_6` / `SPACE_8` for section spacing, overlays, and canvas-adjacent offsets
#[allow(dead_code)]
pub const SPACE_1: f32 = 4.0;
pub const SPACE_2: f32 = 8.0;
pub const SPACE_3: f32 = 12.0;
pub const SPACE_4: f32 = 16.0;
#[allow(dead_code)]
pub const SPACE_6: f32 = 24.0;
#[allow(dead_code)]
pub const SPACE_8: f32 = 32.0;

/// Standard drawer and panel padding.
#[allow(dead_code)]
pub const PANEL_PADDING: egui::Margin = egui::Margin {
    left: SPACE_4 as i8,
    right: SPACE_4 as i8,
    top: SPACE_4 as i8,
    bottom: SPACE_4 as i8,
};

/// Preferred gap between stacked sections.
#[allow(dead_code)]
pub const SECTION_GAP: f32 = SPACE_6;

/// Standard control height for compact toolbar and drawer controls.
pub const CONTROL_HEIGHT: f32 = 28.0;

/// Radius roles.
///
/// Usage:
/// - `RADIUS_NONE` for structural fills, rules, underlines, and canvas geometry
/// - `RADIUS_MICRO` for tiny marks, active dots, thumbnails, and swatches under 24px
/// - `RADIUS_INLINE` for quiet inline setting controls where text stays primary
/// - `RADIUS_COMPACT` for rows, chips, hints, and small grouped containers
/// - `RADIUS_CONTROL` for explicit buttons and direct-manipulation toolbar targets
/// - `RADIUS_PANEL` for floating panels, overlays, and major framed surfaces
pub const RADIUS_NONE: u8 = 0;
pub const RADIUS_MICRO: u8 = 3;
pub const RADIUS_INLINE: u8 = 4;
pub const RADIUS_COMPACT: u8 = 6;
pub const RADIUS_CONTROL: u8 = 8;
pub const RADIUS_PANEL: u8 = 12;

/// Surface roles.
///
/// Usage:
/// - `BACKGROUND` for the canvas and app-level backdrop
/// - `SURFACE` for panels, drawers, and resting controls
/// - `PALETTE_SURFACE` for the command palette panel (slightly warmer than `SURFACE`)
/// - `SURFACE_ELEVATED` for hover states and selected surfaces that need separation
pub const BACKGROUND: egui::Color32 = SHELL_BG;
pub const SURFACE: egui::Color32 = PANEL_BG;
pub const PALETTE_SURFACE: egui::Color32 = PANEL_BG_PALETTE;
pub const SURFACE_ELEVATED: egui::Color32 = PANEL_BG_ELEVATED;

/// Border and text roles.
///
/// Usage:
/// - `BORDER` for quiet separators and resting control outlines
/// - `BORDER_STRONG` when a control needs stronger affordance
/// - `TEXT` for primary readable copy and overlay strokes
/// - `TEXT_MUTED` for helper text and secondary values
/// - `TEXT_SUBTLE` for section labels, table headers, and low-emphasis metadata
/// - `ACCENT` for active fills and the few states that need emphasis without adding a new hue
/// - `DESTRUCTIVE` for destructive labels and confirmation actions only
pub const BORDER: egui::Color32 = egui::Color32::from_rgba_premultiplied(20, 20, 20, 20);
pub const BORDER_STRONG: egui::Color32 = egui::Color32::from_rgba_premultiplied(31, 31, 31, 31);
pub const TEXT: egui::Color32 = color(0xF0, 0xF0, 0xF0);
pub const TEXT_MUTED: egui::Color32 = color(0x8C, 0x8C, 0x8C);
pub const TEXT_SUBTLE: egui::Color32 = color(0x66, 0x66, 0x66);
pub const ACCENT: egui::Color32 = ACCENT_GOLD;
pub const INDEXED_DOT: egui::Color32 = INDEXED_GREEN;
pub const DESTRUCTIVE: egui::Color32 = DESTRUCTIVE_RED;
pub const DESTRUCTIVE_STROKE: egui::Color32 =
    egui::Color32::from_rgba_premultiplied(119, 35, 22, 150);
pub const DESTRUCTIVE_FILL: egui::Color32 = egui::Color32::from_rgba_premultiplied(33, 10, 6, 42);

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
    style.visuals.window_corner_radius = egui::CornerRadius::same(RADIUS_NONE);
    style.visuals.menu_corner_radius = egui::CornerRadius::same(RADIUS_NONE);
    style.visuals.window_stroke = stroke_subtle();
    style.visuals.widgets.noninteractive.bg_fill = BACKGROUND;
    style.visuals.widgets.noninteractive.bg_stroke = stroke_subtle();
    style.visuals.widgets.noninteractive.corner_radius = egui::CornerRadius::same(RADIUS_NONE);
    style.visuals.widgets.noninteractive.fg_stroke = stroke_strong();
    style.visuals.widgets.inactive.bg_fill = SURFACE;
    style.visuals.widgets.inactive.bg_stroke = stroke_subtle();
    style.visuals.widgets.inactive.corner_radius = egui::CornerRadius::same(RADIUS_CONTROL);
    style.visuals.widgets.inactive.fg_stroke = stroke_strong();
    style.visuals.widgets.hovered.bg_fill = SURFACE_ELEVATED;
    style.visuals.widgets.hovered.bg_stroke = stroke_strong();
    style.visuals.widgets.hovered.corner_radius = egui::CornerRadius::same(RADIUS_CONTROL);
    style.visuals.widgets.hovered.fg_stroke = stroke_strong();
    style.visuals.widgets.active.bg_fill = BORDER;
    style.visuals.widgets.active.bg_stroke = egui::Stroke::new(1.0, ACCENT);
    style.visuals.widgets.active.corner_radius = egui::CornerRadius::same(RADIUS_CONTROL);
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
