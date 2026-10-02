//! The Group card's vocabulary, shared by every floating card and its
//! controls: the card frame, section captions, separators, inset wells,
//! segmented beds, switches, and tooltips.
//!
//! Inside a card, controls are [`theme::CONTROL_HEIGHT`] tall and sit
//! [`ITEM_SPACING`] apart; [`section`] adds 6 more above each caption, so
//! sections read 14 apart.

use eframe::egui::{self, Align2, Color32, FontId, Pos2, Response, Sense, Stroke, Vec2};

use super::theme;

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

/// A hairline rule between groups that need no caption, with 4 clear above
/// and below.
pub(crate) fn separator(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), 1.0 + 2.0 * theme::SPACE_1),
        Sense::hover(),
    );
    ui.painter().hline(
        rect.x_range().shrink(6.0),
        rect.center().y,
        Stroke::new(1.0_f32, theme::SURFACE_RAISED),
    );
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

/// Disabled controls draw at this opacity.
pub(crate) fn fade(enabled: bool) -> f32 {
    if enabled { 1.0 } else { 0.4 }
}
