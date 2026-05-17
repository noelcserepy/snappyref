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
        let label_color = if selected {
            crate::ui::theme::TEXT
        } else if response.hovered() {
            crate::ui::theme::TEXT_MUTED
        } else {
            crate::ui::theme::TEXT_SUBTLE
        };
        let font = ui.style().text_styles[&egui::TextStyle::Body].clone();
        let galley = ui
            .painter()
            .layout_no_wrap((*label).to_owned(), font, label_color);
        let label_pos = egui::pos2(
            cell.center().x - galley.size().x * 0.5,
            cell.center().y - galley.size().y * 0.5 + 2.0,
        );
        ui.painter().galley(label_pos, galley, label_color);
    }
}

pub(super) fn paint_sidebar_section_header(ui: &mut egui::Ui, title: &str, detail: Option<&str>) {
    let pad = crate::ui::theme::SPACE_3;
    let indent_rect = egui::Rect::from_min_max(
        egui::pos2(ui.max_rect().left() + pad, ui.cursor().top()),
        egui::pos2(ui.max_rect().right() - pad, ui.cursor().top() + 22.0),
    );
    ui.scope_builder(egui::UiBuilder::new().max_rect(indent_rect), |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = crate::ui::theme::SPACE_2;
            ui.label(
                egui::RichText::new(title)
                    .text_style(crate::ui::theme::display_text_style())
                    .color(crate::ui::theme::TEXT),
            );
            if let Some(detail) = detail {
                ui.with_layout(crate::ui::layout::row_end_center_y_layout(), |ui| {
                    ui.label(
                        egui::RichText::new(detail)
                            .text_style(egui::TextStyle::Small)
                            .color(crate::ui::theme::TEXT_SUBTLE),
                    );
                });
            }
        });
    });
    ui.add_space(22.0 + crate::ui::theme::SPACE_2);
}

pub(super) fn paint_sidebar_subcard(ui: &mut egui::Ui, add_contents: impl FnOnce(&mut egui::Ui)) {
    let pad = crate::ui::theme::SPACE_2;
    let indent_rect = egui::Rect::from_min_max(
        egui::pos2(ui.max_rect().left() + pad, ui.cursor().top()),
        egui::pos2(ui.max_rect().right() - pad, ui.max_rect().bottom()),
    );
    ui.scope_builder(egui::UiBuilder::new().max_rect(indent_rect), |ui| {
        egui::Frame::new()
            .fill(crate::ui::theme::QUIET_INSET_FILL)
            .stroke(crate::ui::theme::stroke_subtle())
            .corner_radius(egui::CornerRadius::same(crate::ui::theme::RADIUS_COMPACT))
            .inner_margin(egui::Margin {
                left: crate::ui::theme::SPACE_3 as i8,
                right: crate::ui::theme::SPACE_3 as i8,
                top: crate::ui::theme::SPACE_1 as i8,
                bottom: crate::ui::theme::SPACE_1 as i8,
            })
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
                add_contents(ui);
            });
    });
}

pub(crate) fn show_sidebar_hint(
    ctx: &egui::Context,
    screen_rect: egui::Rect,
    sidebar_open: bool,
    active_board_name: &str,
) -> bool {
    if sidebar_open {
        return false;
    }

    let mut clicked = false;
    egui::Area::new(egui::Id::new("sidebar_hint"))
        .order(egui::Order::Foreground)
        .fixed_pos(egui::pos2(
            screen_rect.left() + crate::ui::theme::SPACE_4,
            screen_rect.bottom() - crate::ui::theme::SPACE_4,
        ))
        .anchor(
            egui::Align2::LEFT_BOTTOM,
            egui::vec2(crate::ui::theme::SPACE_4, -crate::ui::theme::SPACE_4),
        )
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(crate::ui::theme::SURFACE_ELEVATED)
                .stroke(crate::ui::theme::stroke_subtle())
                .corner_radius(egui::CornerRadius::same(crate::ui::theme::RADIUS_COMPACT))
                .inner_margin(egui::Margin {
                    left: 8,
                    right: 8,
                    top: 6,
                    bottom: 6,
                })
                .show(ui, |ui| {
                    let label = format!(
                        "{}  {}",
                        truncate_board_name(active_board_name, 28),
                        shortcuts::display_shortcut_label(shortcuts::sidebar_shortcut_label())
                    );
                    let galley = ui.painter().layout_no_wrap(
                        label,
                        egui::TextStyle::Small.resolve(ui.style()),
                        crate::ui::theme::TEXT_MUTED,
                    );
                    let (rect, _) = ui.allocate_exact_size(galley.size(), egui::Sense::hover());
                    ui.painter()
                        .galley(rect.min, galley, crate::ui::theme::TEXT_MUTED);
                    let hovered = ui.ctx().input(|input| {
                        input
                            .pointer
                            .latest_pos()
                            .is_some_and(|pos| rect.contains(pos))
                    });
                    if hovered {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    clicked = hovered && ui.input(|input| input.pointer.primary_clicked());
                });
        });
    clicked
}

fn truncate_board_name(name: &str, max_chars: usize) -> String {
    if name.chars().count() <= max_chars {
        return name.to_owned();
    }
    let mut truncated = name
        .chars()
        .take(max_chars.saturating_sub(3))
        .collect::<String>();
    truncated.push_str("...");
    truncated
}
