use eframe::egui;

use crate::app::{CORE_SHORTCUTS, LAYOUT_AND_HISTORY_SHORTCUTS, SupportedShortcut};
use crate::ui::{shortcut, theme};

pub(super) fn paint_supported_shortcuts_section(ui: &mut egui::Ui) {
    let pad = crate::ui::theme::SPACE_3;
    let indent_rect = egui::Rect::from_min_max(
        egui::pos2(ui.max_rect().left() + pad, ui.cursor().top()),
        egui::pos2(ui.max_rect().right() - pad, ui.max_rect().bottom()),
    );
    ui.scope_builder(egui::UiBuilder::new().max_rect(indent_rect), |ui| {
        ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
        paint_shortcut_group(ui, "Core actions", CORE_SHORTCUTS);
        ui.add_space(crate::ui::theme::SPACE_3);
        paint_shortcut_group(ui, "Layout + history", LAYOUT_AND_HISTORY_SHORTCUTS);
    });
}

fn paint_shortcut_group(ui: &mut egui::Ui, title: &str, shortcuts: &[SupportedShortcut]) {
    ui.label(crate::ui::theme::editorial_caption(title));
    ui.add_space(crate::ui::theme::SPACE_1);
    for shortcut in shortcuts.iter() {
        paint_shortcut_row(ui, shortcut);
    }
}

/// Action on the left, key chips right-aligned for scanning.
fn paint_shortcut_row(ui: &mut egui::Ui, row: &SupportedShortcut) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), ROW_HEIGHT),
        egui::Sense::hover(),
    );
    let keys = ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(egui::Layout::right_to_left(egui::Align::Center)),
        |ui| shortcut::show(ui, row.keys).rect,
    );
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(rect.with_max_x(keys.inner.left() - theme::SPACE_2))
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
        |ui| {
            ui.add(
                egui::Label::new(
                    egui::RichText::new(row.action)
                        .size(theme::TYPE_DETAIL)
                        .color(theme::TEXT),
                )
                .truncate(),
            );
        },
    );
}

/// Chips need 4 above and below to stay clear of their neighbors.
const ROW_HEIGHT: f32 = 26.0;
