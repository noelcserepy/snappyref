use eframe::egui;

use super::{SidebarTab, shortcuts};

pub(super) fn paint_sidebar_tabs(ui: &mut egui::Ui, sidebar_tab: &mut SidebarTab) {
    let bar_height = 32.0;
    let (_id, rect) = ui.allocate_space(egui::vec2(ui.available_width(), bar_height + 1.0));
    let pad_x = crate::ui::theme::SPACE_3;
    let tab_rect = egui::Rect::from_min_max(egui::pos2(rect.left() + pad_x, rect.top()), rect.max);
    let tabs: [(&str, SidebarTab); 2] = [
        ("Boards", SidebarTab::Boards),
        ("Settings", SidebarTab::Options),
    ];
    let tab_width = (tab_rect.width() - pad_x) / tabs.len() as f32;
    let mut active_label_rect = None;
    for (index, (label, tab_value)) in tabs.iter().enumerate() {
        let cell_min = egui::pos2(tab_rect.left() + tab_width * index as f32, tab_rect.top());
        let cell = egui::Rect::from_min_size(cell_min, egui::vec2(tab_width, bar_height));
        let selected = *sidebar_tab == *tab_value;
        let response = ui.interact(
            cell,
            egui::Id::new(("sidebar_tab", label)),
            egui::Sense::click(),
        );
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), *label)
        });
        if response.clicked() {
            *sidebar_tab = *tab_value;
        }
        let hover = crate::ui::motion::hover_amount(
            ui.ctx(),
            egui::Id::new(("sidebar_tab_hover", label)),
            response.hovered() && !selected,
        );
        let press = crate::ui::motion::press_amount(
            ui.ctx(),
            egui::Id::new(("sidebar_tab_press", label)),
            response.is_pointer_button_down_on(),
        );
        let label_color = if selected {
            crate::ui::theme::TEXT
        } else {
            crate::ui::motion::mix(
                crate::ui::theme::TEXT_SUBTLE,
                crate::ui::theme::TEXT_MUTED,
                hover,
            )
        };
        let galley = crate::ui::theme::editorial_caption_galley(ui, label, label_color);
        let label_pos = egui::pos2(
            cell.center().x - galley.size().x * 0.5,
            cell.center().y - galley.size().y * 0.5 + 2.0 + press * 0.5,
        );
        let label_size = galley.size();
        ui.painter().galley(label_pos, galley, label_color);
        if selected {
            active_label_rect = Some(egui::Rect::from_min_size(label_pos, label_size));
        }
    }
    // Active-tab indicator: a single accent underline that slides between
    // tab positions instead of repainting in place.
    if let Some(label_rect) = active_label_rect {
        let center_x = crate::ui::motion::slide_indicator(
            ui.ctx(),
            egui::Id::new("sidebar_tab_indicator_x"),
            label_rect.center().x,
        );
        let half_width = crate::ui::motion::slide_indicator(
            ui.ctx(),
            egui::Id::new("sidebar_tab_indicator_half_width"),
            label_rect.width() * 0.5,
        );
        let underline_y = label_rect.bottom() + 4.0;
        ui.painter().line_segment(
            [
                egui::pos2(center_x - half_width, underline_y),
                egui::pos2(center_x + half_width, underline_y),
            ],
            egui::Stroke::new(2.0_f32, crate::ui::theme::ACCENT),
        );
    }
}

/// Editorial section caption: a quiet uppercase microcap that names the
/// section. Spacing and the caption carry the grouping; there is no frame.
pub(super) fn paint_sidebar_section_header(ui: &mut egui::Ui, title: &str, detail: Option<&str>) {
    let pad = crate::ui::theme::SPACE_3;
    let indent_rect = egui::Rect::from_min_max(
        egui::pos2(ui.max_rect().left() + pad, ui.cursor().top()),
        egui::pos2(ui.max_rect().right() - pad, ui.cursor().top() + 16.0),
    );
    ui.scope_builder(egui::UiBuilder::new().max_rect(indent_rect), |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = crate::ui::theme::SPACE_2;
            ui.label(crate::ui::theme::editorial_caption(title));
            if let Some(detail) = detail {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new(detail)
                            .text_style(egui::TextStyle::Small)
                            .color(crate::ui::theme::TEXT_SUBTLE),
                    );
                });
            }
        });
    });
    ui.add_space(16.0 + crate::ui::theme::SPACE_2);
}

/// Flat sidebar section: alignment and padding only, no fill or stroke.
/// Structure is carried by the section caption above it and the spacing
/// around it.
pub(super) fn paint_sidebar_subcard(ui: &mut egui::Ui, add_contents: impl FnOnce(&mut egui::Ui)) {
    let pad = crate::ui::theme::SPACE_2;
    let indent_rect = egui::Rect::from_min_max(
        egui::pos2(ui.max_rect().left() + pad, ui.cursor().top()),
        egui::pos2(ui.max_rect().right() - pad, ui.max_rect().bottom()),
    );
    ui.scope_builder(egui::UiBuilder::new().max_rect(indent_rect), |ui| {
        egui::Frame::new()
            .inner_margin(egui::Margin {
                left: crate::ui::theme::SPACE_3 as i8,
                right: crate::ui::theme::SPACE_3 as i8,
                top: 0,
                bottom: 0,
            })
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
                add_contents(ui);
            });
    });
}

/// Collapsed sidebar tab: a short pull tab on the left screen edge, level
/// with the sidebar top and sized to its shortcut label.
pub(crate) fn show_sidebar_edge(
    ctx: &egui::Context,
    screen_rect: egui::Rect,
    sidebar_open: bool,
) -> bool {
    use crate::ui::{motion, theme};
    if sidebar_open {
        return false;
    }
    egui::Area::new(egui::Id::new("sidebar_edge"))
        .order(egui::Order::Foreground)
        .fixed_pos(super::sidebar_rect(screen_rect).min)
        .show(ctx, |ui| {
            let galley = ui.painter().layout_no_wrap(
                shortcuts::sidebar_shortcut_label().to_owned(),
                egui::FontId::proportional(13.0),
                egui::Color32::PLACEHOLDER,
            );
            let size = galley.size() + egui::vec2(2.0 * theme::SPACE_3, theme::SPACE_3);
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
                motion::mix(theme::SURFACE_ELEVATED, theme::TEXT_SUBTLE, hover * 0.25),
            );
            let color = motion::mix(theme::TEXT_MUTED, theme::TEXT, hover);
            ui.painter()
                .galley(rect.center() - galley.size() * 0.5, galley, color);
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
                        egui::TextStyle::Small.resolve(ui.style()),
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
