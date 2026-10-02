//! Sidebar chrome outside the card: the pull tab and the status pill.

use eframe::egui;

/// Collapsed sidebar tab: a short pull tab on the left screen edge, level
/// with the sidebar top and sized to its shortcut chips.
pub(crate) fn show_sidebar_edge(
    ctx: &egui::Context,
    screen_rect: egui::Rect,
    sidebar_open: bool,
) -> bool {
    use crate::ui::{motion, shortcut, theme};
    if sidebar_open {
        return false;
    }
    egui::Area::new(egui::Id::new("sidebar_edge"))
        .order(egui::Order::Foreground)
        .fixed_pos(egui::pos2(
            screen_rect.left(),
            super::sidebar_rect(screen_rect).top(),
        ))
        .show(ctx, |ui| {
            let keys = crate::app::TOGGLE_SIDEBAR;
            let size = egui::vec2(
                shortcut::width(ui.painter(), keys) + 2.0 * theme::SPACE_2,
                theme::CONTROL_HEIGHT,
            );
            let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
            response.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Toggle sidebar")
            });
            let hover = motion::hover_amount(
                ui.ctx(),
                egui::Id::new("sidebar_edge_hover"),
                response.hovered(),
            );
            let radius = theme::RADIUS_CONTROL;
            ui.painter().rect_filled(
                rect,
                egui::CornerRadius {
                    nw: 0,
                    ne: radius,
                    sw: 0,
                    se: radius,
                },
                motion::mix(theme::SURFACE, theme::SURFACE_ELEVATED, hover),
            );
            let color = motion::mix(theme::TEXT_MUTED, theme::TEXT, hover);
            shortcut::paint(
                ui.painter(),
                rect.center(),
                egui::Align2::CENTER_CENTER,
                keys,
                color,
            );
            if response.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            response.clicked()
        })
        .inner
}

/// Transient status pill: the result of the last action or a storage error.
/// Sits bottom-left, beside the sidebar when it is open, in the same
/// quiet muted-on-elevated style so it reads as chrome, not canvas. Call
/// every frame so the entrance restarts cleanly for the next message.
pub(crate) fn show_status_message(
    ctx: &egui::Context,
    screen_rect: egui::Rect,
    sidebar_open: bool,
    status: Option<&str>,
) {
    let entrance =
        crate::ui::motion::overlay_entrance(ctx, egui::Id::new("status_message"), status.is_some());
    let Some(status) = status else {
        return;
    };
    let anchor = egui::pos2(
        screen_rect.left()
            + crate::ui::theme::SPACE_4
            + if sidebar_open {
                super::SIDEBAR_WIDTH
            } else {
                0.0
            },
        screen_rect.bottom() - crate::ui::theme::SPACE_4,
    );
    let settle = crate::ui::motion::settle_offset(entrance);
    egui::Area::new(egui::Id::new("status_message"))
        .order(egui::Order::Foreground)
        .interactable(false)
        .pivot(egui::Align2::LEFT_BOTTOM)
        .fixed_pos(anchor + egui::vec2(0.0, settle))
        .show(ctx, |ui| {
            ui.set_opacity(entrance.clamp(0.0, 1.0));
            egui::Frame::new()
                .fill(crate::ui::theme::SURFACE_ELEVATED)
                .corner_radius(egui::CornerRadius::same(crate::ui::theme::RADIUS_COMPACT))
                .inner_margin(egui::Margin {
                    left: 8,
                    right: 8,
                    top: 6,
                    bottom: 6,
                })
                .show(ui, |ui| {
                    // Long import paths and errors wrap instead of running
                    // across the canvas.
                    let galley = ui.painter().layout(
                        status.to_owned(),
                        egui::FontId::proportional(crate::ui::theme::TYPE_DETAIL),
                        crate::ui::theme::TEXT_MUTED,
                        STATUS_MESSAGE_MAX_WIDTH,
                    );
                    let (rect, _) = ui.allocate_exact_size(galley.size(), egui::Sense::hover());
                    ui.painter()
                        .galley(rect.min, galley, crate::ui::theme::TEXT_MUTED);
                });
        });
}

const STATUS_MESSAGE_MAX_WIDTH: f32 = 360.0;
