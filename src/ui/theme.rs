use eframe::egui::{self, FontData, FontDefinitions, FontFamily, FontId, TextStyle};

const DM_SANS_VARIABLE: &[u8] = include_bytes!("../../assets/fonts/DMSans-Variable.ttf");
/// JetBrains Mono 2.304 Regular subset to the macOS modifier symbols
/// (⌘ ⌥ ⇧ ⌃), which DM Sans lacks. One source keeps them the same size in
/// every shortcut label.
const MODIFIER_KEYS: &[u8] = include_bytes!("../../assets/fonts/JetBrainsMono-ModifierKeys.ttf");

const SHELL_BG: egui::Color32 = color(0x0A, 0x0A, 0x0A);
const PANEL_BG: egui::Color32 = color(0x16, 0x15, 0x14);
const PANEL_BG_ELEVATED: egui::Color32 = color(0x22, 0x21, 0x1F);
const ACCENT_GOLD: egui::Color32 = color(0xE4, 0xCE, 0xB0);
const DESTRUCTIVE_RED: egui::Color32 = color(0xCA, 0x3C, 0x25);

/// Tailwind-like 4px spacing scale for layout-driven hierarchy.
///
/// Usage:
/// - `SPACE_1` / `SPACE_2` for dense control gaps, optical insets, and inline rhythm
/// - `SPACE_3` / `SPACE_4` for default control padding and compact panel rhythm
/// - `SPACE_6` for section spacing, overlays, and canvas-adjacent offsets
pub const SPACE_1: f32 = 4.0;
pub const SPACE_2: f32 = 8.0;
pub const SPACE_3: f32 = 12.0;
pub const SPACE_4: f32 = 16.0;
pub const SPACE_6: f32 = 24.0;

/// Control height, set by the Group card: wells, segments, switches, and
/// toolbar buttons.
pub const CONTROL_HEIGHT: f32 = 30.0;

/// Type scale, set by the Group card. Captions stay on [`editorial_caption`].
///
/// Usage:
/// - `TYPE_TITLE` for card titles
/// - `TYPE_CONTROL` for control labels, menu rows, and field values
/// - `TYPE_DETAIL` for subtitles, hint labels, status lines, header actions, and tooltips
/// - `TYPE_CHIP` for key chips and small toggle chips
pub const TYPE_TITLE: f32 = 16.0;
pub const TYPE_CONTROL: f32 = 13.0;
pub const TYPE_DETAIL: f32 = 12.0;
pub const TYPE_CHIP: f32 = 11.0;

/// Radius roles.
///
/// Usage:
/// - `RADIUS_NONE` for structural fills, rules, underlines, and canvas geometry
/// - `RADIUS_MICRO` for tiny marks, active dots, thumbnails, and swatches under 24px
/// - `RADIUS_INLINE` for key chips
/// - `RADIUS_COMPACT` for tooltips, toasts, and small grouped containers
/// - `RADIUS_CONTROL` for card controls, menu rows, buttons, and toolbar targets
/// - `RADIUS_PANEL` for menus and edge-docked panels: 8px rows plus 4px padding
/// - `RADIUS_CARD` for floating cards: the Group card, search palette, group picker, and dialogs
pub const RADIUS_NONE: u8 = 0;
pub const RADIUS_MICRO: u8 = 3;
pub const RADIUS_INLINE: u8 = 4;
pub const RADIUS_COMPACT: u8 = 6;
pub const RADIUS_CONTROL: u8 = 8;
pub const RADIUS_PANEL: u8 = 12;
pub const RADIUS_CARD: u8 = 14;

/// Surface roles.
///
/// Usage:
/// - `BACKGROUND` for the canvas and app-level backdrop
/// - `SURFACE` for cards, sidebars, menus, and the search palette
/// - `SURFACE_PRESSED` for segmented beds and pressed controls
/// - `SURFACE_ELEVATED` for wells, hover states, and selected surfaces that need separation
/// - `SURFACE_RAISED` for marks that sit on wells: key chips, resting toggle
///   chips, the off switch track, and hairline rules inside cards
/// - `TOOLTIP` for tooltips, a step above everything so they read over cards
pub const BACKGROUND: egui::Color32 = SHELL_BG;
pub const SURFACE: egui::Color32 = PANEL_BG;
pub const SURFACE_PRESSED: egui::Color32 = color(0x1B, 0x1A, 0x19);
pub const SURFACE_ELEVATED: egui::Color32 = PANEL_BG_ELEVATED;
pub const SURFACE_RAISED: egui::Color32 = color(0x2A, 0x28, 0x26);
pub const TOOLTIP: egui::Color32 = color(0x2E, 0x2C, 0x2A);

/// Border and text roles.
///
/// Usage:
/// - `BORDER` for quiet separators and resting control outlines
/// - `BORDER_STRONG` when a control needs stronger affordance
/// - `TEXT` for primary readable copy and overlay strokes
/// - `TEXT_MUTED` for helper text and secondary values
/// - `TEXT_SUBTLE` for section labels, table headers, and low-emphasis metadata
/// - `ACCENT` for active fills and the few states that need emphasis without adding a new hue
/// - `ACCENT_*` color accents for canvas guides and other explicit color comparisons
/// - `GREEN` for green guide and indexed/ready semantic-search status
/// - `DESTRUCTIVE` for destructive labels and confirmation buttons only
/// - `DESTRUCTIVE_EDGE` / `DESTRUCTIVE_FILL` for failed content states
pub const BORDER: egui::Color32 = egui::Color32::from_rgba_premultiplied(20, 20, 20, 20);
pub const BORDER_STRONG: egui::Color32 = egui::Color32::from_rgba_premultiplied(31, 31, 31, 31);
pub const TEXT: egui::Color32 = color(0xF0, 0xF0, 0xF0);
pub const TEXT_MUTED: egui::Color32 = color(0x8C, 0x8C, 0x8C);
pub const TEXT_SUBTLE: egui::Color32 = color(0x66, 0x66, 0x66);
pub const ACCENT: egui::Color32 = ACCENT_GOLD;
pub const GREEN: egui::Color32 = color(0x32, 0x9F, 0x5B);
pub const ACCENT_ORANGE: egui::Color32 = color(0xD1, 0x7B, 0x0F);
pub const DESTRUCTIVE: egui::Color32 = DESTRUCTIVE_RED;
pub const DESTRUCTIVE_EDGE: egui::Color32 =
    egui::Color32::from_rgba_premultiplied(119, 35, 22, 150);
pub const DESTRUCTIVE_FILL: egui::Color32 = egui::Color32::from_rgba_premultiplied(33, 10, 6, 42);

/// Editorial caption tracking.
///
/// `CAPTION_TRACKING` is the extra letter spacing (in points) applied to
/// uppercase editorial captions produced by [`editorial_caption`]. It exists only to compensate uppercase
/// microcaps set at `Small` size; do not apply it to body copy, buttons,
/// or mixed-case labels.
pub const CAPTION_TRACKING: f32 = 1.0;

/// Motion roles for the chosen flat-plus-fast motion language.
///
/// The static visual base is flat and editorial. Motion is reserved for fast
/// state feedback, tiny settle entrances, canvas acquisition, and sliding
/// active indicators. Avoid decorative loops and large travel.
///
/// Usage:
/// - `MOTION_INDICATOR_SECONDS` slide duration for active-tab/selection
///   indicators moving between positions. Not for content movement.
/// - `MOTION_SETTLE_SECONDS` and `MOTION_SETTLE_DISTANCE` for dialogs,
///   sidebar, context menus, and toolbars: opacity plus a tiny settle.
/// - `MOTION_FEEDBACK_SECONDS` for hover/press/acquire feedback.
/// - `MOTION_CONTENT_SECONDS` for fast canvas loading/hover fades.
/// - `STATUS_MESSAGE_SECONDS` how long a transient status message
///   (action result or storage error) stays visible bottom-left.
pub const MOTION_INDICATOR_SECONDS: f32 = 0.14;
pub const MOTION_SETTLE_SECONDS: f32 = 0.14;
pub const MOTION_FEEDBACK_SECONDS: f32 = 0.12;
pub const MOTION_CONTENT_SECONDS: f32 = 0.12;
pub const MOTION_PULSE_SECONDS: f32 = 0.16;
pub const MOTION_SETTLE_DISTANCE: f32 = 2.0;
pub const MOTION_PRESS_SCALE: f32 = 0.985;
pub const MOTION_ENTER_SCALE: f32 = 0.985;
pub const STATUS_MESSAGE_SECONDS: f32 = 4.0;
pub const HOVER_HAIRLINE_OPACITY: f32 = 0.35;

/// Editorial caption text: the quiet uppercase microcap used for section
/// labels, list group titles, and other structural captions. Captions name
/// structure; they are never interactive copy, helper text, or values.
pub fn editorial_caption(text: &str) -> egui::RichText {
    egui::RichText::new(text.to_uppercase())
        .text_style(TextStyle::Small)
        .color(TEXT_SUBTLE)
        .extra_letter_spacing(CAPTION_TRACKING)
}

pub fn configure_theme(ctx: &egui::Context) {
    configure_fonts(ctx);
    ctx.set_theme(egui::Theme::Dark);
    ctx.all_styles_mut(apply_style);
}

fn configure_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    // Ahead of egui's defaults, in fallback order.
    let bundled_fonts = [
        ("dm_sans_variable", DM_SANS_VARIABLE),
        ("modifier_keys", MODIFIER_KEYS),
    ];

    for (index, (name, data)) in bundled_fonts.into_iter().enumerate() {
        fonts
            .font_data
            .insert(name.to_owned(), FontData::from_static(data).into());
        for family in [FontFamily::Proportional, FontFamily::Monospace] {
            fonts
                .families
                .get_mut(&family)
                .expect("default font family")
                .insert(index, name.to_owned());
        }
    }

    ctx.set_fonts(fonts);
}

fn apply_style(style: &mut egui::Style) {
    // egui's default widgets follow the type scale. `Small` exists only for
    // `editorial_caption` microcaps.
    style.text_styles = [
        (TextStyle::Heading, FontId::proportional(TYPE_TITLE)),
        (TextStyle::Body, FontId::proportional(TYPE_DETAIL)),
        (TextStyle::Button, FontId::proportional(TYPE_CHIP)),
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
    style.visuals.menu_corner_radius = egui::CornerRadius::same(RADIUS_COMPACT);
    // Borders are nearly eliminated: floating chrome separates from the
    // canvas with surface tone and soft shadow, not outlines.
    style.visuals.window_stroke = egui::Stroke::NONE;
    style.visuals.window_shadow = egui::epaint::Shadow {
        offset: [0, 8],
        blur: 32,
        spread: 0,
        color: egui::Color32::from_black_alpha(140),
    };
    style.visuals.popup_shadow = egui::epaint::Shadow {
        offset: [0, 4],
        blur: 16,
        spread: 0,
        color: egui::Color32::from_black_alpha(120),
    };
    style.visuals.widgets.noninteractive.bg_fill = BACKGROUND;
    style.visuals.widgets.noninteractive.bg_stroke = stroke_subtle();
    style.visuals.widgets.noninteractive.corner_radius = egui::CornerRadius::same(RADIUS_NONE);
    style.visuals.widgets.noninteractive.fg_stroke = stroke_strong();
    style.visuals.widgets.inactive.bg_fill = SURFACE;
    style.visuals.widgets.inactive.bg_stroke = egui::Stroke::NONE;
    style.visuals.widgets.inactive.corner_radius = egui::CornerRadius::same(RADIUS_CONTROL);
    style.visuals.widgets.inactive.fg_stroke = stroke_strong();
    style.visuals.widgets.hovered.bg_fill = SURFACE_ELEVATED;
    style.visuals.widgets.hovered.bg_stroke = egui::Stroke::NONE;
    style.visuals.widgets.hovered.corner_radius = egui::CornerRadius::same(RADIUS_CONTROL);
    style.visuals.widgets.hovered.fg_stroke = stroke_strong();
    style.visuals.widgets.active.bg_fill = SURFACE_PRESSED;
    style.visuals.widgets.active.bg_stroke = egui::Stroke::new(1.0_f32, ACCENT);
    style.visuals.widgets.active.corner_radius = egui::CornerRadius::same(RADIUS_CONTROL);
    style.visuals.widgets.active.fg_stroke = stroke_strong();
    style.visuals.selection.bg_fill = SURFACE_ELEVATED;
    // Focus carries the single accent thread.
    style.visuals.selection.stroke = egui::Stroke::new(1.0_f32, ACCENT);
    style.visuals.hyperlink_color = ACCENT;
    style.animation_time = MOTION_FEEDBACK_SECONDS;
    style.spacing.item_spacing = egui::vec2(SPACE_3, SPACE_3);
    style.spacing.button_padding = egui::vec2(SPACE_3, SPACE_2);
}

pub fn stroke_subtle() -> egui::Stroke {
    egui::Stroke::new(1.0_f32, BORDER)
}

pub fn stroke_strong() -> egui::Stroke {
    egui::Stroke::new(1.0_f32, TEXT)
}

pub fn motion_easing(t: f32) -> f32 {
    egui::emath::easing::cubic_out(t)
}

const fn color(r: u8, g: u8, b: u8) -> egui::Color32 {
    egui::Color32::from_rgb(r, g, b)
}

#[cfg(test)]
mod tests {
    use eframe::egui::{self, FontId};

    #[test]
    fn macos_modifier_symbols_have_glyphs() {
        let ctx = egui::Context::default();
        super::configure_theme(&ctx);
        crate::ui::test_frame::run_ui(&ctx, egui::RawInput::default(), |_| {});

        for font in [FontId::proportional(12.0), FontId::monospace(10.0)] {
            assert!(ctx.fonts_mut(|fonts| fonts.has_glyphs(&font, "⌘⌥⇧⌃")));
        }
    }
}
