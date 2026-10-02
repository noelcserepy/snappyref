//! The Keys page: every supported shortcut, action left and chips right.

use eframe::egui;

use crate::app::{CORE_SHORTCUTS, LAYOUT_AND_HISTORY_SHORTCUTS, SupportedShortcut};
use crate::ui::{card, shortcut, theme};

/// Chips need 4 above and below to stay clear of their neighbors.
const ROW_HEIGHT: f32 = 26.0;

pub(super) fn show(ui: &mut egui::Ui) {
    group(ui, "Core actions", CORE_SHORTCUTS);
    group(ui, "Layout + history", LAYOUT_AND_HISTORY_SHORTCUTS);
}

fn group(ui: &mut egui::Ui, title: &str, rows: &[SupportedShortcut]) {
    card::section(ui, title);
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        for row in rows {
            shortcut_row(ui, row);
        }
    });
}

fn shortcut_row(ui: &mut egui::Ui, row: &SupportedShortcut) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), ROW_HEIGHT),
        egui::Sense::hover(),
    );
    let keys = ui
        .scope_builder(
            egui::UiBuilder::new()
                .max_rect(rect)
                .layout(egui::Layout::right_to_left(egui::Align::Center)),
            |ui| shortcut::show(ui, row.keys).rect,
        )
        .inner;
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(rect.with_max_x(keys.left() - theme::SPACE_2))
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
