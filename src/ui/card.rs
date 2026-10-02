//! The Group card's vocabulary, shared by every floating card and its
//! controls: the card frame, section captions, inset wells, segmented beds,
//! switches, the primary button and quiet text actions, progress tracks,
//! status lines, and tooltips.
//!
//! Inside a card, controls are [`theme::CONTROL_HEIGHT`] tall and sit
//! [`ITEM_SPACING`] apart; [`section`] adds 6 more above each caption, so
//! sections read 14 apart.

use eframe::egui::{self, Align2, Color32, FontId, Pos2, Response, Sense, Stroke, Vec2};

use super::{motion, theme};

/// Card padding.
pub(crate) const PADDING: f32 = theme::SPACE_3;
/// Controls sit 6 apart on a line, and lines 8 apart.
pub(crate) const ITEM_SPACING: Vec2 = egui::vec2(6.0, 8.0);
/// A bed's radius keeps its 8px cells concentric inside the 2px margin.
const BED_RADIUS: u8 = 10;

/// The floating card: surface fill, card radius, deep soft shadow, and
/// padding. Menus tighten the padding and radius together; see `RADIUS_PANEL`.
pub(crate) fn frame() -> egui::Frame {
    egui::Frame::new()
        .fill(theme::SURFACE)
        .corner_radius(theme::RADIUS_CARD)
        .inner_margin(PADDING)
        .shadow(egui::epaint::Shadow {
            offset: [0, 12],
            blur: 40,
            spread: 0,
            color: Color32::from_black_alpha(180),
        })
}

/// A section caption with the extra space that separates it from the
/// section above.
pub(crate) fn section(ui: &mut egui::Ui, title: &str) {
    ui.add_space(14.0 - ITEM_SPACING.y);
    ui.label(theme::editorial_caption(title));
}

/// An inset well `width` wide: the field and switch background. Contents
/// lay out left to right, vertically centered, inside 10px side padding.
pub(crate) fn well<R>(
    ui: &mut egui::Ui,
    width: f32,
    contents: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    egui::Frame::new()
        .fill(theme::SURFACE_ELEVATED)
        .corner_radius(theme::RADIUS_CONTROL)
        .inner_margin(egui::Margin::symmetric(10, 0))
        .show(ui, |ui| {
            let inner = width - 20.0;
            ui.allocate_ui_with_layout(
                egui::vec2(inner, theme::CONTROL_HEIGHT),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.set_min_width(inner);
                    contents(ui)
                },
            )
            .inner
        })
        .inner
}

/// A segmented control's bed. Fill it with [`cell`]s on `SURFACE_PRESSED`.
pub(crate) fn bed(ui: &mut egui::Ui, contents: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .fill(theme::SURFACE_PRESSED)
        .corner_radius(BED_RADIUS)
        .inner_margin(2)
        .show(ui, |ui| {
            ui.allocate_ui_with_layout(
                egui::vec2(0.0, theme::CONTROL_HEIGHT),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    contents(ui);
                },
            );
        });
}

/// A named, clickable cell that reports its selected state. Selected cells
/// fill gold; others show `fill` at rest and lift on hover. Paint the face
/// with [`ink`].
pub(crate) fn cell(
    ui: &mut egui::Ui,
    name: &str,
    size: Vec2,
    selected: bool,
    enabled: bool,
    fill: Color32,
) -> Response {
    let response = hit(ui, name, size, selected, enabled);
    let color = if selected {
        theme::ACCENT
    } else if response.is_pointer_button_down_on() {
        theme::SURFACE_PRESSED
    } else if response.hovered() {
        theme::SURFACE_ELEVATED
    } else {
        fill
    };
    ui.painter()
        .rect_filled(response.rect, theme::RADIUS_CONTROL, color);
    focus_ring(ui, &response);
    response
}

/// Ink for a cell's face: dark on gold, muted or full text otherwise, and
/// faded while disabled.
pub(crate) fn ink(selected: bool, enabled: bool, muted: bool) -> Color32 {
    let color = if selected {
        theme::BACKGROUND
    } else if muted {
        theme::TEXT_MUTED
    } else {
        theme::TEXT
    };
    color.gamma_multiply(fade(enabled))
}

/// A named on/off row: icon and name on the left, track on the right.
/// `icon` paints the 14px icon centered at the given point.
pub(crate) fn switch(
    ui: &mut egui::Ui,
    name: &str,
    width: f32,
    on: bool,
    enabled: bool,
    icon: impl FnOnce(&egui::Painter, Pos2, Color32),
) -> Response {
    let response = hit(
        ui,
        name,
        egui::vec2(width, theme::CONTROL_HEIGHT),
        on,
        enabled,
    );
    let r = response.rect;
    ui.painter()
        .rect_filled(r, theme::RADIUS_CONTROL, theme::SURFACE_ELEVATED);
    focus_ring(ui, &response);
    let p = ui.painter();
    let fade = fade(enabled);
    icon(
        p,
        r.left_center() + egui::vec2(17.0, 0.0),
        theme::TEXT_MUTED.gamma_multiply(fade),
    );
    p.text(
        r.left_center() + egui::vec2(30.0, 0.0),
        Align2::LEFT_CENTER,
        name,
        FontId::proportional(theme::TYPE_CONTROL),
        theme::TEXT.gamma_multiply(fade),
    );
    let track = egui::Rect::from_center_size(
        r.right_center() - egui::vec2(25.0, 0.0),
        egui::vec2(30.0, 18.0),
    );
    let track_fill = if on {
        theme::ACCENT
    } else {
        theme::SURFACE_RAISED
    };
    p.rect_filled(track, 9, track_fill.gamma_multiply(fade));
    let knob = if on {
        theme::BACKGROUND
    } else if response.hovered() {
        theme::TEXT
    } else {
        theme::TEXT_MUTED
    };
    p.circle_filled(
        track.center() + egui::vec2(if on { 6.0 } else { -6.0 }, 0.0),
        7.0,
        knob.gamma_multiply(fade),
    );
    response
}

/// The card's primary action: a gold button `width` wide with dark ink.
/// A card has at most one.
pub(crate) fn button(ui: &mut egui::Ui, label: &str, width: f32) -> Response {
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(width, theme::CONTROL_HEIGHT), Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    let fill = if response.is_pointer_button_down_on() {
        motion::mix(theme::ACCENT, theme::BACKGROUND, 0.15)
    } else if response.hovered() {
        motion::mix(theme::ACCENT, theme::TEXT, 0.3)
    } else {
        theme::ACCENT
    };
    let p = ui.painter();
    p.rect_filled(rect, theme::RADIUS_CONTROL, fill);
    p.text(
        rect.center(),
        Align2::CENTER_CENTER,
        label,
        FontId::proportional(theme::TYPE_CONTROL),
        theme::BACKGROUND,
    );
    response
}

/// A quiet text action, like the Group card's Ungroup: muted text that
/// lifts on hover.
pub(crate) fn action(ui: &mut egui::Ui, label: &str, enabled: bool) -> Response {
    let font = FontId::proportional(theme::TYPE_DETAIL);
    let text_width = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font.clone(), theme::TEXT)
        .size()
        .x;
    let response = cell(
        ui,
        label,
        egui::vec2(text_width + 16.0, 26.0),
        false,
        enabled,
        Color32::TRANSPARENT,
    );
    ui.painter().text(
        response.rect.center(),
        Align2::CENTER_CENTER,
        label,
        font,
        ink(false, enabled, true),
    );
    response
}

/// A determinate progress track `width` wide: gold fill on the well tone.
pub(crate) fn progress(ui: &mut egui::Ui, width: f32, fraction: f32) -> Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 4.0), Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, 2, theme::SURFACE_ELEVATED);
    let mut filled = rect;
    filled.set_width(rect.width() * fraction.clamp(0.0, 1.0));
    p.rect_filled(filled, 2, theme::ACCENT);
    response
}

/// A status dot in `color` and its label, on the hint label's type.
pub(crate) fn status(ui: &mut egui::Ui, color: Color32, label: &str) -> Response {
    let galley = ui.painter().layout_no_wrap(
        label.to_owned(),
        FontId::proportional(theme::TYPE_DETAIL),
        theme::TEXT_MUTED,
    );
    let dot = 6.0;
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(dot + theme::SPACE_2 + galley.size().x, 18.0),
        Sense::hover(),
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, label));
    let p = ui.painter();
    p.circle_filled(
        rect.left_center() + egui::vec2(dot * 0.5, 0.0),
        dot * 0.5,
        color,
    );
    p.galley(
        egui::pos2(
            rect.left() + dot + theme::SPACE_2,
            rect.center().y - galley.size().y * 0.5,
        ),
        galley,
        theme::TEXT_MUTED,
    );
    response
}

/// Card tooltips sit above their control on the tooltip tone, because the
/// global popup fill matches the card and would vanish against it.
pub(crate) fn tip(response: Response, text: &str) -> Response {
    let mut tooltip = if response.enabled() {
        egui::Tooltip::for_enabled(&response)
    } else {
        egui::Tooltip::for_disabled(&response)
    };
    tooltip.popup = tooltip.popup.align(egui::RectAlign::TOP).gap(8.0).frame(
        egui::Frame::new()
            .fill(theme::TOOLTIP)
            .corner_radius(theme::RADIUS_COMPACT)
            .inner_margin(egui::Margin::symmetric(8, 4)),
    );
    tooltip.show(|ui| {
        ui.label(
            egui::RichText::new(text)
                .size(theme::TYPE_DETAIL)
                .color(theme::TEXT),
        );
    });
    response
}

fn hit(ui: &mut egui::Ui, name: &str, size: Vec2, selected: bool, enabled: bool) -> Response {
    ui.add_enabled_ui(enabled, |ui| {
        let (_, response) = ui.allocate_exact_size(size, Sense::click());
        response.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::Button, enabled, selected, name)
        });
        response
    })
    .inner
}

fn focus_ring(ui: &egui::Ui, response: &Response) {
    if response.has_focus() {
        ui.painter().rect_stroke(
            response.rect,
            theme::RADIUS_CONTROL,
            Stroke::new(1.0_f32, theme::ACCENT),
            egui::StrokeKind::Inside,
        );
    }
}

fn fade(enabled: bool) -> f32 {
    if enabled { 1.0 } else { 0.4 }
}
